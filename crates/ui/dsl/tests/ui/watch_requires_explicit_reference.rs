use tela_ui_dsl::{Signal, ViewBuild, ui};

fn main() {
    let mut build = ViewBuild::<()>::new();
    let count = Signal::new(0_u32);
    let _ = ui!(build {
        @watch(value, count);
        <Text>{"Count"}</Text>
    });
}
