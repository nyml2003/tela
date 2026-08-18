//! 无头 Root/Part 组合和完整通用组件目录。

use std::collections::{BTreeMap, BTreeSet};

use tela_contract::SemanticKey;

use crate::{ComponentPartPath, ComponentPath};

/// 通用组件目录的一级能力域。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentFamily {
    /// 基础视觉与布局能力。
    FoundationLayout,
    /// 数据录入与受控控件。
    DataEntry,
    /// 导航、集合和数据结构。
    NavigationCollections,
    /// 展示、反馈和浮层。
    DisplayFeedbackLayers,
}

/// 根组件的交互结构 archetype。
///
/// 目录中保留每个组件的公开名称；archetype 只复用已经证实相同的状态、部件和输入
/// 语义，不把视觉表达或业务含义收敛成一个万能组件。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentArchetype {
    /// 静态内容、布局或状态展示。
    Content,
    /// 可由点击、确认键或快捷键激活的动作控件。
    Action,
    /// 由 IME 与受控值驱动的文本输入。
    TextInput,
    /// 打开、选择或切换稳定项的选择控件。
    Selection,
    /// 按连续值、范围或可拖动手柄工作的控件。
    Range,
    /// 列表、树、导航或可分页集合。
    Collection,
    /// 受控 open 状态和取消/关闭语义的浮层。
    Layer,
    /// 以原始指针和 Kernel 手势仲裁为主的连续交互。
    Gesture,
}

/// 矩阵中某一输入或状态列是否适用于一个根组件。
///
/// 不适用不是静默跳过：必须附带稳定的语义理由，测试会验证理由非空。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixApplicability {
    /// 该列是这个组件的必测行为。
    Required,
    /// 该列不适用，并说明为什么。
    NotApplicable(&'static str),
}

impl MatrixApplicability {
    /// 返回该列是否需要实际行为测试。
    pub const fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }

    /// 返回不适用时的语义理由。
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Required => None,
            Self::NotApplicable(reason) => Some(reason),
        }
    }
}

/// 每个公开根组件都继承的一份可枚举验收矩阵。
///
/// 默认投影、受控状态和视觉参考始终必测；其余列由组件的行为 archetype 明确声明为
/// 必测或不适用，避免只给常见组件写 smoke test。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentMatrix {
    /// 默认 Root/Part 投影。
    pub default_projection: MatrixApplicability,
    /// Application 传入快照后的受控状态投影。
    pub controlled_state: MatrixApplicability,
    /// disabled 状态及其非交互保证。
    pub disabled: MatrixApplicability,
    /// loading 状态投影。
    pub loading: MatrixApplicability,
    /// error 状态投影。
    pub error: MatrixApplicability,
    /// 键盘与焦点路径。
    pub keyboard: MatrixApplicability,
    /// 触摸、原始指针或 Kernel 手势路径。
    pub touch: MatrixApplicability,
    /// HeadlessEvent 的类型化输出。
    pub events: MatrixApplicability,
    /// 树级和 Raster 视觉参考。
    pub visual_reference: MatrixApplicability,
}

/// 一个公开根组件族的目录记录。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentSpec {
    /// 所属一级能力域。
    pub family: ComponentFamily,
    /// 稳定的公开根组件名。
    pub name: &'static str,
}

/// Root 可声明的受控状态槽。
///
/// 这些槽描述组件语义，而不是 Application 的字段名。业务字段仍然通过 `BindId` 的
/// `ValueChange` 单独处理；例如 `Input` 的 `Value` 槽可以投影 `profile.name`，但
/// `ComponentRoot` 不保存那个业务路径。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ComponentState {
    /// 不透明内容或展示数据。
    Content,
    /// 单个受控值。
    Value,
    /// 当前选中项或选中集合。
    Selection,
    /// 浮层、折叠区或展开列表是否打开。
    Open,
    /// 可展开集合的展开项。
    Expanded,
    /// 可重排或可滚动的数据项快照。
    Items,
    /// 当前查询、过滤条件或搜索词。
    Query,
    /// 数值范围、双端范围或可调整尺寸。
    Range,
    /// 当前页或当前位置。
    CurrentPage,
    /// 进度、倒计时或状态数值。
    Progress,
    /// 禁用态。
    Disabled,
    /// 加载态。
    Loading,
    /// 错误态或错误说明。
    Error,
}

/// Root 对外声明的组件域事件类别。
///
/// `HeadlessEvent` 携带实际事件和载荷；此枚举是可静态检查的合约目录，避免任意
/// `UiAction` 被注册到不属于该组件的事件名称上。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ComponentEventKind {
    /// 激活通用操作。
    Activate,
    /// 选择稳定 item key。
    Select,
    /// 请求改变 open 状态。
    OpenChange,
    /// 请求关闭当前组件。
    Dismiss,
    /// 请求取消当前交互。
    Cancel,
    /// 字段值变化。
    ValueChange,
    /// IME 编辑、selection 或取消生命周期。
    TextInput,
    /// 焦点变化。
    FocusChange,
    /// hover 状态变化。
    HoverChange,
    /// 通用滚动意图。
    Scroll,
    /// 原始指针帧。
    Pointer,
    /// Kernel 仲裁后的手势。
    Gesture,
    /// 导航、页码或层级改变。
    Navigate,
    /// 请求刷新可滚动内容。
    Refresh,
}

/// 一个根组件在不同 Kit 中的内置 recipe 覆盖范围。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecipeSupport {
    /// desktop-kit 是否提供内置 recipe。
    pub desktop: bool,
    /// mobile-kit 是否提供内置 recipe。
    pub mobile: bool,
}

impl RecipeSupport {
    /// 两种形态都提供内置 recipe。
    pub const BOTH: Self = Self {
        desktop: true,
        mobile: true,
    };
    /// 仅 desktop-kit 提供内置 recipe。
    pub const DESKTOP: Self = Self {
        desktop: true,
        mobile: false,
    };
    /// 仅 mobile-kit 提供内置 recipe。
    pub const MOBILE: Self = Self {
        desktop: false,
        mobile: true,
    };
}

/// 一个根组件的可枚举无头契约。
///
/// 同一类组件可以共享稳定交互语义，但每一个 [`ComponentSpec`] 都通过
/// [`ComponentSpec::contract`] 取得一个明确契约，因而目录项不是没有行为的占位名称。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentContract {
    /// 可复用的交互结构；公开组件名仍由 [`ComponentSpec`] 保持。
    pub archetype: ComponentArchetype,
    /// Application 可以投影的受控状态槽。
    pub states: &'static [ComponentState],
    /// 一个可完成 Root 至少应具备的 Part 角色。
    ///
    /// `ComponentRoot::new` 可用于逐步组合，`ComponentRoot::validate_complete` 与 kit
    /// recipe 会要求这些角色全部存在，防止只有一个名称或空节点就被当成组件。
    pub required_parts: &'static [ComponentPartRole],
    /// Root 允许包含的具名部件角色。
    pub parts: &'static [ComponentPartRole],
    /// Root 可注册的组件事件。
    pub events: &'static [ComponentEventKind],
    /// 两种内置视觉形态的可用范围。
    pub recipes: RecipeSupport,
    /// 本根组件必须通过的统一验收矩阵。
    pub matrix: ComponentMatrix,
}

impl ComponentSpec {
    /// 返回该公开根组件的无头契约。
    pub fn contract(&self) -> &'static ComponentContract {
        component_contract(self.name)
    }

    /// 返回这个组件共享的交互结构 archetype。
    pub fn archetype(&self) -> ComponentArchetype {
        self.contract().archetype
    }

    /// 返回这个组件的可枚举验收矩阵。
    pub fn matrix(&self) -> &'static ComponentMatrix {
        &self.contract().matrix
    }

    /// 用默认受控状态和完整必需 Part 创建一个可直接交给 kit 的 Root。
    pub fn root(&'static self, path: impl Into<ComponentPath>) -> ComponentRoot {
        ComponentRoot::standard(self, path)
    }
}

/// Tela 的完整通用根组件目录。
///
/// 该常量只计算根组件族；Root、Part、事件类型和 kit 内部 recipe 不增加目录计数。
pub const COMPONENT_CATALOG: &[ComponentSpec] = &[
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Box",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Text",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Icon",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Image",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "ImagePreview",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "AspectRatio",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Flex",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Grid",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Layout",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Space",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Divider",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "ScrollArea",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Splitter",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Resizable",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Card",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Avatar",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Badge",
    },
    ComponentSpec {
        family: ComponentFamily::FoundationLayout,
        name: "Tag",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Button",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "IconButton",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "ToggleButton",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Link",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Form",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Field",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Input",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Textarea",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Search",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "PasswordInput",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "InputNumber",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "InputOtp",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Mentions",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Checkbox",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "CheckboxGroup",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Radio",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "RadioGroup",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Switch",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Select",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Combobox",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "AutoComplete",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Cascader",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "TreeSelect",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Transfer",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Slider",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "RangeSlider",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Stepper",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Rate",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "ColorPicker",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "DatePicker",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "TimePicker",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Calendar",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Picker",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "NumberKeyboard",
    },
    ComponentSpec {
        family: ComponentFamily::DataEntry,
        name: "Upload",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Tabs",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Segmented",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Menu",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "DropdownMenu",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "ContextMenu",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "NavigationMenu",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Breadcrumb",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Pagination",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Steps",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Anchor",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Affix",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "BackTop",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "NavBar",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Sidebar",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Tabbar",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "IndexBar",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "ActionBar",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Table",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "List",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "VirtualList",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Tree",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Collapse",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Carousel",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Swipe",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "SwipeCell",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Cell",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "CellGroup",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "FloatingActionButton",
    },
    ComponentSpec {
        family: ComponentFamily::NavigationCollections,
        name: "Sticky",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Descriptions",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Statistic",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Progress",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Circle",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Skeleton",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Empty",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Result",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Alert",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "NoticeBar",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Timeline",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Countdown",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "QRCode",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Watermark",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "TextEllipsis",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Dialog",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "AlertDialog",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Drawer",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Sheet",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Popup",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Popover",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Tooltip",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Overlay",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "ActionSheet",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "ShareSheet",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Toast",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Message",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Notification",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Loading",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Spin",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Popconfirm",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "Tour",
    },
    ComponentSpec {
        family: ComponentFamily::DisplayFeedbackLayers,
        name: "PullRefresh",
    },
];

/// 在目录中查询一个根组件的规格。
pub fn component_spec(name: &str) -> Option<&'static ComponentSpec> {
    COMPONENT_CATALOG.iter().find(|spec| spec.name == name)
}

/// 114 个公开 Root 的具名入口。
///
/// Rust 调用方通过 `components::Tabs::root("settings.tabs")` 这类 API 构造语义 Root，
/// 而不是把组件名作为任意字符串散落在应用中。视觉节点仍由各 kit 的内置 recipe 投影。
#[allow(dead_code)]
pub mod components {
    use super::{ComponentPath, ComponentRoot, component_spec};

    macro_rules! roots {
        ($($name:ident => $wire:literal),+ $(,)?) => {
            $(
                #[doc = concat!("`", $wire, "` 的无头 Root 入口。")]
                #[derive(Clone, Copy, Debug, Default)]
                pub struct $name;

                impl $name {
                    #[doc = concat!("创建 `", $wire, "` 的完整默认受控语义 Root。")]
                    pub fn root(path: impl Into<ComponentPath>) -> ComponentRoot {
                        ComponentRoot::standard(
                            component_spec($wire)
                                .expect("公开组件目录必须包含这个 Root"),
                            path,
                        )
                    }

                    #[doc = concat!("创建 `", $wire, "` 的空组合 Root，调用方必须在投影前补齐必需 Part。")]
                    pub fn compose(path: impl Into<ComponentPath>) -> ComponentRoot {
                        ComponentRoot::new(
                            component_spec($wire)
                                .expect("公开组件目录必须包含这个 Root"),
                            path,
                        )
                    }
                }
            )+
        };
    }

    roots! {
        Box => "Box",
        Text => "Text",
        Icon => "Icon",
        Image => "Image",
        ImagePreview => "ImagePreview",
        AspectRatio => "AspectRatio",
        Flex => "Flex",
        Grid => "Grid",
        Layout => "Layout",
        Space => "Space",
        Divider => "Divider",
        ScrollArea => "ScrollArea",
        Splitter => "Splitter",
        Resizable => "Resizable",
        Card => "Card",
        Avatar => "Avatar",
        Badge => "Badge",
        Tag => "Tag",
        Button => "Button",
        IconButton => "IconButton",
        ToggleButton => "ToggleButton",
        Link => "Link",
        Form => "Form",
        Field => "Field",
        Input => "Input",
        Textarea => "Textarea",
        Search => "Search",
        PasswordInput => "PasswordInput",
        InputNumber => "InputNumber",
        InputOtp => "InputOtp",
        Mentions => "Mentions",
        Checkbox => "Checkbox",
        CheckboxGroup => "CheckboxGroup",
        Radio => "Radio",
        RadioGroup => "RadioGroup",
        Switch => "Switch",
        Select => "Select",
        Combobox => "Combobox",
        AutoComplete => "AutoComplete",
        Cascader => "Cascader",
        TreeSelect => "TreeSelect",
        Transfer => "Transfer",
        Slider => "Slider",
        RangeSlider => "RangeSlider",
        Stepper => "Stepper",
        Rate => "Rate",
        ColorPicker => "ColorPicker",
        DatePicker => "DatePicker",
        TimePicker => "TimePicker",
        Calendar => "Calendar",
        Picker => "Picker",
        NumberKeyboard => "NumberKeyboard",
        Upload => "Upload",
        Tabs => "Tabs",
        Segmented => "Segmented",
        Menu => "Menu",
        DropdownMenu => "DropdownMenu",
        ContextMenu => "ContextMenu",
        NavigationMenu => "NavigationMenu",
        Breadcrumb => "Breadcrumb",
        Pagination => "Pagination",
        Steps => "Steps",
        Anchor => "Anchor",
        Affix => "Affix",
        BackTop => "BackTop",
        NavBar => "NavBar",
        Sidebar => "Sidebar",
        Tabbar => "Tabbar",
        IndexBar => "IndexBar",
        ActionBar => "ActionBar",
        Table => "Table",
        List => "List",
        VirtualList => "VirtualList",
        Tree => "Tree",
        Collapse => "Collapse",
        Carousel => "Carousel",
        Swipe => "Swipe",
        SwipeCell => "SwipeCell",
        Cell => "Cell",
        CellGroup => "CellGroup",
        FloatingActionButton => "FloatingActionButton",
        Sticky => "Sticky",
        Descriptions => "Descriptions",
        Statistic => "Statistic",
        Progress => "Progress",
        Circle => "Circle",
        Skeleton => "Skeleton",
        Empty => "Empty",
        Result => "Result",
        Alert => "Alert",
        NoticeBar => "NoticeBar",
        Timeline => "Timeline",
        Countdown => "Countdown",
        QRCode => "QRCode",
        Watermark => "Watermark",
        TextEllipsis => "TextEllipsis",
        Dialog => "Dialog",
        AlertDialog => "AlertDialog",
        Drawer => "Drawer",
        Sheet => "Sheet",
        Popup => "Popup",
        Popover => "Popover",
        Tooltip => "Tooltip",
        Overlay => "Overlay",
        ActionSheet => "ActionSheet",
        ShareSheet => "ShareSheet",
        Toast => "Toast",
        Message => "Message",
        Notification => "Notification",
        Loading => "Loading",
        Spin => "Spin",
        Popconfirm => "Popconfirm",
        Tour => "Tour",
        PullRefresh => "PullRefresh",
    }
}

/// Headless Root 可读取的受控状态快照。
#[derive(Clone, Debug, PartialEq)]
pub enum ControlledValue {
    /// 布尔状态，例如 open、checked 或 disabled。
    Bool(bool),
    /// 字符串状态，例如 value、query 或 active key。
    Text(String),
    /// 数值状态，例如 rate、progress 或 input number。
    Number(f64),
    /// 一组稳定 key，例如多选或展开集合。
    Keys(Vec<String>),
}

/// Root 内部一个具名部件的角色。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentPartRole {
    /// 根部件本身。
    Root,
    /// 打开、选择或激活的触发器。
    Trigger,
    /// 承载主要内容的区域。
    Content,
    /// 可重排集合中的一项。
    Item,
    /// 可见名称或标签。
    Label,
    /// 辅助说明文本。
    Description,
    /// 关闭或取消控件。
    Close,
    /// 文本或值输入控件。
    Input,
    /// 通用交互控件。
    Control,
    /// 当前状态指示器。
    Indicator,
    /// 顶部区域。
    Header,
    /// 底部区域。
    Footer,
    /// 被提升到浮层层级的内容。
    Overlay,
    /// 可拖动、可调整大小或范围选择的手柄。
    Handle,
}

impl ComponentPartRole {
    fn segment(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Trigger => "trigger",
            Self::Content => "content",
            Self::Item => "item",
            Self::Label => "label",
            Self::Description => "description",
            Self::Close => "close",
            Self::Input => "input",
            Self::Control => "control",
            Self::Indicator => "indicator",
            Self::Header => "header",
            Self::Footer => "footer",
            Self::Overlay => "overlay",
            Self::Handle => "handle",
        }
    }
}

const CONTENT_STATES: &[ComponentState] = &[
    ComponentState::Content,
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const CONTENT_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Content,
    ComponentPartRole::Label,
    ComponentPartRole::Description,
    ComponentPartRole::Indicator,
    ComponentPartRole::Header,
    ComponentPartRole::Footer,
];
const CONTENT_EVENTS: &[ComponentEventKind] = &[];
const CONTENT_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Content,
    ComponentPartRole::Label,
    ComponentPartRole::Indicator,
];

const ACTION_STATES: &[ComponentState] = &[
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const ACTION_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Control,
    ComponentPartRole::Label,
    ComponentPartRole::Description,
    ComponentPartRole::Indicator,
];
const ACTION_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::Activate,
    ComponentEventKind::FocusChange,
    ComponentEventKind::HoverChange,
];
const ACTION_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Label,
    ComponentPartRole::Indicator,
];

const INPUT_STATES: &[ComponentState] = &[
    ComponentState::Value,
    ComponentState::Query,
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const INPUT_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Input,
    ComponentPartRole::Control,
    ComponentPartRole::Label,
    ComponentPartRole::Description,
    ComponentPartRole::Indicator,
];
const INPUT_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::ValueChange,
    ComponentEventKind::TextInput,
    ComponentEventKind::FocusChange,
    ComponentEventKind::Cancel,
];
const INPUT_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Input,
    ComponentPartRole::Control,
    ComponentPartRole::Label,
    ComponentPartRole::Indicator,
];

const SELECT_STATES: &[ComponentState] = &[
    ComponentState::Value,
    ComponentState::Selection,
    ComponentState::Open,
    ComponentState::Items,
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const SELECT_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Content,
    ComponentPartRole::Item,
    ComponentPartRole::Label,
    ComponentPartRole::Description,
    ComponentPartRole::Overlay,
    ComponentPartRole::Control,
    ComponentPartRole::Indicator,
];
const SELECT_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::Select,
    ComponentEventKind::OpenChange,
    ComponentEventKind::Cancel,
    ComponentEventKind::FocusChange,
];
const SELECT_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Content,
    ComponentPartRole::Item,
    ComponentPartRole::Control,
    ComponentPartRole::Indicator,
];

const RANGE_STATES: &[ComponentState] = &[
    ComponentState::Value,
    ComponentState::Range,
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const RANGE_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Control,
    ComponentPartRole::Handle,
    ComponentPartRole::Label,
    ComponentPartRole::Indicator,
];
const RANGE_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::ValueChange,
    ComponentEventKind::Pointer,
    ComponentEventKind::Gesture,
    ComponentEventKind::FocusChange,
];
const RANGE_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Control,
    ComponentPartRole::Handle,
    ComponentPartRole::Indicator,
];

const COLLECTION_STATES: &[ComponentState] = &[
    ComponentState::Items,
    ComponentState::Selection,
    ComponentState::Expanded,
    ComponentState::CurrentPage,
    ComponentState::Query,
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const COLLECTION_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Content,
    ComponentPartRole::Item,
    ComponentPartRole::Label,
    ComponentPartRole::Description,
    ComponentPartRole::Control,
    ComponentPartRole::Indicator,
    ComponentPartRole::Header,
    ComponentPartRole::Footer,
    ComponentPartRole::Handle,
];
const COLLECTION_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::Activate,
    ComponentEventKind::Select,
    ComponentEventKind::Navigate,
    ComponentEventKind::Scroll,
    ComponentEventKind::Pointer,
    ComponentEventKind::Gesture,
    ComponentEventKind::FocusChange,
];
const COLLECTION_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Header,
    ComponentPartRole::Content,
    ComponentPartRole::Item,
    ComponentPartRole::Control,
    ComponentPartRole::Indicator,
];

const LAYER_STATES: &[ComponentState] = &[
    ComponentState::Open,
    ComponentState::Content,
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const LAYER_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Content,
    ComponentPartRole::Header,
    ComponentPartRole::Footer,
    ComponentPartRole::Close,
    ComponentPartRole::Overlay,
    ComponentPartRole::Control,
    ComponentPartRole::Label,
    ComponentPartRole::Description,
    ComponentPartRole::Indicator,
];
const LAYER_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::OpenChange,
    ComponentEventKind::Dismiss,
    ComponentEventKind::Cancel,
    ComponentEventKind::Activate,
    ComponentEventKind::FocusChange,
];
const LAYER_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Trigger,
    ComponentPartRole::Overlay,
    ComponentPartRole::Content,
    ComponentPartRole::Close,
    ComponentPartRole::Control,
    ComponentPartRole::Indicator,
];

const GESTURE_STATES: &[ComponentState] = &[
    ComponentState::Items,
    ComponentState::Selection,
    ComponentState::Range,
    ComponentState::Disabled,
    ComponentState::Loading,
    ComponentState::Error,
];
const GESTURE_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Content,
    ComponentPartRole::Item,
    ComponentPartRole::Handle,
    ComponentPartRole::Indicator,
    ComponentPartRole::Control,
];
const GESTURE_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::Pointer,
    ComponentEventKind::Gesture,
    ComponentEventKind::Activate,
];
const SCROLL_GESTURE_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::Pointer,
    ComponentEventKind::Gesture,
    ComponentEventKind::Scroll,
    ComponentEventKind::Activate,
];
const PULL_REFRESH_EVENTS: &[ComponentEventKind] = &[
    ComponentEventKind::Pointer,
    ComponentEventKind::Gesture,
    ComponentEventKind::Refresh,
    ComponentEventKind::Activate,
];
const GESTURE_REQUIRED_PARTS: &[ComponentPartRole] = &[
    ComponentPartRole::Root,
    ComponentPartRole::Content,
    ComponentPartRole::Item,
    ComponentPartRole::Handle,
    ComponentPartRole::Indicator,
];

const REQUIRED: MatrixApplicability = MatrixApplicability::Required;
const NO_STATIC_INPUT: MatrixApplicability =
    MatrixApplicability::NotApplicable("静态内容不声明焦点、键盘或指针交互");
const NO_STATIC_EVENT: MatrixApplicability =
    MatrixApplicability::NotApplicable("静态内容不向 Application 发送组件控制事件");
const NO_GESTURE_KEYBOARD: MatrixApplicability =
    MatrixApplicability::NotApplicable("连续手势根组件没有默认的键盘等价操作");

const CONTENT_MATRIX: ComponentMatrix = ComponentMatrix {
    default_projection: REQUIRED,
    controlled_state: REQUIRED,
    disabled: REQUIRED,
    loading: REQUIRED,
    error: REQUIRED,
    keyboard: NO_STATIC_INPUT,
    touch: NO_STATIC_INPUT,
    events: NO_STATIC_EVENT,
    visual_reference: REQUIRED,
};
const INTERACTIVE_MATRIX: ComponentMatrix = ComponentMatrix {
    default_projection: REQUIRED,
    controlled_state: REQUIRED,
    disabled: REQUIRED,
    loading: REQUIRED,
    error: REQUIRED,
    keyboard: REQUIRED,
    touch: REQUIRED,
    events: REQUIRED,
    visual_reference: REQUIRED,
};
const GESTURE_MATRIX: ComponentMatrix = ComponentMatrix {
    keyboard: NO_GESTURE_KEYBOARD,
    ..INTERACTIVE_MATRIX
};

const CONTENT_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::Content,
    states: CONTENT_STATES,
    required_parts: CONTENT_REQUIRED_PARTS,
    parts: CONTENT_PARTS,
    events: CONTENT_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: CONTENT_MATRIX,
};
const ACTION_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::Action,
    states: ACTION_STATES,
    required_parts: ACTION_REQUIRED_PARTS,
    parts: ACTION_PARTS,
    events: ACTION_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: INTERACTIVE_MATRIX,
};
const INPUT_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::TextInput,
    states: INPUT_STATES,
    required_parts: INPUT_REQUIRED_PARTS,
    parts: INPUT_PARTS,
    events: INPUT_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: INTERACTIVE_MATRIX,
};
const SELECT_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::Selection,
    states: SELECT_STATES,
    required_parts: SELECT_REQUIRED_PARTS,
    parts: SELECT_PARTS,
    events: SELECT_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: INTERACTIVE_MATRIX,
};
const RANGE_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::Range,
    states: RANGE_STATES,
    required_parts: RANGE_REQUIRED_PARTS,
    parts: RANGE_PARTS,
    events: RANGE_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: INTERACTIVE_MATRIX,
};
const COLLECTION_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::Collection,
    states: COLLECTION_STATES,
    required_parts: COLLECTION_REQUIRED_PARTS,
    parts: COLLECTION_PARTS,
    events: COLLECTION_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: INTERACTIVE_MATRIX,
};
const DESKTOP_COLLECTION_CONTRACT: ComponentContract = ComponentContract {
    recipes: RecipeSupport::DESKTOP,
    ..COLLECTION_CONTRACT
};
const MOBILE_COLLECTION_CONTRACT: ComponentContract = ComponentContract {
    recipes: RecipeSupport::MOBILE,
    ..COLLECTION_CONTRACT
};
const LAYER_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::Layer,
    states: LAYER_STATES,
    required_parts: LAYER_REQUIRED_PARTS,
    parts: LAYER_PARTS,
    events: LAYER_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: INTERACTIVE_MATRIX,
};
const MOBILE_LAYER_CONTRACT: ComponentContract = ComponentContract {
    recipes: RecipeSupport::MOBILE,
    ..LAYER_CONTRACT
};
const GESTURE_CONTRACT: ComponentContract = ComponentContract {
    archetype: ComponentArchetype::Gesture,
    states: GESTURE_STATES,
    required_parts: GESTURE_REQUIRED_PARTS,
    parts: GESTURE_PARTS,
    events: GESTURE_EVENTS,
    recipes: RecipeSupport::BOTH,
    matrix: GESTURE_MATRIX,
};
const SCROLL_GESTURE_CONTRACT: ComponentContract = ComponentContract {
    events: SCROLL_GESTURE_EVENTS,
    ..GESTURE_CONTRACT
};
const PULL_REFRESH_CONTRACT: ComponentContract = ComponentContract {
    events: PULL_REFRESH_EVENTS,
    ..GESTURE_CONTRACT
};

/// 返回目录项的完整无头契约。
///
/// 所有根组件都映射到一个有状态槽、部件和事件的契约。匹配按语义族收敛而非按当前
/// demo 是否使用收敛，保证新增应用无需重新定义底层交互模型。
pub fn component_contract(name: &str) -> &'static ComponentContract {
    match name {
        "Button" | "IconButton" | "ToggleButton" | "Link" | "FloatingActionButton" => {
            &ACTION_CONTRACT
        }
        "Input" | "Textarea" | "Search" | "PasswordInput" | "InputNumber" | "InputOtp"
        | "Mentions" | "Form" | "Field" | "Upload" => &INPUT_CONTRACT,
        "Checkbox" | "CheckboxGroup" | "Radio" | "RadioGroup" | "Switch" | "Select"
        | "Combobox" | "AutoComplete" | "Cascader" | "TreeSelect" | "Calendar" | "Picker"
        | "DatePicker" | "TimePicker" | "ColorPicker" => &SELECT_CONTRACT,
        "Slider" | "RangeSlider" | "Stepper" | "Rate" | "Splitter" | "Resizable" => &RANGE_CONTRACT,
        "Tabs" | "Segmented" | "Menu" | "DropdownMenu" | "ContextMenu" | "NavigationMenu"
        | "Breadcrumb" | "Pagination" | "Steps" | "Anchor" | "Affix" | "BackTop" | "NavBar"
        | "Sidebar" | "Table" | "List" | "VirtualList" | "Tree" | "Collapse" | "Cell"
        | "CellGroup" | "Sticky" | "Descriptions" | "Timeline" => &COLLECTION_CONTRACT,
        "Transfer" => &DESKTOP_COLLECTION_CONTRACT,
        "Tabbar" | "IndexBar" | "ActionBar" | "SwipeCell" | "NumberKeyboard" => {
            &MOBILE_COLLECTION_CONTRACT
        }
        "Carousel" | "Swipe" | "ImagePreview" => &GESTURE_CONTRACT,
        "ScrollArea" => &SCROLL_GESTURE_CONTRACT,
        "PullRefresh" => &PULL_REFRESH_CONTRACT,
        "Dialog" | "AlertDialog" | "Drawer" | "Sheet" | "Popup" | "Popover" | "Tooltip"
        | "Overlay" | "ActionSheet" | "ShareSheet" | "Popconfirm" | "Tour" => &LAYER_CONTRACT,
        "Toast" | "Message" | "Notification" => &MOBILE_LAYER_CONTRACT,
        "Box" | "Text" | "Icon" | "Image" | "AspectRatio" | "Flex" | "Grid" | "Layout"
        | "Space" | "Divider" | "Card" | "Avatar" | "Badge" | "Tag" | "Progress" | "Circle"
        | "Skeleton" | "Empty" | "Result" | "Alert" | "NoticeBar" | "Countdown" | "QRCode"
        | "Watermark" | "TextEllipsis" | "Loading" | "Spin" | "Statistic" => &CONTENT_CONTRACT,
        _ => &CONTENT_CONTRACT,
    }
}

/// 一个 Headless Root 的稳定部件记录。
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentPart {
    path: ComponentPartPath,
    role: ComponentPartRole,
    key: SemanticKey,
    disabled: bool,
}

impl ComponentPart {
    /// 返回稳定部件路径。
    pub fn path(&self) -> &ComponentPartPath {
        &self.path
    }

    /// 返回部件角色。
    pub fn role(&self) -> ComponentPartRole {
        self.role
    }

    /// 返回用于重排稳定性的语义 key。
    pub fn key(&self) -> &SemanticKey {
        &self.key
    }

    /// 返回当前受控 disabled 状态。
    pub fn disabled(&self) -> bool {
        self.disabled
    }
}

/// 一个不包含视觉节点的组件 Root。
///
/// kit 使用它的受控状态和具名部件生成不同形态的视觉树；Application 使用路径和部件
/// key 连接 Root/Part 语义与 EventRegistry。Application 的 Signal watch 属于
/// `tela-ui-dsl`，并以 Kernel `SemanticKey` 建立订阅。
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentRoot {
    spec: &'static ComponentSpec,
    path: ComponentPath,
    controlled: BTreeMap<ComponentState, ControlledValue>,
    parts: Vec<ComponentPart>,
}

impl ComponentRoot {
    /// 创建指定目录根组件的语义 Root。
    ///
    /// 这是逐部件组合入口；需要交给 kit 或 `EventRegistry::register_component` 的 Root
    /// 应改用 [`Self::standard`]，或在最后通过 [`Self::validate_complete`] 验证。
    pub fn new(spec: &'static ComponentSpec, path: impl Into<ComponentPath>) -> Self {
        Self {
            spec,
            path: path.into(),
            controlled: BTreeMap::new(),
            parts: Vec::new(),
        }
    }

    /// 创建一个包含默认受控状态和必需 Part 的完整 Root。
    ///
    /// 默认 Part 是可被 kit 投影和 EventRegistry 路由的最小真实组合，不是只带组件名称
    /// 的展示节点。调用方可以继续追加同角色、不同稳定 key 的 Item/Control。
    pub fn standard(spec: &'static ComponentSpec, path: impl Into<ComponentPath>) -> Self {
        let mut root = Self::new(spec, path);
        for state in spec.contract().states {
            root.controlled
                .insert(*state, default_controlled_value(*state, spec.name));
        }
        for role in spec.contract().required_parts {
            for suffix in default_part_suffixes(*role) {
                let key = SemanticKey(format!("{}.{}.{}", root.path, role.segment(), suffix));
                let path = root.path.part(role.segment()).item(suffix);
                root.parts.push(ComponentPart {
                    path,
                    role: *role,
                    key,
                    disabled: false,
                });
            }
        }
        root
    }

    /// 写入一个由 Application 提供的受控状态快照。
    ///
    /// 状态槽由该组件的 [`ComponentContract`] 固定，不能用业务字段字符串临时扩展。
    /// 这让同一 Root 可以被 desktop/mobile recipe 一致投影，而字段业务绑定仍留给
    /// `BindId` 的 `ValueChange`。
    pub fn state(mut self, state: ComponentState, value: ControlledValue) -> Self {
        self.controlled.insert(state, value);
        self
    }

    /// 添加一个启用的具名部件。
    pub fn part(self, role: ComponentPartRole, key: SemanticKey) -> Self {
        self.part_with_disabled(role, key, false)
    }

    /// 添加一个具名部件，并显式给出它的 disabled 状态。
    pub fn part_with_disabled(
        mut self,
        role: ComponentPartRole,
        key: SemanticKey,
        disabled: bool,
    ) -> Self {
        let path = self.path.part(role.segment()).item(&key.0);
        self.parts.push(ComponentPart {
            path,
            role,
            key,
            disabled,
        });
        self
    }

    /// 返回目录规格。
    pub fn spec(&self) -> &'static ComponentSpec {
        self.spec
    }

    /// 返回 Root 稳定路径。
    pub fn path(&self) -> &ComponentPath {
        &self.path
    }

    /// 查询一个受控状态快照。
    pub fn state_value(&self, state: ComponentState) -> Option<&ControlledValue> {
        self.controlled.get(&state)
    }

    /// 返回 Root 受控的 disabled 状态。
    pub fn is_disabled(&self) -> bool {
        matches!(
            self.state_value(ComponentState::Disabled),
            Some(ControlledValue::Bool(true))
        )
    }

    /// 返回 Root 受控的 loading 状态。
    pub fn is_loading(&self) -> bool {
        matches!(
            self.state_value(ComponentState::Loading),
            Some(ControlledValue::Bool(true))
        )
    }

    /// 返回 Root 受控的 open 状态。
    pub fn is_open(&self) -> bool {
        matches!(
            self.state_value(ComponentState::Open),
            Some(ControlledValue::Bool(true))
        )
    }

    /// 返回 Root 受控的错误说明；空字符串表示没有错误说明。
    pub fn error_message(&self) -> Option<&str> {
        match self.state_value(ComponentState::Error) {
            Some(ControlledValue::Text(message)) if !message.is_empty() => Some(message),
            _ => None,
        }
    }

    /// 返回部件列表。
    pub fn parts(&self) -> &[ComponentPart] {
        &self.parts
    }

    /// 校验 Root 内部不存在重复的稳定 item key。
    pub fn validate(&self) -> Result<(), HeadlessBuildError> {
        let contract = self.spec.contract();
        for state in self.controlled.keys().copied() {
            if !contract.states.contains(&state) {
                return Err(HeadlessBuildError::UnsupportedState {
                    component: self.spec.name,
                    state,
                });
            }
        }
        let mut keys = BTreeSet::new();
        for part in &self.parts {
            if !contract.parts.contains(&part.role) {
                return Err(HeadlessBuildError::UnsupportedPart {
                    component: self.spec.name,
                    role: part.role,
                });
            }
            if !keys.insert(part.key.clone()) {
                return Err(HeadlessBuildError::DuplicatePartKey(part.key.clone()));
            }
        }
        Ok(())
    }

    /// 校验这个 Root 已经具备可投影组件的所有必需 Part。
    ///
    /// 允许一个角色出现多次（例如多个 Item），但每个 `required_parts` 角色必须至少有
    /// 一个实例。这样在组合阶段仍可逐步构建，最终进入 kit/事件矩阵时不会把空壳当成
    /// 完整组件。
    pub fn validate_complete(&self) -> Result<(), HeadlessBuildError> {
        self.validate()?;
        for role in self.spec.contract().required_parts {
            if !self.parts.iter().any(|part| part.role == *role) {
                return Err(HeadlessBuildError::MissingRequiredPart {
                    component: self.spec.name,
                    role: *role,
                });
            }
        }
        Ok(())
    }
}

fn default_controlled_value(state: ComponentState, component: &str) -> ControlledValue {
    match state {
        ComponentState::Content => ControlledValue::Text(component.to_owned()),
        ComponentState::Value | ComponentState::Query | ComponentState::Error => {
            ControlledValue::Text(String::new())
        }
        ComponentState::Selection | ComponentState::Expanded => ControlledValue::Keys(Vec::new()),
        ComponentState::Open | ComponentState::Disabled | ComponentState::Loading => {
            ControlledValue::Bool(false)
        }
        ComponentState::Items => {
            ControlledValue::Keys(vec!["primary".to_owned(), "secondary".to_owned()])
        }
        ComponentState::Range | ComponentState::Progress => ControlledValue::Number(0.0),
        ComponentState::CurrentPage => ControlledValue::Number(1.0),
    }
}

fn default_part_suffixes(role: ComponentPartRole) -> Vec<&'static str> {
    match role {
        // 两个稳定 item 让默认集合具备实际重排/选择的最小集合，而不是单项假列表。
        ComponentPartRole::Item => vec!["primary", "secondary"],
        _ => vec![role.segment()],
    }
}

/// 无头组件结构不满足稳定组合约束时的错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadlessBuildError {
    /// 同一个 Root 内部的两个部件使用了相同的稳定 item key。
    DuplicatePartKey(SemanticKey),
    /// Root 投影了该组件契约中不存在的受控状态槽。
    UnsupportedState {
        /// 公开根组件名。
        component: &'static str,
        /// 不被该根组件声明的状态槽。
        state: ComponentState,
    },
    /// Root 投影了该组件契约中不存在的部件角色。
    UnsupportedPart {
        /// 公开根组件名。
        component: &'static str,
        /// 不被该根组件声明的部件角色。
        role: ComponentPartRole,
    },
    /// Root 在最终投影前缺少该组件声明为必需的 Part。
    MissingRequiredPart {
        /// 公开根组件名。
        component: &'static str,
        /// 缺少的部件角色。
        role: ComponentPartRole,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tela_contract::SemanticKey;

    use super::{
        COMPONENT_CATALOG, ComponentFamily, ComponentPartRole, ComponentRoot, ComponentState,
        ControlledValue, HeadlessBuildError, MatrixApplicability, component_spec, components,
    };

    #[test]
    fn catalog_has_all_root_component_families_once() {
        assert_eq!(COMPONENT_CATALOG.len(), 114);
        assert_eq!(
            COMPONENT_CATALOG
                .iter()
                .filter(|spec| spec.family == ComponentFamily::FoundationLayout)
                .count(),
            18
        );
        assert_eq!(
            COMPONENT_CATALOG
                .iter()
                .filter(|spec| spec.family == ComponentFamily::DataEntry)
                .count(),
            35
        );
        assert_eq!(
            COMPONENT_CATALOG
                .iter()
                .filter(|spec| spec.family == ComponentFamily::NavigationCollections)
                .count(),
            29
        );
        assert_eq!(
            COMPONENT_CATALOG
                .iter()
                .filter(|spec| spec.family == ComponentFamily::DisplayFeedbackLayers)
                .count(),
            32
        );
        let names: BTreeSet<_> = COMPONENT_CATALOG.iter().map(|spec| spec.name).collect();
        assert_eq!(names.len(), COMPONENT_CATALOG.len());
    }

    #[test]
    fn every_catalog_entry_has_a_complete_contract_and_explicit_matrix() {
        for spec in COMPONENT_CATALOG {
            let contract = spec.contract();
            assert!(
                !contract.states.is_empty(),
                "{} must declare controlled state slots",
                spec.name
            );
            assert!(
                !contract.parts.is_empty(),
                "{} must declare component parts",
                spec.name
            );
            assert!(
                !contract.required_parts.is_empty(),
                "{} must declare required component parts",
                spec.name
            );
            assert!(
                contract.recipes.desktop || contract.recipes.mobile,
                "{} must be projected by at least one kit",
                spec.name
            );
            for column in [
                contract.matrix.default_projection,
                contract.matrix.controlled_state,
                contract.matrix.disabled,
                contract.matrix.loading,
                contract.matrix.error,
                contract.matrix.keyboard,
                contract.matrix.touch,
                contract.matrix.events,
                contract.matrix.visual_reference,
            ] {
                if let MatrixApplicability::NotApplicable(reason) = column {
                    assert!(
                        !reason.trim().is_empty(),
                        "{} cannot silently skip a matrix column",
                        spec.name
                    );
                }
            }
            let root = spec.root(format!("matrix.{}", spec.name));
            assert_eq!(
                root.validate_complete(),
                Ok(()),
                "{} must have a usable default root",
                spec.name
            );
        }
    }

    #[test]
    fn root_keeps_controlled_state_and_stable_part_paths() {
        let tabs = components::Tabs::root("settings.tabs")
            .state(
                ComponentState::Selection,
                ControlledValue::Text("appearance".to_owned()),
            )
            .part(
                ComponentPartRole::Trigger,
                SemanticKey("appearance".to_owned()),
            )
            .part_with_disabled(
                ComponentPartRole::Trigger,
                SemanticKey("advanced".to_owned()),
                true,
            );

        assert_eq!(
            tabs.state_value(ComponentState::Selection),
            Some(&ControlledValue::Text("appearance".to_owned()))
        );
        let appearance = tabs
            .parts()
            .iter()
            .find(|part| part.key() == &SemanticKey("appearance".to_owned()))
            .expect("explicit appearance trigger");
        let advanced = tabs
            .parts()
            .iter()
            .find(|part| part.key() == &SemanticKey("advanced".to_owned()))
            .expect("explicit disabled trigger");
        assert_eq!(
            appearance.path().as_str(),
            r#"settings.tabs.trigger["appearance"]"#
        );
        assert!(advanced.disabled());
        assert_eq!(tabs.validate_complete(), Ok(()));
    }

    #[test]
    fn root_rejects_states_and_parts_outside_its_declared_contract() {
        let invalid_state = components::Button::root("toolbar.new")
            .state(ComponentState::Open, ControlledValue::Bool(true));
        assert_eq!(
            invalid_state.validate(),
            Err(HeadlessBuildError::UnsupportedState {
                component: "Button",
                state: ComponentState::Open,
            })
        );

        let invalid_part = components::Text::root("detail.title")
            .part(ComponentPartRole::Handle, SemanticKey("resize".to_owned()));
        assert_eq!(
            invalid_part.validate(),
            Err(HeadlessBuildError::UnsupportedPart {
                component: "Text",
                role: ComponentPartRole::Handle,
            })
        );
    }

    #[test]
    fn duplicate_part_keys_fail_before_a_kit_projects_them() {
        let duplicate = ComponentRoot::new(component_spec("Tabs").expect("Tabs"), "settings.tabs")
            .part(ComponentPartRole::Trigger, SemanticKey("same".to_owned()))
            .part(ComponentPartRole::Content, SemanticKey("same".to_owned()));

        assert_eq!(
            duplicate.validate(),
            Err(HeadlessBuildError::DuplicatePartKey(SemanticKey(
                "same".to_owned()
            )))
        );
    }

    #[test]
    fn incomplete_composition_cannot_be_projected_as_a_complete_component() {
        let incomplete = components::Dialog::compose("settings.dialog").part(
            ComponentPartRole::Trigger,
            SemanticKey("settings.dialog.trigger".to_owned()),
        );
        assert_eq!(
            incomplete.validate_complete(),
            Err(HeadlessBuildError::MissingRequiredPart {
                component: "Dialog",
                role: ComponentPartRole::Root,
            })
        );
    }
}
