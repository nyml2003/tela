//! 应用层：帧协调、路由、受控草稿与网络作业队列。
//!
//! 对宿主/guest 只暴露两个纯网络接口：[`App::take_pending_net_jobs`]（本帧要发什么）与
//! [`App::ingest_net_response`]（一个响应回来了）。桥调用本身只发生在产品装配
//! （products/cc-guest），应用层保持可无头测试。

use std::collections::VecDeque;

use tela_cc_protocol::{
    DEFAULT_POLL_INTERVAL_MS, Event, MAX_POLL_INTERVAL_MS, NetHttpMethod, NetHttpRequest,
    NetHttpResponse, PermissionDecision, SYNC_DEFAULT_LIMIT,
};
use tela_contract::{
    FocusAppearance, InputEvent, Insets, KernelInteraction, NodeId, NodeKind, PhysicalKey,
    PointerEvent, ScrollState, SemanticKey, TextInputEvent, TextSelection, UiFrame, UiLayoutError,
    UiNode, UiResources, Viewport,
};
use tela_core::{DefaultApplicationProfile, UiTree, ViewStateStore};
use tela_ui_dsl::{
    AnimationClock, AnimationSchedule, FrameCoordinator, FrameToken, FramedInteraction, Signal,
};

use crate::domain::{self, World};
use crate::presentation::{
    ChatProps, ChatRow, PermissionCardView, SessionsProps, render_chat_dsl, render_sessions_dsl,
};

/// Initial mobile logical size before a target host reports its real content area.
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 412.0,
    height: 917.0,
};

const DRAFT_KEY: &str = "cc.draft";
const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: tela_contract::Color::rgba(0.145, 0.388, 0.922, 1.0),
    width: 2.0,
    inset: 2.0,
};

/// 路由：会话列表或某个会话的聊天屏。
#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Sessions,
    Chat(String),
}

/// 应用级 typed action（由 DSL `ActionTarget` 产生）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CcAction {
    GoBack,
    OpenSession(String),
    NewSession,
    DraftChanged(String),
    ClearDraft,
    /// 提交草稿（携带提交值；应用以受控草稿信号为准）。
    SubmitDraft(String),
    SendDraft,
    ApprovePermission,
    DenyPermission,
}

/// 一条待发出的网络作业。
#[derive(Clone, Debug, PartialEq, Eq)]
enum NetJob {
    CreateSession {
        client_request_id: String,
    },
    SendMessage {
        session_id: String,
        text: String,
        client_msg_id: String,
    },
    PermissionDecision {
        permission_id: String,
        decision: PermissionDecision,
    },
}

/// 本地乐观消息：发出但尚未被 `turn_started` 确认。
#[derive(Clone, Debug, PartialEq, Eq)]
struct OptimisticMessage {
    session_id: String,
    text: String,
}

/// 轮询与退避调度：guest 的逻辑时钟驱动，宿主按 `PollTick` 唤醒。
#[derive(Clone, Debug)]
struct NetScheduler {
    interval_ms: u64,
    next_poll_ms: u64,
    sync_in_flight: bool,
    in_flight_since_ms: u64,
    outbox: VecDeque<NetJob>,
    optimistic: Vec<OptimisticMessage>,
    next_client_id: u64,
}

impl NetScheduler {
    fn new() -> Self {
        Self {
            interval_ms: DEFAULT_POLL_INTERVAL_MS,
            next_poll_ms: 0,
            sync_in_flight: false,
            in_flight_since_ms: 0,
            outbox: VecDeque::new(),
            optimistic: Vec::new(),
            next_client_id: 1,
        }
    }

    fn next_id(&mut self) -> String {
        let id = format!("c{}", self.next_client_id);
        self.next_client_id += 1;
        id
    }

    fn backoff(&mut self) {
        self.interval_ms =
            (self.interval_ms * 2).clamp(DEFAULT_POLL_INTERVAL_MS, MAX_POLL_INTERVAL_MS);
    }

    fn recover(&mut self) {
        self.interval_ms = DEFAULT_POLL_INTERVAL_MS;
    }
}

/// A complete CC Remote application session.
pub struct App {
    resources: &'static dyn UiResources,
    world: World,
    route: Signal<Route>,
    history: Vec<Route>,
    draft: Signal<String>,
    net: NetScheduler,
    viewport: Viewport,
    safe_area: Insets,
    profile: DefaultApplicationProfile,
    view_state: ViewStateStore,
    scroll_key: Option<SemanticKey>,
    frames: FrameCoordinator<CcAction>,
    projection_invalidated: bool,
    animation_clock: AnimationClock,
}

impl App {
    /// Creates the CC Remote session with product-selected visual resources.
    pub fn new(resources: &'static dyn UiResources) -> Self {
        Self {
            resources,
            world: World::default(),
            route: Signal::new(Route::Sessions),
            history: Vec::new(),
            draft: Signal::new(String::new()),
            net: NetScheduler::new(),
            viewport: DEFAULT_VIEWPORT,
            safe_area: Insets::all(0.0),
            profile: DefaultApplicationProfile::new(),
            view_state: ViewStateStore::new(),
            scroll_key: None,
            frames: FrameCoordinator::new(),
            projection_invalidated: true,
            animation_clock: AnimationClock::default(),
        }
    }

    /// 本帧应用要发出的网络作业（轮询 + 出站 POST 队列）。
    ///
    /// 由产品装配在每个 apply 之后调用；`now_ms` 是宿主注入的单调逻辑时钟
    /// （`AppEvent::Wake`/`Tick` 的时间戳）。
    pub fn take_pending_net_jobs(&mut self, now_ms: u64) -> Vec<NetHttpRequest> {
        let mut jobs = Vec::new();
        while let Some(job) = self.net.outbox.pop_front() {
            jobs.push(self.encode_job(job));
        }
        // 卡死的 sync（宿主丢失响应）按 15s 超时回收并退避。
        if self.net.sync_in_flight && now_ms.saturating_sub(self.net.in_flight_since_ms) > 15_000 {
            self.net.sync_in_flight = false;
            self.net.backoff();
        }
        if !self.net.sync_in_flight && now_ms >= self.net.next_poll_ms {
            self.net.sync_in_flight = true;
            self.net.in_flight_since_ms = now_ms;
            jobs.push(NetHttpRequest {
                method: NetHttpMethod::Get,
                path: format!(
                    "/v1/sync?since={}&limit={}",
                    self.world.cursor, SYNC_DEFAULT_LIMIT
                ),
                body: None,
            });
        }
        jobs
    }

    /// 一个网络响应回来了；按形状分类并推进世界状态。返回是否需要新帧。
    pub fn ingest_net_response(&mut self, response: NetHttpResponse, now_ms: u64) -> bool {
        let _ = now_ms;
        if response.status == 0 {
            self.net.sync_in_flight = false;
            self.net.backoff();
            self.net.next_poll_ms = now_ms.saturating_add(self.net.interval_ms);
            self.push_notice(&format!(
                "连接中继失败：{}",
                String::from_utf8_lossy(&response.body)
            ));
            self.invalidate_frame();
            return true;
        }
        // 响应按 JSON 形状自描述（桥只有 request_id 关联，不携带作业类型）。
        let body: Option<serde_json::Value> = serde_json::from_slice(&response.body).ok();
        let Some(body) = body else {
            self.push_notice(&format!("中继响应不是 JSON（HTTP {}）", response.status));
            self.invalidate_frame();
            return true;
        };
        let object = body.as_object();

        if response.status == 409
            && object
                .and_then(|map| map.get("error"))
                .and_then(|value| value.as_str())
                == Some("cursor_reset")
        {
            self.resync_from_scratch(now_ms);
            return true;
        }

        if object.is_some_and(|map| map.contains_key("events")) {
            return self.ingest_sync_response(&body, now_ms);
        } else if object.is_some_and(|map| map.contains_key("resolved_by")) {
            // 权限 POST 的受理；真实结果由 permission_resolved 事件驱动 UI。
            self.net.recover();
            self.net.next_poll_ms = now_ms;
        } else if object.is_some_and(|map| map.contains_key("accepted")) {
            // 会话创建/发消息的受理；尽快轮询拿 turn_started/回执。
            self.net.recover();
            self.net.next_poll_ms = now_ms;
        } else if response.status >= 400 {
            let text = object
                .and_then(|map| map.get("error"))
                .and_then(|value| value.as_str())
                .unwrap_or("未知错误")
                .to_owned();
            self.push_notice(&format!("请求被拒（HTTP {}）：{text}", response.status));
            self.net.backoff();
            self.net.next_poll_ms = now_ms.saturating_add(self.net.interval_ms);
            self.invalidate_frame();
            return true;
        }
        // 2xx 的其它形状（health 等）当前不会主动请求；静默忽略。
        false
    }

    /// Updates the logical mobile content area.
    pub fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        let viewport = Viewport {
            width: width.max(240.0),
            height: height.max(320.0),
        };
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    /// Updates the native system-bar exclusion area expressed in logical pixels.
    #[cfg(any(test, feature = "native-app"))]
    pub fn set_safe_area(&mut self, safe_area: Insets) -> bool {
        let safe_area = Insets {
            top: safe_area.top.max(0.0),
            right: safe_area.right.max(0.0),
            bottom: safe_area.bottom.max(0.0),
            left: safe_area.left.max(0.0),
        };
        if self.safe_area == safe_area {
            return false;
        }
        self.safe_area = safe_area;
        self.invalidate_frame();
        true
    }

    /// Ensures the current projection and frame exist.
    ///
    /// A failed candidate leaves the previously published tree, frame, watch graph, action map,
    /// view state, and frame token untouched (候选帧事务语义与 mobile-demo 一致)。
    pub fn ensure_frame(&mut self) -> bool {
        if self.frames.active().is_some()
            && !self.projection_invalidated
            && !self.frames.runtime().has_dirty()
        {
            return false;
        }

        self.frames.runtime().begin_frame();
        let dirty = self.frames.runtime().take_dirty();
        let mut candidate_state = self.view_state.clone();
        let focused_before = draft_is_focused(&candidate_state);
        let mut prepared = match self.prepare_projection(focused_before) {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!(
                    "tela-cc-remote: retain previous frame after view build failure: {error}"
                );
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };

        self.profile
            .reconcile_tree(prepared.tree(), &mut candidate_state);
        self.profile
            .ensure_modal_focus(prepared.tree(), &mut candidate_state);
        let focused_after = draft_is_focused(&candidate_state);
        if focused_before != focused_after {
            prepared = match self.prepare_projection(focused_after) {
                Ok(prepared) => prepared,
                Err(error) => {
                    eprintln!(
                        "tela-cc-remote: retain previous frame after focused view build failure: {error}"
                    );
                    self.frames.runtime().restore_dirty(dirty);
                    return false;
                }
            };
            self.profile
                .reconcile_tree(prepared.tree(), &mut candidate_state);
            self.profile
                .ensure_modal_focus(prepared.tree(), &mut candidate_state);
        }

        let scroll_key = discover_scroll_key(prepared.tree());
        let mut scroll_inputs = scroll_inputs_for(&candidate_state, scroll_key.as_ref());
        let mut frame = match self.profile.resolve_candidate(
            prepared.tree(),
            self.viewport,
            self.resources.text_measurer(),
            &scroll_inputs,
            &candidate_state,
            Some(FOCUS_APPEARANCE),
        ) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!(
                    "tela-cc-remote: retain previous frame after candidate resolve failure: {error:?}"
                );
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };
        if clamp_scroll_states(&mut candidate_state, &frame) {
            scroll_inputs = scroll_inputs_for(&candidate_state, scroll_key.as_ref());
            frame = match self.profile.resolve_candidate(
                prepared.tree(),
                self.viewport,
                self.resources.text_measurer(),
                &scroll_inputs,
                &candidate_state,
                Some(FOCUS_APPEARANCE),
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    eprintln!(
                        "tela-cc-remote: retain previous frame after clamped candidate resolve failure: {error:?}"
                    );
                    self.frames.runtime().restore_dirty(dirty);
                    return false;
                }
            };
        }

        let resolved = prepared
            .resolve(|_| Ok::<_, UiLayoutError>(frame))
            .expect("an already resolved cc candidate cannot fail a second time");
        let view_state = &mut self.view_state;
        let active_scroll_key = &mut self.scroll_key;
        let projection_invalidated = &mut self.projection_invalidated;
        self.frames.commit_with(resolved, |_| {
            *view_state = candidate_state;
            *active_scroll_key = scroll_key;
            *projection_invalidated = false;
        });
        let _ = self.frames.take_component_lifecycle_events();
        true
    }

    /// Returns the resolved Tela frame for the current screen.
    pub fn frame(&self) -> &UiFrame {
        self.frames
            .active()
            .expect("cc frame must be ensured")
            .frame()
    }

    /// Returns the currently published frame token, or `0` before the first successful frame.
    pub fn active_frame_token(&self) -> u64 {
        self.frames.active().map_or(0, |frame| frame.token().get())
    }

    /// Delivers a normalized pointer event for the currently active frame (同步测试便捷入口).
    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.handle_pointer_for_frame(token.get(), event)
    }

    /// Delivers a normalized pointer event with the token of the source frame.
    pub fn handle_pointer_for_frame(&mut self, frame_token: u64, event: PointerEvent) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Pointer(event));
        let changed = self.handle_framed_interactions(token, &actions);
        if changed {
            self.invalidate_frame();
        }
        actions.len() as u32
    }

    /// Handles the small platform key vocabulary for the currently active frame.
    pub fn handle_key(&mut self, physical_key: u16) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.handle_key_for_frame(token.get(), physical_key)
    }

    /// Handles a platform key with the token of the frame that owned input focus.
    pub fn handle_key_for_frame(&mut self, frame_token: u64, physical_key: u16) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        match PhysicalKey::from_code(physical_key) {
            Some(PhysicalKey::Escape) if self.input_focused() => {
                self.input_cancel_for_frame(token.get())
            }
            Some(PhysicalKey::Escape) => u32::from(self.go_back()),
            Some(PhysicalKey::Enter) => self.input_enter_for_frame(token.get()),
            _ => 0,
        }
    }

    /// Replaces the controlled draft value for the currently active frame.
    pub fn set_input_value(&mut self, value: String) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.set_input_value_for_frame(token.get(), value)
    }

    /// Replaces the controlled draft value with the token of the focused native text channel.
    pub fn set_input_value_for_frame(&mut self, frame_token: u64, value: String) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        if !self.input_focused() {
            return 0;
        }
        u32::from(self.dispatch_text_input(
            token,
            TextInputEvent::Edit {
                selection: TextSelection::collapsed(value.len() as u32),
                value,
                composing: false,
            },
        ))
    }

    /// The platform text channel became focused for the currently active frame.
    pub fn input_focus(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_focus_for_frame(token.get())
    }

    /// The platform text channel became focused for a particular rendered frame.
    pub fn input_focus_for_frame(&mut self, frame_token: u64) -> u32 {
        u32::from(self.accept_frame_token(frame_token).is_some() && self.input_focused())
    }

    /// The platform text channel lost focus for the currently active frame.
    pub fn input_blur(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_blur_for_frame(token.get())
    }

    /// The platform text channel lost focus for a particular rendered frame.
    pub fn input_blur_for_frame(&mut self, frame_token: u64) -> u32 {
        u32::from(self.accept_frame_token(frame_token).is_some() && self.blur_input())
    }

    /// Commits the current draft interaction for the currently active frame.
    pub fn input_enter(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_enter_for_frame(token.get())
    }

    /// Commits the current draft interaction with its source frame token.
    ///
    /// 提交语义即发送：focused 草稿的 Commit 走 DSL `on_submit` 产生 `SendDraft`。
    pub fn input_enter_for_frame(&mut self, frame_token: u64) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        let committed = if self.input_focused() {
            let value = self.draft.get();
            self.dispatch_text_input(
                token,
                TextInputEvent::Commit {
                    selection: TextSelection::collapsed(value.len() as u32),
                    value,
                },
            )
        } else {
            false
        };
        u32::from(committed || self.blur_input())
    }

    /// Cancels the current draft interaction for the currently active frame.
    pub fn input_cancel(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_cancel_for_frame(token.get())
    }

    /// Cancels the current draft interaction with its source frame token.
    pub fn input_cancel_for_frame(&mut self, frame_token: u64) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        if !self.input_focused() {
            return 0;
        }
        let canceled = self.dispatch_text_input(
            token,
            TextInputEvent::Cancel {
                selection: TextSelection::collapsed(self.draft.get().len() as u32),
            },
        );
        let blurred = self.blur_input();
        u32::from(canceled || blurred)
    }

    /// Composition markers are accepted only for the frame that owns the active native editor.
    pub fn composition_changed(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.composition_changed_for_frame(token.get())
    }

    /// Records a composition transition with the source frame token.
    pub fn composition_changed_for_frame(&mut self, frame_token: u64) -> u32 {
        u32::from(self.accept_frame_token(frame_token).is_some() && self.input_focused())
    }

    /// Whether the native text channel should be attached.
    pub fn input_focused(&self) -> bool {
        draft_is_focused(&self.view_state)
    }

    /// Current controlled draft value.
    pub fn input_value(&self) -> String {
        self.draft.get()
    }

    /// 同步宿主注入的单调时钟；没有活跃动画时只更新时间基，不产生新帧。
    pub fn animation_tick(&mut self, timestamp_ms: u64) -> bool {
        if timestamp_ms < self.animation_clock.timestamp_ms {
            return false;
        }
        self.animation_clock = AnimationClock { timestamp_ms };
        if !self.animation_schedule().active {
            return false;
        }
        self.invalidate_frame();
        true
    }

    /// 当前成功帧请求的动画调度。
    pub fn animation_schedule(&self) -> AnimationSchedule {
        self.frames
            .active()
            .map(|frame| frame.animation_schedule())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // 内部：投影、路由、网络。
    // -----------------------------------------------------------------------

    fn prepare_projection(
        &mut self,
        draft_focused: bool,
    ) -> Result<tela_ui_dsl::PreparedFrame<CcAction>, String> {
        let draft = self.draft.get();
        let root = match self.route.get() {
            Route::Sessions => {
                let mut sessions: Vec<&domain::Session> = self.world.sessions.iter().collect();
                sessions.sort_by_key(|session| std::cmp::Reverse(session.last_seq));
                let props = SessionsProps {
                    viewport: self.viewport,
                    safe_area: self.safe_area,
                    agent_online: self.world.agent_online,
                    sessions,
                    notices: &self.world.notices,
                    icons: self.resources.icon_provider(),
                };
                render_sessions_dsl(&mut self.frames.begin_build(), props)
                    .map_err(|error| error.to_string())?
            }
            Route::Chat(ref session_id) => {
                let title = self
                    .world
                    .sessions
                    .iter()
                    .find(|session| session.id == *session_id)
                    .map(|session| session.title.clone())
                    .unwrap_or_else(|| "会话".to_owned());
                let rows = self.chat_rows(session_id);
                let permission = self.permission_view(session_id);
                let props = ChatProps {
                    viewport: self.viewport,
                    safe_area: self.safe_area,
                    title: &title,
                    can_go_back: true,
                    draft: &draft,
                    draft_signal: self.draft.clone(),
                    draft_focused,
                    rows,
                    permission,
                    icons: self.resources.icon_provider(),
                };
                render_chat_dsl(&mut self.frames.begin_build(), props)
                    .map_err(|error| error.to_string())?
            }
        };
        self.frames.prepare(root).map_err(|error| error.to_string())
    }

    /// 组装聊天行视图模型：世界事件项 + 乐观消息，工具结果并卡。
    fn chat_rows(&self, session_id: &str) -> Vec<ChatRow> {
        use crate::domain::ChatItem;
        let mut rows = Vec::new();
        let chat = self.world.chats.get(session_id);
        if let Some(chat) = chat {
            for item in &chat.items {
                match item {
                    ChatItem::UserText { text, .. } => rows.push(ChatRow::User {
                        text: text.clone(),
                        pending: false,
                    }),
                    ChatItem::AssistantText { text, .. } => {
                        rows.push(ChatRow::Assistant { text: text.clone() })
                    }
                    ChatItem::ToolUse {
                        tool_use_id,
                        tool_name,
                        input_json,
                        ..
                    } => rows.push(ChatRow::Tool {
                        tool_name: tool_name.clone(),
                        input_json: input_json.clone(),
                        result: None,
                        tool_use_id: tool_use_id.clone(),
                    }),
                    ChatItem::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    } => {
                        let attached = rows.iter_mut().rev().find(|row| {
                            matches!(row, ChatRow::Tool { tool_use_id: existing, .. } if existing == tool_use_id)
                        });
                        if let Some(ChatRow::Tool { result, .. }) = attached {
                            *result = Some((content.clone(), *is_error));
                        } else {
                            rows.push(ChatRow::Tool {
                                tool_name: "tool".to_owned(),
                                input_json: String::new(),
                                result: Some((content.clone(), *is_error)),
                                tool_use_id: tool_use_id.clone(),
                            });
                        }
                    }
                    ChatItem::TurnResult {
                        subtype,
                        cost_usd,
                        duration_ms,
                        ..
                    } => rows.push(ChatRow::TurnEnd {
                        subtype: subtype.clone(),
                        cost_usd: *cost_usd,
                        duration_ms: *duration_ms,
                    }),
                    ChatItem::Notice { text } => rows.push(ChatRow::Notice { text: text.clone() }),
                }
            }
        }
        for optimistic in &self.net.optimistic {
            if optimistic.session_id == session_id {
                rows.push(ChatRow::User {
                    text: optimistic.text.clone(),
                    pending: true,
                });
            }
        }
        rows
    }

    fn permission_view(&self, session_id: &str) -> Option<PermissionCardView> {
        let card = self
            .world
            .chats
            .get(session_id)
            .and_then(|chat| chat.permission.as_ref())?;
        Some(PermissionCardView {
            permission_id: card.permission_id.clone(),
            tool_name: card.tool_name.clone(),
            input_summary: card.input_summary.clone(),
            resolution: card.resolution,
        })
    }

    fn encode_job(&mut self, job: NetJob) -> NetHttpRequest {
        match job {
            NetJob::CreateSession { client_request_id } => NetHttpRequest {
                method: NetHttpMethod::Post,
                path: "/v1/sessions".to_owned(),
                body: Some(
                    serde_json::json!({ "client_request_id": client_request_id })
                        .to_string()
                        .into_bytes(),
                ),
            },
            NetJob::SendMessage {
                session_id,
                text,
                client_msg_id,
            } => NetHttpRequest {
                method: NetHttpMethod::Post,
                path: format!("/v1/sessions/{session_id}/messages"),
                body: Some(
                    serde_json::json!({ "text": text, "client_msg_id": client_msg_id })
                        .to_string()
                        .into_bytes(),
                ),
            },
            NetJob::PermissionDecision {
                permission_id,
                decision,
            } => NetHttpRequest {
                method: NetHttpMethod::Post,
                path: format!("/v1/permissions/{permission_id}"),
                body: Some(
                    serde_json::json!({ "decision": decision })
                        .to_string()
                        .into_bytes(),
                ),
            },
        }
    }

    fn ingest_sync_response(&mut self, body: &serde_json::Value, now_ms: u64) -> bool {
        let parsed: Result<tela_cc_protocol::SyncResponse, _> =
            serde_json::from_value(body.clone());
        let Ok(response) = parsed else {
            self.push_notice("sync 响应解析失败");
            self.net.sync_in_flight = false;
            self.net.backoff();
            self.net.next_poll_ms = now_ms.saturating_add(self.net.interval_ms);
            self.invalidate_frame();
            return true;
        };
        self.net.sync_in_flight = false;
        let mut changed = false;
        for event in &response.events {
            changed |= domain::apply_event(&mut self.world, event);
        }
        if response.truncated {
            self.net.next_poll_ms = now_ms;
        } else {
            self.net.recover();
            self.net.next_poll_ms = now_ms.saturating_add(self.net.interval_ms);
        }
        let online_changed = self.world.agent_online != response.agent_online;
        if online_changed {
            self.world.agent_online = response.agent_online;
            changed = true;
        }
        self.reconcile_optimistic();
        let route_lost =
            matches!(self.route.get(), Route::Chat(id) if !self.world.chats.contains_key(&id));
        if route_lost {
            self.route.set(Route::Sessions);
            self.history.clear();
            changed = true;
        }
        if changed {
            self.invalidate_frame();
        }
        changed
    }

    fn resync_from_scratch(&mut self, now_ms: u64) {
        // 中继日志重启：本地投影作废，从 0 全量重拉（v1 取舍，见 docs/038）。
        self.world = World::default();
        if matches!(self.route.get(), Route::Chat(_)) {
            self.route.set(Route::Sessions);
            self.history.clear();
        }
        self.net.sync_in_flight = false;
        self.net.recover();
        self.net.next_poll_ms = now_ms;
        self.invalidate_frame();
    }

    fn reconcile_optimistic(&mut self) {
        // turn_started 已经落成事件侧 UserText；同文本的乐观消息即可销账。
        self.net.optimistic.retain(|message| {
            !self
                .world
                .chats
                .get(&message.session_id)
                .is_some_and(|chat| {
                    chat.items.iter().any(|item| {
                        matches!(item, domain::ChatItem::UserText { text, .. } if text == &message.text)
                    })
                })
        });
    }

    fn push_notice(&mut self, text: &str) {
        let event = Event {
            seq: self.world.cursor + 1,
            ts_ms: self.animation_clock.timestamp_ms,
            kind: tela_cc_protocol::EventKind::Notice {
                level: tela_cc_protocol::NoticeLevel::Error,
                text: text.to_owned(),
            },
        };
        // 本地通知不入真实游标：直接借用 reducer 的去重与容量逻辑。
        let _ = domain::apply_event(&mut self.world, &event);
        self.world.cursor -= 1;
    }

    fn current_frame_token(&mut self) -> Option<FrameToken> {
        self.ensure_frame();
        self.frames.active().map(|frame| frame.token())
    }

    fn accept_frame_token(&mut self, raw: u64) -> Option<FrameToken> {
        self.ensure_frame();
        let token = FrameToken::from_raw(raw)?;
        self.frames
            .active()
            .is_some_and(|active| active.token() == token)
            .then_some(token)
    }

    fn handle_framed_interactions(
        &mut self,
        token: FrameToken,
        actions: &[KernelInteraction],
    ) -> bool {
        let mut changed = false;
        for action in actions.iter().cloned() {
            let framed = FramedInteraction::new(token, action);
            if !self.frames.accepts_interaction(&framed) {
                continue;
            }
            if let Some(action) = self.frames.dispatch_interaction(&framed) {
                changed |= self.handle_application_action(action);
                continue;
            }
            if self
                .frames
                .dispatch_component_interaction(&framed)
                .is_some()
            {
                changed = true;
                continue;
            }
            match framed.into_parts().1 {
                KernelInteraction::Scroll { node_id, delta } => {
                    changed |= self.handle_scroll(node_id, delta.y)
                }
                KernelInteraction::RequestFocus { .. } | KernelInteraction::FocusChanged { .. } => {
                    changed = true
                }
                _ => {}
            }
        }
        changed
    }

    fn handle_application_action(&mut self, action: CcAction) -> bool {
        match action {
            CcAction::GoBack => self.go_back(),
            CcAction::OpenSession(session_id) => self.open_session(&session_id),
            CcAction::NewSession => self.new_session(),
            CcAction::DraftChanged(value) => self.set_draft(value),
            CcAction::ClearDraft => self.set_draft(String::new()),
            CcAction::SubmitDraft(_) => self.send_draft(),
            CcAction::SendDraft => self.send_draft(),
            CcAction::ApprovePermission | CcAction::DenyPermission => {
                self.decide_permission(matches!(action, CcAction::ApprovePermission))
            }
        }
    }

    fn dispatch_text_input(&mut self, token: FrameToken, event: TextInputEvent) -> bool {
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Text(event));
        let changed = self.handle_framed_interactions(token, &actions);
        if changed {
            self.invalidate_frame_unless_dirty();
        }
        changed
    }

    fn set_draft(&mut self, value: String) -> bool {
        if self.draft.get() == value {
            return false;
        }
        self.draft.set(value);
        self.invalidate_frame_unless_dirty();
        true
    }

    fn open_session(&mut self, session_id: &str) -> bool {
        if !self.world.chats.contains_key(session_id)
            && !self.world.sessions.iter().any(|s| s.id == session_id)
        {
            return false;
        }
        self.history.push(self.route.get());
        self.route.set(Route::Chat(session_id.to_owned()));
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn new_session(&mut self) -> bool {
        let client_request_id = self.net.next_id();
        self.net
            .outbox
            .push_back(NetJob::CreateSession { client_request_id });
        self.net.next_poll_ms = self.animation_clock.timestamp_ms;
        self.invalidate_frame();
        true
    }

    fn send_draft(&mut self) -> bool {
        let Route::Chat(session_id) = self.route.get() else {
            return false;
        };
        let text = self.draft.get().trim().to_owned();
        if text.is_empty() {
            return false;
        }
        let client_msg_id = self.net.next_id();
        self.net.optimistic.push(OptimisticMessage {
            session_id: session_id.clone(),
            text: text.clone(),
        });
        self.net.outbox.push_back(NetJob::SendMessage {
            session_id,
            text,
            client_msg_id,
        });
        self.net.next_poll_ms = self.animation_clock.timestamp_ms;
        self.draft.set(String::new());
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn decide_permission(&mut self, allow: bool) -> bool {
        let Route::Chat(session_id) = self.route.get() else {
            return false;
        };
        let Some(card) = self
            .world
            .chats
            .get(&session_id)
            .and_then(|chat| chat.permission.as_ref())
        else {
            return false;
        };
        if card.resolution.is_some() {
            return false;
        }
        // 决策在入队时即定；encode_job 不再读当前世界。
        let permission_id = card.permission_id.clone();
        let decision = if allow {
            PermissionDecision::Allow
        } else {
            PermissionDecision::Deny
        };
        self.net.outbox.push_back(NetJob::PermissionDecision {
            permission_id,
            decision,
        });
        self.net.next_poll_ms = self.animation_clock.timestamp_ms;
        self.invalidate_frame();
        true
    }

    fn go_back(&mut self) -> bool {
        let Some(previous) = self.history.pop() else {
            if matches!(self.route.get(), Route::Sessions) {
                return false;
            }
            self.route.set(Route::Sessions);
            self.invalidate_frame();
            return true;
        };
        self.route.set(previous);
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn blur_input(&mut self) -> bool {
        if !self.input_focused() {
            return false;
        }
        self.view_state.clear_current_focus();
        self.invalidate_frame();
        true
    }

    fn handle_scroll(&mut self, node_id: NodeId, delta_y: f32) -> bool {
        let Some(bounds) = self.frames.active().and_then(|active| {
            let frame = active.frame();
            frame
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.node_id == node_id)
        }) else {
            return false;
        };
        let mut state = self.view_state.scroll(&bounds.key);
        let next = (state.offset_y + delta_y).clamp(0.0, bounds.max_offset_y);
        if (next - state.offset_y).abs() < f32::EPSILON {
            return false;
        }
        state.offset_y = next;
        self.view_state.set_scroll(bounds.key.clone(), state);
        true
    }

    fn reset_scroll(&mut self) {
        if let Some(key) = self.scroll_key.clone() {
            self.view_state.set_scroll(key, ScrollState::default());
        }
    }

    fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
    }

    /// 控制器状态变化后的失效入口：Signal 订阅已标脏时不再全局失效，
    /// 让 `ensure_frame` 走 dirty 驱动的重建路径（见其入口短路条件）。
    fn invalidate_frame_unless_dirty(&mut self) {
        if !self.frames.runtime().has_dirty() {
            self.invalidate_frame();
        }
    }
}

fn discover_scroll_key(tree: &UiTree) -> Option<SemanticKey> {
    fn visit(node: &UiNode, keys: &[SemanticKey], index: &mut usize) -> Option<SemanticKey> {
        let key = keys.get(*index).cloned();
        *index += 1;
        if matches!(
            node.kind,
            NodeKind::ScrollView | NodeKind::VirtualListView(_)
        ) {
            return key;
        }
        for child in &node.children {
            if let Some(key) = visit(child, keys, index) {
                return Some(key);
            }
        }
        None
    }
    visit(tree.root(), tree.keys(), &mut 0)
}

fn draft_is_focused(view_state: &ViewStateStore) -> bool {
    view_state
        .current_focus_key()
        .is_some_and(|key| key.0 == DRAFT_KEY)
}

fn scroll_inputs_for(
    view_state: &ViewStateStore,
    scroll_key: Option<&SemanticKey>,
) -> std::collections::HashMap<SemanticKey, ScrollState> {
    scroll_key
        .map(|key| (key.clone(), view_state.scroll(key)))
        .into_iter()
        .collect()
}

fn clamp_scroll_states(view_state: &mut ViewStateStore, frame: &UiFrame) -> bool {
    let mut changed = false;
    for bounds in &frame.scroll_bounds {
        let state = view_state.scroll(&bounds.key);
        let next = ScrollState {
            offset_x: state.offset_x.clamp(0.0, bounds.max_offset_x),
            offset_y: state.offset_y.clamp(0.0, bounds.max_offset_y),
        };
        if next != state {
            view_state.set_scroll(bounds.key.clone(), next);
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use tela_cc_protocol::{
        Event, EventKind, MAX_POLL_INTERVAL_MS, NetHttpMethod, NetHttpResponse, NoticeLevel,
        PermissionDecision, PermissionResolver,
    };
    use tela_contract::{IconProvider, Insets, UiResources};

    use super::{App, Route};

    static TEST_TEXT_MEASURER: tela_text_resources::ControlledTextMeasurer =
        tela_text_resources::ControlledTextMeasurer;
    static TEST_ICON_PROVIDER: tela_icon_resources::MaterialIconFontProvider =
        tela_icon_resources::MaterialIconFontProvider;
    static TEST_RESOURCES: TestResources = TestResources;

    struct TestResources;

    impl UiResources for TestResources {
        fn text_measurer(&self) -> &dyn tela_contract::TextMeasurer {
            &TEST_TEXT_MEASURER
        }

        fn icon_provider(&self) -> &dyn IconProvider {
            &TEST_ICON_PROVIDER
        }
    }

    fn app() -> App {
        App::new(&TEST_RESOURCES)
    }

    fn sync_response(cursor: u64, kinds: Vec<EventKind>) -> NetHttpResponse {
        let events: Vec<Event> = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| Event {
                seq: cursor + index as u64 + 1,
                ts_ms: 0,
                kind,
            })
            .collect();
        let body = serde_json::json!({
            "events": events,
            "cursor": cursor + events.len() as u64,
            "truncated": false,
            "agent_online": true,
            "server_time_ms": 0,
        });
        NetHttpResponse {
            status: 200,
            body: body.to_string().into_bytes(),
            truncated: false,
        }
    }

    fn seeded_world_events() -> Vec<EventKind> {
        vec![
            EventKind::AgentStatus {
                online: true,
                agent_id: "desktop-wsl".to_owned(),
            },
            EventKind::SessionCreated {
                session_id: "s1".to_owned(),
                title: Some("tela".to_owned()),
            },
        ]
    }

    #[test]
    fn first_wake_emits_the_initial_sync_request() {
        let mut app = app();
        let jobs = app.take_pending_net_jobs(0);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].method, NetHttpMethod::Get);
        assert_eq!(jobs[0].path, "/v1/sync?since=0&limit=200");
        // sync 在途：同帧不再发第二次。
        assert!(app.take_pending_net_jobs(100).is_empty());
    }

    #[test]
    fn sync_response_advances_cursor_and_schedules_next_poll() {
        let mut app = app();
        let _ = app.take_pending_net_jobs(0);
        app.ingest_net_response(sync_response(0, seeded_world_events()), 0);
        assert_eq!(app.world_cursor(), 2);
        assert!(app.agent_online());
        // 退避恢复后的下一次轮询在 interval 之后。
        assert!(app.take_pending_net_jobs(1_000).is_empty());
        let jobs = app.take_pending_net_jobs(1_500);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].path, "/v1/sync?since=2&limit=200");
    }

    #[test]
    fn transport_failure_backs_off_and_caps() {
        let mut app = app();
        let _ = app.take_pending_net_jobs(0);
        app.ingest_net_response(NetHttpResponse::transport_error("dial refused"), 0);
        // 退避翻倍：失败后下一次轮询间隔 3000 → 6000 → 12000 → 15000 → 15000（封顶）。
        let mut now = 0;
        for step in 1..=5 {
            let interval = (1_500 * (1_u64 << step)).min(MAX_POLL_INTERVAL_MS);
            assert!(
                app.take_pending_net_jobs(now + interval - 1).is_empty(),
                "step {step}: 退避期内不应轮询"
            );
            let jobs = app.take_pending_net_jobs(now + interval);
            assert_eq!(jobs.len(), 1, "step {step}: 退避到期应轮询");
            app.ingest_net_response(NetHttpResponse::transport_error("down"), now + interval);
            now += interval;
        }
    }

    #[test]
    fn cursor_reset_rebuilds_the_world_and_repolls_immediately() {
        let mut app = app();
        let _ = app.take_pending_net_jobs(0);
        app.ingest_net_response(sync_response(0, seeded_world_events()), 0);
        assert_eq!(app.world_cursor(), 2);
        app.ingest_net_response(
            NetHttpResponse {
                status: 409,
                body: br#"{"error":"cursor_reset"}"#.to_vec(),
                truncated: false,
            },
            2_000,
        );
        assert_eq!(app.world_cursor(), 0);
        assert!(matches!(app.route_snapshot(), Route::Sessions));
        let jobs = app.take_pending_net_jobs(2_000);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].path, "/v1/sync?since=0&limit=200");
    }

    #[test]
    fn send_draft_queues_post_with_optimistic_row() {
        let mut app = app();
        let _ = app.take_pending_net_jobs(0);
        app.ingest_net_response(sync_response(0, seeded_world_events()), 0);
        assert!(app.open_session_for_test("s1"));
        assert!(app.set_draft_for_test("看一下当前项目"));
        assert!(app.send_draft_for_test());

        let jobs = app.take_pending_net_jobs(0);
        assert_eq!(jobs.len(), 2, "POST 消息 + 立即 sync");
        assert!(
            jobs.iter()
                .any(|job| job.path == "/v1/sessions/s1/messages"
                    && job.method == NetHttpMethod::Post)
        );
        assert!(app.chat_rows_for_test("s1")
            .iter()
            .any(|row| matches!(row, crate::presentation::ChatRow::User { text, pending: true } if text == "看一下当前项目")));

        // turn_started 到达后乐观消息销账。
        app.ingest_net_response(
            sync_response(
                2,
                vec![EventKind::TurnStarted {
                    session_id: "s1".to_owned(),
                    turn_id: "turn-3".to_owned(),
                    user_text: "看一下当前项目".to_owned(),
                }],
            ),
            10,
        );
        let rows = app.chat_rows_for_test("s1");
        assert!(rows.iter().any(|row| matches!(row, crate::presentation::ChatRow::User { text, pending: false } if text == "看一下当前项目")));
        assert!(!rows.iter().any(|row| matches!(
            row,
            crate::presentation::ChatRow::User { pending: true, .. }
        )));
    }

    #[test]
    fn permission_decision_queues_allow_post_from_the_card() {
        let mut app = app();
        let _ = app.take_pending_net_jobs(0);
        app.ingest_net_response(sync_response(0, seeded_world_events()), 0);
        assert!(app.open_session_for_test("s1"));
        app.ingest_net_response(
            sync_response(
                2,
                vec![EventKind::PermissionRequested {
                    permission_id: "p1".to_owned(),
                    session_id: "s1".to_owned(),
                    turn_id: "turn-3".to_owned(),
                    tool_name: "Bash".to_owned(),
                    input_summary: "echo hi".to_owned(),
                    expires_at_ms: 9_999,
                }],
            ),
            10,
        );
        assert!(app.decide_permission_for_test(true));
        let jobs = app.take_pending_net_jobs(10);
        let post = jobs
            .iter()
            .find(|job| job.path == "/v1/permissions/p1")
            .expect("permission POST queued");
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap_or(b"")).expect("body json");
        assert_eq!(body["decision"], "allow");

        // 已决的卡不再受理第二次决策。
        app.ingest_net_response(
            sync_response(
                3,
                vec![EventKind::PermissionResolved {
                    permission_id: "p1".to_owned(),
                    decision: PermissionDecision::Allow,
                    resolved_by: PermissionResolver::Phone,
                }],
            ),
            20,
        );
        assert!(!app.decide_permission_for_test(false));
    }

    #[test]
    fn frames_render_both_routes_with_commands() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(!app.frame().commands.is_empty());
        let _ = app.take_pending_net_jobs(0);
        app.ingest_net_response(sync_response(0, seeded_world_events()), 0);
        assert!(app.ensure_frame());
        assert!(app.open_session_for_test("s1"));
        assert!(app.ensure_frame());
        assert_eq!(app.route_snapshot(), Route::Chat("s1".to_owned()));
        // 系统返回键从聊天屏回到列表。
        assert_eq!(app.handle_key(tela_contract::PhysicalKey::Escape as u16), 1);
        assert_eq!(app.route_snapshot(), Route::Sessions);
    }

    #[test]
    fn safe_area_normalizes_and_invalidates() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(app.set_safe_area(Insets {
            top: 47.0,
            right: -2.0,
            bottom: 34.0,
            left: 0.0,
        }));
        assert_eq!(app.safe_area_for_test().right, 0.0);
        assert!(app.ensure_frame());
    }

    // 测试辅助：私有状态访问。
    impl App {
        fn world_cursor(&self) -> u64 {
            self.world.cursor
        }
        fn agent_online(&self) -> bool {
            self.world.agent_online
        }
        fn route_snapshot(&self) -> Route {
            self.route.get()
        }
        fn set_draft_for_test(&mut self, value: &str) -> bool {
            self.set_draft(value.to_owned())
        }
        fn open_session_for_test(&mut self, id: &str) -> bool {
            self.open_session(id)
        }
        fn send_draft_for_test(&mut self) -> bool {
            self.send_draft()
        }
        fn decide_permission_for_test(&mut self, allow: bool) -> bool {
            self.decide_permission(allow)
        }
        fn chat_rows_for_test(&self, id: &str) -> Vec<crate::presentation::ChatRow> {
            self.chat_rows(id)
        }
        fn safe_area_for_test(&self) -> Insets {
            self.safe_area
        }
        fn world_notices(&self) -> usize {
            self.world.notices.len()
        }
    }

    #[test]
    fn notice_events_dedupe_on_repeats() {
        let mut app = app();
        let _ = app.take_pending_net_jobs(0);
        let notice = EventKind::Notice {
            level: NoticeLevel::Error,
            text: "agent offline".to_owned(),
        };
        app.ingest_net_response(sync_response(0, vec![notice.clone(), notice]), 0);
        assert_eq!(app.world_notices(), 1);
    }
}
