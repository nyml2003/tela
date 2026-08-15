//! 文件操作 modal：受控输入、确认与取消。

use tela_contract::{
    BorderRadius, Color, Fill, InteractConcern, KeymapScopeId, LayoutConcern, OverlaySpec,
    SemanticKey, ShortcutScopeSpec, Size, StackAlign, UiNode, VisualConcern,
};
use tela_core::builder::{LayoutContainer, LogicalContainer, Primitive};
use tela_ui::{DraftInput, DraftInputSnapshot};

use crate::domain::{FileManagerSession, OperationKind};

use super::shared::{
    BORDER, BORDER_WIDTH, CONTROL_RADIUS, SECONDARY, SHELL_RADIUS, SURFACE, TEXT, command_button,
    fixed, text,
};

pub const OPERATION_MODAL_KEY: &str = "operation-modal";

pub fn operation_modal(
    session: &FileManagerSession,
    input: Option<DraftInputSnapshot>,
    input_focused: bool,
    width: f32,
    height: f32,
) -> UiNode {
    let Some(operation) = &session.operation else {
        return LayoutContainer::row(Vec::<UiNode>::new()).into();
    };
    let title = match operation.kind {
        OperationKind::NewFolder => "新建文件夹",
        OperationKind::Rename => "重命名",
        OperationKind::MoveToDesign => "移动到目录",
        OperationKind::AddTag => "添加标签",
        OperationKind::Trash => "移至回收站",
    };
    let needs_input = !matches!(
        operation.kind,
        OperationKind::MoveToDesign | OperationKind::Trash
    );
    let message = match operation.kind {
        OperationKind::MoveToDesign => "将选中项目移动到“设计”目录？",
        OperationKind::Trash => "将选中项目移至回收站？",
        _ => "确认后才会写入内存工作区。",
    };
    let mut controls = vec![text(title, 16.0, TEXT), text(message, 13.0, SECONDARY)];
    if needs_input {
        controls.push(fixed(
            DraftInput::new(
                input.expect("有文本操作时必须同步 DraftInput 快照"),
                "operation.value",
            )
            .placeholder("输入名称")
            .focused(input_focused)
            .border_radius(CONTROL_RADIUS)
            .into_node(),
            300.0,
            32.0,
        ));
    }
    controls.push(
        LayoutContainer::row([
            LayoutContainer::spacer().into(),
            command_button("取消", 64.0, "operation.cancel", false, false),
            command_button("确认", 64.0, "operation.confirm", false, false),
        ])
        .layout(LayoutConcern {
            width: Some(Size::percent(1.0)),
            gap: 8.0,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into(),
    );
    let controls: UiNode = LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        id: KeymapScopeId("file-manager.operation".to_owned()),
    })
    .children(controls)
    .into();
    let mut panel: UiNode = LayoutContainer::column([controls])
        .layout(LayoutConcern {
            width: Some(Size::fixed(360.0)),
            height: Some(Size::fixed(if needs_input { 220.0 } else { 180.0 })),
            padding: tela_contract::Insets::all(22.0),
            border_width: BORDER_WIDTH,
            gap: 16.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            border_color: Some(BORDER),
            border_radius: BorderRadius::all(SHELL_RADIUS),
            ..VisualConcern::default()
        })
        .into();
    panel.identity = Some(tela_contract::IdentityConcern {
        semantic_key: Some(SemanticKey(OPERATION_MODAL_KEY.to_owned())),
        ..tela_contract::IdentityConcern::default()
    });
    let backdrop_surface: UiNode = LayoutContainer::frame(Primitive::rect())
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::rgba(0.02, 0.04, 0.08, 0.42))),
            ..VisualConcern::default()
        })
        .into();
    let backdrop: UiNode = LayoutContainer::stack([
        backdrop_surface,
        LayoutContainer::overlay(
            panel,
            OverlaySpec {
                align: StackAlign::Center,
                ..OverlaySpec::default()
            },
        )
        .into(),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(height)),
        ..LayoutConcern::default()
    })
    .into();
    let mut backdrop: UiNode = LayoutContainer::overlay(
        backdrop,
        OverlaySpec {
            fill_width: true,
            fill_height: true,
            ..OverlaySpec::default()
        },
    )
    .into();
    backdrop.interact = Some(InteractConcern {
        modal: true,
        ..InteractConcern::default()
    });
    backdrop
}
