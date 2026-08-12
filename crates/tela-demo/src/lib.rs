//! tela 最小浏览器演示。
//!
//! 场景包含页面布局、图片、输入框和按钮；CPU 与 WebGPU 都必须消费同一个
//! `UiNode -> UiTree -> UiFrame` 结果。后端差异只存在于最后的帧提交。

#![cfg_attr(feature = "webgpu", allow(dead_code))]

mod frame_trace;
#[cfg(feature = "webgpu")]
mod wasm;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use tela_contract::{
    BindId, BorderRadius, Color, ContentConcern, Fill, FlexDirection, InputEvent, LayoutConcern,
    NodeId, NodeKind, Point, PointerEvent, ScrollState, SemanticKey, Size, TextMeasureRequest,
    TextMeasurer, TextMetrics, TextureRef, UiAction, UiFrame, UiNode, Value, Viewport,
    VisualConcern,
};
use tela_core::builder::{LayoutContainer, Primitive};
use tela_core::{UiTree, ViewStateStore, handle_input};
use tela_widgets::{Button, ButtonVariant, ImageBackground, Input, Table, Td, Tr};

/// 所有后端共享的逻辑画布尺寸。
pub const VIEWPORT: Viewport = Viewport {
    width: 800.0,
    height: 600.0,
};

const HEADER: Color = Color::rgba(0.12, 0.31, 0.58, 1.0);
const SIDEBAR: Color = Color::rgba(0.90, 0.36, 0.20, 1.0);
const CLIP_OVERFLOW: Color = Color::rgba(0.95, 0.70, 0.18, 1.0);
const MAIN: Color = Color::rgba(0.16, 0.60, 0.43, 1.0);
const MAIN_BORDER: Color = Color::rgba(0.05, 0.24, 0.17, 1.0);
const FOOTER: Color = Color::rgba(0.22, 0.25, 0.31, 1.0);
const SIDEBAR_WIDTH: f32 = 160.0;
const MAIN_WIDTH: f32 = 640.0;
const CONTENT_HEIGHT: f32 = 464.0;
const TABLE_WIDTH: f32 = 612.0;
const TABLE_HEADER_HEIGHT: f32 = 28.0;
const TABLE_BODY_HEIGHT: f32 = 368.0;
const TABLE_ROW_HEIGHT: f32 = 26.0;
const TABLE_TOTAL_ROWS: u32 = 1_000;
const TABLE_OVERSCAN: u32 = 2;
/// 浏览器宿主注册到所有 renderer 的图片资源 id。
pub const DEMO_IMAGE_TEXTURE: &str = "demo.image";

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
}

/// demo 使用的轻量文字度量；字号和行高保持与 widgets 的 TextContent 一致。
struct DemoTextMeasurer;

impl TextMeasurer for DemoTextMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        let line_count = request.text.split('\n').count().max(1) as u32;
        let width = request
            .text
            .split('\n')
            .map(|line| {
                line.chars()
                    .map(|character| {
                        if character.is_ascii() {
                            request.font_size * 0.56
                        } else {
                            request.font_size
                        }
                    })
                    .sum::<f32>()
            })
            .fold(0.0, f32::max);
        let width = request.max_width.map_or(width, |max| width.min(max));
        TextMetrics {
            width,
            height: line_count as f32 * request.line_height,
            line_count,
        }
    }
}

/// 最小 demo 运行时。逻辑帧缓存与 renderer 的呈现节奏独立。
struct App {
    frame: Option<UiFrame>,
    tree: Option<UiTree>,
    view_state: ViewStateStore,
    button_selected: bool,
    blue_visible: bool,
    input_value: String,
    input_key: Option<SemanticKey>,
    submit_key: Option<SemanticKey>,
    table_scroll_key: Option<SemanticKey>,
    clickable_keys: BTreeSet<SemanticKey>,
    table_scroll: ScrollState,
    input_value_upload: Vec<u8>,
    frame_trace: Vec<u8>,
    cpu_rendered: bool,
    cpu_bitmap: Vec<u8>,
    textures: BTreeMap<TextureRef, tela_render_raster::BitmapRGBA8>,
    image_upload: Vec<u8>,
}

impl App {
    fn new() -> Self {
        Self {
            frame: None,
            tree: None,
            view_state: ViewStateStore::new(),
            button_selected: false,
            blue_visible: true,
            input_value: String::new(),
            input_key: None,
            submit_key: None,
            table_scroll_key: None,
            clickable_keys: BTreeSet::new(),
            table_scroll: ScrollState::default(),
            input_value_upload: Vec::new(),
            frame_trace: Vec::new(),
            cpu_rendered: false,
            cpu_bitmap: Vec::new(),
            textures: BTreeMap::new(),
            image_upload: Vec::new(),
        }
    }

    /// 确保共享逻辑帧存在；返回值表示本次是否发生了场景构建与布局。
    fn ensure_frame(&mut self) -> bool {
        if self.frame.is_none() {
            let (first_row_index, visible_rows) = self.table_window();
            let tree = UiTree::new(scene_node(
                self.button_selected,
                self.blue_visible,
                &self.input_value,
                self.input_focused(),
                self.submit_hovered(),
                first_row_index,
                visible_rows,
            ))
            .expect("最小 demo 场景必须合法");
            let mut scroll_inputs = HashMap::new();
            if let Some(key) = &self.table_scroll_key {
                scroll_inputs.insert(key.clone(), self.table_scroll);
            }
            let frame = tree
                .resolve(VIEWPORT, &DemoTextMeasurer, &scroll_inputs)
                .expect("最小 demo 场景必须可布局");
            self.frame_trace = frame_trace::to_json(&frame).into_bytes();
            let keys = discover_control_keys(&tree);
            self.input_key = keys.input;
            self.submit_key = keys.submit;
            self.table_scroll_key = keys.table_scroll;
            self.clickable_keys = keys.clickable;
            self.tree = Some(tree);
            self.frame = Some(frame);
            self.cpu_rendered = false;
            true
        } else {
            false
        }
    }

    /// 已缓存的唯一逻辑帧。调用方先经 `ensure_frame` 保证其存在。
    fn frame(&self) -> &UiFrame {
        self.frame.as_ref().expect("共享逻辑帧必须已构建")
    }

    /// 与 `frame` 同时缓存的、由同一 `UiFrame` 投影出的 UTF-8 调试 JSON。
    fn frame_trace(&self) -> &[u8] {
        debug_assert!(self.frame.is_some());
        &self.frame_trace
    }

    fn invalidate_frame(&mut self) {
        self.frame = None;
        self.tree = None;
        self.frame_trace.clear();
        self.cpu_rendered = false;
    }

    /// 将宿主传入的逻辑坐标交给 tela-core；动作只用于更新 demo 的受控视觉状态。
    fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        self.ensure_frame();
        let frame = self.frame().clone();
        let tree = self.tree.as_ref().expect("共享逻辑树必须已构建");
        let actions = handle_input(
            tree,
            &frame,
            &mut self.view_state,
            &InputEvent::Pointer(event),
        );
        let clicked_submit = actions.iter().any(|action| {
            let UiAction::Click { node_id } = action else {
                return false;
            };
            node_key(tree, *node_id).is_some_and(|key| self.submit_key.as_ref() == Some(key))
        });
        let mut scrolled = false;
        for action in &actions {
            let UiAction::Scroll { node_id, delta } = action else {
                continue;
            };
            if node_key(tree, *node_id)
                .is_some_and(|key| self.table_scroll_key.as_ref() == Some(key))
            {
                let max_offset = table_max_scroll();
                let next = (self.table_scroll.offset_y + delta.y).clamp(0.0, max_offset);
                if (next - self.table_scroll.offset_y).abs() > f32::EPSILON {
                    self.table_scroll.offset_y = next;
                    scrolled = true;
                }
            }
        }
        let count = u32::try_from(actions.len()).expect("交互动作数必须可编码");
        if clicked_submit && !self.input_value.is_empty() {
            self.button_selected = !self.button_selected;
            self.blue_visible = !self.blue_visible;
        }
        if count != 0 || scrolled {
            self.invalidate_frame();
        }
        count
    }

    fn input_focused(&self) -> bool {
        self.view_state
            .current_focus_key()
            .is_some_and(|key| self.input_key.as_ref() == Some(key))
    }

    fn submit_hovered(&self) -> bool {
        self.view_state
            .hover_key()
            .is_some_and(|key| self.submit_key.as_ref() == Some(key))
    }

    /// 供浏览器宿主映射为 CSS cursor 的轻量意图：0=默认，1=文本，2=手型。
    fn pointer_cursor(&self) -> u32 {
        let hover = self.view_state.hover_key();
        if hover.is_some_and(|key| self.input_key.as_ref() == Some(key)) {
            1
        } else if hover.is_some_and(|key| self.clickable_keys.contains(key)) {
            2
        } else {
            0
        }
    }

    fn table_window(&self) -> (u32, u32) {
        let first_visible = (self.table_scroll.offset_y / TABLE_ROW_HEIGHT).floor() as u32;
        let first = first_visible.saturating_sub(TABLE_OVERSCAN);
        let visible = (TABLE_BODY_HEIGHT / TABLE_ROW_HEIGHT).ceil() as u32;
        let count = (visible + TABLE_OVERSCAN * 2).min(TABLE_TOTAL_ROWS - first);
        (first, count)
    }

    /// 接收浏览器 IME/键盘适配层生成的受控值变更。
    fn set_input_value(&mut self, value: String) -> u32 {
        if !self.input_focused() || self.input_value == value {
            return 0;
        }
        let action = UiAction::ValueChange {
            bind_id: BindId("demo.input".to_owned()),
            value: Value::String(value),
        };
        let changed = match action {
            UiAction::ValueChange {
                bind_id,
                value: Value::String(value),
            } if bind_id.0 == "demo.input" => {
                self.input_value = value;
                true
            }
            _ => false,
        };
        if changed {
            self.invalidate_frame();
        }
        u32::from(changed)
    }

    fn begin_input_value_upload(&mut self, bytes: usize) -> *mut u8 {
        self.input_value_upload.resize(bytes, 0);
        self.input_value_upload.as_mut_ptr()
    }

    fn finish_input_value_upload(&mut self, bytes: usize) -> u32 {
        if bytes != self.input_value_upload.len() {
            self.input_value_upload.clear();
            return 0;
        }
        let bytes = std::mem::take(&mut self.input_value_upload);
        let Ok(value) = String::from_utf8(bytes) else {
            return 0;
        };
        self.set_input_value(value)
    }

    /// CPU 仅在共享逻辑帧变更时重新光栅化。
    fn render_cpu_if_needed(&mut self) -> bool {
        self.ensure_frame();
        if self.cpu_rendered {
            return false;
        }
        let mut config =
            tela_render_raster::RasterConfig::default_with(Color::rgba(1.0, 1.0, 1.0, 1.0));
        config.textures = self.textures.clone();
        self.cpu_bitmap = tela_render_raster::render_frame(self.frame(), &config).pixels;
        self.cpu_rendered = true;
        true
    }

    fn begin_demo_image_upload(&mut self, bytes: usize) -> *mut u8 {
        self.image_upload.resize(bytes, 0);
        self.image_upload.as_mut_ptr()
    }

    fn finish_demo_image_upload(&mut self, width: u32, height: u32) -> bool {
        let Some(expected) = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .map(|bytes| bytes as usize)
        else {
            self.image_upload.clear();
            return false;
        };
        let pixels = std::mem::take(&mut self.image_upload);
        if width == 0 || height == 0 || pixels.len() != expected {
            return false;
        }
        self.textures.insert(
            TextureRef(DEMO_IMAGE_TEXTURE.to_owned()),
            tela_render_raster::BitmapRGBA8 {
                width,
                height,
                pixels,
            },
        );
        // 图像资源就绪不改 UiFrame，但 CPU 位图需要在下一 tick 重建。
        self.cpu_rendered = false;
        true
    }
}

/// 无文字矩形，作为页面区域的基础视觉原语。
fn solid_rect(color: Color, width: Size, height: Size) -> tela_contract::UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(width),
            height: Some(height),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(color)),
            ..VisualConcern::default()
        })
        .into()
}

/// 共享 tela 场景：header / 等宽侧栏与内容 / footer，以及主内容中的图片背景、输入框和按钮。
/// header 左侧刻意包含累计溢出的子矩形，用 clip 验证后端 scissor。
fn scene_node(
    button_selected: bool,
    blue_visible: bool,
    input_value: &str,
    input_focused: bool,
    submit_hovered: bool,
    first_row_index: u32,
    visible_rows: u32,
) -> UiNode {
    let header_clip: tela_contract::UiNode = LayoutContainer::flex([
        solid_rect(SIDEBAR, Size::fixed(80.0), Size::fixed(80.0)),
        solid_rect(CLIP_OVERFLOW, Size::fixed(160.0), Size::fixed(80.0)),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(200.0)),
        height: Some(Size::fixed(80.0)),
        clip: true,
        ..LayoutConcern::default()
    })
    .into();
    let header = LayoutContainer::flex([header_clip])
        .layout(LayoutConcern {
            width: Some(Size::fixed(800.0)),
            height: Some(Size::fixed(80.0)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: blue_visible.then_some(Fill::Solid(HEADER)),
            ..VisualConcern::default()
        })
        .into();
    let sidebar = solid_rect(
        SIDEBAR,
        Size::fixed(SIDEBAR_WIDTH),
        Size::fixed(CONTENT_HEIGHT),
    );
    let input = Input::new()
        .bind_id("demo.input")
        .value(input_value)
        .placeholder("请输入内容")
        .focused(input_focused)
        .into_node();
    let button = Button::new("提交")
        .width(112.0)
        .height(32.0)
        .hovered(submit_hovered)
        .selected(button_selected)
        .into_node();
    let toolbar = LayoutContainer::flex([input, button]).layout(LayoutConcern {
        width: Some(Size::fixed(TABLE_WIDTH)),
        height: Some(Size::fixed(32.0)),
        gap: 8.0,
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    });

    let column_widths = [54.0, 100.0, 76.0, 80.0, 74.0, 92.0, 136.0];
    let header_labels = ["ID", "名称", "状态", "分类", "进度", "更新", "操作"];
    let header_cells = header_labels
        .into_iter()
        .zip(column_widths)
        .map(|(label, width)| Td::text(label).width(Size::fixed(width)).into())
        .collect();
    let table_header = Tr::new(header_cells)
        .height(TABLE_HEADER_HEIGHT)
        .background(Color::rgba(0.91, 0.94, 0.98, 1.0));
    let rows = (first_row_index..first_row_index.saturating_add(visible_rows))
        .take_while(|index| *index < TABLE_TOTAL_ROWS)
        .map(|index| {
            let operation = LayoutContainer::flex([
                Button::new("删除")
                    .width(40.0)
                    .height(20.0)
                    .variant(ButtonVariant::Danger)
                    .text_metrics(11.0, 14.0),
                Button::new("修改")
                    .width(40.0)
                    .height(20.0)
                    .variant(ButtonVariant::Warning)
                    .text_metrics(11.0, 14.0),
                Button::new("查看")
                    .width(40.0)
                    .height(20.0)
                    .text_metrics(11.0, 14.0),
            ])
            .layout(LayoutConcern {
                gap: 4.0,
                main_align: tela_contract::MainAlign::Center,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            });
            let values = [
                format!("{}", index + 1),
                format!("项目 {:04}", index + 1),
                if index % 3 == 0 {
                    "运行中"
                } else if index % 3 == 1 {
                    "待处理"
                } else {
                    "已完成"
                }
                .to_owned(),
                if index % 2 == 0 { "标准" } else { "扩展" }.to_owned(),
                format!("{}%", (index * 7) % 101),
                format!("08-{:02}", (index % 28) + 1),
            ];
            let mut cells: Vec<UiNode> = values
                .into_iter()
                .zip(column_widths.into_iter().take(6))
                .map(|(value, width)| Td::text(value).width(Size::fixed(width)).into())
                .collect();
            cells.push(
                Td::new(vec![operation.into()])
                    .width(Size::fixed(column_widths[6]))
                    .into(),
            );
            Tr::data_row(format!("row-{index}"), cells)
                .height(TABLE_ROW_HEIGHT)
                .background(if index % 2 == 0 {
                    Color::WHITE
                } else {
                    Color::rgba(0.98, 0.99, 1.0, 1.0)
                })
                .into()
        })
        .collect();
    let table: UiNode = Table::new(table_header)
        .virtual_rows(TABLE_TOTAL_ROWS, first_row_index, rows)
        .width(TABLE_WIDTH)
        .header_height(TABLE_HEADER_HEIGHT)
        .body_height(TABLE_BODY_HEIGHT)
        .row_metrics(TABLE_ROW_HEIGHT, 0.0, TABLE_OVERSCAN)
        .into();
    let main_card = LayoutContainer::flex([toolbar.into(), table])
        .layout(LayoutConcern {
            width: Some(Size::fixed(MAIN_WIDTH)),
            height: Some(Size::fixed(CONTENT_HEIGHT)),
            direction: FlexDirection::Column,
            gap: 8.0,
            padding: tela_contract::Insets::all(8.0),
            border_width: 6.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(MAIN)),
            border_color: Some(MAIN_BORDER),
            border_radius: BorderRadius::all(18.0),
            ..VisualConcern::default()
        });
    let main: tela_contract::UiNode = ImageBackground::new(DEMO_IMAGE_TEXTURE, main_card).into();
    let content = LayoutContainer::flex([sidebar, main])
        .layout(LayoutConcern {
            width: Some(Size::fixed(VIEWPORT.width)),
            height: Some(Size::fixed(CONTENT_HEIGHT)),
            direction: FlexDirection::Row,
            ..LayoutConcern::default()
        })
        .into();
    let footer = solid_rect(FOOTER, Size::fixed(800.0), Size::fixed(56.0));
    LayoutContainer::flex([header, content, footer])
        .layout(LayoutConcern {
            width: Some(Size::fixed(VIEWPORT.width)),
            height: Some(Size::fixed(VIEWPORT.height)),
            direction: FlexDirection::Column,
            ..LayoutConcern::default()
        })
        .into()
}

struct ControlKeys {
    input: Option<SemanticKey>,
    submit: Option<SemanticKey>,
    table_scroll: Option<SemanticKey>,
    clickable: BTreeSet<SemanticKey>,
}

fn discover_control_keys(tree: &UiTree) -> ControlKeys {
    fn visit(node: &UiNode, keys: &[SemanticKey], index: &mut usize, found: &mut ControlKeys) {
        let key = keys.get(*index).cloned();
        *index += 1;
        if let Some(key) = key {
            if node
                .interact
                .as_ref()
                .is_some_and(|interact| interact.text_input)
            {
                found.input = Some(key.clone());
            }
            if node
                .interact
                .as_ref()
                .is_some_and(|interact| interact.clickable)
            {
                found.clickable.insert(key.clone());
                if node_contains_text(node, "提交") {
                    found.submit = Some(key.clone());
                }
            }
            if matches!(node.kind, NodeKind::VirtualListView(_)) {
                found.table_scroll = Some(key.clone());
            }
        }
        for child in &node.children {
            visit(child, keys, index, found);
        }
    }
    let mut found = ControlKeys {
        input: None,
        submit: None,
        table_scroll: None,
        clickable: BTreeSet::new(),
    };
    let mut index = 0;
    visit(tree.root(), tree.keys(), &mut index, &mut found);
    found
}

fn node_contains_text(node: &UiNode, target: &str) -> bool {
    matches!(node.content, Some(ContentConcern::Text(ref text)) if text.text == target)
        || node
            .children
            .iter()
            .any(|child| node_contains_text(child, target))
}

fn node_key(tree: &UiTree, node_id: NodeId) -> Option<&SemanticKey> {
    tree.node_ids()
        .iter()
        .position(|id| *id == node_id)
        .and_then(|index| tree.keys().get(index))
}

fn table_max_scroll() -> f32 {
    (TABLE_TOTAL_ROWS as f32 * TABLE_ROW_HEIGHT - TABLE_BODY_HEIGHT).max(0.0)
}

pub(crate) fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> T {
    APP.with(|app| f(&mut app.borrow_mut()))
}

/// wasm WebGPU 路径的计时辅助。
#[cfg(feature = "webgpu")]
pub(crate) fn now_ms() -> f32 {
    js_sys::Date::now() as f32
}

/// CPU 后端帧推进：仅在共享 `UiFrame` 更新时光栅化。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_tick() -> u32 {
    u32::from(with_app(App::render_cpu_if_needed))
}

/// CPU 宿主注入的逻辑坐标指针按下事件。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_down(x: f32, y: f32) -> u32 {
    with_app(|app| {
        app.handle_pointer(PointerEvent::Down {
            position: Point { x, y },
        })
    })
}

/// CPU 宿主注入的逻辑坐标指针移动事件。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_move(x: f32, y: f32) -> u32 {
    with_app(|app| {
        app.handle_pointer(PointerEvent::Move {
            position: Point { x, y },
        })
    })
}

/// CPU 宿主注入的滚轮事件；表体的滚动归属仍由 tela-core 命中测试决定。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_scroll(x: f32, y: f32, delta_x: f32, delta_y: f32) -> u32 {
    with_app(|app| {
        app.handle_pointer(PointerEvent::Scroll {
            position: Point { x, y },
            delta: Point {
                x: delta_x,
                y: delta_y,
            },
        })
    })
}

/// CPU 宿主读取当前 Input 焦点，决定是否将原生文本输入交给该控件。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_focused() -> u32 {
    u32::from(with_app(|app| app.input_focused()))
}

/// 当前 hover 目标的浏览器 cursor 意图：0=默认，1=文本，2=手型。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_cursor() -> u32 {
    with_app(|app| app.pointer_cursor())
}

/// 为浏览器写入 UTF-8 的受控 Input 值预留 wasm 内存。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_value_begin(bytes: u32) -> *mut u8 {
    with_app(|app| app.begin_input_value_upload(bytes as usize))
}

/// 提交浏览器输入适配器写入的 UTF-8 受控 Input 值。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_value_finish(bytes: u32) -> u32 {
    with_app(|app| app.finish_input_value_upload(bytes as usize))
}

/// CPU 位图指针。宿主必须先调用 `demo_tick` 提交共享帧。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_ptr() -> *const u8 {
    with_app(|app| app.cpu_bitmap.as_ptr())
}

/// CPU 位图尺寸（RGBA8，逻辑像素与物理像素均为 800×600）。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_size() -> u32 {
    VIEWPORT.width as u32 | ((VIEWPORT.height as u32) << 16)
}

/// 为 CPU raster demo 预留一段 wasm 内存，供浏览器写入 demo 图片的 RGBA8 字节。
///
/// 资源 URL/base64 的加载与解码仍在浏览器适配器；此函数仅是 raw wasm 宿主的字节桥接。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_image_upload_begin(bytes: u32) -> *mut u8 {
    with_app(|app| app.begin_demo_image_upload(bytes as usize))
}

/// 提交上一段由 [`demo_image_upload_begin`] 分配的 RGBA8 图片。
/// 返回 1 表示已注册，0 表示尺寸或字节长度非法。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_image_upload_finish(width: u32, height: u32) -> u32 {
    u32::from(with_app(|app| app.finish_demo_image_upload(width, height)))
}

/// 共享 `UiFrame` 的结构化 JSON 指针。长度由 `demo_frame_trace_len` 返回。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_trace_ptr() -> *const u8 {
    with_app(|app| {
        app.ensure_frame();
        app.frame_trace().as_ptr()
    })
}

/// 共享 `UiFrame` 的结构化 JSON 字节长度。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_trace_len() -> u32 {
    with_app(|app| {
        app.ensure_frame();
        u32::try_from(app.frame_trace().len()).expect("trace 长度必须可编码")
    })
}

/// CPU WASM 构建标识。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_wasm_version() -> u32 {
    option_env!("TELA_BUILD_TS")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control_center(app: &App, key: &SemanticKey) -> Point {
        let tree = app.tree.as_ref().expect("tree");
        let node_id = tree
            .node_ids()
            .iter()
            .zip(tree.keys())
            .find_map(|(id, node_key)| (node_key == key).then_some(*id))
            .expect("control key");
        let rect = app
            .frame()
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("control hit region")
            .rect;
        Point {
            x: rect.x + rect.w / 2.0,
            y: rect.y + rect.h / 2.0,
        }
    }

    #[test]
    fn shared_scene_resolves_to_layout_table_and_image() {
        let mut app = App::new();
        assert!(app.ensure_frame());
        let frame = app.frame();
        assert_eq!(frame.viewport, VIEWPORT);
        assert!(frame.commands.len() > 50, "表格应产生多行绘制命令");
        assert!(frame.commands.iter().any(|command| {
            matches!(command.payload, tela_contract::DrawPayload::Rect { fill: Some(color), .. } if color == SIDEBAR)
        }));
        let image = frame
            .commands
            .iter()
            .find(|command| matches!(command.payload, tela_contract::DrawPayload::Image { .. }))
            .expect("demo 必须有图片背景命令");
        assert_eq!((image.geometry.x, image.geometry.y), (SIDEBAR_WIDTH, 80.0));
        assert_eq!(
            (image.geometry.w, image.geometry.h),
            (MAIN_WIDTH, CONTENT_HEIGHT)
        );
        assert_eq!(
            image.payload,
            tela_contract::DrawPayload::Image {
                texture: TextureRef(DEMO_IMAGE_TEXTURE.to_owned()),
            }
        );
        let main = frame
            .commands
            .iter()
            .find(|command| {
                matches!(
                    command.payload,
                    tela_contract::DrawPayload::RoundedRect { fill: Some(fill), .. }
                        if fill == MAIN
                )
            })
            .expect("demo 必须有主卡片圆角命令");
        assert_eq!((main.geometry.x, main.geometry.y), (SIDEBAR_WIDTH, 80.0));
        assert_eq!(
            (main.geometry.w, main.geometry.h),
            (MAIN_WIDTH, CONTENT_HEIGHT)
        );
        let texts: Vec<&tela_contract::TextContent> = frame
            .commands
            .iter()
            .filter_map(|command| match &command.payload {
                tela_contract::DrawPayload::Text { text } => Some(text),
                _ => None,
            })
            .collect();
        assert!(texts.len() > 20);
        assert!(texts.iter().any(|text| text.text == "请输入内容"));
        assert!(texts.iter().any(|text| text.text == "提交"));
        assert!(texts.iter().any(|text| text.text == "操作"));
        assert!(texts.iter().any(|text| text.text == "查看"));
        assert!(texts.iter().any(|text| text.text == "项目 0001"));
        assert!(!tree_keys(app.tree.as_ref().unwrap()).contains(&"row-999".to_owned()));
        let footer = frame.commands.last().expect("footer 必须是最后一条命令");
        assert_eq!(
            (
                footer.geometry.x,
                footer.geometry.y,
                footer.geometry.w,
                footer.geometry.h
            ),
            (0.0, 544.0, 800.0, 56.0)
        );
        assert_eq!(
            footer.payload,
            tela_contract::DrawPayload::Rect {
                fill: Some(FOOTER),
                border: None,
            }
        );
        assert!(frame.hit_regions.iter().any(|region| {
            Some(region.node_id)
                == app
                    .tree
                    .as_ref()
                    .and_then(|tree| {
                        tree.node_ids().get(
                            tree.keys()
                                .iter()
                                .position(|key| Some(key) == app.table_scroll_key.as_ref())?,
                        )
                    })
                    .copied()
        }));
    }

    #[test]
    fn cached_frame_is_not_recomputed() {
        let mut app = App::new();
        assert!(app.ensure_frame());
        let frame = app.frame() as *const UiFrame;
        assert!(!app.ensure_frame());
        assert!(std::ptr::eq(frame, app.frame()));
    }

    #[test]
    fn cached_trace_describes_the_shared_frame() {
        let mut app = App::new();
        app.ensure_frame();
        let trace = std::str::from_utf8(app.frame_trace()).expect("trace 必须是 UTF-8");
        assert!(trace.starts_with("{\"viewport\":{\"width\":800,\"height\":600},\"commands\":["));
        assert!(trace.contains("\"kind\":\"rounded_rect\""));
        assert!(trace.contains("\"kind\":\"image\",\"texture\":\"demo.image\""));
        assert!(trace.contains("\"kind\":\"text\""));
        assert!(trace.contains("请输入内容"));
        assert!(trace.contains("操作"));
        assert!(trace.contains("项目 0001"));
        assert!(trace.contains("\"hit_regions\":["));
        assert!(trace.ends_with("]}"));
    }

    #[test]
    fn demo_text_measurement_counts_wide_characters() {
        let measurer = DemoTextMeasurer;
        let metrics = measurer.measure(&TextMeasureRequest {
            text: "请输入内容",
            font: &tela_contract::FontRef("noto".to_owned()),
            font_size: 13.0,
            line_height: 18.2,
            max_width: None,
        });
        assert!((metrics.width - 65.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pointer_events_reach_core_and_update_control_state() {
        let mut app = App::new();
        app.input_value = "ready".to_owned();
        app.ensure_frame();
        let position = control_center(&app, app.submit_key.as_ref().expect("submit key"));
        assert!(app.handle_pointer(PointerEvent::Down { position }) > 0);
        assert!(app.button_selected);
        assert!(!app.blue_visible);
        assert!(app.frame.is_none(), "交互动作后应重建受控帧");
    }

    #[test]
    fn empty_input_submit_does_not_toggle_blue_rectangle() {
        let mut app = App::new();
        app.ensure_frame();
        let position = control_center(&app, app.submit_key.as_ref().expect("submit key"));
        assert!(app.handle_pointer(PointerEvent::Down { position }) > 0);
        assert!(!app.button_selected);
        assert!(app.blue_visible);
    }

    #[test]
    fn hover_state_exposes_cursor_intent_to_the_browser_host() {
        let mut app = App::new();
        app.ensure_frame();
        assert!(
            app.handle_pointer(PointerEvent::Move {
                position: control_center(&app, app.input_key.as_ref().expect("input key")),
            }) > 0
        );
        assert_eq!(app.pointer_cursor(), 1);
        app.ensure_frame();
        assert!(
            app.handle_pointer(PointerEvent::Move {
                position: control_center(&app, app.submit_key.as_ref().expect("submit key")),
            }) > 0
        );
        assert_eq!(app.pointer_cursor(), 2);
        assert!(
            app.handle_pointer(PointerEvent::Move {
                position: Point { x: -1.0, y: -1.0 },
            }) > 0
        );
        assert_eq!(app.pointer_cursor(), 0);
    }

    #[test]
    fn focused_input_accepts_host_value_change() {
        let mut app = App::new();
        app.ensure_frame();
        assert!(
            app.handle_pointer(PointerEvent::Down {
                position: control_center(&app, app.input_key.as_ref().expect("input key")),
            }) > 0
        );
        assert!(app.input_focused());
        assert_eq!(app.set_input_value("你好 tela".to_owned()), 1);
        assert!(app.ensure_frame());
        assert!(app.frame().commands.iter().any(|command| {
            matches!(
                &command.payload,
                tela_contract::DrawPayload::Text { text } if text.text == "你好 tela"
            )
        }));
    }

    fn tree_keys(tree: &UiTree) -> Vec<String> {
        tree.keys().iter().map(|key| key.0.clone()).collect()
    }

    #[test]
    fn virtual_table_builds_window_and_scrolls_rows() {
        let mut app = App::new();
        app.ensure_frame();
        let before = tree_keys(app.tree.as_ref().unwrap());
        assert!(before.contains(&"row-0".to_owned()));
        assert!(!before.contains(&"row-30".to_owned()));
        let table_center = control_center(
            &app,
            app.table_scroll_key.as_ref().expect("table scroll key"),
        );
        assert!(
            app.handle_pointer(PointerEvent::Scroll {
                position: table_center,
                delta: Point { x: 0.0, y: 260.0 },
            }) > 0
        );
        app.ensure_frame();
        let after = tree_keys(app.tree.as_ref().unwrap());
        assert!(after.contains(&"row-8".to_owned()));
        assert!(!after.contains(&"row-0".to_owned()));
    }
}
