#[test]
fn ui_macro_rejects_invalid_declarative_forms() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
