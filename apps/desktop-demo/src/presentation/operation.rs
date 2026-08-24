//! 文件操作 modal：受控输入、确认与取消。

use tela_contract::{
    Color, Fill, KeymapScopeId, LayoutConcern, Size, StackAlign, UiNode, VisualConcern,
};
use tela_core::builder::{LayoutContainer, Primitive};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult, into_view_child, ui};

use crate::domain::{FileManagerSession, OperationKind};

use super::shared::{
    BORDER, BORDER_WIDTH, SECONDARY, SHELL_RADIUS, SURFACE, TEXT, command_button, text,
};

pub const OPERATION_MODAL_KEY: &str = "operation-modal";

/// 用声明式组件输入构建最终候选 modal；输入的 owner plans 保持在返回值中。
pub fn operation_modal_view<A>(
    build: &mut ViewBuild<A>,
    session: &FileManagerSession,
    input: Option<ViewOutput<A>>,
    width: f32,
    height: f32,
) -> ViewResult<ViewOutput<A>> {
    let operation = session
        .operation
        .as_ref()
        .expect("operation modal view requires an active operation");
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
    let title = into_view_child::<A, UiNode>(text(title, 16.0, TEXT))?;
    let message = into_view_child::<A, UiNode>(text(message, 13.0, SECONDARY))?;
    let actions = into_view_child::<A, UiNode>(
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
    )?;
    let controls = if let Some(input) = input {
        ui!(build {
            <ShortcutScope id={KeymapScopeId("file-manager.operation".to_owned())}>
                { title }
                { message }
                { input }
                { actions }
            </ShortcutScope>
        })?
    } else {
        ui!(build {
            <ShortcutScope id={KeymapScopeId("file-manager.operation".to_owned())}>
                { title }
                { message }
                { actions }
            </ShortcutScope>
        })?
    };
    let panel = ui!(build {
        <Column
            key={OPERATION_MODAL_KEY}
            width={360.0}
            height={if needs_input { 220.0 } else { 180.0 }}
            padding={tela_contract::Insets::all(22.0)}
            border_width={BORDER_WIDTH}
            gap={16.0}
            fill={Fill::Solid(SURFACE)}
            border_color={BORDER}
            border_radius={SHELL_RADIUS}
        >
            { controls }
        </Column>
    })?;
    let backdrop_surface = into_view_child::<A, UiNode>(
        LayoutContainer::frame(Primitive::rect())
            .layout(LayoutConcern {
                width: Some(Size::fixed(width)),
                height: Some(Size::fixed(height)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::rgba(0.02, 0.04, 0.08, 0.42))),
                ..VisualConcern::default()
            })
            .into(),
    )?;
    ui!(build {
        <Overlay fill_width={true} fill_height={true} modal={true}>
            <Stack width={width} height={height}>
                { backdrop_surface }
                <Overlay align={StackAlign::Center}>
                    { panel }
                </Overlay>
            </Stack>
        </Overlay>
    })
}
