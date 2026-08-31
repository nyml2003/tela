//! Named slots are static template declarations. A dynamic expression would turn one component
//! invocation into a runtime slot registry, so the macro rejects it before type checking.

use tela_ui_dsl::ui;

fn main() {
    let slot = "header";
    let _ = ui!(build {
        <Host>
            <Fragment slot={slot}>
                <Text value={"title"} />
            </Fragment>
        </Host>
    });
}
