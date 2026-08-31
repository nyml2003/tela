//! `#[bind]` is an explicit read-only Signal edge, never a plain dynamic prop.

use tela_ui_dsl::{DslComponent, NodePresentation};

fn write_value(_value: &u32, _presentation: &mut NodePresentation) {}

#[derive(DslComponent)]
struct InvalidBinding {
    #[bind(paint = write_value)]
    value: u32,
}

fn main() {}
