//! AppKit application and window ownership for the macOS development shell.

use std::cell::OnceCell;

use objc2::{
    DefinedClass, MainThreadOnly, define_class, rc::Retained, runtime::ProtocolObject, sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer,
};
use tela_native_sdk_runtime::PlatformLaunchOptions;

use crate::view::TelaView;

struct AppDelegateIvars {
    options: PlatformLaunchOptions,
    window: OnceCell<Retained<NSWindow>>,
    view: OnceCell<Retained<TelaView>>,
    poll_timer: OnceCell<Retained<NSTimer>>,
}

define_class!(
    // SAFETY: NSObject does not impose subclass initialization or deallocation requirements, and
    // AppDelegate has no Drop implementation.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppDelegateIvars]
    struct AppDelegate;

    // SAFETY: NSObjectProtocol has no additional requirements for this pure delegate object.
    unsafe impl NSObjectProtocol for AppDelegate {}

    // SAFETY: AppKit invokes the declared selector with the exact notification parameter below.
    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.create_window();
        }
    }

    // SAFETY: AppKit invokes the declared selectors with the documented window notifications.
    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            if let Some(view) = self.ivars().view.get() {
                view.window_key_changed(true);
            }
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            if let Some(view) = self.ivars().view.get() {
                view.window_key_changed(false);
            }
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            if let Some(view) = self.ivars().view.get() {
                view.begin_close();
            }
            if let Some(timer) = self.ivars().poll_timer.get() {
                timer.invalidate();
            }
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }

    // SAFETY: `pollStartup:` is registered as the NSTimer target selector below and has the
    // matching `NSTimer` argument.
    impl AppDelegate {
        #[unsafe(method(pollStartup:))]
        fn poll_startup(&self, _timer: &NSTimer) {
            if let Some(view) = self.ivars().view.get() {
                view.poll_background_work();
            }
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, options: PlatformLaunchOptions) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars {
            options,
            window: OnceCell::new(),
            view: OnceCell::new(),
            poll_timer: OnceCell::new(),
        });
        // SAFETY: this invokes NSObject's documented initializer for a newly allocated delegate.
        unsafe { objc2::msg_send![super(this), init] }
    }

    fn create_window(&self) {
        let mtm = self.mtm();
        let view = TelaView::new(mtm, self.ivars().options.clone());
        // SAFETY: the style/backing values are standard AppKit constants, and this window is kept
        // strongly alive in the delegate until `windowWillClose:` has torn down the content view.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 840.0)),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: a programmatically-created window is retained by AppDelegate, so AppKit must not
        // add an independent release when the close button is used.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("TELA Files"));
        window.setContentMinSize(NSSize::new(640.0, 480.0));
        window.setContentView(Some(&view));
        window.setAcceptsMouseMovedEvents(true);
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window.center();

        let _ = self.ivars().view.set(view.clone());
        let _ = self.ivars().window.set(window.clone());
        // SAFETY: `pollStartup:` is declared above on this delegate and the timer is invalidated
        // before the delegate/window are released.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.01,
                self,
                sel!(pollStartup:),
                None,
                true,
            )
        };
        let _ = self.ivars().poll_timer.set(timer);

        window.makeKeyAndOrderFront(None);
        let _ = window.makeFirstResponder(Some(&view));
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

/// Runs the AppKit application loop on the process main thread.
pub fn run(options: PlatformLaunchOptions) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        "tela-macos-sdk must initialize AppKit on the process main thread".to_owned()
    })?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = AppDelegate::new(mtm, options);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}
