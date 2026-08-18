//! Tela 的无头组件语义层。
//!
//! 本 crate 定义稳定的组件 Root / Part 路径与 EventRegistry。Application 的 Signal、
//! 帧期失效调度与 DSL 动作协调属于 `tela-ui-dsl`，不再由 Headless 持有。
//! 它不依赖任何视觉 kit、Renderer、Target 或业务 crate。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod component;
mod event;
mod path;

pub use component::{
    COMPONENT_CATALOG, ComponentArchetype, ComponentContract, ComponentEventKind, ComponentFamily,
    ComponentMatrix, ComponentPart, ComponentPartRole, ComponentRoot, ComponentSpec,
    ComponentState, ControlledValue, HeadlessBuildError, MatrixApplicability, RecipeSupport,
    component_contract, component_spec, components,
};
pub use event::{
    ActionTrigger, EventFrame, EventRegistrationError, EventRegistry, HeadlessEvent, RoutedEvent,
};
pub use path::{ComponentPartPath, ComponentPath};
