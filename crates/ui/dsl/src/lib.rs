//! Tela Application / Composition runtime.
//!
//! This crate owns application-facing frame plans: explicit capability scopes, Signal watches,
//! typed action routing, and the `ui!` macro re-export. It deliberately depends only on Kernel
//! contract/core crates and never on Headless or a visual kit.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// `ui!` may also expand inside this package's examples and unit targets. Referencing the library
// through its package name keeps those expansions distinct from an example binary's `crate`.
extern crate self as tela_ui_dsl;

mod action;
mod context;
mod frame;
mod runtime;
mod signal;
mod view;

pub use action::{ActionFrame, ActionRegistry, DslTrigger, TextActionMap, with_context};
pub use context::{ProvidedValue, ViewContext};
pub use frame::{
    ActiveFrame, FrameCoordinator, FramePrepareError, FrameToken, FramedUiAction, PreparedFrame,
    ResolvedFrame,
};
pub use runtime::{ComponentRuntime, FrameInvalidator};
pub use signal::{Signal, SignalId, SignalSubscription};
pub use tela_ui_dsl_macros::ui;
pub use view::{
    ActionTarget, Body, IntoViewChild, ItemKey, ViewBuild, ViewBuildError, ViewChild, ViewNode,
    ViewOutput, ViewResult, ViewSite, into_view_child,
};

/// 由过程宏生成代码使用的稳定内部 re-export。
///
/// 应用不需要导入此模块；它避免宏展开要求每个使用 DSL 的 crate 都直接声明 Kernel
/// contract 依赖。
#[doc(hidden)]
pub mod __private {
    pub use tela_contract::*;
}

#[cfg(test)]
mod macro_hygiene_tests {
    use super::{ViewBuild, ViewOutput, ViewResult, ui};

    #[allow(dead_code)]
    #[derive(Clone)]
    struct Action;

    fn render(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
        ui!(build {
            <Frame>
                <Text>{"runtime crate"}</Text>
            </Frame>
        })
    }

    #[test]
    fn ui_macro_expands_inside_the_runtime_crate() {
        let mut build = ViewBuild::new();
        assert!(render(&mut build).is_ok());
    }
}
