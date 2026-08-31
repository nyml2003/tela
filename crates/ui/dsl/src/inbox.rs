//! 生命周期绑定的非 HostInput 组件事件邮箱。
//!
//! 它和候选 `Output` 队列刻意分离：此处只保存从定时器、任务或网络回调送回 UI 线程的
//! “组件自己的 Event”。消息出队后才由 [`crate::FrameCoordinator`] 创建候选事务；后台
//! 线程永远不会取得候选 State、`ComponentIdentity` 或任意组件路由能力。

use std::{
    any::Any,
    collections::VecDeque,
    marker::PhantomData,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use crate::{
    ViewSite,
    candidate::{ComponentLease, ComponentLeaseToken},
};

/// 外部组件事件邮箱的固定容量。
///
/// 这是 Host ingress 的背压边界，不是候选 Output 的 16 批/4096 条预算。发送者必须处理
/// [`ComponentEventSendError::Full`]，不能让后台生产者无界占用 UI 内存。
const COMPONENT_EVENT_MAILBOX_CAPACITY: usize = 4096;

/// Host 用来请求 UI 线程开始下一帧的窄端口。
///
/// 一个生命周期绑定的 sender 在后台线程成功入队后只会调用这一个方法。实现者负责把
/// 工作切回自己的 UI 调度器；它不能直接调用组件 handler 或读取候选 State。
pub trait ComponentEventInvalidator: Send + Sync {
    /// 请求 UI 调度器尽快处理已入队的组件事件。
    fn request_component_event_frame(&self);
}

/// 组件 `setup` 或已提交 `Mounted` capability 取得的、仅能向自身投递类型化 Event 的能力。
///
/// 该值可以安全地交给该组件拥有的定时器、任务或流回调。它既不暴露组件 identity，也不
/// 允许选择其他接收者；组件卸载、类型替换或同 key 重建后，旧 sender 的消息会在 UI
/// 线程按内部 lease 自动丢弃。
pub struct ComponentEventSender<E> {
    dispatcher: ComponentEventDispatcher,
    target: ComponentLeaseToken,
    event_name: &'static str,
    site: ViewSite,
    marker: PhantomData<fn(E)>,
}

impl<E> Clone for ComponentEventSender<E> {
    fn clone(&self) -> Self {
        Self {
            dispatcher: self.dispatcher.clone(),
            target: self.target,
            event_name: self.event_name,
            site: self.site,
            marker: PhantomData,
        }
    }
}

impl<E: Send + 'static> ComponentEventSender<E> {
    /// 把一个自有 Event 交给 UI 线程的下一次候选事务。
    ///
    /// 成功仅表示事件已进入有界邮箱，尚不表示 handler 已执行、更不表示 UI 已呈现。若
    /// 协调器已销毁，或邮箱正处于背压状态，原始 event 会随错误一并返还给调用者。
    pub fn send(&self, event: E) -> Result<(), ComponentEventSendError<E>> {
        self.dispatcher
            .enqueue(self.target, self.event_name, self.site, event)
    }
}

/// 组件事件 sender 未能接纳一个 Event 的原因。
///
/// 错误保留 payload 所有权，调用方可选择记录、合并、重试或在自己的任务取消路径中释放。
pub enum ComponentEventSendError<E> {
    /// sender 对应的 `FrameCoordinator` 已被销毁。
    Closed(E),
    /// UI 尚未消费旧事件，邮箱已达到固定背压容量。
    Full(E),
}

impl<E> ComponentEventSendError<E> {
    /// 返回没有被邮箱接纳的原始 Event。
    pub fn into_event(self) -> E {
        match self {
            Self::Closed(event) | Self::Full(event) => event,
        }
    }

    /// 是否因为协调器已经销毁而失败。
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed(_))
    }

    /// 是否因为邮箱背压而失败。
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full(_))
    }
}

#[derive(Clone, Default)]
pub(crate) struct ComponentEventDispatcher {
    inner: Weak<ComponentEventMailboxInner>,
}

impl ComponentEventDispatcher {
    pub(crate) fn sender<E>(
        &self,
        lease: ComponentLease,
        site: ViewSite,
    ) -> ComponentEventSender<E> {
        ComponentEventSender {
            dispatcher: self.clone(),
            target: lease.event_token(),
            event_name: std::any::type_name::<E>(),
            site,
            marker: PhantomData,
        }
    }

    fn enqueue<E: Send + 'static>(
        &self,
        target: ComponentLeaseToken,
        event_name: &'static str,
        site: ViewSite,
        event: E,
    ) -> Result<(), ComponentEventSendError<E>> {
        let Some(inner) = self.inner.upgrade() else {
            return Err(ComponentEventSendError::Closed(event));
        };
        {
            let mut queue = lock_recover(&inner.queue);
            if queue.len() >= COMPONENT_EVENT_MAILBOX_CAPACITY {
                return Err(ComponentEventSendError::Full(event));
            }
            queue.push_back(InboundComponentEvent {
                target,
                event_name,
                site,
                event: Box::new(event),
            });
        }
        inner.request_frame();
        Ok(())
    }
}

/// 协调器私有的线程安全 ingress 邮箱。
pub(crate) struct ComponentEventMailbox {
    inner: Arc<ComponentEventMailboxInner>,
}

impl Default for ComponentEventMailbox {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentEventMailbox {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(ComponentEventMailboxInner {
                queue: Mutex::new(VecDeque::new()),
                invalidator: Mutex::new(None),
            }),
        }
    }

    pub(crate) fn dispatcher(&self) -> ComponentEventDispatcher {
        ComponentEventDispatcher {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// 取走一个固定 ingress 快照。并发后来入队的事件留给下一次 UI 事务，绝不会插进
    /// 正在处理的这一批。
    pub(crate) fn drain_snapshot(&self) -> VecDeque<InboundComponentEvent> {
        std::mem::take(&mut *lock_recover(&self.inner.queue))
    }

    pub(crate) fn has_pending(&self) -> bool {
        !lock_recover(&self.inner.queue).is_empty()
    }

    pub(crate) fn set_invalidator(&self, invalidator: Arc<dyn ComponentEventInvalidator>) {
        *lock_recover(&self.inner.invalidator) = Some(Arc::downgrade(&invalidator));
        if self.has_pending() {
            invalidator.request_component_event_frame();
        }
    }

    pub(crate) fn clear_invalidator(&self) {
        *lock_recover(&self.inner.invalidator) = None;
    }
}

struct ComponentEventMailboxInner {
    queue: Mutex<VecDeque<InboundComponentEvent>>,
    invalidator: Mutex<Option<Weak<dyn ComponentEventInvalidator>>>,
}

impl ComponentEventMailboxInner {
    fn request_frame(&self) {
        let invalidator = lock_recover(&self.invalidator)
            .as_ref()
            .and_then(Weak::upgrade);
        if let Some(invalidator) = invalidator {
            invalidator.request_component_event_frame();
        }
    }
}

/// 一个已拥有的外部组件 Event。它只在 FrameCoordinator 的 UI 线程中被打开。
pub(crate) struct InboundComponentEvent {
    target: ComponentLeaseToken,
    event_name: &'static str,
    site: ViewSite,
    event: Box<dyn Any + Send>,
}

impl InboundComponentEvent {
    pub(crate) fn target(&self) -> ComponentLeaseToken {
        self.target
    }

    pub(crate) fn event_name(&self) -> &'static str {
        self.event_name
    }

    pub(crate) fn site(&self) -> ViewSite {
        self.site
    }

    pub(crate) fn into_event(self) -> Box<dyn Any> {
        self.event
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
