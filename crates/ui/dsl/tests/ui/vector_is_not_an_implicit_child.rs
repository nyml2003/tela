use tela_ui_dsl::{ViewBuild, ui};
use tela_ui_dsl::__private::{NodeKind, UiNode};

fn main() -> Result<(), tela_ui_dsl::ViewBuildError> {
    let nodes = vec![UiNode::new(NodeKind::Text)];
    let mut build = ViewBuild::<()>::new();
    let _ = ui!(build {
        <Column>
            { nodes }
        </Column>
    });
    Ok(())
}
