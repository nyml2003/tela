//! 共享会话运行时（`tela-app_runtime::Application` + `DesktopDemoController`）上的
//! 行为回归。几何/墨心/光栅断言与旧运行时逐字保持；访问器映射到共享运行时的新入口。

use std::collections::BTreeSet;

use super::controller::{DesktopDemoController, FOCUS_APPEARANCE, demo_config};
use super::{Intent, apply_intent};
use crate::domain::FileCommand;
use crate::presentation::shared::{
    APP_INSET, BORDER, BORDER_WIDTH, SHELL_BOTTOM_RADIUS, SHELL_TOP_RADIUS, STATUS_BAR_H, SURFACE,
    TOP_BAR_H,
};
use tela_app_runtime::Application;
use tela_contract::{
    Color, IconName, PointerEvent, ScrollState, SemanticKey, UiFrame, UiResources, Viewport,
};
use tela_icon_resources::MaterialIconFontProvider;
use tela_render_raster::{RasterConfig, render_frame};
use tela_text_resources::ControlledTextMeasurer;
use tela_ui_foundation::Icon;

static TEST_TEXT_MEASURER: ControlledTextMeasurer = ControlledTextMeasurer;
static TEST_ICON_PROVIDER: MaterialIconFontProvider = MaterialIconFontProvider;
static TEST_RESOURCES: TestResources = TestResources;

struct TestResources;

impl UiResources for TestResources {
    fn text_measurer(&self) -> &dyn tela_contract::TextMeasurer {
        &TEST_TEXT_MEASURER
    }

    fn icon_provider(&self) -> &dyn tela_contract::IconProvider {
        &TEST_ICON_PROVIDER
    }
}

type DemoApplication = Application<Intent, DesktopDemoController>;

fn app() -> DemoApplication {
    Application::new(
        &TEST_RESOURCES,
        DesktopDemoController::new(&TEST_RESOURCES),
        demo_config(),
    )
}

/// 构建候选并驱动呈现到收敛：组件 Output 引发的连锁失效也一并提交。
fn ensure_and_present(application: &mut DemoApplication) {
    assert!(application.ensure_frame());
    let mut guard = 0;
    while application.frame_presented() {
        guard += 1;
        assert!(guard < 8, "呈现循环必须收敛");
        assert!(application.ensure_frame());
    }
}

fn active_frame(application: &DemoApplication) -> &UiFrame {
    application
        .active()
        .map(|(_tree, frame)| frame)
        .expect("呈现后必须有 active 帧")
}

fn frame_trace(application: &DemoApplication) -> String {
    crate::frame_trace::to_json(active_frame(application))
}

fn detail_scroll_key(application: &DemoApplication) -> SemanticKey {
    application
        .scroll_keys()
        .get(1)
        .cloned()
        .expect("详情区应拥有 core 分配的滚动 key")
}

fn click_semantic_key(application: &mut DemoApplication, key: &str) {
    application.ensure_frame();
    let (node_id, position) = {
        let (tree, frame) = application.active().expect("呈现后必须有 active 帧");
        let node_id = tree
            .node_id_for_key(&SemanticKey(key.to_owned()))
            .expect("交互动作键应存在");
        let hit = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("交互动作键应有命中区");
        (
            node_id,
            tela_contract::Point {
                x: hit.rect.x + hit.rect.w.min(320.0) / 2.0,
                y: hit.rect.y + hit.rect.h / 2.0,
            },
        )
    };
    let _ = node_id;
    application.handle_pointer(PointerEvent::mouse_down(position));
    application.handle_pointer(PointerEvent::mouse_up(position));
    ensure_and_present(application);
}

fn visible_ink_center(command: &tela_contract::DrawCommand) -> f32 {
    let tela_contract::DrawPayload::Text { text, baseline_y } = &command.payload else {
        panic!("只应对文本命令计算墨迹中心");
    };
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    tela_text_resources::rasterize_glyphs(
        text,
        tela_text_resources::GlyphRasterOptions {
            origin_x: command.geometry.x,
            baseline_y: *baseline_y,
            scale: 1.0,
            wrap_width: command.geometry.w,
        },
        |event| {
            if let tela_text_resources::GlyphRasterEvent::Coverage { y, coverage, .. } = event
                && coverage > 0.0
            {
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        },
    );
    assert!(min_y <= max_y, "文本必须产生可见墨迹");
    (min_y + max_y) as f32 * 0.5
}

fn icon_glyph(name: IconName) -> String {
    let node = Icon::new(name)
        .resolve_with(&TEST_ICON_PROVIDER)
        .expect("test resources must cover standard icons")
        .into_node();
    match node.content {
        Some(tela_contract::ContentConcern::Text(text)) => text.text,
        other => panic!("material icon must lower to text, got {other:?}"),
    }
}

#[test]
fn file_manager_shell_is_full_viewport_and_contains_client_regions() {
    let mut application = app();
    application.set_viewport(1440.0, 900.0, 1.0);
    ensure_and_present(&mut application);
    assert_eq!(
        active_frame(&application).viewport,
        Viewport {
            width: 1440.0,
            height: 900.0
        }
    );
    let labels: Vec<String> = active_frame(&application)
        .commands
        .iter()
        .filter_map(|command| match &command.payload {
            tela_contract::DrawPayload::Text { text, .. } => Some(text.text.clone()),
            _ => None,
        })
        .collect();
    for label in ["TELA 文件", "新建", "工作区", "README.md"] {
        assert!(labels.contains(&label.to_owned()), "缺少 {label}");
    }
}

#[test]
fn desktop_shell_remains_visible_on_the_weak_raster_fallback() {
    let mut application = app();
    application.set_viewport(960.0, 640.0, 1.0);
    ensure_and_present(&mut application);

    let bitmap = render_frame(
        active_frame(&application),
        &RasterConfig::default_with(Color::rgba(1.0, 1.0, 1.0, 1.0)),
    );
    assert_eq!((bitmap.width, bitmap.height), (960, 640));
    assert!(
        bitmap
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel != [255, 255, 255, 255]),
        "桌面业务视图不能退化为空白 raster"
    );

    // raster 只保留弱宿主可见性诊断；视觉 golden 由 WGPU 离屏回读负责。
}

#[test]
fn client_shell_insets_and_rounds_its_chrome_without_shrinking_the_viewport() {
    let mut application = app();
    application.set_viewport(1440.0, 900.0, 1.0);
    ensure_and_present(&mut application);

    let top_bar = active_frame(&application)
        .commands
        .iter()
        .find_map(|command| match &command.payload {
            tela_contract::DrawPayload::RoundedRect {
                fill: Some(fill),
                border: Some(border),
                radius,
            } if matches!(fill, tela_contract::Fill::Linear(_))
                && border.color == BORDER
                && border.width == BORDER_WIDTH
                && *radius == SHELL_TOP_RADIUS
                && (command.geometry.y - APP_INSET).abs() <= f32::EPSILON
                && (command.geometry.h - TOP_BAR_H).abs() <= f32::EPSILON =>
            {
                Some(command.geometry)
            }
            _ => None,
        })
        .expect("正常视口的顶栏必须以带圆角的客户端外框绘制");
    let status_bar = active_frame(&application)
        .commands
        .iter()
        .find_map(|command| match &command.payload {
            tela_contract::DrawPayload::RoundedRect {
                fill: Some(fill),
                border: Some(border),
                radius,
            } if *fill == tela_contract::Fill::Solid(SURFACE)
                && border.color == BORDER
                && border.width == BORDER_WIDTH
                && *radius == SHELL_BOTTOM_RADIUS
                && (command.geometry.h - STATUS_BAR_H).abs() <= f32::EPSILON =>
            {
                Some(command.geometry)
            }
            _ => None,
        })
        .expect("正常视口的状态栏必须闭合客户端外框的底部圆角");

    assert!((top_bar.x - APP_INSET).abs() <= f32::EPSILON);
    assert!((top_bar.w - (1440.0 - APP_INSET * 2.0)).abs() <= f32::EPSILON);
    assert!((status_bar.x - APP_INSET).abs() <= f32::EPSILON);
    assert!((status_bar.y + status_bar.h - (900.0 - APP_INSET)).abs() <= f32::EPSILON);
    assert_eq!(
        active_frame(&application).viewport,
        Viewport {
            width: 1440.0,
            height: 900.0,
        },
        "客户端留白只能作用于应用工作区，不能缩小 Canvas 的逻辑视口",
    );
}

#[test]
fn hero_image_icon_uses_its_full_layout_box_without_overflow() {
    let mut application = app();
    application.set_viewport(2048.0, 488.0, 1.0);
    ensure_and_present(&mut application);

    let image_icon = icon_glyph(IconName::Image);
    let (geometry, baseline_y, text) = active_frame(&application)
        .commands
        .iter()
        .find_map(|command| match &command.payload {
            tela_contract::DrawPayload::Text { text, baseline_y }
                if text.text == image_icon
                    && text.font.as_str() == tela_contract::TextStyleRef::ICON =>
            {
                Some((command.geometry, *baseline_y, text.clone()))
            }
            _ => None,
        })
        .expect("根目录应显示 hero.png 的图片图标");
    assert_eq!(
        geometry.h, text.line_height,
        "图片图标的布局盒不得被表格单元格压缩"
    );
    let top = geometry.y.floor() as i32;
    let bottom = (geometry.y + geometry.h).ceil() as i32;
    let mut ink_pixels = Vec::new();
    let mut overflow_pixels = Vec::new();
    tela_text_resources::rasterize_glyphs(
        &text,
        tela_text_resources::GlyphRasterOptions {
            origin_x: geometry.x,
            baseline_y,
            scale: 1.0,
            wrap_width: geometry.w,
        },
        |event| {
            if let tela_text_resources::GlyphRasterEvent::Coverage { x, y, coverage } = event
                && coverage > 0.75
            {
                ink_pixels.push((x, y));
                if y < top || y >= bottom {
                    overflow_pixels.push((x, y));
                }
            }
        },
    );
    assert!(
        overflow_pixels.is_empty(),
        "完整 20px 图标行盒内不应再有溢出墨迹: {overflow_pixels:?}"
    );
    assert!(!ink_pixels.is_empty(), "图片图标必须产生可见墨迹");
}

#[test]
fn brand_icon_and_label_align_their_visible_ink_centers() {
    let mut application = app();
    ensure_and_present(&mut application);

    let brand_icon = icon_glyph(IconName::FolderOpen);
    let commands: Vec<_> = active_frame(&application)
        .commands
        .iter()
        .filter(|command| {
            matches!(
                &command.payload,
                tela_contract::DrawPayload::Text { text, .. }
                    if (text.text == brand_icon
                        && text.font.as_str() == tela_contract::TextStyleRef::ICON)
                        || text.text == "TELA 文件"
            )
        })
        .collect();
    assert_eq!(commands.len(), 2, "品牌应只产生一个图标与一个标题");

    let icon_center = visible_ink_center(commands[0]);
    let label_center = visible_ink_center(commands[1]);
    assert!(
        (icon_center - label_center).abs() <= 3.0,
        "品牌图标和标题的可见中心应对齐: {icon_center} != {label_center}"
    );
}

#[test]
fn navigation_icon_and_label_align_their_visible_ink_centers() {
    let mut application = app();
    ensure_and_present(&mut application);

    let folder = icon_glyph(IconName::Folder);
    let label = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            command.geometry.x < 264.0
                && matches!(&command.payload,
                    tela_contract::DrawPayload::Text { text, .. } if text.text == "设计")
        })
        .expect("侧栏应显示设计标签");
    let icon = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            command.geometry.x < label.geometry.x
                && (command.geometry.y - label.geometry.y).abs() <= 4.0
                && matches!(
                    &command.payload,
                    tela_contract::DrawPayload::Text { text, .. }
                        if text.text == folder
                            && text.font.as_str() == tela_contract::TextStyleRef::ICON
                )
        })
        .expect("设计标签同一行应显示文件夹图标");

    let icon_center = visible_ink_center(icon);
    let label_center = visible_ink_center(label);
    assert!(
        (icon_center - label_center).abs() <= 3.0,
        "导航图标和标题的可见中心应对齐: {icon_center} != {label_center}"
    );
}

#[test]
fn file_list_icon_and_label_align_their_visible_ink_centers() {
    let mut application = app();
    application.controller_mut().session.current_dir = 3;
    ensure_and_present(&mut application);

    let document = icon_glyph(IconName::Document);
    let label = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            matches!(&command.payload,
                tela_contract::DrawPayload::Text { text, .. } if text.text == "layout.rs")
        })
        .expect("源码目录应显示 layout.rs");
    let icon = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            command.geometry.x < label.geometry.x
                && (command.geometry.y - label.geometry.y).abs() <= 4.0
                && matches!(
                    &command.payload,
                    tela_contract::DrawPayload::Text { text, .. }
                        if text.text == document
                            && text.font.as_str() == tela_contract::TextStyleRef::ICON
                )
        })
        .expect("layout.rs 同一行应显示文本文档图标");

    let icon_center = visible_ink_center(icon);
    let label_center = visible_ink_center(label);
    assert!(
        (icon_center - label_center).abs() <= 3.0,
        "文件列表图标和标题的可见中心应对齐: {icon_center} != {label_center}"
    );
}

#[test]
fn focused_file_row_centers_its_visible_content_inside_the_focus_ring() {
    let mut application = app();
    ensure_and_present(&mut application);
    let key = application
        .active()
        .expect("呈现后必须有 active 帧")
        .0
        .focusable_nodes()
        .into_iter()
        .find(|(key, _)| key == &SemanticKey("entry-3".to_owned()))
        .map(|(key, _)| key)
        .expect("源码目录行应使用虚拟列表的稳定业务 key");
    application.set_current_focus_key(Some(key));
    ensure_and_present(&mut application);

    let folder = icon_glyph(IconName::Folder);
    let label = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            command.geometry.x > 264.0
                && matches!(&command.payload,
                    tela_contract::DrawPayload::Text { text, .. } if text.text == "源码")
        })
        .expect("文件列表应显示源码目录标签");
    let icon = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            command.geometry.x < label.geometry.x
                && (command.geometry.y - label.geometry.y).abs() <= 4.0
                && matches!(
                    &command.payload,
                    tela_contract::DrawPayload::Text { text, .. }
                        if text.text == folder
                            && text.font.as_str() == tela_contract::TextStyleRef::ICON
                )
        })
        .expect("文件列表源码目录同一行应显示文件夹图标");
    let focus_ring = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            command.geometry.y <= icon.geometry.y
                && command.geometry.y + command.geometry.h >= icon.geometry.y + icon.geometry.h
                && matches!(
                    &command.payload,
                    tela_contract::DrawPayload::RoundedRect {
                        fill: None,
                        border: Some(border),
                        ..
                    } if border.color == FOCUS_APPEARANCE.color
                        && border.width == FOCUS_APPEARANCE.width
                )
        })
        .expect("聚焦文件行必须投影自身的 FocusRing");

    let focus_radius = match &focus_ring.payload {
        tela_contract::DrawPayload::RoundedRect { radius, .. } => *radius,
        _ => unreachable!("FocusRing 已按 RoundedRect 筛选"),
    };
    assert_eq!(
        focus_radius,
        tela_contract::BorderRadius::all(crate::presentation::shared::ROW_RADIUS),
        "焦点环必须继承文件行圆角，不能退化为矩形",
    );

    let ring_center = focus_ring.geometry.y + focus_ring.geometry.h / 2.0;
    for (name, command) in [("图标", icon), ("文字", label)] {
        let ink_center = visible_ink_center(command);
        assert!(
            (ink_center - ring_center).abs() <= 1.0,
            "焦点行{name}的可见中心应位于 FocusRing 中心: {ink_center} != {ring_center}"
        );
    }
}

#[test]
fn toolbar_icon_and_label_align_their_visible_ink_centers() {
    let mut application = app();
    ensure_and_present(&mut application);

    let add = icon_glyph(IconName::Add);
    let label = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            matches!(&command.payload,
                tela_contract::DrawPayload::Text { text, .. } if text.text == "新建")
        })
        .expect("工具栏应显示新建标签");
    let icon = active_frame(&application)
        .commands
        .iter()
        .find(|command| {
            command.geometry.x < label.geometry.x
                && (command.geometry.y - label.geometry.y).abs() <= 4.0
                && matches!(
                    &command.payload,
                    tela_contract::DrawPayload::Text { text, .. }
                        if text.text == add
                            && text.font.as_str() == tela_contract::TextStyleRef::ICON
                )
        })
        .expect("新建标签同一行应显示新增图标");

    let icon_center = visible_ink_center(icon);
    let label_center = visible_ink_center(label);
    assert!(
        (icon_center - label_center).abs() <= 1.0,
        "工具栏图标和标签的可见中心应对齐: {icon_center} != {label_center}"
    );
}

#[test]
fn selecting_a_text_file_switches_to_read_only_preview() {
    let mut application = app();
    application.controller_mut().session.select(5);
    ensure_and_present(&mut application);
    let trace = frame_trace(&application);
    assert!(trace.contains("Tela 工作区"));
    assert!(trace.contains("只读说明"));
}

#[test]
fn controller_executes_memory_commands_without_manual_tela_keys() {
    let mut application = app();
    let controller = application.controller_mut();
    controller.session.select(5);
    controller
        .model
        .apply(&mut controller.session, FileCommand::CopySelected);
    let copied = *controller.session.selected.iter().next().unwrap();
    assert_ne!(copied, 5);
    controller
        .model
        .apply(&mut controller.session, FileCommand::Undo);
    assert!(controller.model.entry(copied).is_none());
}

#[test]
fn typed_intents_update_selection_navigation_and_commands() {
    let mut application = app();
    assert!(application.dispatch_action(Intent::Select(5)));
    assert_eq!(
        application.controller().session.selected,
        BTreeSet::from([5])
    );
    assert!(application.dispatch_action(Intent::Command(FileCommand::ToggleView)));
    assert_eq!(
        application.controller().session.view,
        crate::domain::DirectoryView::Grid
    );
    assert!(application.dispatch_action(Intent::SetFilter(crate::domain::EntryFilter::Favorites)));
    assert_eq!(
        application.controller().session.filter,
        crate::domain::EntryFilter::Favorites
    );
    assert!(application.dispatch_action(Intent::OpenFolder(2)));
    assert_eq!(application.controller().session.current_dir, 2);
    assert_eq!(
        application.controller().session.filter,
        crate::domain::EntryFilter::All
    );
    assert!(application.dispatch_action(Intent::BeginOperation(
        crate::domain::OperationKind::NewFolder,
    )));
    assert!(application.dispatch_action(Intent::ConfirmOperation));
    assert!(
        application
            .controller()
            .model
            .entries_in_filtered(
                2,
                "",
                application.controller().session.filter,
                application.controller().session.sort
            )
            .iter()
            .any(|entry| entry.name.starts_with("新建文件夹"))
    );
}

#[test]
fn opening_a_short_directory_resets_detail_scroll_and_keeps_all_rows_inside_its_clip() {
    let mut application = app();
    application.set_viewport(1280.0, 320.0, 1.0);
    ensure_and_present(&mut application);
    let detail_key = detail_scroll_key(&application);
    let root_max = active_frame(&application)
        .scroll_bounds
        .iter()
        .find(|bounds| bounds.key == detail_key)
        .map(|bounds| bounds.max_offset_y)
        .expect("详情虚拟列表应报告滚动边界");
    assert!(root_max > 0.0, "短视口下根目录应能滚动");

    application.set_scroll(
        detail_key.clone(),
        ScrollState {
            offset_x: 0.0,
            offset_y: root_max,
        },
    );
    ensure_and_present(&mut application);

    assert!(application.dispatch_action(Intent::OpenFolder(2)));
    assert_eq!(application.controller().session.current_dir, 2);
    assert_eq!(
        application.view_state().scroll(&detail_key),
        ScrollState::default()
    );
    ensure_and_present(&mut application);

    let detail_bounds = active_frame(&application)
        .scroll_bounds
        .iter()
        .find(|bounds| bounds.key == detail_key)
        .expect("切换后的详情列表仍应报告滚动边界");
    assert_eq!(
        detail_bounds.max_offset_y, 0.0,
        "两项短目录不可保留滚动范围"
    );
    assert_eq!(
        application
            .controller()
            .model
            .entries_in_filtered(
                2,
                "",
                application.controller().session.filter,
                application.controller().session.sort
            )
            .len(),
        2,
        "设计目录只显示直接子项"
    );
    for name in ["icons.svg", "tokens.json"] {
        let command = active_frame(&application)
            .commands
            .iter()
            .find(|command| {
                matches!(&command.payload,
                    tela_contract::DrawPayload::Text { text, .. } if text.text == name)
            })
            .unwrap_or_else(|| panic!("切换目录后应显示 {name}"));
        assert!(
            command.geometry.y >= detail_bounds.viewport.y,
            "{name} 不得被旧滚动偏移推到详情 clip 顶部之外"
        );
        assert!(
            command.geometry.y + command.geometry.h
                <= detail_bounds.viewport.y + detail_bounds.viewport.h,
            "{name} 必须完整位于详情可视区域"
        );
    }
}

#[test]
fn viewport_breakpoints_keep_a_fixed_client_root() {
    let mut application = app();
    for (width, height) in [(1440.0, 900.0), (1199.0, 800.0), (899.0, 720.0)] {
        application.set_viewport(width, height, 1.0);
        ensure_and_present(&mut application);
        assert_eq!(
            active_frame(&application).viewport,
            Viewport { width, height }
        );
        assert!(
            active_frame(&application)
                .commands
                .iter()
                .any(|command| matches!(&command.payload,
            tela_contract::DrawPayload::Text { text, .. } if text.text == "TELA 文件"))
        );
    }
}

#[test]
fn operation_modal_requires_confirm_and_writes_its_controlled_draft() {
    let mut application = app();
    assert!(application.dispatch_action(Intent::BeginOperation(
        crate::domain::OperationKind::NewFolder,
    )));
    assert_eq!(
        application
            .controller()
            .session
            .operation
            .as_ref()
            .map(|draft| &draft.value),
        Some(&"新建文件夹".to_owned())
    );
    ensure_and_present(&mut application);
    assert_eq!(application.set_input_value("验收目录".to_owned()), 1);
    assert_eq!(application.input_enter(), 1);
    // 组件 Output 是事务性的：present 才排空（真实宿主每个事件后都发布呈现）。
    ensure_and_present(&mut application);
    assert!(
        application
            .controller()
            .model
            .entries_in_filtered(
                1,
                "",
                application.controller().session.filter,
                application.controller().session.sort
            )
            .iter()
            .all(|entry| entry.name != "验收目录")
    );
    assert!(application.dispatch_action(Intent::ConfirmOperation));
    assert!(application.controller().session.operation.is_none());
    assert!(
        application
            .controller()
            .model
            .entries_in_filtered(
                1,
                "",
                application.controller().session.filter,
                application.controller().session.sort
            )
            .iter()
            .any(|entry| entry.name == "验收目录")
    );
    assert!(
        application.dispatch_action(Intent::BeginOperation(crate::domain::OperationKind::Rename,))
    );
    assert!(application.dispatch_action(Intent::CancelOperation));
    assert!(application.controller().session.operation.is_none());
    assert_eq!(application.controller().session.notice, "已取消操作");
}

#[test]
fn operation_draft_commits_at_boundaries_and_does_not_survive_a_reopen() {
    let mut application = app();
    assert!(application.dispatch_action(Intent::BeginOperation(
        crate::domain::OperationKind::NewFolder,
    )));
    ensure_and_present(&mut application);
    assert_eq!(application.set_input_value("仅本地草稿".to_owned()), 1);
    assert_eq!(
        application
            .controller()
            .session
            .operation
            .as_ref()
            .map(|draft| draft.value.as_str()),
        Some("新建文件夹")
    );
    assert_eq!(application.composition_start(), 1);
    assert_eq!(application.input_enter(), 0, "IME 组合期间不能提交");
    assert_eq!(application.composition_end(), 1);
    assert_eq!(application.input_blur(), 1);
    // blur 的 Commit Output 在 present 时排空并写入业务草稿。
    ensure_and_present(&mut application);
    assert_eq!(
        application
            .controller()
            .session
            .operation
            .as_ref()
            .map(|draft| draft.value.as_str()),
        Some("仅本地草稿")
    );
    assert!(application.dispatch_action(Intent::CancelOperation));
    assert!(
        application.dispatch_action(Intent::BeginOperation(crate::domain::OperationKind::AddTag,))
    );
    ensure_and_present(&mut application);
    assert_eq!(application.input_value(), "重点");
    assert_eq!(application.set_input_value("临时标签".to_owned()), 1);
    assert_eq!(application.input_cancel(), 1);
    assert_eq!(application.input_value(), "重点");
    assert!(application.dispatch_action(Intent::CancelOperation));
    assert!(
        application.dispatch_action(Intent::BeginOperation(crate::domain::OperationKind::AddTag,))
    );
    ensure_and_present(&mut application);
    assert_eq!(application.input_value(), "重点");

    assert!(application.dispatch_action(Intent::CancelOperation));
    application.controller_mut().session.select(5);
    assert!(
        application.dispatch_action(Intent::BeginOperation(crate::domain::OperationKind::Rename,))
    );
    ensure_and_present(&mut application);
    assert_eq!(
        application.set_input_value("README-已重命名.md".to_owned()),
        1
    );
    assert_eq!(application.input_enter(), 1);
    ensure_and_present(&mut application);
    assert!(application.dispatch_action(Intent::ConfirmOperation));
    assert_eq!(
        application
            .controller()
            .model
            .entry(5)
            .map(|entry| entry.name.as_str()),
        Some("README-已重命名.md")
    );
}

#[test]
fn narrow_navigation_overlays_instead_of_shrinking_the_detail_pane() {
    fn readme_x(application: &DemoApplication) -> f32 {
        active_frame(application)
            .commands
            .iter()
            .find_map(|command| match &command.payload {
                tela_contract::DrawPayload::Text { text, .. } if text.text == "README.md" => {
                    Some(command.geometry.x)
                }
                _ => None,
            })
            .expect("README 应显示")
    }
    let mut application = app();
    application.set_viewport(1199.0, 800.0, 1.0);
    ensure_and_present(&mut application);
    let before = readme_x(&application);
    assert!(application.dispatch_action(Intent::ToggleNavigation));
    ensure_and_present(&mut application);
    assert_eq!(readme_x(&application), before);
    let trace = frame_trace(&application);
    assert!(trace.contains("文件夹"), "窄屏抽屉应显示目录树");
}

#[test]
fn canvas_hit_testing_routes_semantic_actions_through_core() {
    let mut application = app();
    ensure_and_present(&mut application);
    assert!(active_frame(&application).hit_regions.iter().all(|region| {
        region.rect.x.is_finite()
            && region.rect.y.is_finite()
            && region.rect.w.is_finite()
            && region.rect.h.is_finite()
    }));
    click_semantic_key(&mut application, "entry-5");
    assert_eq!(
        application.controller().session.selected,
        BTreeSet::from([5])
    );
    click_semantic_key(&mut application, "folder.open.1");
    assert_eq!(application.controller().session.current_dir, 1);
    click_semantic_key(&mut application, "folder.open.2");
    assert_eq!(application.controller().session.current_dir, 2);
    click_semantic_key(&mut application, "entry-8");
    click_semantic_key(&mut application, "command.rename");
    assert!(application.controller().session.operation.is_some());
    click_semantic_key(&mut application, "operation.confirm");
    assert!(application.controller().session.operation.is_none());
    assert_eq!(
        application
            .controller()
            .model
            .entry(5)
            .expect("README 存在")
            .name,
        "README.md"
    );
}

#[test]
fn toolbar_hover_is_projected_from_core_view_state_by_semantic_action_key() {
    let mut application = app();
    ensure_and_present(&mut application);
    assert!(
        !application.on_animation_tick(5_000),
        "空闲时钟同步不应产生新帧"
    );
    let position = {
        let (tree, frame) = application.active().expect("呈现后必须有 active 帧");
        let node_id = tree
            .node_id_for_key(&SemanticKey("command.new-folder".to_owned()))
            .expect("Toolbar 新建项应存在");
        let hit = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("Toolbar 新建项应可命中");
        tela_contract::Point {
            x: hit.rect.x + 1.0,
            y: hit.rect.y + 1.0,
        }
    };
    application.handle_pointer(PointerEvent::mouse_move(position));
    ensure_and_present(&mut application);
    assert_eq!(
        application
            .controller()
            .hovered_action_key()
            .map(|key| key.0.as_str()),
        Some("command.new-folder")
    );
    assert!(application.animation_schedule().active);
    assert!(application.on_animation_tick(5_080));
    ensure_and_present(&mut application);
    assert!(application.animation_schedule().active);
    application.handle_pointer(PointerEvent::mouse_move(tela_contract::Point {
        x: -1.0,
        y: -1.0,
    }));
    ensure_and_present(&mut application);
    assert_eq!(
        application.controller().hovered_action_key(),
        None,
        "离开必须恢复状态栏投影"
    );
    assert!(
        application.animation_schedule().active,
        "离开时应从当前插值值 retarget"
    );
    assert!(application.on_animation_tick(5_320));
    ensure_and_present(&mut application);
    assert!(!application.animation_schedule().active);
}

#[test]
fn unloading_a_hovered_toolbar_node_clears_the_status_projection() {
    let mut application = app();
    application.controller_mut().session.select(5);
    ensure_and_present(&mut application);
    let position = {
        let (tree, frame) = application.active().expect("呈现后必须有 active 帧");
        let node_id = tree
            .node_id_for_key(&SemanticKey("command.rename".to_owned()))
            .expect("选中项目后 Toolbar 重命名项应存在");
        let hit = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("Toolbar 重命名项应可命中");
        tela_contract::Point {
            x: hit.rect.x + 1.0,
            y: hit.rect.y + 1.0,
        }
    };
    application.handle_pointer(PointerEvent::mouse_move(position));
    ensure_and_present(&mut application);
    assert_eq!(
        application
            .controller()
            .hovered_action_key()
            .map(|key| key.0.as_str()),
        Some("command.rename")
    );

    application.controller_mut().session.selected.clear();
    application.invalidate_frame();
    ensure_and_present(&mut application);
    assert_eq!(
        application.controller().hovered_action_key(),
        None,
        "已卸载节点的 core hover key 不得继续投影旧状态栏说明"
    );
}

#[test]
fn raw_keyboard_moves_default_focus_and_projects_a_focus_ring() {
    let mut application = app();
    ensure_and_present(&mut application);
    assert_eq!(
        application.handle_key(0x2b, 0, false),
        1,
        "Tab 应被默认键位表消费"
    );
    let first = application
        .view_state()
        .current_focus_key()
        .cloned()
        .expect("Tab 后 core 应持有默认焦点");
    ensure_and_present(&mut application);
    assert!(active_frame(&application).commands.iter().any(|command| {
        matches!(
            &command.payload,
            tela_contract::DrawPayload::RoundedRect {
                fill: None,
                border: Some(border),
                ..
            } if border.color == FOCUS_APPEARANCE.color && border.width == FOCUS_APPEARANCE.width
        )
    }), "焦点变化必须在同一帧投影可见 FocusRing");

    assert_eq!(
        application.handle_key(0x51, 0, false),
        1,
        "ArrowDown 应被默认键位表消费"
    );
    let second = application
        .view_state()
        .current_focus_key()
        .cloned()
        .expect("方向键后应仍有焦点");
    assert_ne!(
        first, second,
        "方向意图由焦点图/树序推进，而不是依赖页面手写 key"
    );
}

#[test]
fn runtime_keymap_replacement_is_atomic_and_changes_the_next_key() {
    let mut application = app();
    ensure_and_present(&mut application);
    let replacement = r#"{
        "version": 1,
        "revision": 2,
        "default_layer": [
            {"key":"KeyA","intent":{"type":"focus_next"}}
        ]
    }"#;
    assert!(application.replace_keymap_json(replacement).is_ok());
    assert_eq!(
        application.handle_key(0x2b, 0, false),
        0,
        "旧 Tab 绑定不应残留"
    );
    assert_eq!(application.handle_key(0x04, 0, false), 1, "新快照立即生效");
    let focused = application.view_state().current_focus_key().cloned();
    assert!(focused.is_some());

    let invalid = r#"{
        "version": 1,
        "revision": 1,
        "default_layer": [
            {"key":"KeyB","intent":{"type":"focus_next"}}
        ]
    }"#;
    assert!(application.replace_keymap_json(invalid).is_err());
    assert_eq!(
        application.handle_key(0x04, 0, false),
        1,
        "拒绝快照后保留旧表"
    );
}

#[test]
fn escape_closes_modal_and_restores_the_saved_background_focus() {
    let mut application = app();
    ensure_and_present(&mut application);
    assert_eq!(application.handle_key(0x2b, 0, false), 1);
    let background_focus = application.view_state().current_focus_key().cloned();
    assert!(application.dispatch_action(Intent::BeginOperation(
        crate::domain::OperationKind::NewFolder,
    )));
    ensure_and_present(&mut application);
    assert!(application.controller().session.operation.is_some());
    let modal_focus = application.view_state().current_focus_key().cloned();
    assert_ne!(
        modal_focus, background_focus,
        "打开模态后 core 自动进入模态焦点域"
    );
    assert_eq!(
        application.handle_key(0x29, 0, false),
        1,
        "Escape 应进入 Cancel 意图"
    );
    assert!(
        application.controller().session.operation.is_none(),
        "Cancel 动作关闭业务模态"
    );
    ensure_and_present(&mut application);
    assert_eq!(
        application.view_state().current_focus_key(),
        background_focus.as_ref()
    );
}

#[test]
fn tab_leaving_a_text_input_returns_arrow_keys_to_the_core_focus_graph() {
    let mut application = app();
    ensure_and_present(&mut application);
    assert_eq!(
        application.handle_key(0x2b, 0, false),
        1,
        "Tab 应进入搜索输入"
    );
    assert!(
        application.input_focused(),
        "当前 core 焦点是输入时才接管 DOM 文本编辑"
    );
    assert_eq!(
        application.input_focus(),
        1,
        "DOM 焦点只记录 core 已判定的输入目标"
    );

    assert_eq!(
        application.handle_key(0x2b, 0, false),
        1,
        "第二次 Tab 应离开输入"
    );
    assert!(
        !application.input_focused(),
        "弹窗或页面存在输入框不等于它仍拥有键盘方向键"
    );
    assert_eq!(
        application.input_blur(),
        0,
        "无草稿时 DOM blur 不应产生业务写入"
    );
    let after_tab = application
        .view_state()
        .current_focus_key()
        .cloned()
        .expect("Tab 后应有下一个焦点目标");

    assert_eq!(
        application.handle_key(0x51, 0, false),
        1,
        "ArrowDown 应重新由默认键位表映射到 core"
    );
    assert_ne!(
        application.view_state().current_focus_key(),
        Some(&after_tab),
        "方向导航不能被已经失焦的隐藏 textarea 吞掉"
    );
}

#[test]
fn modal_keymap_scope_overrides_the_default_snapshot_layer() {
    let mut application = app();
    ensure_and_present(&mut application);
    assert!(application.dispatch_action(Intent::BeginOperation(
        crate::domain::OperationKind::NewFolder,
    )));
    ensure_and_present(&mut application);
    assert!(
        application
            .focused_input_key()
            .is_some_and(|key| key.0 == "operation.value"),
        "默认模态焦点落在首个输入控件"
    );

    let replacement = r#"{
        "version": 1,
        "revision": 2,
        "default_layer": [
            {"key":"KeyA","intent":{"type":"focus_next"}}
        ],
        "scoped_layers": {
            "file-manager.operation": [
                {"key":"KeyA","intent":{"type":"cancel"}}
            ]
        }
    }"#;
    assert!(application.replace_keymap_json(replacement).is_ok());
    assert_eq!(application.handle_key(0x04, 0, false), 1);
    assert!(
        application.controller().session.operation.is_none(),
        "模态内层 KeymapScopeId 必须先于默认层命中"
    );
}

#[test]
fn domain_apply_intent_still_drives_the_model_directly() {
    // 保留对纯域函数的直接回归（不经共享运行时）。
    let mut model = crate::domain::FileManagerModel::sample();
    let mut session = crate::domain::FileManagerSession::default();
    apply_intent(&mut model, &mut session, Intent::Select(5));
    assert_eq!(session.selected, BTreeSet::from([5]));
}
