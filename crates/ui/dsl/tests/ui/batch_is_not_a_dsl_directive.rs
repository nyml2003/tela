use tela_ui_dsl::{Signal, ViewBuild, ui};

fn main() {
    let mut build = ViewBuild::<()>::new();
    let count = Signal::new(0_u32);
    let _ = ui!(build {
        @batch {
            count.set(1);
        }
        <Text>{"Count"}</Text>
    });
}
