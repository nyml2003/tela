use tela_ui_dsl::{ViewBuild, ui};

fn main() {
    let mut build = ViewBuild::<()>::new();
    let _ = ui!(build {
        <Text>{"first"}</Text>
        @provide("late".to_owned(): String);
    });
}
