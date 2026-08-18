use tela_ui_dsl::{ViewBuild, ui};

fn main() {
    let mut build = ViewBuild::<()>::new();
    let _ = ui!(build {
        @provide("browse".to_owned());
        <Text>{"Browse"}</Text>
    });
}
