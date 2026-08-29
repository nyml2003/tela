//! Tela application controller for the agent workbench.

use tela_app_runtime::{
    AppController, Application, ApplicationConfig, ControllerOutcome, FrameContext,
};
use tela_contract::{FocusAppearance, UiResources, Viewport};
use tela_ui_dsl::{Computed, Signal, ViewBuild, ViewOutput, ViewResult, computed};

use crate::agent::{Agent, MockChatModel, RunReport, Task};
use crate::presentation::{AgentViewProps, render_agent};

const MODEL_ID: &str = "mock-openai-agent-1";
const INITIAL_GOAL: &str = "检查 Tela Agent Demo 的运行状态";
const EXAMPLE_GOAL: &str = "创建两个任务：完成 Tela Agent Demo，再检查浏览器验收";

/// Semantic role used to lay out a visible conversation message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayRole {
    /// Goal submitted by the user.
    User,
    /// Final response produced by the agent loop.
    Assistant,
    /// Recoverable execution error.
    Error,
}

/// One user-visible conversation entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayMessage {
    /// Stable role used by the presentation layer.
    pub role: DisplayRole,
    /// Visible message content.
    pub content: String,
}

/// Typed actions emitted by the Tela view tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentAction {
    /// Updates the controlled prompt field.
    DraftChanged(String),
    /// Submits the text snapshot carried by the input channel.
    SubmitDraft(String),
    /// Submits the controller's current draft through the send button.
    SendDraft,
    /// Clears the controlled prompt field.
    ClearDraft,
    /// Runs a deterministic multi-tool example.
    RunExample,
    /// Recreates the model, conversation, task store, and initial diagnostic run.
    Reset,
}

/// Agent application controller and persistent in-Wasm session state.
///
/// 可见状态全部持有为 `Signal`：写入相同值不触发帧，值变化由 `#[watch]` 组件
/// 的订阅标脏驱动重建，而不是每次动作后全局失效投影。派生值（turns）经
/// `computed` 建为图节点：依赖即构造参数，源变重算、值等零传播。
pub struct AgentDemoController {
    resources: &'static dyn UiResources,
    agent: Agent<MockChatModel>,
    draft: Signal<String>,
    messages: Signal<Vec<DisplayMessage>>,
    turns: Computed<u32>,
    last_report: Signal<Option<RunReport>>,
    tasks: Signal<Vec<Task>>,
    last_error: Signal<Option<String>>,
}

impl AgentDemoController {
    /// Creates the workbench and performs a first inspect-tool run.
    pub fn new(resources: &'static dyn UiResources) -> Self {
        let draft = Signal::new(String::new());
        let messages = Signal::new(Vec::new());
        // 派生节点：依赖（messages）即构造参数；setup 期创建、控制器持有。
        let turns = computed(&messages, |messages| (messages.len() / 2) as u32);
        let mut controller = Self {
            resources,
            agent: Agent::new(MockChatModel::new(), MODEL_ID),
            draft,
            messages,
            turns,
            last_report: Signal::new(None),
            tasks: Signal::new(Vec::new()),
            last_error: Signal::new(None),
        };
        controller.submit(INITIAL_GOAL.to_owned());
        controller
    }

    /// Controlled prompt value.
    pub fn draft(&self) -> String {
        self.draft.get()
    }

    /// Visible user/assistant conversation.
    pub fn messages(&self) -> Vec<DisplayMessage> {
        self.messages.get()
    }

    /// Most recent complete run, including its tool trace.
    pub fn last_report(&self) -> Option<RunReport> {
        self.last_report.get()
    }

    /// Persistent tasks created through the local tool runtime.
    pub fn tasks(&self) -> Vec<Task> {
        self.tasks.get()
    }

    fn submit(&mut self, value: String) -> bool {
        let goal = value.trim().to_owned();
        if goal.is_empty() {
            return false;
        }
        let mut messages = self.messages.get();
        messages.push(DisplayMessage {
            role: DisplayRole::User,
            content: goal.clone(),
        });
        let (report, error) = match self.agent.run(goal) {
            Ok(report) => {
                messages.push(DisplayMessage {
                    role: DisplayRole::Assistant,
                    content: report.answer.clone(),
                });
                (Some(report), None)
            }
            Err(error) => {
                let message = error.to_string();
                messages.push(DisplayMessage {
                    role: DisplayRole::Error,
                    content: message.clone(),
                });
                (None, Some(message))
            }
        };
        self.draft.set(String::new());
        self.messages.set(messages);
        self.last_report.set(report);
        self.last_error.set(error);
        self.tasks.set(self.agent.tasks().to_vec());
        true
    }

    fn reset(&mut self) {
        self.agent = Agent::new(MockChatModel::new(), MODEL_ID);
        self.draft.set(String::new());
        self.messages.set(Vec::new());
        self.last_report.set(None);
        self.tasks.set(Vec::new());
        self.last_error.set(None);
        self.submit(INITIAL_GOAL.to_owned());
    }
}

impl AppController<AgentAction> for AgentDemoController {
    fn render(
        &mut self,
        build: &mut ViewBuild<AgentAction>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<AgentAction>> {
        render_agent(
            build,
            AgentViewProps {
                viewport: ctx.viewport,
                viewport_signal: ctx.viewport_signal.clone(),
                draft: self.draft.clone(),
                messages: self.messages.clone(),
                turns: self.turns.clone(),
                report: self.last_report.clone(),
                tasks: self.tasks.clone(),
                error: self.last_error.clone(),
                draft_focused: ctx
                    .focus_key
                    .as_ref()
                    .is_some_and(|key| key.0 == "agent.prompt"),
                hover_key: ctx.hover_key.as_ref().map(|key| key.0.as_str()),
                icons: self.resources.icon_provider(),
            },
        )
    }

    fn handle_action(&mut self, action: AgentAction) -> ControllerOutcome {
        let changed = match action {
            AgentAction::DraftChanged(value) => {
                let changed = self.draft.get() != value;
                self.draft.set(value);
                changed
            }
            AgentAction::SubmitDraft(value) => self.submit(value),
            AgentAction::SendDraft => self.submit(self.draft.get()),
            AgentAction::ClearDraft => {
                let changed = self.draft.get() != String::new();
                self.draft.set(String::new());
                changed
            }
            AgentAction::RunExample => self.submit(EXAMPLE_GOAL.to_owned()),
            AgentAction::Reset => {
                self.reset();
                true
            }
        };
        ControllerOutcome::changed(changed)
    }
}

/// Fully assembled in-process application used by the static Web product.
pub type AgentDemoApp = Application<AgentAction, AgentDemoController>;

/// Product-neutral application configuration for the workbench.
pub fn agent_demo_config() -> ApplicationConfig {
    ApplicationConfig {
        initial_viewport: Viewport {
            width: 1200.0,
            height: 760.0,
        },
        focus_appearance: Some(FocusAppearance {
            color: tela_contract::Color::rgba(0.08, 0.48, 0.36, 1.0),
            width: 2.0,
            inset: 1.0,
        }),
        ..ApplicationConfig::default()
    }
}

/// Creates a complete agent application session with product-selected resources.
pub fn new_agent_demo(resources: &'static dyn UiResources) -> AgentDemoApp {
    Application::new(
        resources,
        AgentDemoController::new(resources),
        agent_demo_config(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_app_session::{AppEvent, ApplicationSession};
    use tela_contract::UiResourceSet;
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_text_resources::ControlledTextMeasurer;

    static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
        UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

    #[test]
    fn initial_session_runs_the_inspect_tool() {
        let controller = AgentDemoController::new(&RESOURCES);

        let report = controller.last_report().expect("initial report");
        assert_eq!(report.rounds, 2);
        assert_eq!(report.tool_calls, 1);
        assert!(controller.messages()[1].content.contains("单 Wasm"));
    }

    #[test]
    fn typed_actions_create_persistent_tasks_and_reset_them() {
        let mut controller = AgentDemoController::new(&RESOURCES);
        controller.handle_action(AgentAction::RunExample);
        assert_eq!(controller.tasks().len(), 2);

        controller.handle_action(AgentAction::Reset);
        assert!(controller.tasks().is_empty());
        assert_eq!(controller.last_report().expect("reset report").id, 1);
    }

    #[test]
    fn application_publishes_a_frame_at_mobile_width() {
        let mut app = new_agent_demo(&RESOURCES);
        let outcome = ApplicationSession::initialize(&mut app).expect("initialize");
        assert!(outcome.publish_requested);
        ApplicationSession::dispatch(
            &mut app,
            AppEvent::Viewport {
                width: 390.0,
                height: 844.0,
            },
        )
        .expect("mobile viewport");
        let publication = ApplicationSession::publish(&mut app).expect("mobile frame");
        assert_eq!(publication.frame.viewport.width, 390.0);
        assert_eq!(publication.frame.viewport.height, 844.0);
    }

    /// 宿主完整帧生命周期：publish 阶段 resolve，presented 阶段原子提交
    /// （订阅安装、#[memo] 缓存提交都发生在 presented）。
    fn publish_and_present(app: &mut AgentDemoApp) {
        let token = ApplicationSession::publish(app).expect("publish").token;
        ApplicationSession::presented(app, token).expect("presented");
    }

    #[test]
    fn signal_write_renders_without_projection_invalidation() {
        let mut app = new_agent_demo(&RESOURCES);
        ApplicationSession::initialize(&mut app).expect("initialize");
        publish_and_present(&mut app);
        assert!(app.frame_is_current());

        // 被订阅的 signal 写入：dirty 驱动失效，publish 后恢复 current。
        app.dispatch_action(AgentAction::DraftChanged("列出当前任务".to_owned()));
        assert!(!app.frame_is_current());
        publish_and_present(&mut app);
        assert!(app.frame_is_current());

        // 相同值写入被相等性短路：无 dirty、无失效，帧保持 current。
        app.dispatch_action(AgentAction::DraftChanged("列出当前任务".to_owned()));
        assert!(app.frame_is_current());

        // 未被订阅路径（重置重建全部状态）仍可靠全局失效兜底。
        app.dispatch_action(AgentAction::Reset);
        assert!(!app.frame_is_current());
        publish_and_present(&mut app);
        assert!(app.frame_is_current());
    }

    #[test]
    fn memo_skips_unrelated_panels_on_signal_frames() {
        let mut app = new_agent_demo(&RESOURCES);
        ApplicationSession::initialize(&mut app).expect("initialize");
        publish_and_present(&mut app);

        // 首个 signal 驱动帧：记忆化开始记录（无缓存可命中，全量渲染）。
        app.dispatch_action(AgentAction::DraftChanged("a".to_owned()));
        publish_and_present(&mut app);
        let baseline = crate::presentation::trace_renders();

        // 再次打字：trace 面板的订阅与 props 均未变化，命中 #[memo] 缓存。
        app.dispatch_action(AgentAction::DraftChanged("ab".to_owned()));
        publish_and_present(&mut app);
        assert_eq!(
            crate::presentation::trace_renders(),
            baseline,
            "不相关面板命中 #[memo] 缓存"
        );

        // 运行示例改变了报告/任务 signal：缓存被脏标穿透。
        app.dispatch_action(AgentAction::RunExample);
        publish_and_present(&mut app);
        assert!(
            crate::presentation::trace_renders() > baseline,
            "订阅的 signal 变化必须重渲染"
        );
    }
}
