//! 会话驱动器：`Box<dyn ApplicationSession>` 之上的发布/呈现/令牌/效应簿记。
//!
//! 静态壳（进程内应用）与 bundle 壳（经 `tela_guest_runtime::GuestSession` 适配的 WASM
//! guest）共用这一层：派发后急切发布候选帧，呈现回执在绘制成功后到达，事务性窗口
//! 命令随呈现排空。本模块不含平台类型，可在任意宿主上用 mock 会话单测。

use std::collections::VecDeque;

use tela_app_session::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameInput, AppPublication, AppStatus,
    ApplicationSession, CursorKind,
};
use tela_contract::{HitRole, Point, UiFrame, WindowCommand};

/// 一个会话的壳侧驱动状态：候选帧 → 呈现帧的握手与窗口命令队列。
pub(crate) struct SessionDriver {
    app: Box<dyn ApplicationSession>,
    presented: Option<AppPublication>,
    candidate: Option<AppPublication>,
    /// 上一次发布失败或派发请求了新帧但尚未发布时保持为真，下一次时机重试。
    publish_pending: bool,
    ready_window_commands: VecDeque<WindowCommand>,
}

impl SessionDriver {
    /// 初始化会话并急切发布首帧；失败时保留错误（壳据此显示 Failed 诊断页）。
    pub(crate) fn new(app: Box<dyn ApplicationSession>) -> Result<Self, String> {
        let mut driver = Self {
            app,
            presented: None,
            candidate: None,
            publish_pending: false,
            ready_window_commands: VecDeque::new(),
        };
        let outcome = driver.app.initialize().map_err(|error| error.to_string())?;
        driver.publish_pending = outcome.publish_requested;
        driver.flush();
        Ok(driver)
    }

    /// 派发一个事件；应用请求发布时立即构建候选帧（急切发布）。
    pub(crate) fn dispatch(&mut self, event: AppEvent) -> AppDispatchOutcome {
        let outcome = match self.app.dispatch(event) {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("tela win32: application dispatch failed: {error}");
                return AppDispatchOutcome::IDLE;
            }
        };
        if outcome.publish_requested {
            self.publish_pending = true;
        }
        self.flush();
        outcome
    }

    /// 派发一帧输入；仅当来源令牌等于已呈现令牌时才进入应用（陈旧帧输入丢弃）。
    pub(crate) fn dispatch_frame_input(&mut self, input: AppFrameInput) -> AppDispatchOutcome {
        let Some(publication) = &self.presented else {
            return AppDispatchOutcome::IDLE;
        };
        let source_frame_token = publication.token;
        self.dispatch(AppEvent::FrameInput {
            source_frame_token,
            input,
        })
    }

    /// 绘制前的兜底发布：发布失败后在下一次绘制重试。
    pub(crate) fn flush(&mut self) {
        if !self.publish_pending {
            return;
        }
        if let Some(candidate) = self.candidate.take() {
            self.app.rejected(candidate.token);
        }
        match self.app.publish() {
            Ok(publication) => {
                self.candidate = Some(publication);
                self.publish_pending = false;
            }
            Err(error) => {
                // 保留 pending 标志：下一次派发或绘制时机重试，旧候选已被拒绝。
                eprintln!("tela win32: application publish failed: {error}");
            }
        }
    }

    /// 通知会话候选帧已成功呈现；排空事务性窗口命令。返回是否需要再发布。
    pub(crate) fn frame_presented(&mut self) -> bool {
        let Some(publication) = self.candidate.take() else {
            return false;
        };
        let token = publication.token;
        let effects = publication.effects.clone();
        match self.app.presented(token) {
            Ok(outcome) => {
                self.presented = Some(publication);
                self.publish_pending |= outcome.publish_requested;
                for effect in effects {
                    let AppEffect::Window(command) = effect;
                    self.ready_window_commands.push_back(command);
                }
                outcome.publish_requested
            }
            Err(error) => {
                eprintln!("tela win32: application presentation acknowledgement failed: {error}");
                self.app.rejected(token);
                self.publish_pending = true;
                false
            }
        }
    }

    /// 丢弃未呈现的候选帧（surface 丢失等场景）；下一次时机重新发布。
    pub(crate) fn frame_rejected(&mut self) {
        if let Some(candidate) = self.candidate.take() {
            self.app.rejected(candidate.token);
            self.publish_pending = true;
        }
    }

    /// 使已呈现帧失效：几何变化后，旧帧不得再路由输入。
    pub(crate) fn invalidate_presented(&mut self) {
        self.presented = None;
    }

    /// 当前应渲染的帧：候选（优先）或已呈现帧。
    pub(crate) fn frame(&self) -> &UiFrame {
        &self
            .candidate
            .as_ref()
            .or(self.presented.as_ref())
            .expect("session frame must be published")
            .frame
    }

    /// 当前候选（优先）或已呈现帧的非绘制状态。
    pub(crate) fn status(&self) -> Option<&AppStatus> {
        self.candidate
            .as_ref()
            .or(self.presented.as_ref())
            .map(|publication| &publication.status)
    }

    /// 受控文本输入是否聚焦（文本通道 reconcile 输入）。
    pub(crate) fn input_focused(&self) -> bool {
        self.status().is_some_and(|status| status.input_focused)
    }

    /// 受控文本输入的当前值（WM_CHAR 增量编辑基线）。
    pub(crate) fn input_value(&self) -> String {
        self.status()
            .map(|status| status.input_value.clone())
            .unwrap_or_default()
    }

    /// 客户区光标形状。
    pub(crate) fn cursor(&self) -> CursorKind {
        self.status()
            .map_or(CursorKind::Default, |status| status.cursor)
    }

    /// 命中已呈现帧的非客户角色（自绘标题栏拖拽判定）。
    pub(crate) fn hit_role_at(&self, point: Point) -> HitRole {
        let Some(publication) = &self.presented else {
            return HitRole::Client;
        };
        publication
            .frame
            .hit_regions
            .iter()
            .rev()
            .find(|region| {
                point.x >= region.rect.x
                    && point.y >= region.rect.y
                    && point.x < region.rect.x + region.rect.w
                    && point.y < region.rect.y + region.rect.h
                    && region.clip.is_none_or(|clip| {
                        point.x >= clip.rect.x
                            && point.y >= clip.rect.y
                            && point.x < clip.rect.x + clip.rect.w
                            && point.y < clip.rect.y + clip.rect.h
                    })
            })
            .map_or(HitRole::Client, |region| region.role)
    }

    /// 取出一条随呈现排空的事务性窗口命令。
    pub(crate) fn take_window_command(&mut self) -> Option<WindowCommand> {
        self.ready_window_commands.pop_front()
    }

    /// 会话即将销毁。
    pub(crate) fn close(&mut self) {
        self.app.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use tela_app_session::{AppFrameToken, SessionError};
    use tela_contract::Viewport;

    /// 内部可变的 mock 会话：驱动器持有 Box，测试经 Rc 断言调用序列。
    struct MockSession {
        log: Rc<RefCell<Vec<String>>>,
        fail_publish: Cell<bool>,
        window_effects: Cell<bool>,
    }

    impl MockSession {
        fn new() -> (Self, Rc<RefCell<Vec<String>>>) {
            let log = Rc::new(RefCell::new(Vec::new()));
            (
                Self {
                    log: Rc::clone(&log),
                    fail_publish: Cell::new(false),
                    window_effects: Cell::new(false),
                },
                log,
            )
        }

        fn publication(token: u64, with_window_effect: bool) -> AppPublication {
            let token = AppFrameToken::new(token).expect("non-zero");
            AppPublication {
                token,
                frame: empty_frame(),
                status: AppStatus {
                    frame_token: Some(token),
                    cursor: CursorKind::Default,
                    input_focused: false,
                    input_value: String::new(),
                    animation_active: false,
                    next_deadline_ms: None,
                },
                effects: if with_window_effect {
                    vec![AppEffect::Window(WindowCommand::Minimize)]
                } else {
                    Vec::new()
                },
            }
        }
    }

    fn empty_frame() -> UiFrame {
        UiFrame {
            viewport: Viewport {
                width: 16.0,
                height: 8.0,
            },
            commands: Vec::new(),
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        }
    }

    impl ApplicationSession for MockSession {
        fn initialize(&mut self) -> Result<AppDispatchOutcome, SessionError> {
            self.log.borrow_mut().push("init".to_owned());
            Ok(AppDispatchOutcome {
                handled: true,
                publish_requested: true,
            })
        }

        fn dispatch(&mut self, event: AppEvent) -> Result<AppDispatchOutcome, SessionError> {
            let name = match event {
                AppEvent::Viewport { .. } => "viewport",
                AppEvent::FrameInput { .. } => "frame-input",
                _ => "other",
            };
            self.log.borrow_mut().push(name.to_owned());
            Ok(AppDispatchOutcome {
                handled: true,
                publish_requested: true,
            })
        }

        fn publish(&mut self) -> Result<AppPublication, SessionError> {
            self.log.borrow_mut().push("publish".to_owned());
            if self.fail_publish.replace(false) {
                return Err(SessionError::new("mock publish failure"));
            }
            Ok(Self::publication(7, self.window_effects.get()))
        }

        fn presented(&mut self, token: AppFrameToken) -> Result<AppDispatchOutcome, SessionError> {
            self.log
                .borrow_mut()
                .push(format!("presented:{}", token.get()));
            Ok(AppDispatchOutcome::IDLE)
        }

        fn rejected(&mut self, token: AppFrameToken) {
            self.log
                .borrow_mut()
                .push(format!("rejected:{}", token.get()));
        }

        fn close(&mut self) {
            self.log.borrow_mut().push("close".to_owned());
        }
    }

    fn mock_driver() -> (SessionDriver, Rc<RefCell<Vec<String>>>) {
        let (mock, log) = MockSession::new();
        let driver = SessionDriver::new(Box::new(mock)).expect("mock driver");
        (driver, log)
    }

    fn dispatch_viewport(driver: &mut SessionDriver) {
        driver.dispatch(AppEvent::Viewport {
            width: 100.0,
            height: 80.0,
        });
    }

    #[test]
    fn new_initializes_and_eagerly_publishes_the_first_frame() {
        let (driver, log) = mock_driver();
        assert_eq!(*log.borrow(), vec!["init", "publish"]);
        assert_eq!(driver.frame().viewport.width, 16.0, "首帧已就绪");
    }

    #[test]
    fn dispatch_publishes_a_new_candidate_immediately() {
        let (mut driver, log) = mock_driver();
        log.borrow_mut().clear();
        dispatch_viewport(&mut driver);
        assert_eq!(
            *log.borrow(),
            vec!["viewport", "rejected:7", "publish"],
            "派发请求发布时：拒绝旧候选并立即发布"
        );
    }

    #[test]
    fn presented_moves_candidate_and_drains_window_commands() {
        let (mock, _log) = MockSession::new();
        mock.window_effects.set(true);
        let mut driver = SessionDriver::new(Box::new(mock)).expect("mock driver");
        assert_eq!(driver.take_window_command(), None, "呈现前不执行效应");
        assert!(!driver.frame_presented());
        // frame_presented 第一次已消费候选；再发布一次后呈现。
        dispatch_viewport(&mut driver);
        assert!(!driver.frame_presented());
        assert_eq!(
            driver.take_window_command(),
            Some(WindowCommand::Minimize),
            "窗口命令随呈现排空"
        );
        assert!(driver.status().is_some());
    }

    #[test]
    fn frame_input_is_dropped_until_a_frame_is_presented() {
        let (mut driver, log) = mock_driver();
        log.borrow_mut().clear();
        let outcome = driver.dispatch_frame_input(AppFrameInput::InputEnter);
        assert!(!outcome.handled, "无已呈现帧时输入必须丢弃");
        assert_eq!(*log.borrow(), Vec::<String>::new());

        dispatch_viewport(&mut driver);
        driver.frame_presented();
        log.borrow_mut().clear();
        let outcome = driver.dispatch_frame_input(AppFrameInput::InputEnter);
        assert!(outcome.handled, "呈现后的输入按令牌放行");
        assert_eq!(
            *log.borrow(),
            vec!["frame-input", "publish"],
            "已呈现的候选不会被 rejected，直接发布新候选"
        );
    }

    #[test]
    fn invalidate_presented_blocks_input_routing_again() {
        let (mut driver, _log) = mock_driver();
        driver.frame_presented();
        driver.invalidate_presented();
        let outcome = driver.dispatch_frame_input(AppFrameInput::InputEnter);
        assert!(!outcome.handled, "几何失效后旧帧不得路由输入");
    }

    #[test]
    fn rejected_candidate_is_republished_on_the_next_dispatch() {
        let (mut driver, log) = mock_driver();
        log.borrow_mut().clear();
        driver.frame_rejected();
        assert_eq!(*log.borrow(), vec!["rejected:7"], "候选被拒绝后等待重发布");
        dispatch_viewport(&mut driver);
        assert_eq!(driver.frame().viewport.width, 16.0, "下一次派发恢复候选帧");
    }

    #[test]
    fn publish_failure_is_retried_at_the_next_opportunity() {
        let (mock, _log) = MockSession::new();
        mock.fail_publish.set(true); // 仅首次发布失败，之后自动复位
        let mut driver = SessionDriver::new(Box::new(mock)).expect("mock driver");
        dispatch_viewport(&mut driver);
        assert_eq!(
            driver.frame().viewport.width,
            16.0,
            "首次发布失败后，下一次派发时机重试成功"
        );
    }

    #[test]
    fn hit_role_defaults_to_client_without_a_presented_frame() {
        let (driver, _log) = mock_driver();
        assert_eq!(
            driver.hit_role_at(Point { x: 1.0, y: 1.0 }),
            HitRole::Client
        );
    }

    #[test]
    fn close_forwards_to_the_session() {
        let (mut driver, log) = mock_driver();
        driver.close();
        assert!(log.borrow().contains(&"close".to_owned()));
    }
}
