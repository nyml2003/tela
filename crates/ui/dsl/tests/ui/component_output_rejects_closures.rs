#[allow(unused_imports)]
use tela_ui_dsl::prelude::Text;
use tela_ui_dsl::{ViewBuild, ui};

fn main() {
    let mut build = ViewBuild::<()>::new();
    let _ = ui!(build {
        <Text value={"ready"} output={|_: ()| Some(())} />
    });
}
