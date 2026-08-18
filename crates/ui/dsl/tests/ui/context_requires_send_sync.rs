use std::rc::Rc;

use tela_ui_dsl::{ViewBuild, ui};

fn main() -> Result<(), tela_ui_dsl::ViewBuildError> {
    let mut build = ViewBuild::<()>::new();
    let local_only = Rc::new("local capability".to_owned());
    let _ = ui!(build {
        @provide(local_only: Rc<String>);
        <Frame>
            <Text>{"never builds"}</Text>
        </Frame>
    });
    Ok(())
}
