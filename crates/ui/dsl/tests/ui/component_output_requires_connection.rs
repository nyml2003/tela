use tela_contract::{NodeKind, UiNode};
use tela_ui_dsl::{
    Children, ComponentAssembleContext, DslComponent, UiSpec, ViewBuild, ViewOutput, ViewResult, ui,
};

#[derive(Clone, Default)]
struct EmitsProps;

struct Emits;
struct EmitsSpec;

impl DslComponent for Emits {
    type UiSpec<A: 'static> = EmitsSpec;
}

impl<A: 'static> UiSpec<A> for EmitsSpec {
    type Props = EmitsProps;
    type State = ();
    type Event = ();
    type Output = u8;

    fn assemble<'a>(
        _context: &mut ComponentAssembleContext<'_, A>,
        _props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        Ok(ViewOutput::opaque(UiNode::new(NodeKind::View)))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut owned_build = ViewBuild::<()>::new();
    let build = &mut owned_build;
    let _ = ui!(build {
        <Emits />
    });
    Ok(())
}
