use tela_ui_dsl::FrameCoordinator;

enum NonCloneAction {
    Save,
}

fn main() {
    let _ = FrameCoordinator::<NonCloneAction>::new();
}
