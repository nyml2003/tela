//! Tela Application / Composition runtime.
//!
//! This crate owns application-facing frame plans: explicit capability scopes, Signal watches,
//! typed action routing, and the `ui!` macro re-export. It deliberately depends only on Kernel
//! contract/core crates and never on a visual kit.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// `ui!` may also expand inside this package's examples and unit targets. Referencing the library
// through its package name keeps those expansions distinct from an example binary's `crate`.
extern crate self as tela_ui_dsl;

mod animation;
mod candidate;
mod component;
mod computed;
mod context;
mod frame;
mod inbox;
mod interaction;
mod lifecycle;
mod memo;
mod owner;
mod runtime;
mod signal;
mod slots;
mod view;

pub use animation::{
    AnimationClock, AnimationController, AnimationSample, AnimationSchedule, Easing, Interpolate,
    TransitionExt, TransitionSpec, TransitionTarget,
};
pub use candidate::IgnoredOutput;
#[doc(hidden)]
pub use candidate::OutputConnection;
pub use component::prelude;
pub use component::{
    DslComponent, DslPropSlot, ErasedStructuralValue, For, ForContext, ForItems, ForKey,
    ForKeySlot, ForProps, ForRow, ForRowSlot, ForSpec, Show, ShowBranch, ShowBranchSlot,
    ShowContext, ShowProps, ShowSpec, ShowTest, ShowTestSlot, StructuralSource,
    StructuralSourceSlot, Switch, SwitchBranch, SwitchBranchSlot, SwitchContext, SwitchProps,
    SwitchRenderer, SwitchRendererSlot, SwitchSpec, TextColor, TextValue, UiSpec,
};
pub use computed::{Computed, computed, computed2, computed3};
pub use context::{ProvidedValue, ViewContext};
pub use frame::{
    ActiveFrame, ComponentDispatchError, ComponentEventDispatchReport, FrameCommitError,
    FrameCoordinator, FramePrepareError, FrameToken, PreparedFrame, ResolvedFrame,
    StaleSignalVersion,
};
pub use inbox::{ComponentEventInvalidator, ComponentEventSendError, ComponentEventSender};
pub use interaction::{FramedInteraction, InteractionIndex, LogicalPathIndex};
pub use lifecycle::{
    ComponentAssembleContext, ComponentOutcome, ComponentSetupContext, assemble_component,
    assemble_component_with_output, component_output_mapper, default_component_props,
};
pub use owner::{
    ComponentDispatch, ComponentEffectScope, ComponentHostInputRoutePlan, ComponentHostInputSpec,
    ComponentIdentity, ComponentInput, ComponentLifecycleEvent, component_host_input_route,
};
pub use runtime::{ComponentRuntime, DirtySet, FrameInvalidator};
pub use signal::{Signal, SignalId, SignalSnapshot, SignalSubscription, SignalWriter, signal};
pub use slots::{
    BindingSlot, BindingSlotDyn, BindingSlotKind, DuplicateListKey, ListFactory, ListReconcile,
    NodePresentation, SlotDamage, SlotGroup, SlotSelection, SlotSelector, StaticBindingSelector,
    StaticBindingTable,
};
pub use tela_ui_dsl_macros::{DslComponent, ui};
pub use view::{
    Body, Children, IntoViewChild, ItemKey, NamedSlot, RetainedChildren, RetainedSlots, SlotName,
    ViewBuild, ViewBuildError, ViewChild, ViewNode, ViewOutput, ViewResult, ViewSite,
    into_view_child,
};

/// 显式丢弃一个组件 Output 的 `@output` mapper。
///
/// 只能在调用点写成 `@output={ignore_output}`；它不会创建跨组件事件、AppAction 或
/// Effect。非空 Output 因而仍然必须由调用点明确处理或明确忽略。
pub fn ignore_output<T>(_output: T) -> IgnoredOutput {
    IgnoredOutput::default()
}

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
    use super::prelude::*;
    use super::{ForContext, ShowContext, ViewBuild, ViewOutput, ViewResult, ui};

    #[allow(dead_code)]
    #[derive(Clone)]
    struct Action;

    fn render(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
        ui!(build {
            <Frame>
                <Text value={"runtime crate"} />
            </Frame>
        })
    }

    #[derive(Clone)]
    struct RowValue {
        id: u64,
        label: String,
    }

    fn row_key(context: ForContext<RowValue>) -> String {
        context.item.id.to_string()
    }

    fn render_row(
        build: &mut ViewBuild<Action>,
        context: ForContext<RowValue>,
    ) -> ViewResult<ViewOutput<Action>> {
        ui!(build {
            <View>
                <Text value={context.item.label} />
            </View>
        })
    }

    #[derive(Clone)]
    struct Account {
        signed_in: bool,
    }

    fn is_signed_in(account: &Account) -> bool {
        account.signed_in
    }

    fn render_account(
        build: &mut ViewBuild<Action>,
        _context: ShowContext<Account>,
    ) -> ViewResult<ViewOutput<Action>> {
        ui!(build {
            <View><Text value={"account"} /></View>
        })
    }

    fn render_login(
        build: &mut ViewBuild<Action>,
        _context: ShowContext<Account>,
    ) -> ViewResult<ViewOutput<Action>> {
        ui!(build {
            <View><Text value={"login"} /></View>
        })
    }

    fn render_structural_components(
        build: &mut ViewBuild<Action>,
    ) -> ViewResult<ViewOutput<Action>> {
        let rows = vec![RowValue {
            id: 7,
            label: "seven".to_owned(),
        }];
        let account = Account { signed_in: true };
        ui!(build {
            <Column>
                <For each={rows} key={row_key} row={render_row} />
                <Show
                    value={account}
                    test={is_signed_in}
                    then={render_account}
                    fallback={render_login}
                />
            </Column>
        })
    }

    #[test]
    fn ui_macro_expands_inside_the_runtime_crate() {
        let mut build = ViewBuild::new();
        assert!(render(&mut build).is_ok());
    }

    #[test]
    fn registered_structural_components_use_normal_props() {
        let mut build = ViewBuild::new();
        assert!(render_structural_components(&mut build).is_ok());
    }
}
