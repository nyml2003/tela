//! tela 演示应用：浏览器 canvas 复杂页面，覆盖 80% 库能力。
//!
//! 覆盖清单：
//! - 布局：Flex row/column、Stack（Content + FillOverlay 角标）、ScrollView、虚拟列表
//!   （定高 + semantic-id + 滚轮可视区）、margin/padding/gap、MinMax、对齐；
//! - 绘制：圆角卡片、渐变、圆/椭圆、中文文字、draw_order；
//! - 身份：auto-stable-identity 列表（增删/重排状态保持）、semantic-id 虚拟列表、
//!   ViewStateStore（滚动位置随 key 保持）；
//! - 更新：Dirty 布局缓存（LayoutCache，仅脏节点重算）；
//! - 交互：指针命中 → UiAction、Tab/方向键焦点转移、确认/取消、模态栈拦截、
//!   局部快捷键（Ctrl+S）、FocusChanged 高亮；
//! - 渲染：软件光栅位图 → canvas ImageData 呈现（像素确定性基准）。

use std::cell::RefCell;

use tela_contract::{
    Color, Fill, FontRef, IdentityConcern, InputEvent, Key, KeyCombo, KeyState, LayoutConcern,
    Modifiers, Point, PointerEvent, RawKeyboardEvent, SemanticKey, ShortcutId, ShortcutMapping,
    ShortcutScopeSpec, Size, TextContent, TextMeasureRequest, TextMeasurer, TextMetrics, UiAction,
    UiNode, Viewport, VirtualListSpec, VisualConcern,
};
use tela_core::builder::{LayoutContainer, LogicalContainer, Primitive};
use tela_core::{IdentityAllocator, LayoutCache, UiTree, ViewStateStore, handle_input};

/// 逻辑画布尺寸（布局与交互坐标，独立于像素密度）。
pub const VIEWPORT: Viewport = Viewport {
    width: 480.0,
    height: 360.0,
};

/// 渲染缩放：位图像素 = 逻辑坐标 × DPI_SCALE（与 HTML canvas 像素一一对应）。
pub const DPI_SCALE: f32 = 2.0;

/// 滚动列表与虚拟列表的 key（由树结构固定，见 build_tree 注释）。
const SCROLL_KEY: &str = "/0/0/0/2/0/";
const VIRTUAL_KEY: &str = "/0/0/0/2/1/";

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
}

/// 应用：宿主数据 + 基座跨帧状态。
struct App {
    items: Vec<String>,
    virtual_offset: f32,
    modal_open: bool,
    log: Vec<String>,
    allocator: IdentityAllocator,
    cache: LayoutCache,
    state: ViewStateStore,
    measurer: DemoMeasurer,
    bitmap: Vec<u8>,
    pending_actions: Vec<UiAction>,
    last_keys: Vec<SemanticKey>,
}

impl App {
    fn new() -> Self {
        App {
            items: vec!["条目 A".into(), "条目 B".into(), "条目 C".into()],
            virtual_offset: 0.0,
            modal_open: false,
            log: Vec::new(),
            allocator: IdentityAllocator::new(),
            cache: LayoutCache::new(),
            state: ViewStateStore::new(),
            measurer: DemoMeasurer,
            bitmap: Vec::new(),
            pending_actions: Vec::new(),
            last_keys: Vec::new(),
        }
    }

    fn log(&mut self, msg: String) {
        self.log.push(msg);
        if self.log.len() > 6 {
            self.log.remove(0);
        }
    }

    /// 构建树 + Dirty resolve + 光栅渲染 → 位图像素。
    fn render(&mut self) {
        let root = build_tree(self);
        let tree = UiTree::new_with_allocator(root, &mut self.allocator).expect("树合法");
        self.last_keys = tree.keys().to_vec();
        let scrolls = std::collections::HashMap::from([
            (
                SemanticKey(SCROLL_KEY.to_string()),
                self.state.scroll(&SemanticKey(SCROLL_KEY.to_string())),
            ),
            (
                SemanticKey(VIRTUAL_KEY.to_string()),
                tela_contract::ScrollState {
                    offset_x: 0.0,
                    offset_y: self.virtual_offset,
                },
            ),
        ]);
        let frame = tree
            .resolve_dirty(VIEWPORT, &self.measurer, &scrolls, &mut self.cache)
            .expect("resolve 成功");
        let mut config =
            tela_render_raster::RasterConfig::default_with(Color::rgba(0.07, 0.08, 0.10, 1.0));
        config.dpi_scale = DPI_SCALE;
        let bitmap = tela_render_raster::render_frame(&frame, &config);
        self.bitmap = bitmap.pixels;
        // 滚动位置保持：写入视图状态仓库（宿主从仓库取 offset 组装 scroll_inputs）。
        self.state.set_scroll(
            SemanticKey(SCROLL_KEY.to_string()),
            self.state.scroll(&SemanticKey(SCROLL_KEY.to_string())),
        );
    }

    /// 处理输入事件（当前数据快照的树），暂存动作，渲染反馈帧。
    fn handle(&mut self, event: InputEvent) {
        let root = build_tree(self);
        let tree = UiTree::new_with_allocator(root, &mut self.allocator).expect("树合法");
        self.last_keys = tree.keys().to_vec();
        let scrolls = std::collections::HashMap::from([
            (
                SemanticKey(SCROLL_KEY.to_string()),
                self.state.scroll(&SemanticKey(SCROLL_KEY.to_string())),
            ),
            (
                SemanticKey(VIRTUAL_KEY.to_string()),
                tela_contract::ScrollState {
                    offset_x: 0.0,
                    offset_y: self.virtual_offset,
                },
            ),
        ]);
        let frame = tree
            .resolve_dirty(VIEWPORT, &self.measurer, &scrolls, &mut self.cache)
            .expect("resolve 成功");
        self.pending_actions = handle_input(&tree, &frame, &mut self.state, &event);
        let mut config =
            tela_render_raster::RasterConfig::default_with(Color::rgba(0.07, 0.08, 0.10, 1.0));
        config.dpi_scale = DPI_SCALE;
        let bitmap = tela_render_raster::render_frame(&frame, &config);
        self.bitmap = bitmap.pixels;
    }

    /// 应用挂起的动作（宿主执行业务意图）→ 渲染新帧。
    fn apply_pending(&mut self) {
        let actions = std::mem::take(&mut self.pending_actions);
        for action in &actions {
            match action {
                UiAction::Click { node_id } => {
                    let key = self.last_keys.get(node_id.0 as usize).cloned();
                    if let Some(key) = key {
                        match key.0.as_str() {
                            "btn-add" => {
                                let next = self.items.len() + 1;
                                self.items.push(format!("条目 {next}"));
                            }
                            "btn-del" => {
                                self.items.pop();
                            }
                            "btn-shuffle" if self.items.len() > 1 => {
                                let last = self.items.len() - 1;
                                self.items.swap(0, last);
                            }
                            "btn-modal" => {
                                self.modal_open = true;
                                self.state
                                    .push_modal(SemanticKey("modal-layer".to_string()));
                            }
                            "btn-close" => {
                                self.modal_open = false;
                                self.state.pop_modal();
                            }
                            _ => {}
                        }
                        self.log(format!("点击 {}", key.0));
                    }
                }
                UiAction::CloseModal { .. } => {
                    self.modal_open = false;
                    self.state.pop_modal();
                    self.log("取消键关闭模态".into());
                }
                UiAction::ShortcutActivated { shortcut_id } => {
                    self.log(format!("快捷键 {shortcut_id:?}"));
                }
                UiAction::FocusChanged { to, .. } => {
                    if let Some(id) = to
                        && let Some(key) = self.last_keys.get(id.0 as usize)
                    {
                        self.log(format!("焦点 {}", key.0));
                    }
                }
                _ => {}
            }
        }
        self.render();
    }
}

/// 纯函数文本度量（近似规则，中英文同宽处理）。
struct DemoMeasurer;

impl TextMeasurer for DemoMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        let width = request.text.chars().count() as f32 * request.font_size * 0.55;
        TextMetrics {
            width,
            height: request.line_height,
            line_count: 1,
        }
    }
}

// ---------- 节点构造 helpers ----------

fn text(text: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: text.to_string(),
        font: FontRef("noto".to_string()),
        font_size: size,
        line_height: size * 1.4,
        color,
    })
    .into()
}

fn card(width: f32, height: f32, fill: Color, radius: f32) -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(fill)),
            border_radius: tela_contract::BorderRadius::all(radius),
            ..VisualConcern::default()
        })
        .into()
}

/// 可聚焦可点击按钮（容器组件承载 key + interact）。
///
/// 背景 = 容器自身 visual（圆角卡片），label = 容器 children（Flex 居中排布）——
/// 与"内容嵌套在容器内"的组件直觉一致；不依赖 Stack 浮层（FillOverlay 保留给真正
/// 不参与尺寸的浮层，如卡片角标）。
fn button(key: &str, label: &str, width: f32) -> UiNode {
    LayoutContainer::flex([Primitive::text(TextContent {
        text: label.to_string(),
        font: FontRef("noto".to_string()),
        font_size: 12.0,
        line_height: 12.0 * 1.4,
        color: Color::WHITE,
    })])
    .visual(VisualConcern {
        fill: Some(Fill::Solid(Color::rgba(0.16, 0.34, 0.6, 1.0))),
        border_radius: tela_contract::BorderRadius::all(6.0),
        ..VisualConcern::default()
    })
    .identity(IdentityConcern {
        semantic_key: Some(SemanticKey(key.to_string())),
        ..IdentityConcern::default()
    })
    .interact(tela_contract::InteractConcern {
        clickable: true,
        focusable: true,
        ..Default::default()
    })
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(26.0)),
        main_align: tela_contract::MainAlign::Center,
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into()
}

trait IntoNode: Into<UiNode> {
    fn into_node(self) -> UiNode {
        self.into()
    }
}
impl<T: Into<UiNode>> IntoNode for T {}

// ---------- 树构建（数据快照 → UiNode） ----------

/// 树结构（DFS 序 key 约定）：
/// ModalHost(/0/) → [ShortcutScope(/0/0/) → Flex(/0/0/0/) → [toolbar(/0/0/0/0/), 卡片(/0/0/0/1/),
///   main(/0/0/0/2/) → [scroll(/0/0/0/2/0/), virtual(/0/0/0/2/1/)], status(/0/0/0/3/)],
///   modal-layer(/0/1/)]
fn build_tree(app: &App) -> UiNode {
    let focused = app.state.current_focus_key().cloned();

    // 工具栏。
    let toolbar = LayoutContainer::flex([
        button("btn-add", "添加条目", 80.0),
        button("btn-del", "删除条目", 80.0),
        button("btn-modal", "打开弹窗", 80.0),
        button("btn-shuffle", "随机重排", 80.0),
        text("Ctrl+S 保存", 11.0, Color::rgba(0.55, 0.55, 0.6, 1.0)),
    ])
    .layout(LayoutConcern {
        gap: 8.0,
        padding: tela_contract::Insets::all(8.0),
        ..LayoutConcern::default()
    })
    .into_node();

    // 渐变卡片（Stack：渐变底 + FillOverlay 角标 + 标题）。
    let gradient_card = LayoutContainer::stack([
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(300.0)),
                height: Some(Size::fixed(64.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Linear(tela_contract::Gradient {
                    kind: tela_contract::GradientKind::Linear {
                        start: Point { x: 0.0, y: 0.0 },
                        end: Point { x: 300.0, y: 0.0 },
                    },
                    stops: vec![
                        tela_contract::ColorStop {
                            position: 0.0,
                            color: Color::rgba(0.15, 0.35, 0.85, 1.0),
                        },
                        tela_contract::ColorStop {
                            position: 1.0,
                            color: Color::rgba(0.65, 0.2, 0.85, 1.0),
                        },
                    ],
                })),
                border_radius: tela_contract::BorderRadius::all(12.0),
                ..VisualConcern::default()
            })
            .into(),
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(48.0)),
                height: Some(Size::fixed(18.0)),
                stack_layer: tela_contract::StackLayer::FillOverlay,
                stack_align: Some(tela_contract::StackAlign::TopRight),
                stack_offset: tela_contract::PixelOffset { x: -6.0, y: 6.0 },
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::rgba(0.95, 0.3, 0.3, 1.0))),
                border_radius: tela_contract::BorderRadius::all(9.0),
                ..VisualConcern::default()
            })
            .into(),
        text("数据面板", 16.0, Color::WHITE),
    ])
    .into_node();

    // 滚动列表（auto-stable 身份 + 焦点高亮 + 圆点装饰）。
    let stable_items: Vec<UiNode> = app
        .items
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let focused_here = focused.as_ref() == Some(&SemanticKey(format!("item-{i}")));
            let bg = if focused_here {
                Color::rgba(0.25, 0.5, 0.85, 1.0)
            } else {
                Color::rgba(0.18, 0.19, 0.24, 1.0)
            };
            LayoutContainer::flex([
                card(140.0, 20.0, bg, 5.0),
                text(name, 12.0, Color::WHITE),
                Primitive::circle()
                    .layout(LayoutConcern {
                        width: Some(Size::fixed(10.0)),
                        height: Some(Size::fixed(10.0)),
                        ..LayoutConcern::default()
                    })
                    .visual(VisualConcern {
                        fill: Some(Fill::Solid(if i % 2 == 0 {
                            Color::rgba(0.3, 0.9, 0.4, 1.0)
                        } else {
                            Color::rgba(0.9, 0.7, 0.3, 1.0)
                        })),
                        ..VisualConcern::default()
                    })
                    .into(),
            ])
            .identity(IdentityConcern {
                semantic_key: Some(SemanticKey(format!("item-{i}"))),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                height: Some(Size::fixed(26.0)),
                ..LayoutConcern::default()
            })
            .into()
        })
        .collect();
    let stable_scope = LogicalContainer::identity_scope()
        .identity(IdentityConcern {
            key_strategy: tela_contract::KeyStrategy::AutoStableIdentity,
            ..IdentityConcern::default()
        })
        .children(stable_items)
        .into_node();
    let scroll = LayoutContainer::scroll_view([stable_scope])
        .layout(LayoutConcern {
            width: Some(Size::fixed(190.0)),
            height: Some(Size::fixed(150.0)),
            ..LayoutConcern::default()
        })
        .into_node();

    // 虚拟列表：业务按 offset 构建可视范围 item（semantic-id 强制，见 006-6）。
    let item_h = 22.0f32;
    let spacing = 4.0f32;
    let stride = item_h + spacing;
    let total = 100usize;
    let first_visible = (app.virtual_offset / stride).floor() as usize;
    let visible_count = (150.0 / stride).ceil() as usize + 2;
    let virtual_items: Vec<UiNode> = (first_visible..(first_visible + visible_count).min(total))
        .map(|i| {
            LayoutContainer::flex([
                card(150.0, 18.0, Color::rgba(0.16, 0.3, 0.5, 1.0), 4.0),
                text(&format!("虚拟项 #{i}"), 11.0, Color::WHITE),
            ])
            .identity(IdentityConcern {
                semantic_key: Some(SemanticKey(format!("vitem-{i}"))),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                height: Some(Size::fixed(item_h)),
                ..LayoutConcern::default()
            })
            .into()
        })
        .collect();
    let virtual_list = LayoutContainer::virtual_list(
        VirtualListSpec {
            item_height: item_h,
            item_spacing: spacing,
            overscan: 2,
        },
        virtual_items,
    )
    .layout(LayoutConcern {
        width: Some(Size::fixed(190.0)),
        height: Some(Size::fixed(150.0)),
        ..LayoutConcern::default()
    })
    .into_node();

    let main = LayoutContainer::flex([scroll, virtual_list])
        .layout(LayoutConcern {
            gap: 12.0,
            padding: tela_contract::Insets::all(8.0),
            ..LayoutConcern::default()
        })
        .into_node();

    // 状态栏（日志）。
    let status_text = app
        .log
        .last()
        .cloned()
        .unwrap_or_else(|| "就绪".to_string());
    let status =
        LayoutContainer::flex([text(&status_text, 11.0, Color::rgba(0.7, 0.7, 0.75, 1.0))])
            .layout(LayoutConcern {
                padding: tela_contract::Insets::all(6.0),
                ..LayoutConcern::default()
            })
            .into_node();

    // ShortcutScope：Ctrl+S → SAVE（局部快捷键，见 008-2.11）。
    let shortcut_scope = LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        mappings: vec![ShortcutMapping {
            combo: KeyCombo {
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                key: Key::Char('s'),
            },
            shortcut: ShortcutId::Save,
        }],
    })
    .children([
        LayoutContainer::flex([toolbar, gradient_card, main, status])
            .layout(LayoutConcern {
                direction: tela_contract::FlexDirection::Column,
                gap: 6.0,
                ..LayoutConcern::default()
            })
            .into_node(),
    ])
    .into_node();

    // 模态层：全屏遮罩（content，参与尺寸）+ 居中卡片（FillOverlay，不参与尺寸）。
    // 弹窗自身是 ModalHost 的直接子（ModalHost 子层叠放），内部用 Stack 分层。
    let modal_layer: Vec<UiNode> = if app.modal_open {
        vec![
            LayoutContainer::stack::<[UiNode; 2]>([
                Primitive::rect()
                    .layout(LayoutConcern {
                        width: Some(Size::fill()),
                        height: Some(Size::fill()),
                        stack_layer: tela_contract::StackLayer::Content,
                        ..LayoutConcern::default()
                    })
                    .visual(VisualConcern {
                        fill: Some(Fill::Solid(Color::rgba(0.0, 0.0, 0.0, 0.55))),
                        ..VisualConcern::default()
                    })
                    .into(),
                LayoutContainer::stack([
                    card(240.0, 110.0, Color::rgba(0.14, 0.14, 0.17, 0.99), 10.0),
                    text("弹窗", 16.0, Color::WHITE),
                    button("btn-close", "关闭弹窗", 96.0),
                ])
                .layout(LayoutConcern {
                    stack_layer: tela_contract::StackLayer::FillOverlay,
                    stack_align: Some(tela_contract::StackAlign::Center),
                    ..LayoutConcern::default()
                })
                .into(),
            ])
            .layout(LayoutConcern {
                width: Some(Size::fill()),
                height: Some(Size::fill()),
                ..LayoutConcern::default()
            })
            .into(),
        ]
    } else {
        Vec::new()
    };

    LogicalContainer::modal_host()
        .children([
            shortcut_scope.into_node(),
            LogicalContainer::group()
                .identity(IdentityConcern {
                    semantic_key: Some(SemanticKey("modal-layer".to_string())),
                    ..IdentityConcern::default()
                })
                .children(modal_layer)
                .into_node(),
        ])
        .into_node()
}

// ---------- wasm 导出（纯 extern ABI，无 wasm-bindgen） ----------

fn with_app<F: FnOnce(&mut App) -> R, R>(f: F) -> R {
    APP.with(|cell| f(&mut cell.borrow_mut()))
}

/// 指针事件：kind 0=down 1=up 2=move 3=scroll（dx/dy 为滚轮增量）。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer(x: f32, y: f32, kind: u32, dx: f32, dy: f32) {
    with_app(|app| {
        let event = match kind {
            0 => InputEvent::Pointer(PointerEvent::Down {
                position: Point { x, y },
            }),
            1 => InputEvent::Pointer(PointerEvent::Up {
                position: Point { x, y },
            }),
            2 => InputEvent::Pointer(PointerEvent::Move {
                position: Point { x, y },
            }),
            _ => InputEvent::Pointer(PointerEvent::Scroll {
                position: Point { x, y },
                delta: Point { x: dx, y: dy },
            }),
        };
        if kind == 3 {
            // 滚轮：滚动列表与虚拟列表（宿主数据驱动）。
            app.virtual_offset = (app.virtual_offset + dy).clamp(0.0, 90.0 * 26.0);
        } else {
            app.handle(event);
            app.apply_pending();
            return;
        }
        app.handle(event);
        app.apply_pending();
    })
}

/// 键盘：0=Tab 1=Enter 2=Esc 3=Up 4=Down 5=Left 6=Right 7=Ctrl+S。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_key(key_code: u32, shift: u32) {
    with_app(|app| {
        let key = match key_code {
            0 => Key::Tab,
            1 => Key::Enter,
            2 => Key::Escape,
            3 => Key::ArrowUp,
            4 => Key::ArrowDown,
            5 => Key::ArrowLeft,
            6 => Key::ArrowRight,
            _ => Key::Char('s'),
        };
        let event = InputEvent::Key(RawKeyboardEvent {
            key,
            modifiers: Modifiers {
                shift: shift != 0,
                ctrl: key_code == 7,
                ..Modifiers::default()
            },
            state: KeyState::Pressed,
            repeat: false,
        });
        app.handle(event);
        app.apply_pending();
    })
}

/// 帧像素指针（RGBA8）。首次调用（或未渲染过）时先渲染首帧，
/// 保证取到的缓冲区与 `demo_frame_size` 一致（HTML 首帧即 present）。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_ptr() -> *const u8 {
    with_app(|app| {
        if app.bitmap.is_empty() {
            app.render();
        }
        app.bitmap.as_ptr()
    })
}

/// 帧尺寸（位图像素）：width | (height << 16)。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_size() -> u32 {
    let w = (VIEWPORT.width * DPI_SCALE) as u32;
    let h = (VIEWPORT.height * DPI_SCALE) as u32;
    w | (h << 16)
}

/// 最近日志（UTF-8 指针 + 长度）。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_last_log_len() -> u32 {
    with_app(|app| app.log.last().map(|s| s.len() as u32).unwrap_or(0))
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_last_log_ptr() -> *const u8 {
    with_app(|app| {
        app.log
            .last()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use tela_contract::DrawPayload;

    #[test]
    fn button_label_overlays_its_background() {
        let tree = UiTree::new(button("button", "添加条目", 80.0)).expect("button tree is valid");
        let frame = tree
            .resolve(VIEWPORT, &DemoMeasurer, &HashMap::new())
            .expect("button resolves");

        assert!(matches!(
            frame.commands[0].payload,
            DrawPayload::RoundedRect { .. }
        ));
        assert!(matches!(
            frame.commands[1].payload,
            DrawPayload::Text { .. }
        ));

        let background = frame.commands[0].geometry;
        let label = frame.commands[1].geometry;
        assert!(label.x >= background.x && label.y >= background.y);
        assert!(label.x + label.w <= background.x + background.w);
        assert!(label.y + label.h <= background.y + background.h);
        assert!(
            ((label.x + label.w / 2.0) - (background.x + background.w / 2.0)).abs() < f32::EPSILON
        );
        assert!(
            ((label.y + label.h / 2.0) - (background.y + background.h / 2.0)).abs() < f32::EPSILON
        );
    }
}
