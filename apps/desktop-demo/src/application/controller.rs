//! 文件管理器控制器：共享会话运行时（`tela-app-runtime`）之上的域投影与意图处理。
//!
//! 帧协调、滚动钳制、模态栈、键位表、IME 与文本通道全部由 `Application` 提供；这里只
//! 保留真正的应用职责：把 [`FrameContext`] 投影成 `AppShell` props、声明动态动作锚点、
//! 把 core 交互事实翻译成 [`Intent`]。

use tela_app_runtime::{
    AppController, ApplicationConfig, ControllerOutcome, FrameContext, keymap::KeymapSnapshot,
};
use tela_contract::{
    Color, FocusAppearance, KernelInteraction, KeyboardIntent, Modifiers, PhysicalKey, ScrollState,
    SemanticKey, ShortcutId, UiResources, Viewport,
};
use tela_core::UiTree;
use tela_desktop_ui_dsl::{DraftInputCommit, DraftInputProps, DraftInputView};
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult, ViewSite};

use super::{Intent, apply_intent};
use crate::domain::{
    EntryFilter, FileCommand, FileManagerModel, FileManagerSession, OperationKind,
};
use crate::presentation::operation::OPERATION_MODAL_KEY;
use crate::presentation::shell::{AppShell, AppShellProps};

/// 初次加载默认逻辑尺寸；浏览器启动后由宿主覆盖为实际 CSS 视口。
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 1280.0,
    height: 800.0,
};

/// 焦点环外观（产品装配注入共享运行时）。
pub const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: Color::rgba(0.15, 0.39, 0.92, 1.0),
    width: 2.0,
    inset: 2.0,
};

const SEARCH_KEY: &str = "file.search";
const OPERATION_INPUT_KEY: &str = "operation.value";
const TOOLBAR_PREFIX: &str = "command.";

/// 文件管理器默认键位：共享导航基线 + Ctrl+Z 撤销。
pub fn default_keymap() -> KeymapSnapshot {
    KeymapSnapshot::navigation_default().with_default_binding(
        PhysicalKey::KeyZ,
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
        KeyboardIntent::Invoke(ShortcutId::Undo),
    )
}

/// 组装文件管理器的共享运行时配置（产品装配与测试共用）。
pub fn demo_config() -> ApplicationConfig {
    ApplicationConfig {
        initial_viewport: DEFAULT_VIEWPORT,
        focus_appearance: Some(FOCUS_APPEARANCE),
        keymap: default_keymap(),
    }
}

fn commit_search(commit: DraftInputCommit) -> Option<Intent> {
    Some(Intent::SetQuery(commit.value))
}

fn commit_operation(commit: DraftInputCommit) -> Option<Intent> {
    Some(Intent::SetOperationValue(commit.value))
}

/// 逐帧动态点击锚点表：静态命令/过滤器/操作 + 每个可见条目一个 key。
///
/// 键可能不在当前树中（列表窗口化）；运行时的定稿遍只为存活键挂载锚点。
pub fn desktop_action_bindings(
    model: &FileManagerModel,
    session: &FileManagerSession,
) -> Vec<(SemanticKey, Intent)> {
    let mut bindings = vec![
        (
            SemanticKey("navigation.toggle".to_owned()),
            Intent::ToggleNavigation,
        ),
        (
            SemanticKey("command.new-folder".to_owned()),
            Intent::BeginOperation(OperationKind::NewFolder),
        ),
        (
            SemanticKey("command.rename".to_owned()),
            Intent::BeginOperation(OperationKind::Rename),
        ),
        (
            SemanticKey("command.copy".to_owned()),
            Intent::Command(FileCommand::CopySelected),
        ),
        (
            SemanticKey("command.move-design".to_owned()),
            Intent::BeginOperation(OperationKind::MoveToDesign),
        ),
        (
            SemanticKey("command.trash".to_owned()),
            Intent::BeginOperation(OperationKind::Trash),
        ),
        (
            SemanticKey("command.restore".to_owned()),
            Intent::Command(FileCommand::RestoreSelected),
        ),
        (
            SemanticKey("command.favorite".to_owned()),
            Intent::Command(FileCommand::ToggleFavorite),
        ),
        (
            SemanticKey("command.toggle-view".to_owned()),
            Intent::Command(FileCommand::ToggleView),
        ),
        (
            SemanticKey("command.toggle-sort".to_owned()),
            Intent::Command(FileCommand::ToggleSort),
        ),
        (
            SemanticKey("command.toggle-filter".to_owned()),
            Intent::Command(FileCommand::ToggleFilter),
        ),
        (
            SemanticKey("command.add-tag".to_owned()),
            Intent::BeginOperation(OperationKind::AddTag),
        ),
        (
            SemanticKey("command.undo".to_owned()),
            Intent::Command(FileCommand::Undo),
        ),
        (
            SemanticKey("filter.all".to_owned()),
            Intent::SetFilter(EntryFilter::All),
        ),
        (
            SemanticKey("filter.favorites".to_owned()),
            Intent::SetFilter(EntryFilter::Favorites),
        ),
        (
            SemanticKey("filter.tagged".to_owned()),
            Intent::SetFilter(EntryFilter::Tagged),
        ),
        (
            SemanticKey("filter.trash".to_owned()),
            Intent::SetFilter(EntryFilter::Trash),
        ),
        (
            SemanticKey("operation.confirm".to_owned()),
            Intent::ConfirmOperation,
        ),
        (
            SemanticKey("operation.cancel".to_owned()),
            Intent::CancelOperation,
        ),
    ];
    bindings.extend(model.folders().into_iter().map(|entry| {
        (
            SemanticKey(format!("folder.open.{}", entry.id)),
            Intent::OpenFolder(entry.id),
        )
    }));
    bindings.extend(model.entries().map(|entry| {
        (
            SemanticKey(format!("entry-{}", entry.id)),
            Intent::Select(entry.id),
        )
    }));
    if session.selected.is_empty() {
        bindings.retain(|(key, _)| {
            !matches!(
                key.0.as_str(),
                "command.rename"
                    | "command.copy"
                    | "command.move-design"
                    | "command.favorite"
                    | "command.add-tag"
                    | "command.trash"
                    | "command.restore"
            )
        });
    }
    bindings
}

fn intent_allowed_during_operation(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::SetOperationValue(_) | Intent::ConfirmOperation | Intent::CancelOperation
    )
}

fn intent_replaces_detail_content(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::Command(_)
            | Intent::OpenFolder(_)
            | Intent::SetFilter(_)
            | Intent::SetQuery(_)
            | Intent::ConfirmOperation
    )
}

/// 文件管理器域控制器：领域模型 + 页面会话 + 逐帧学习到的投影。
pub struct DesktopDemoController {
    resources: &'static dyn UiResources,
    /// 实体真源（含撤销栈）。
    pub(crate) model: FileManagerModel,
    /// 页面状态（当前目录、选择、查询、操作草稿）。
    pub(crate) session: FileManagerSession,
    /// 状态栏投影：当前帧实际悬停的工具栏动作 key（前缀 + 存活过滤）。
    hovered_action_key: Option<SemanticKey>,
    /// 详情虚拟列表滚动容器 key（发现序第二位），内容替换时归零。
    detail_scroll_key: Option<SemanticKey>,
}

impl DesktopDemoController {
    /// 用产品装配选择的视觉资源启动文件管理器域。
    pub fn new(resources: &'static dyn UiResources) -> Self {
        Self {
            resources,
            model: FileManagerModel::sample(),
            session: FileManagerSession::default(),
            hovered_action_key: None,
            detail_scroll_key: None,
        }
    }

    /// 状态栏当前高亮的工具栏动作 key（测试与诊断入口）。
    pub fn hovered_action_key(&self) -> Option<&SemanticKey> {
        self.hovered_action_key.as_ref()
    }

    fn operation_accepts_input(&self) -> bool {
        self.session.operation.as_ref().is_some_and(|operation| {
            !matches!(
                operation.kind,
                OperationKind::MoveToDesign | OperationKind::Trash
            )
        })
    }

    fn render_shell(
        &mut self,
        build: &mut ViewBuild<Intent>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<Intent>> {
        // 先学习投影（写状态），再构造借用 self 的 props：焦点身份、悬停键与滚动偏移
        // 都来自共享运行时的收敛结果；悬停投影只保留工具栏动作前缀的键。
        self.detail_scroll_key = ctx
            .scroll_offsets
            .get(1)
            .map(|(key, _)| key.clone())
            .or_else(|| self.detail_scroll_key.clone());
        self.hovered_action_key = ctx
            .hover_key
            .clone()
            .filter(|key| key.0.starts_with(TOOLBAR_PREFIX));
        let detail_scroll_y = self
            .detail_scroll_key
            .as_ref()
            .and_then(|key| {
                ctx.scroll_offsets
                    .iter()
                    .find(|(candidate, _)| candidate == key)
            })
            .map_or(0.0, |(_, offset)| *offset);
        let props = AppShellProps {
            model: &self.model,
            session: &self.session,
            viewport: ctx.viewport,
            search_focused: ctx
                .focus_key
                .as_ref()
                .is_some_and(|key| key.0 == SEARCH_KEY),
            operation_focused: self.operation_accepts_input()
                && ctx
                    .focus_key
                    .as_ref()
                    .is_some_and(|key| key.0 == OPERATION_INPUT_KEY),
            hovered_action_key: self.hovered_action_key.clone(),
            detail_scroll_y,
            icons: self.resources.icon_provider(),
        };
        let site = ViewSite::new(file!(), line!(), column!());
        let horizontal_inset =
            crate::presentation::shared::APP_INSET.min((props.viewport.width - 1.0).max(0.0) * 0.5);
        let shell_width = (props.viewport.width - horizontal_inset * 2.0).max(1.0);
        let search_input = DraftInputView::render_for(
            build,
            DraftInputProps {
                value: Some(self.session.query.clone()),
                placeholder: Some("搜索文件和目录".to_owned()),
                focused: Some(props.search_focused),
                width: Some((shell_width * 0.32).clamp(180.0, 420.0)),
                height: Some(28.0),
                border_radius: Some(crate::presentation::shared::CONTROL_RADIUS),
                key: Some(SEARCH_KEY.to_owned()),
                ..DraftInputProps::default()
            },
            commit_search,
            site,
        )?;
        let operation_input = self
            .session
            .operation
            .as_ref()
            .and_then(|operation| {
                self.operation_accepts_input().then(|| {
                    DraftInputView::render_for(
                        build,
                        DraftInputProps {
                            value: Some(operation.value.clone()),
                            placeholder: Some("输入名称".to_owned()),
                            focused: Some(props.operation_focused),
                            width: Some(300.0),
                            height: Some(32.0),
                            border_radius: Some(crate::presentation::shared::CONTROL_RADIUS),
                            key: Some(OPERATION_INPUT_KEY.to_owned()),
                            ..DraftInputProps::default()
                        },
                        commit_operation,
                        site,
                    )
                })
            })
            .transpose()?;
        AppShell::render_view(build, &props, search_input, operation_input)
    }
}

impl AppController<Intent> for DesktopDemoController {
    fn render(
        &mut self,
        build: &mut ViewBuild<Intent>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<Intent>> {
        self.render_shell(build, ctx)
    }

    fn handle_action(&mut self, intent: Intent) -> ControllerOutcome {
        // 操作模态期间的意图门控：模态外的动作全部让路。
        if self.session.operation.is_some() && !intent_allowed_during_operation(&intent) {
            return ControllerOutcome::changed(false);
        }
        // 详情内容被整体替换时归零旧滚动偏移（键来自渲染期学到的容器发现序）。
        let scroll_resets = if intent_replaces_detail_content(&intent) {
            self.detail_scroll_key
                .clone()
                .map(|key| vec![key])
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        apply_intent(&mut self.model, &mut self.session, intent);
        ControllerOutcome {
            changed: true,
            effects: Vec::new(),
            scroll_resets,
        }
    }

    fn modal_key(&self) -> Option<SemanticKey> {
        self.session
            .operation
            .is_some()
            .then(|| SemanticKey(OPERATION_MODAL_KEY.to_owned()))
    }

    fn anchor_actions(&mut self) -> Vec<(SemanticKey, Intent)> {
        desktop_action_bindings(&self.model, &self.session)
    }

    fn on_kernel_interaction(&mut self, interaction: &KernelInteraction) -> ControllerOutcome {
        match interaction {
            KernelInteraction::CloseModal { .. } if self.session.operation.is_some() => {
                self.handle_action(Intent::CancelOperation)
            }
            KernelInteraction::ShortcutActivated {
                shortcut_id: ShortcutId::Undo,
            } if self.session.operation.is_none() => {
                self.handle_action(Intent::Command(FileCommand::Undo))
            }
            _ => ControllerOutcome::changed(false),
        }
    }
}

/// 供测试断言使用的树探针：当前帧里是否存在某个语义键。
pub fn tree_contains_key(tree: &UiTree, key: &str) -> bool {
    tree.keys().iter().any(|candidate| candidate.0 == key)
}

/// 读取详情滚动容器当前偏移（测试辅助）。
pub fn detail_scroll_offset(
    application: &tela_app_runtime::Application<Intent, DesktopDemoController>,
) -> f32 {
    application
        .scroll_keys()
        .get(1)
        .map(|key| application.view_state().scroll(key).offset_y)
        .unwrap_or_default()
}

/// 重置详情滚动容器（测试辅助；等价于旧运行时的 reset_detail_scroll）。
pub fn reset_detail_scroll(
    application: &mut tela_app_runtime::Application<Intent, DesktopDemoController>,
) {
    if let Some(key) = application.scroll_keys().get(1).cloned() {
        application.set_scroll(key, ScrollState::default());
    }
}
