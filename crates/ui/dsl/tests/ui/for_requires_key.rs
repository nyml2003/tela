use tela_ui_dsl::{ViewBuild, ui};

fn main() {
    let values = ["first"];
    let mut build = ViewBuild::<()>::new();
    let _ = ui!(build {
        <Column>
            <For each={values}>
                {|value| <Frame><Text value={value} /></Frame>}
            </For>
        </Column>
    });
}
