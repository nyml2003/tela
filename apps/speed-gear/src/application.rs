//! 变速齿轮应用控制器：只管理业务事实，组件中间态由组件状态对象持有。

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use tela_app_runtime::{AppController, ControllerOutcome, FrameContext};
use tela_app_session::AppEffect;
use tela_contract::{FocusAppearance, UiResources, WindowCommand};
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult};

use crate::domain::{
    ConnectStatus, ConnectionState, ProcessIdentity, Rate, SpeedBackend, SpeedGearState,
};
use crate::presentation::render_root;

/// 变速齿轮焦点外观。
pub const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: tela_contract::Color::rgba(0.0, 0.42, 0.86, 1.0),
    width: 2.0,
    inset: 1.0,
};

/// 应用动作。
#[derive(Clone, Debug, PartialEq)]
pub enum SpeedGearAction {
    /// 选择进程。
    Select(ProcessIdentity),
    /// 显式连接当前目标。
    Connect,
    /// 停止当前连接。
    Stop,
    /// 设置倍率。
    SetRate(Rate),
    /// Transfer 提交的最终目标集合。
    TransferTargets(BTreeSet<String>),
    /// 重新枚举进程。
    RefreshProcesses,
    /// 窗口命令。
    Window(WindowCommand),
}

/// 变速齿轮控制器。
pub struct SpeedGearController {
    resources: &'static dyn UiResources,
    backend: Box<dyn SpeedBackend>,
    state: SpeedGearState,
    last_error: Option<String>,
    last_refresh: Instant,
}

impl SpeedGearController {
    /// 创建控制器。
    pub fn new(resources: &'static dyn UiResources, backend: Box<dyn SpeedBackend>) -> Self {
        Self {
            resources,
            backend,
            state: SpeedGearState::default(),
            last_error: None,
            last_refresh: Instant::now(),
        }
    }

    /// 读取领域状态。
    pub fn state(&self) -> &SpeedGearState {
        &self.state
    }

    /// 读取最后一次可展示错误。
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// 刷新进程列表；调用失败只更新错误，不清空仍然可见的旧列表。
    pub fn refresh_processes(&mut self) -> bool {
        let previous_target = self.state.processes.selected().cloned();
        match self.backend.enumerate(std::process::id()) {
            Ok(items) => {
                self.state.processes.replace(items);
                let target_exited =
                    previous_target.is_some() && self.state.processes.selected().is_none();
                if let Some(target) = previous_target.filter(|_| target_exited) {
                    self.state.connection = ConnectionState::TargetExited(target);
                    self.state.rate = Rate::new(Rate::NORMAL);
                    self.last_error = Some("目标进程已退出，请重新选择目标".to_owned());
                } else {
                    self.last_error = None;
                }
                true
            }
            Err(error) => {
                self.last_error = Some(format_backend_error(&error));
                false
            }
        }
    }

    fn handle(&mut self, action: SpeedGearAction) -> bool {
        self.last_error = None;
        match action {
            SpeedGearAction::Select(identity) => {
                if let ConnectionState::Connected(current) = &self.state.connection
                    && current != &identity
                    && let Err(error) = self.backend.stop(current)
                {
                    self.last_error = Some(format_backend_error(&error));
                    return false;
                }
                match self.state.processes.select(identity.clone()) {
                    Ok(()) => {
                        self.state.connection = ConnectionState::Selected(identity);
                        self.state.rate = Rate::new(Rate::NORMAL);
                        true
                    }
                    Err(error) => {
                        self.last_error = Some(format_backend_error(&error));
                        false
                    }
                }
            }
            SpeedGearAction::Connect => self.connect(),
            SpeedGearAction::Stop => self.stop(),
            SpeedGearAction::SetRate(rate) => self.set_rate(rate),
            SpeedGearAction::TransferTargets(keys) => {
                if let Some(key) = keys.iter().next()
                    && let Ok(pid) = key.parse::<u32>()
                    && let Some(item) = self
                        .state
                        .processes
                        .items()
                        .iter()
                        .find(|item| item.identity.pid == pid)
                {
                    return self.handle(SpeedGearAction::Select(item.identity.clone()));
                }
                if keys.is_empty() {
                    self.stop();
                    self.state.processes.clear_selection();
                    self.state.connection = ConnectionState::NoTarget;
                    self.state.rate = Rate::new(Rate::NORMAL);
                    return true;
                }
                false
            }
            SpeedGearAction::RefreshProcesses => self.refresh_processes(),
            SpeedGearAction::Window(_) => true,
        }
    }

    fn connect(&mut self) -> bool {
        let Some(target) = self.state.processes.selected().cloned() else {
            self.state.connection = ConnectionState::NoTarget;
            return false;
        };
        self.state.connection = ConnectionState::Connecting(target.clone());
        match self.backend.connect(&target) {
            Ok(ConnectStatus::Ready) => {
                self.state.rate = Rate::new(Rate::NORMAL);
                self.state.connection = ConnectionState::Connected(target);
                true
            }
            Ok(ConnectStatus::Pending) => true,
            Err(error) => {
                self.last_error = Some(format_backend_error(&error));
                self.state.connection = ConnectionState::Failed(target, error);
                false
            }
        }
    }

    fn set_rate(&mut self, rate: Rate) -> bool {
        let ConnectionState::Connected(target) = &self.state.connection else {
            return false;
        };
        let target = target.clone();
        match self.backend.set_rate(&target, rate) {
            Ok(()) => {
                self.state.rate = rate;
                true
            }
            Err(error) => {
                self.last_error = Some(format_backend_error(&error));
                false
            }
        }
    }

    fn stop(&mut self) -> bool {
        let target = match &self.state.connection {
            ConnectionState::Connected(target) | ConnectionState::Connecting(target) => {
                target.clone()
            }
            ConnectionState::Failed(target, _) => target.clone(),
            _ => return false,
        };
        let changed = self.backend.stop(&target).is_ok();
        if !changed {
            self.last_error = Some("停止目标调速失败，目标将继续以安全基线运行".to_owned());
        }
        self.state.rate = Rate::new(Rate::NORMAL);
        self.state.connection = ConnectionState::Selected(target);
        changed
    }

    /// 目标退出时清理 UI 状态。
    pub fn target_exited(&mut self) {
        if let Some(target) = self.state.processes.selected().cloned() {
            self.state.connection = ConnectionState::TargetExited(target);
        } else {
            self.state.connection = ConnectionState::NoTarget;
        }
        self.state.rate = Rate::new(Rate::NORMAL);
        self.state.processes.clear_selection();
    }
}

impl AppController<SpeedGearAction> for SpeedGearController {
    fn render(
        &mut self,
        build: &mut ViewBuild<SpeedGearAction>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<SpeedGearAction>> {
        let _resources = self.resources;
        render_root(build, ctx.viewport, &self.state, self.last_error.as_deref())
    }

    fn handle_action(&mut self, action: SpeedGearAction) -> ControllerOutcome {
        let effect = match &action {
            SpeedGearAction::Window(command) => Some(AppEffect::Window(*command)),
            _ => None,
        };
        let changed = self.handle(action);
        effect.map_or_else(
            || ControllerOutcome::changed(changed),
            ControllerOutcome::with_effect,
        )
    }

    fn on_tick(&mut self) -> bool {
        let mut changed = false;
        if let ConnectionState::Connecting(target) = &self.state.connection {
            let target = target.clone();
            match self.backend.poll_connect(&target) {
                Ok(ConnectStatus::Ready) => {
                    self.state.rate = Rate::new(Rate::NORMAL);
                    self.state.connection = ConnectionState::Connected(target);
                    changed = true;
                }
                Ok(ConnectStatus::Pending) => {}
                Err(error) => {
                    self.last_error = Some(format_backend_error(&error));
                    self.state.connection = ConnectionState::Failed(target, error);
                    self.state.rate = Rate::new(Rate::NORMAL);
                    changed = true;
                }
            }
        }
        if let ConnectionState::Connected(target) = &self.state.connection {
            let target = target.clone();
            if let Err(error) = self.backend.heartbeat(&target) {
                self.last_error = Some(format_backend_error(&error));
                self.state.rate = Rate::new(Rate::NORMAL);
                if matches!(error, crate::domain::SpeedBackendError::TargetExited) {
                    self.state.processes.clear_selection();
                    self.state.connection = ConnectionState::TargetExited(target);
                } else {
                    self.state.connection = ConnectionState::Failed(target, error);
                }
                changed = true;
            }
        }
        if self.last_refresh.elapsed() >= Duration::from_secs(2) {
            self.last_refresh = Instant::now();
            changed |= self.refresh_processes();
        }
        changed
    }

    fn on_close(&mut self) {
        let _ = self.stop();
    }
}

fn format_backend_error(error: &crate::domain::SpeedBackendError) -> String {
    match error {
        crate::domain::SpeedBackendError::TargetExited => "目标进程已退出".to_owned(),
        crate::domain::SpeedBackendError::PermissionDenied => {
            "当前权限不足，无法控制目标".to_owned()
        }
        crate::domain::SpeedBackendError::ArchitectureMismatch => {
            "目标不是可用的 Windows x64 进程".to_owned()
        }
        crate::domain::SpeedBackendError::ProtectedTarget => {
            "目标受系统保护，未执行连接".to_owned()
        }
        crate::domain::SpeedBackendError::HookUnavailable => {
            "目标调速模块不可用，未伪装为已连接".to_owned()
        }
        crate::domain::SpeedBackendError::Communication(message) => message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BackendResult, ProcessAccess, ProcessInfo, SpeedBackendError};
    use tela_app_runtime::{Application, ApplicationConfig};
    use tela_contract::{PhysicalKey, Point, PointerEvent, SemanticKey, UiResourceSet, Viewport};
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_text_resources::ControlledTextMeasurer;

    static TEST_RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
        UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

    struct FakeBackend {
        items: Vec<ProcessInfo>,
        connected: bool,
        rates: Vec<Rate>,
    }

    impl SpeedBackend for FakeBackend {
        fn enumerate(&mut self, _self_pid: u32) -> BackendResult<Vec<ProcessInfo>> {
            Ok(self.items.clone())
        }
        fn connect(&mut self, _target: &ProcessIdentity) -> BackendResult<ConnectStatus> {
            self.connected = true;
            Ok(ConnectStatus::Ready)
        }
        fn set_rate(&mut self, _target: &ProcessIdentity, rate: Rate) -> BackendResult<()> {
            self.rates.push(rate);
            Ok(())
        }
        fn stop(&mut self, _target: &ProcessIdentity) -> BackendResult<()> {
            self.connected = false;
            Ok(())
        }
        fn heartbeat(&mut self, _target: &ProcessIdentity) -> BackendResult<()> {
            Ok(())
        }
    }

    fn process() -> ProcessInfo {
        ProcessInfo {
            identity: ProcessIdentity {
                pid: 7,
                creation_time: 70,
            },
            name: "demo.exe".to_owned(),
            path: None,
            visible_window: true,
            access: ProcessAccess::Available,
        }
    }

    #[test]
    fn selection_does_not_connect_until_explicit_connect() {
        let mut controller = SpeedGearController::new(
            &TEST_RESOURCES,
            Box::new(FakeBackend {
                items: vec![process()],
                connected: false,
                rates: Vec::new(),
            }),
        );
        controller.refresh_processes();
        controller
            .handle(SpeedGearAction::Select(process().identity))
            .then_some(());
        assert!(matches!(
            controller.state.connection,
            ConnectionState::Selected(_)
        ));
        assert!(controller.handle(SpeedGearAction::Connect));
        assert!(matches!(
            controller.state.connection,
            ConnectionState::Connected(_)
        ));
    }

    #[test]
    fn failed_rate_update_keeps_last_confirmed_value() {
        struct Failing;
        impl SpeedBackend for Failing {
            fn enumerate(&mut self, _: u32) -> BackendResult<Vec<ProcessInfo>> {
                Ok(vec![process()])
            }
            fn connect(&mut self, _: &ProcessIdentity) -> BackendResult<ConnectStatus> {
                Ok(ConnectStatus::Ready)
            }
            fn set_rate(&mut self, _: &ProcessIdentity, _: Rate) -> BackendResult<()> {
                Err(SpeedBackendError::Communication("no".to_owned()))
            }
            fn stop(&mut self, _: &ProcessIdentity) -> BackendResult<()> {
                Ok(())
            }
            fn heartbeat(&mut self, _: &ProcessIdentity) -> BackendResult<()> {
                Ok(())
            }
        }
        let mut controller = SpeedGearController::new(&TEST_RESOURCES, Box::new(Failing));
        controller.refresh_processes();
        controller.handle(SpeedGearAction::Select(process().identity));
        controller.handle(SpeedGearAction::Connect);
        assert!(!controller.handle(SpeedGearAction::SetRate(Rate::new(2.0))));
        assert_eq!(controller.state.rate, Rate::new(1.0));
    }

    #[test]
    fn first_application_frame_builds_a_valid_tree() {
        let mut controller = SpeedGearController::new(
            &TEST_RESOURCES,
            Box::new(FakeBackend {
                items: vec![process()],
                connected: false,
                rates: Vec::new(),
            }),
        );
        controller.refresh_processes();
        let mut application = Application::new(
            &TEST_RESOURCES,
            controller,
            ApplicationConfig {
                initial_viewport: Viewport {
                    width: 980.0,
                    height: 680.0,
                },
                focus_appearance: Some(crate::FOCUS_APPEARANCE),
            },
        );
        assert!(application.ensure_frame());
        assert!(application.frame_is_current());
        assert!(application.active().is_none());
        application.frame_rejected();
        assert!(application.active().is_none());
        assert!(!application.frame_is_current());
        assert!(application.ensure_frame());
        assert!(!application.frame_presented());
        assert!(application.active().is_some());

        let active_viewport = application.active().expect("presented frame").1.viewport;
        assert!(application.set_viewport(1_120.0, 720.0, 1.0));
        assert!(application.ensure_frame());
        assert_ne!(application.frame().viewport, active_viewport);
        application.frame_rejected();
        assert_eq!(
            application
                .active()
                .expect("retained active frame")
                .1
                .viewport,
            active_viewport
        );
    }

    #[test]
    fn win32_session_routes_semantic_home_to_the_focused_slider() {
        let mut controller = SpeedGearController::new(
            &TEST_RESOURCES,
            Box::new(FakeBackend {
                items: vec![process()],
                connected: false,
                rates: Vec::new(),
            }),
        );
        controller.refresh_processes();
        assert!(controller.handle(SpeedGearAction::Select(process().identity)));
        assert!(controller.handle(SpeedGearAction::Connect));
        let mut application = Application::new(
            &TEST_RESOURCES,
            controller,
            ApplicationConfig {
                initial_viewport: Viewport {
                    width: 980.0,
                    height: 680.0,
                },
                focus_appearance: Some(crate::FOCUS_APPEARANCE),
            },
        );
        assert!(application.ensure_frame());
        application.frame_presented();

        let slider_key = SemanticKey("speed-gear.rate".to_owned());
        let slider_rect = {
            let (tree, frame) = application.active().expect("presented speed gear frame");
            let slider_id = tree
                .node_id_for_key(&slider_key)
                .expect("slider semantic key");
            frame
                .hit_regions
                .iter()
                .find(|region| region.node_id == slider_id)
                .expect("slider hit region")
                .rect
        };
        let center = Point {
            x: slider_rect.x + slider_rect.w * 0.5,
            y: slider_rect.y + slider_rect.h * 0.5,
        };
        assert!(application.handle_pointer(PointerEvent::mouse_down(center)) > 0);
        assert!(application.ensure_frame());
        application.frame_presented();

        assert_eq!(
            application.handle_key(PhysicalKey::Home.code(), 0, false),
            1
        );
        assert!(application.ensure_frame());
        assert!(application.frame_presented());
        assert_eq!(application.controller().state().rate, Rate::new(Rate::MIN));
    }
}
