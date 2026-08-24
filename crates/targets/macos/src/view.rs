//! AppKit content view, normalized input delivery, and main-thread shell state.

use std::{
    cell::{Cell, OnceCell, RefCell},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSCursor, NSEvent, NSFont, NSResponder, NSTextField,
    NSTrackingRectTag, NSView,
};
use objc2_foundation::{
    MainThreadMarker, NSObject, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString,
};
use objc2_quartz_core::CADisplayLink;
use std::rc::Rc;

use tela_app_abi::{
    AppEvent, AppFrameInput, AppFrameToken, AppPointerEvent, AppPointerKind, AppPointerPhase,
    CursorKind,
};
use tela_bridge::BridgeDispatcher;
use tela_desktop_runtime::bridge::{common::BuildConstants, process_bridge_requests};

use crate::providers::MacMetrics;
use tela_contract::UiFrame;
use tela_desktop_runtime::{
    DeviceLossAction, GuestRuntime, PlatformLaunchOptions, ShellLifecycle, ShellPhase,
    TextChannelAction,
};

use crate::{
    gpu::{ClientMetrics, DeviceLossReport, GpuSession, RenderOutcome},
    input, startup,
};

pub(crate) struct TelaViewIvars {
    state: RefCell<ViewState>,
    tracking_rect: Cell<Option<NSTrackingRectTag>>,
    display_link: OnceCell<Retained<CADisplayLink>>,
}

struct ViewState {
    lifecycle: ShellLifecycle,
    runtime: Option<GuestRuntime>,
    frame: Option<UiFrame>,
    frame_token: Option<AppFrameToken>,
    presented_frame_token: Option<AppFrameToken>,
    gpu: Option<GpuSession>,
    startup_rx: Option<Receiver<Result<GuestRuntime, String>>>,
    startup_cancel: Arc<AtomicBool>,
    device_loss: Arc<Mutex<Option<DeviceLossReport>>>,
    gpu_generation: u64,
    surface_retry_deadline: Option<Instant>,
    status_label: Retained<NSTextField>,
    terminal_error: Option<String>,
    bridge: Option<BridgeDispatcher>,
    bridge_metrics: Rc<RefCell<MacMetrics>>,
    animation_epoch: Instant,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TelaMacOSContentView"]
    #[ivars = TelaViewIvars]
    pub(crate) struct TelaView;

    impl TelaView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            // The shared `UiFrame` and browser host use upper-left logical coordinates.
            true
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            self.refresh_tracking_rect();
            self.resize_from_appkit();
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            // SAFETY: this has the exact NSView selector and ABI. Resizing the superclass first
            // ensures `bounds` and backing conversion read the new AppKit geometry below.
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.refresh_tracking_rect();
            self.resize_from_appkit();
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let mut state = self.ivars().state.borrow_mut();
            state.lifecycle.begin_paint();
            if let Err(error) = state.paint(self) {
                state.fail_terminal(self, error);
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            self.window_focus_changed(true);
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            self.window_focus_changed(false);
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.pointer_down(event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.pointer_up(event);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.pointer_move(event, 0);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.pointer_move(event, 1);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.pointer_left_client();
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            self.pointer_scroll(event);
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let mut state = self.ivars().state.borrow_mut();
            if let Err(error) = state.key_down(self, event) {
                state.fail_terminal(self, error);
            }
        }

        // AppKit otherwise consumes some Control-key chords before this view can normalize them.
        #[unsafe(method(_wantsKeyDownForEvent:))]
        fn wants_key_down_for_event(&self, _event: &NSEvent) -> bool {
            true
        }

        #[unsafe(method(animationFrame:))]
        fn animation_frame(&self, display_link: &CADisplayLink) {
            let active = {
                let mut state = self.ivars().state.borrow_mut();
                if let Err(error) = state.tick_animation(self) {
                    state.fail_terminal(self, error);
                }
                state.animation_active()
            };
            display_link.setPaused(!active);
        }
    }
);

impl TelaView {
    /// Creates a main-thread view and immediately starts the one allowed startup worker.
    pub(crate) fn new(mtm: MainThreadMarker, options: PlatformLaunchOptions) -> Retained<Self> {
        let status_label = make_status_label(mtm);
        let (startup_rx, startup_cancel, startup_error) = match startup::start(options) {
            Ok(worker) => (Some(worker.receiver), worker.cancel, None),
            Err(error) => (None, Arc::new(AtomicBool::new(true)), Some(error)),
        };
        let this = mtm.alloc().set_ivars(TelaViewIvars {
            state: RefCell::new(ViewState {
                lifecycle: ShellLifecycle::new(),
                runtime: None,
                frame: None,
                frame_token: None,
                presented_frame_token: None,
                gpu: None,
                startup_rx,
                startup_cancel,
                device_loss: Arc::new(Mutex::new(None)),
                gpu_generation: 0,
                surface_retry_deadline: None,
                status_label,
                terminal_error: None,
                bridge_metrics: Rc::new(RefCell::new(MacMetrics::default())),
                bridge: None,
                animation_epoch: Instant::now(),
            }),
            tracking_rect: Cell::new(None),
            display_link: OnceCell::new(),
        });
        // SAFETY: this invokes NSView's ordinary `init` method on a freshly allocated object with
        // Rust ivars already installed, following objc2's documented custom-view construction.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // SAFETY: `animationFrame:` is declared on this exact object above. Associating the
        // display link with the view makes AppKit follow the view's current screen refresh source.
        let display_link =
            unsafe { this.displayLinkWithTarget_selector(&this, sel!(animationFrame:)) };
        display_link.setPaused(true);
        // SAFETY: the view and callback are main-thread-only and the link is invalidated during
        // close before AppKit tears down the view.
        unsafe {
            display_link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
        }
        let _ = this.ivars().display_link.set(display_link);
        this.setWantsLayer(true);
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let status_label = this.ivars().state.borrow().status_label.clone();
        this.addSubview(&status_label);
        if let Some(error) = startup_error {
            this.startup_failed(error);
        }
        this
    }

    /// Called by the AppKit run-loop timer; it only transfers already-finished background work.
    pub(crate) fn poll_background_work(&self) {
        let active = {
            let mut state = self.ivars().state.borrow_mut();
            state.receive_startup_result(self);
            state.receive_device_loss(self);
            state.poll_surface_retry(self);
            state.animation_active()
        };
        if let Some(display_link) = self.ivars().display_link.get() {
            display_link.setPaused(!active);
        }
    }

    /// Cancels the worker and releases the portable input channel before AppKit tears down the view.
    pub(crate) fn begin_close(&self) {
        let mut state = self.ivars().state.borrow_mut();
        if state.lifecycle.phase() == ShellPhase::Closing {
            return;
        }
        state.startup_cancel.store(true, Ordering::Release);
        if let Err(error) = state.pointer_left_client(self) {
            eprintln!("tela-macos-host: close pointer cleanup: {error}");
        }
        state.cancel_surface_retry();
        let action = state.lifecycle.begin_close();
        if let Err(error) = state.dispatch_text_channel_action(action) {
            eprintln!("tela-macos-host: close text channel: {error}");
        }
        state.presented_frame_token = None;
        state.gpu = None;
        if let Some(display_link) = self.ivars().display_link.get() {
            display_link.setPaused(true);
            display_link.invalidate();
        }
    }

    fn resize_from_appkit(&self) {
        let mut state = self.ivars().state.borrow_mut();
        if let Err(error) = state.resize(self) {
            state.fail_terminal(self, error);
        }
    }

    fn window_focus_changed(&self, focused: bool) {
        let mut state = self.ivars().state.borrow_mut();
        if state.lifecycle.window_focused() == focused {
            return;
        }
        if !focused && let Err(error) = state.pointer_left_client(self) {
            state.fail_terminal(self, error);
            return;
        }
        if let Err(error) = state.synchronize_text_channel(Some(focused)) {
            state.fail_terminal(self, error);
        }
    }

    /// Records a key-window edge from `NSWindowDelegate`, even when AppKit retains the same first
    /// responder while the application moves to the background.
    pub(crate) fn window_key_changed(&self, focused: bool) {
        self.window_focus_changed(focused);
    }

    fn pointer_down(&self, event: &NSEvent) {
        self.dispatch_mouse_pointer(event, AppPointerPhase::Down, 1);
    }

    fn pointer_up(&self, event: &NSEvent) {
        self.dispatch_mouse_pointer(event, AppPointerPhase::Up, 0);
    }

    fn pointer_move(&self, event: &NSEvent, buttons: u16) {
        self.dispatch_mouse_pointer(event, AppPointerPhase::Move, buttons);
    }

    fn dispatch_mouse_pointer(&self, event: &NSEvent, phase: AppPointerPhase, buttons: u16) {
        let Some((x, y)) = self.event_point(event) else {
            return;
        };
        let pointer = AppPointerEvent::new(
            0,
            AppPointerKind::Mouse,
            phase,
            x,
            y,
            buttons,
            appkit_timestamp_micros(event),
            0.0,
            0.0,
        );
        let mut state = self.ivars().state.borrow_mut();
        if let Err(error) = state.pointer(self, AppFrameInput::Pointer(pointer)) {
            state.fail_terminal(self, error);
        }
    }

    fn pointer_scroll(&self, event: &NSEvent) {
        let Some((x, y)) = self.event_point(event) else {
            return;
        };
        let scale = if event.hasPreciseScrollingDeltas() {
            1.0
        } else {
            48.0
        };
        let mut state = self.ivars().state.borrow_mut();
        if let Err(error) = state.pointer(
            self,
            AppFrameInput::Pointer(AppPointerEvent::new(
                0,
                AppPointerKind::Mouse,
                AppPointerPhase::Scroll,
                x,
                y,
                0,
                appkit_timestamp_micros(event),
                event.scrollingDeltaX() as f32 * scale,
                // AppKit reports upward-positive deltas; the portable client convention matches
                // browser/Win32 downward-positive content motion.
                -(event.scrollingDeltaY() as f32 * scale),
            )),
        ) {
            state.fail_terminal(self, error);
        }
    }

    fn pointer_left_client(&self) {
        let mut state = self.ivars().state.borrow_mut();
        if let Err(error) = state.pointer_left_client(self) {
            state.fail_terminal(self, error);
        }
    }

    fn event_point(&self, event: &NSEvent) -> Option<(f32, f32)> {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let bounds = self.bounds();
        if point.x.is_sign_negative()
            || point.y.is_sign_negative()
            || point.x > bounds.size.width
            || point.y > bounds.size.height
        {
            return None;
        }
        Some((point.x as f32, point.y as f32))
    }

    fn refresh_tracking_rect(&self) {
        if let Some(tag) = self.ivars().tracking_rect.take() {
            self.removeTrackingRect(tag);
        }
        let rect = self.bounds();
        // SAFETY: AppKit retains the tracking registration against this installed NSView. The
        // target is `self`, the user data pointer is null, and `begin_close`/view destruction
        // release the owning view before any retained Rust state is dropped.
        let tag = unsafe {
            self.addTrackingRect_owner_userData_assumeInside(rect, self, ptr::null_mut(), false)
        };
        if tag != 0 {
            self.ivars().tracking_rect.set(Some(tag));
        }
    }

    fn startup_failed(&self, error: String) {
        let mut state = self.ivars().state.borrow_mut();
        state.fail_startup(self, error);
    }
}

fn appkit_timestamp_micros(event: &NSEvent) -> u64 {
    (event.timestamp() * 1_000_000.0)
        .max(0.0)
        .min(u64::MAX as f64) as u64
}

impl ViewState {
    fn animation_active(&self) -> bool {
        self.lifecycle.can_render()
            && self.terminal_error.is_none()
            && self
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.status().animation_active)
    }

    fn tick_animation(&mut self, view: &TelaView) -> Result<(), String> {
        if !self.animation_active() {
            return Ok(());
        }
        let timestamp_ms = self.animation_epoch.elapsed().as_millis() as u64;
        self.dispatch_guest(AppEvent::Tick { timestamp_ms })?;
        self.request_redraw(view);
        Ok(())
    }

    fn receive_startup_result(&mut self, view: &TelaView) {
        let result = match self.startup_rx.as_ref() {
            None => return,
            Some(receiver) => match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.startup_rx = None;
                    self.fail_startup(
                        view,
                        "startup worker disconnected before returning a result".to_owned(),
                    );
                    return;
                }
            },
        };
        self.startup_rx = None;
        match result {
            Ok(runtime) => self.install_runtime(view, runtime),
            Err(error) => self.fail_startup(view, error),
        }
    }

    fn install_runtime(&mut self, view: &TelaView, runtime: GuestRuntime) {
        if self.lifecycle.phase() != ShellPhase::Loading {
            return;
        }
        if self.bridge.is_none() {
            self.bridge = Some(crate::providers::build_dispatcher(
                Rc::clone(&self.bridge_metrics),
                &BuildConstants::default(),
                vec![],
            ));
        }
        let activation: Result<(), String> = (|| {
            let frame = runtime.frame().map_err(|error| error.to_string())?;
            let frame_token = runtime.status().frame_token;
            self.frame = Some(frame);
            self.frame_token = frame_token;
            self.presented_frame_token = None;
            self.runtime = Some(runtime);
            let Some(metrics) = self.client_metrics(view) else {
                self.lifecycle.startup_succeeded(false);
                self.set_status("TELA is ready and waiting for a visible content area.");
                return Ok(());
            };
            self.initialize_gpu(view, metrics)?;
            self.lifecycle.startup_succeeded(true);
            self.dispatch_viewport(metrics)?;
            self.status_label.setHidden(true);
            self.request_redraw(view);
            Ok(())
        })();
        if let Err(error) = activation {
            if self.lifecycle.phase() == ShellPhase::Loading {
                self.fail_startup(view, error);
            } else {
                self.fail_terminal(view, error);
            }
        }
    }

    fn fail_startup(&mut self, view: &TelaView, error: String) {
        if self.lifecycle.phase() != ShellPhase::Loading {
            return;
        }
        eprintln!("tela-macos-host: startup failed: {error}");
        self.runtime = None;
        self.frame = None;
        self.frame_token = None;
        self.presented_frame_token = None;
        self.gpu = None;
        self.lifecycle.startup_failed();
        self.set_status(&format!(
            "TELA could not start.\n\n{error}\n\nClose this window and inspect the terminal output."
        ));
        self.status_label.setHidden(false);
        self.request_redraw(view);
    }

    fn fail_terminal(&mut self, view: &TelaView, error: String) {
        if self.terminal_error.is_some() || self.lifecycle.phase() == ShellPhase::Closing {
            return;
        }
        eprintln!("tela-macos-host: {error}");
        self.terminal_error = Some(error.clone());
        self.presented_frame_token = None;
        let action = self.lifecycle.failed();
        if let Err(text_error) = self.dispatch_text_channel_action(action) {
            eprintln!("tela-macos-host: fatal text channel cleanup: {text_error}");
        }
        self.cancel_surface_retry();
        self.gpu = None;
        self.set_status(&format!(
            "TELA stopped because the native renderer failed.\n\n{error}\n\nClose this window and inspect the terminal output."
        ));
        self.status_label.setHidden(false);
        self.request_redraw(view);
    }

    fn client_metrics(&self, view: &TelaView) -> Option<ClientMetrics> {
        let bounds = view.bounds();
        let logical_width = bounds.size.width as f32;
        let logical_height = bounds.size.height as f32;
        if logical_width <= 0.0 || logical_height <= 0.0 {
            return None;
        }
        let backing = view.convertSizeToBacking(bounds.size);
        let width = backing.width.round().max(1.0) as u32;
        let height = backing.height.round().max(1.0) as u32;
        Some(ClientMetrics {
            logical_width,
            logical_height,
            width,
            height,
        })
    }

    fn initialize_gpu(&mut self, view: &TelaView, metrics: ClientMetrics) -> Result<(), String> {
        let generation = self.gpu_generation.wrapping_add(1);
        let gpu = GpuSession::new(view, metrics, generation, Arc::clone(&self.device_loss))?;
        self.gpu = Some(gpu);
        self.gpu_generation = generation;
        Ok(())
    }

    fn resize(&mut self, view: &TelaView) -> Result<(), String> {
        if self.runtime.is_none() {
            return Ok(());
        }
        let Some(metrics) = self.client_metrics(view) else {
            self.lifecycle.client_area_changed(false);
            self.presented_frame_token = None;
            self.cancel_surface_retry();
            return Ok(());
        };
        if self.gpu.is_none() {
            self.initialize_gpu(view, metrics)?;
        } else if let Some(gpu) = self.gpu.as_mut()
            && (gpu.width() != metrics.width || gpu.height() != metrics.height)
        {
            gpu.reconfigure(metrics);
        }
        self.lifecycle.client_area_changed(true);
        if self.lifecycle.can_render() {
            self.status_label.setHidden(true);
        }
        self.dispatch_viewport(metrics)?;
        self.request_redraw(view);
        Ok(())
    }

    fn dispatch_viewport(&mut self, metrics: ClientMetrics) -> Result<(), String> {
        self.bridge_metrics.replace(MacMetrics {
            width: metrics.logical_width as u32,
            height: metrics.logical_height as u32,
            dpr: if metrics.logical_width > 0.0 {
                metrics.width as f32 / metrics.logical_width
            } else {
                1.0
            },
        });
        // The previous client geometry is no longer an eligible source for hit testing.
        self.presented_frame_token = None;
        self.dispatch_guest(AppEvent::Viewport {
            width: metrics.logical_width,
            height: metrics.logical_height,
        })?;
        Ok(())
    }

    fn dispatch_guest(&mut self, event: AppEvent) -> Result<bool, String> {
        let changed = self.dispatch_guest_without_text_reconcile(event)?;
        self.synchronize_text_channel(None)?;
        Ok(changed)
    }

    fn dispatch_guest_without_text_reconcile(&mut self, event: AppEvent) -> Result<bool, String> {
        let (changed, frame, frame_token) = {
            let runtime = self
                .runtime
                .as_mut()
                .ok_or_else(|| "dispatch without a live guest runtime".to_owned())?;
            let changed = runtime
                .dispatch(&event)
                .map_err(|error| error.to_string())?;
            let frame = runtime.frame().map_err(|error| error.to_string())?;
            if let Some(dispatcher) = self.bridge.as_mut() {
                process_bridge_requests(runtime, dispatcher)?;
            }
            (changed, frame, runtime.status().frame_token)
        };
        self.frame = Some(frame);
        self.frame_token = frame_token;
        Ok(changed)
    }

    fn dispatch_presented_input(&mut self, input: AppFrameInput) -> Result<bool, String> {
        let Some(source_frame_token) = self.presented_frame_token else {
            return Ok(false);
        };
        self.synchronize_animation_clock()?;
        self.dispatch_guest(AppEvent::FrameInput {
            source_frame_token,
            input,
        })
    }

    fn dispatch_presented_input_without_text_reconcile(
        &mut self,
        input: AppFrameInput,
    ) -> Result<bool, String> {
        let Some(source_frame_token) = self.presented_frame_token else {
            return Ok(false);
        };
        self.synchronize_animation_clock()?;
        self.dispatch_guest_without_text_reconcile(AppEvent::FrameInput {
            source_frame_token,
            input,
        })
    }

    fn synchronize_animation_clock(&mut self) -> Result<(), String> {
        let timestamp_ms = self.animation_epoch.elapsed().as_millis() as u64;
        let _ = self.dispatch_guest_without_text_reconcile(AppEvent::Tick { timestamp_ms })?;
        Ok(())
    }

    fn synchronize_text_channel(&mut self, window_focus: Option<bool>) -> Result<(), String> {
        let guest_wants_text = self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.status().input_focused);
        let first = match window_focus {
            Some(focused) => self.lifecycle.set_window_focus(focused, guest_wants_text),
            None => self.lifecycle.reconcile_text_channel(guest_wants_text),
        };
        self.dispatch_text_channel_action(first)?;

        // A blur can commit a draft and move guest focus. Reconcile one extra edge without
        // looping native callbacks around guest code indefinitely.
        let guest_wants_text = self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.status().input_focused);
        let second = self.lifecycle.reconcile_text_channel(guest_wants_text);
        self.dispatch_text_channel_action(second)
    }

    fn dispatch_text_channel_action(
        &mut self,
        action: Option<TextChannelAction>,
    ) -> Result<(), String> {
        let Some(action) = action else {
            return Ok(());
        };
        let input = match action {
            TextChannelAction::Focus => AppFrameInput::InputFocus,
            TextChannelAction::Blur => AppFrameInput::InputBlur,
        };
        // This acknowledgement originates from the native editor. Keep it frame-owned, while
        // avoiding recursive text-channel reconciliation from within that reconciliation itself.
        let _ = self.dispatch_presented_input_without_text_reconcile(input)?;
        Ok(())
    }

    fn pointer(&mut self, view: &TelaView, input: AppFrameInput) -> Result<(), String> {
        if !self.lifecycle.can_render() {
            return Ok(());
        }
        self.dispatch_presented_input(input)?;
        self.update_cursor();
        self.request_redraw(view);
        Ok(())
    }

    fn pointer_left_client(&mut self, view: &TelaView) -> Result<(), String> {
        self.pointer(
            view,
            AppFrameInput::Pointer(AppPointerEvent::new(
                0,
                AppPointerKind::Mouse,
                AppPointerPhase::Move,
                -1.0,
                -1.0,
                0,
                0,
                0.0,
                0.0,
            )),
        )
    }

    fn key_down(&mut self, view: &TelaView, event: &NSEvent) -> Result<(), String> {
        if !self.lifecycle.can_render() {
            return Ok(());
        }
        let key_code = event.keyCode();
        let flags = event.modifierFlags();
        let input_focused = self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.status().input_focused);
        if input_focused {
            match key_code {
                36 => {
                    self.dispatch_presented_input(AppFrameInput::InputEnter)?;
                    self.request_redraw(view);
                    return Ok(());
                }
                53 => {
                    self.dispatch_presented_input(AppFrameInput::InputCancel)?;
                    self.request_redraw(view);
                    return Ok(());
                }
                51 => {
                    let value = self
                        .runtime
                        .as_ref()
                        .expect("runtime checked above")
                        .status()
                        .input_value
                        .clone();
                    self.dispatch_presented_input(AppFrameInput::SetInputValue(input::backspace(
                        &value,
                    )))?;
                    self.request_redraw(view);
                    return Ok(());
                }
                48 if !input::has_command_modifier(flags) => {}
                _ if !input::has_command_modifier(flags) => {
                    if let Some(characters) = event.characters()
                        && let Some(value) = input::append_ascii(
                            &self
                                .runtime
                                .as_ref()
                                .expect("runtime checked above")
                                .status()
                                .input_value,
                            &characters.to_string(),
                        )
                    {
                        self.dispatch_presented_input(AppFrameInput::SetInputValue(value))?;
                        self.request_redraw(view);
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        if let Some(physical_key) = input::physical_key(key_code) {
            self.dispatch_presented_input(AppFrameInput::KeyDown {
                physical_key,
                modifier_bits: input::modifier_bits(flags),
                repeat: event.isARepeat(),
            })?;
            self.request_redraw(view);
        }
        Ok(())
    }

    fn paint(&mut self, view: &TelaView) -> Result<(), String> {
        if !self.lifecycle.can_render() {
            return Ok(());
        }
        let frame_token = self.frame_token;
        let frame = self
            .frame
            .as_ref()
            .ok_or_else(|| "render without a resolved UI frame".to_owned())?;
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "render without a live GPU session".to_owned())?;
        match gpu.render(frame) {
            RenderOutcome::Presented { suboptimal } => {
                self.presented_frame_token = frame_token;
                self.lifecycle.surface_presented();
                self.cancel_surface_retry();
                if suboptimal {
                    self.reconfigure_surface(view)?;
                    self.request_redraw(view);
                }
            }
            RenderOutcome::Outdated => {
                self.presented_frame_token = None;
                self.reconfigure_surface(view)?;
                self.request_redraw(view);
            }
            RenderOutcome::Lost => {
                self.presented_frame_token = None;
                self.recreate_surface(view)?;
                self.request_redraw(view);
            }
            RenderOutcome::Timeout => self.schedule_surface_retry(),
            RenderOutcome::Occluded => {}
            RenderOutcome::Validation => {
                return Err("WGPU surface validation failed while acquiring a frame".to_owned());
            }
        }
        Ok(())
    }

    fn reconfigure_surface(&mut self, view: &TelaView) -> Result<(), String> {
        self.presented_frame_token = None;
        let Some(metrics) = self.client_metrics(view) else {
            self.lifecycle.client_area_changed(false);
            return Ok(());
        };
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "reconfigure without a live GPU session".to_owned())?;
        gpu.reconfigure(metrics);
        Ok(())
    }

    fn recreate_surface(&mut self, view: &TelaView) -> Result<(), String> {
        self.presented_frame_token = None;
        let Some(metrics) = self.client_metrics(view) else {
            self.lifecycle.client_area_changed(false);
            return Ok(());
        };
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "recreate surface without a live GPU session".to_owned())?;
        gpu.recreate_surface(view, metrics)?;
        self.dispatch_viewport(metrics)?;
        Ok(())
    }

    fn schedule_surface_retry(&mut self) {
        let Some(delay_ms) = self.lifecycle.surface_timeout() else {
            return;
        };
        self.surface_retry_deadline = Some(Instant::now() + Duration::from_millis(delay_ms.into()));
    }

    fn poll_surface_retry(&mut self, view: &TelaView) {
        let Some(deadline) = self.surface_retry_deadline else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.surface_retry_deadline = None;
        if self.lifecycle.take_surface_retry() {
            self.request_redraw(view);
        }
    }

    fn cancel_surface_retry(&mut self) {
        self.surface_retry_deadline = None;
        self.lifecycle.cancel_surface_retry();
    }

    fn receive_device_loss(&mut self, view: &TelaView) {
        let Some(report) = self
            .device_loss
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
        else {
            return;
        };
        if report.generation != self.gpu_generation || self.gpu.is_none() {
            return;
        }
        match self.lifecycle.device_lost() {
            None => {}
            Some(DeviceLossAction::RecreateGpu) => {
                eprintln!(
                    "tela-macos-host: WGPU device lost, rebuilding once: {}",
                    report.detail
                );
                self.gpu = None;
                self.presented_frame_token = None;
                if self.lifecycle.phase() == ShellPhase::Suspended {
                    return;
                }
                let recovery: Result<(), String> = (|| {
                    let Some(metrics) = self.client_metrics(view) else {
                        self.lifecycle.client_area_changed(false);
                        return Ok(());
                    };
                    self.initialize_gpu(view, metrics)?;
                    self.dispatch_viewport(metrics)?;
                    self.request_redraw(view);
                    Ok(())
                })();
                if let Err(error) = recovery {
                    self.fail_terminal(view, format!("recreate WGPU after device loss: {error}"));
                }
            }
            Some(DeviceLossAction::Exit) => self.fail_terminal(
                view,
                format!(
                    "WGPU device was lost again after recovery: {}",
                    report.detail
                ),
            ),
        }
    }

    fn request_redraw(&mut self, view: &TelaView) {
        if self.lifecycle.request_redraw() {
            view.setNeedsDisplay(true);
        }
    }

    fn update_cursor(&self) {
        let cursor = self
            .runtime
            .as_ref()
            .map(|runtime| runtime.status().cursor)
            .unwrap_or(CursorKind::Default);
        match cursor {
            CursorKind::Default => NSCursor::arrowCursor().set(),
            CursorKind::Text => NSCursor::IBeamCursor().set(),
            CursorKind::Pointer => NSCursor::pointingHandCursor().set(),
        }
    }

    fn set_status(&self, message: &str) {
        self.status_label
            .setStringValue(&NSString::from_str(message));
    }
}

fn make_status_label(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label =
        NSTextField::wrappingLabelWithString(&NSString::from_str("TELA is starting..."), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(28.0, 28.0),
        NSSize::new(680.0, 180.0),
    ));
    label.setFont(Some(&NSFont::systemFontOfSize(16.0)));
    label.setTextColor(Some(&NSColor::labelColor()));
    label.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
    );
    label
}
