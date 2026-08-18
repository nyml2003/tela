use tela_ui_dsl::{ViewBuild, ui};

fn main() {
    let mut build = ViewBuild::<()>::new();
    let _ = ui!(build {
        <ActionTarget action={|| ()}>
            <Row clickable={true}></Row>
        </ActionTarget>
    });
}
