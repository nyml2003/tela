//! A component invocation cannot have two independent builders for one static slot label.

use tela_ui_dsl::ui;

fn main() {
    let _ = ui!(build {
        <Host>
            <Fragment slot={"header"}>
                <Text value={"first"} />
            </Fragment>
            <Fragment slot={"header"}>
                <Text value={"second"} />
            </Fragment>
        </Host>
    });
}
