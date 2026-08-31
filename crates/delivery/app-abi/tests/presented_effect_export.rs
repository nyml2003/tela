//! ABI effect-release regression coverage through the real exported functions.

#![deny(unsafe_code)]

use std::cell::{Cell, RefCell};

use tela_app_abi::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameToken, AppPublication, AppStatus, OUTCOME_OK,
};
use tela_contract::{FrameDamage, RenderPlan, UiFrame, Viewport, WindowCommand};

struct TestGuest {
    next_token: u64,
    effects: Vec<AppEffect>,
    effect_drains: u32,
}

thread_local! {
    static TEST_GUEST: RefCell<TestGuest> = const {
        RefCell::new(TestGuest {
            next_token: 1,
            effects: Vec::new(),
            effect_drains: 0,
        })
    };
    static LAST_TOKEN: Cell<u64> = const { Cell::new(0) };
}

fn reset_guest() {
    TEST_GUEST.with(|guest| {
        *guest.borrow_mut() = TestGuest {
            next_token: 1,
            effects: Vec::new(),
            effect_drains: 0,
        };
    });
    LAST_TOKEN.with(|token| token.set(0));
}

fn with_test_guest<T>(f: impl FnOnce(&mut TestGuest) -> T) -> T {
    TEST_GUEST.with(|guest| f(&mut guest.borrow_mut()))
}

fn apply_test_event(_guest: &mut TestGuest, _event: AppEvent) -> AppDispatchOutcome {
    AppDispatchOutcome::IDLE
}

fn publish_test_guest(guest: &mut TestGuest) -> Result<AppPublication, String> {
    let token = AppFrameToken::new(guest.next_token)
        .ok_or_else(|| "test publication token must be non-zero".to_owned())?;
    guest.next_token = guest.next_token.saturating_add(1);
    LAST_TOKEN.with(|last| last.set(token.get()));
    Ok(AppPublication {
        token,
        frame: RenderPlan::from_flat_frame(UiFrame {
            viewport: Viewport {
                width: 1.0,
                height: 1.0,
            },
            commands: Vec::new(),
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        }),
        damage: FrameDamage::default(),
        spine: Vec::new(),
        retained_tree: None,
        status: AppStatus {
            frame_token: Some(token),
            ..AppStatus::default()
        },
    })
}

fn presented_test_guest(
    guest: &mut TestGuest,
    _token: AppFrameToken,
) -> Result<AppDispatchOutcome, String> {
    guest.effects.push(AppEffect::Window(WindowCommand::Close));
    Ok(AppDispatchOutcome::IDLE)
}

fn take_test_effects(guest: &mut TestGuest) -> Vec<AppEffect> {
    guest.effect_drains = guest.effect_drains.saturating_add(1);
    std::mem::take(&mut guest.effects)
}

tela_app_abi::export_guest! {
    reset = reset_guest;
    with_app = with_test_guest;
    apply = apply_test_event;
    publish = publish_test_guest;
    presented = presented_test_guest;
    effects = take_test_effects;
}

#[test]
fn effects_export_only_after_successful_presented_acknowledgement() {
    assert_ne!(tela_app_init() & OUTCOME_OK, 0);
    assert_eq!(tela_app_presented_effects_len(), 0);
    assert_ne!(tela_app_publish() & OUTCOME_OK, 0);
    let token = LAST_TOKEN.with(Cell::get);
    assert_ne!(token, 0);

    assert_eq!(
        tela_app_presented(token as u32, (token >> 32) as u32) & OUTCOME_OK,
        OUTCOME_OK
    );
    assert!(
        tela_app_presented_effects_len() > 0,
        "the post-present packet must be written after the callback drains the effect batch"
    );
    TEST_GUEST.with(|guest| {
        let guest = guest.borrow();
        assert!(guest.effects.is_empty());
        assert_eq!(guest.effect_drains, 1);
    });
    assert_eq!(
        tela_app_presented(token as u32, (token >> 32) as u32),
        0,
        "the macro must not drain the same successful acknowledgement twice"
    );
    TEST_GUEST.with(|guest| assert_eq!(guest.borrow().effect_drains, 1));
}
