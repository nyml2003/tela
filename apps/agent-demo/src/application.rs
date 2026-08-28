//! Tela application controller for the agent workbench.

use tela_app_runtime::{
    AppController, Application, ApplicationConfig, ControllerOutcome, FrameContext,
};
use tela_contract::{FocusAppearance, UiResources, Viewport};
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult};

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
pub struct AgentDemoController {
    resources: &'static dyn UiResources,
    agent: Agent<MockChatModel>,
    draft: String,
    messages: Vec<DisplayMessage>,
    last_report: Option<RunReport>,
    last_error: Option<String>,
}

impl AgentDemoController {
    /// Creates the workbench and performs a first inspect-tool run.
    pub fn new(resources: &'static dyn UiResources) -> Self {
        let mut controller = Self {
            resources,
            agent: Agent::new(MockChatModel::new(), MODEL_ID),
            draft: String::new(),
            messages: Vec::new(),
            last_report: None,
            last_error: None,
        };
        controller.submit(INITIAL_GOAL.to_owned());
        controller
    }

    /// Controlled prompt value.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Visible user/assistant conversation.
    pub fn messages(&self) -> &[DisplayMessage] {
        &self.messages
    }

    /// Most recent complete run, including its tool trace.
    pub fn last_report(&self) -> Option<&RunReport> {
        self.last_report.as_ref()
    }

    /// Persistent tasks created through the local tool runtime.
    pub fn tasks(&self) -> &[Task] {
        self.agent.tasks()
    }

    fn submit(&mut self, value: String) -> bool {
        let goal = value.trim().to_owned();
        if goal.is_empty() {
            return false;
        }
        self.draft.clear();
        self.messages.push(DisplayMessage {
            role: DisplayRole::User,
            content: goal.clone(),
        });
        match self.agent.run(goal) {
            Ok(report) => {
                self.messages.push(DisplayMessage {
                    role: DisplayRole::Assistant,
                    content: report.answer.clone(),
                });
                self.last_report = Some(report);
                self.last_error = None;
            }
            Err(error) => {
                let message = error.to_string();
                self.messages.push(DisplayMessage {
                    role: DisplayRole::Error,
                    content: message.clone(),
                });
                self.last_error = Some(message);
            }
        }
        true
    }

    fn reset(&mut self) {
        self.agent = Agent::new(MockChatModel::new(), MODEL_ID);
        self.draft.clear();
        self.messages.clear();
        self.last_report = None;
        self.last_error = None;
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
                draft: &self.draft,
                draft_focused: ctx
                    .focus_key
                    .as_ref()
                    .is_some_and(|key| key.0 == "agent.prompt"),
                hover_key: ctx.hover_key.as_ref().map(|key| key.0.as_str()),
                messages: &self.messages,
                report: self.last_report.as_ref(),
                tasks: self.agent.tasks(),
                error: self.last_error.as_deref(),
                icons: self.resources.icon_provider(),
            },
        )
    }

    fn handle_action(&mut self, action: AgentAction) -> ControllerOutcome {
        let changed = match action {
            AgentAction::DraftChanged(value) => {
                let changed = self.draft != value;
                self.draft = value;
                changed
            }
            AgentAction::SubmitDraft(value) => self.submit(value),
            AgentAction::SendDraft => self.submit(self.draft.clone()),
            AgentAction::ClearDraft => {
                let changed = !self.draft.is_empty();
                self.draft.clear();
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
}
