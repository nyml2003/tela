use tela_ui_dsl::{ViewBuild, ui};

fn main() {
    let mut build = ViewBuild::<()>::new();
    let _ = ui!(build {
        <ActionTarget on_input={tela_ui_dsl::with_context(7_u32, |id, value| {
            let _ = (id, value);
        })}>
            <Frame>
                <Text>{"edit"}</Text>
            </Frame>
        </ActionTarget>
    });
}
