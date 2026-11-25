//! The macOS app: an AppKit accessory (menu-bar) app with a borderless,
//! vibrant widget window. Drawing goes through `painter`; behaviour lives in
//! `franconvert_core::ui::Widget`.

mod access;
#[cfg(feature = "drive")]
mod drive;
mod painter;
mod system;

use crate::{shortcut, single_instance, updater};
use block2::RcBlock;
use franconvert_core::session::{Key, Mods};
use franconvert_core::settings::Settings;
use franconvert_core::ui::geom::Point;
use franconvert_core::ui::onboarding::{self, Onboarding};
use franconvert_core::ui::{Command, Cursor, FieldId, UpdateEvent, Widget, WidgetInit, WINDOW_W};
use franconvert_core::{TransliterationEngine, data_dir};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility,
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSColor, NSControlTextEditingDelegate,
    NSCursor, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSFont, NSFocusRingType, NSGraphicsContext, NSImage, NSMenu,
    NSMenuItem, NSStatusBar, NSTextField, NSTextFieldDelegate, NSTextView, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSData, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString, NSTimer,
};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const CHANGELOG: &str = include_str!("../../ui/CHANGELOG.json");

// ── Global state (main thread only) ──────────────────────────────────────

struct Shell {
    mtm: MainThreadMarker,
    overlay: Retained<KeyWindow>,
    canvas: Retained<Canvas>,
    input: Retained<NSTextField>,
    hex: Retained<NSTextField>,
    _field_delegate: Retained<FieldDelegate>,
    onboarding: Option<(Retained<KeyWindow>, Retained<Canvas>)>,
    _status_item: Retained<objc2_app_kit::NSStatusItem>,
    hotkeys: GlobalHotKeyManager,
    hotkey: Option<HotKey>,
    frame_timer: Option<Retained<NSTimer>>,
    wake_timer: Option<Retained<NSTimer>>,
    prev_pid: i32,
    data_dir: PathBuf,
    _keep: Vec<Retained<AnyObject>>,
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
    static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
    static ONBOARD: RefCell<Option<Onboarding>> = const { RefCell::new(None) };
}

fn with_widget<R>(f: impl FnOnce(&mut Widget) -> R) -> Option<R> {
    WIDGET.with(|w| w.try_borrow_mut().ok().and_then(|mut w| w.as_mut().map(f)))
}

fn with_shell<R>(f: impl FnOnce(&mut Shell) -> R) -> Option<R> {
    SHELL.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

thread_local! {
    /// Kept outside `SHELL` so drawing that AppKit runs synchronously while
    /// the shell is borrowed (resize, drag) still sees the right time.
    static START: Instant = Instant::now();
}

fn now() -> f64 {
    START.with(|s| s.elapsed().as_secs_f64())
}

/// Runs `f` on the main thread (from any thread).
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    dispatch2::DispatchQueue::main().exec_async(f);
}

// ── Classes ──────────────────────────────────────────────────────────────

define_class!(
    /// Borderless windows can't become key by default; this one can.
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "FCKeyWindow"]
    struct KeyWindow;

    impl KeyWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            true
        }
    }
);

pub struct CanvasIvars {
    /// 0 = widget, 1 = onboarding.
    kind: Cell<u8>,
    tracking: RefCell<Option<Retained<NSTrackingArea>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "FCCanvas"]
    #[ivars = CanvasIvars]
    struct Canvas;

    impl Canvas {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let Some(gc) = NSGraphicsContext::currentContext() else { return };
            let cg = gc.CGContext();
            let t = now();
            if self.ivars().kind.get() == 1 {
                ONBOARD.with(|o| {
                    if let Some(o) = o.borrow_mut().as_mut() {
                        let mut p = painter::MacPainter::new(&cg, false);
                        o.paint(&mut p);
                    }
                });
                return;
            }
            with_widget(|w| {
                let dys = w.settings.dyslexia_font;
                let mut p = painter::MacPainter::new(&cg, dys);
                w.paint(&mut p, t);
            });
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            if let Some(old) = self.ivars().tracking.borrow_mut().take() {
                self.removeTrackingArea(&old);
            }
            let opts = NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect;
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), self.bounds(), opts, Some(&**self as &NSView as &AnyObject), None)
            };
            self.addTrackingArea(&area);
            *self.ivars().tracking.borrow_mut() = Some(area);
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            let p = self.point(e);
            let clicks = e.clickCount() as u32;
            if self.ivars().kind.get() == 1 {
                ONBOARD.with(|o| {
                    if let Some(o) = o.borrow_mut().as_mut() {
                        o.click(p.x, p.y);
                    }
                });
                onboarding_tick();
                return;
            }
            with_widget(|w| w.mouse_down(p, clicks));
            after_event();
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, e: &NSEvent) {
            if self.ivars().kind.get() == 1 {
                return;
            }
            let p = self.point(e);
            let clicks = e.clickCount() as u32;
            with_widget(|w| w.mouse_up(p, clicks));
            after_event();
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, e: &NSEvent) {
            if self.ivars().kind.get() == 1 {
                return;
            }
            let p = self.point(e);
            with_widget(|w| w.mouse_dragged(p));
            after_event();
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, e: &NSEvent) {
            let p = self.point(e);
            if self.ivars().kind.get() == 1 {
                ONBOARD.with(|o| {
                    if let Some(o) = o.borrow_mut().as_mut() {
                        o.mouse_moved(p.x, p.y);
                    }
                });
                self.setNeedsDisplay(true);
                return;
            }
            with_widget(|w| w.mouse_moved(p));
            after_event();
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _e: &NSEvent) {
            if self.ivars().kind.get() == 0 {
                with_widget(|w| w.mouse_exited());
                after_event();
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, e: &NSEvent) {
            if self.ivars().kind.get() == 1 {
                return;
            }
            let p = self.point(e);
            let mut dy = -e.scrollingDeltaY() as f32;
            if !e.hasPreciseScrollingDeltas() {
                dy *= 10.0;
            }
            with_widget(|w| w.scroll(p, dy));
            after_event();
        }

        #[unsafe(method(cursorUpdate:))]
        fn cursor_update(&self, _e: &NSEvent) {
            apply_cursor();
        }

        // ── VoiceOver: expose the drawn controls ──
        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> bool {
            false
        }

        #[unsafe(method(accessibilityChildren))]
        fn accessibility_children(&self) -> *mut objc2_foundation::NSArray<AnyObject> {
            if self.ivars().kind.get() != 0 {
                return unsafe { msg_send![super(self), accessibilityChildren] };
            }
            let mut kids = access::children(self).to_vec();
            // Keep the real text fields reachable too.
            for v in self.subviews().iter() {
                if !v.isHidden() {
                    // The field's accessible object (its cell / text field role).
                    let el: Option<Retained<AnyObject>> = unsafe {
                        let any: &AnyObject = &v;
                        objc2_app_kit::NSAccessibilityUnignoredDescendant(any)
                    };
                    kids.insert(0, el.unwrap_or_else(|| unsafe { Retained::cast_unchecked(v) }));
                }
            }
            Retained::autorelease_return(objc2_foundation::NSArray::from_retained_slice(&kids))
        }

        #[unsafe(method(accessibilityHitTest:))]
        fn accessibility_hit_test(&self, point: NSPoint) -> *mut AnyObject {
            if self.ivars().kind.get() == 0 {
                if let Some(el) = access::hit_test(point) {
                    return Retained::autorelease_return(el);
                }
            }
            unsafe { msg_send![super(self), accessibilityHitTest: point] }
        }
    }
);

impl Canvas {
    fn new(mtm: MainThreadMarker, frame: NSRect, kind: u8) -> Retained<Canvas> {
        let this = Canvas::alloc(mtm).set_ivars(CanvasIvars { kind: Cell::new(kind), tracking: RefCell::new(None) });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    fn point(&self, e: &NSEvent) -> Point {
        let p = self.convertPoint_fromView(e.locationInWindow(), None);
        Point { x: p.x as f32, y: p.y as f32 }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FCFieldDelegate"]
    struct FieldDelegate;

    unsafe impl NSObjectProtocol for FieldDelegate {}
    unsafe impl NSControlTextEditingDelegate for FieldDelegate {}
    unsafe impl NSTextFieldDelegate for FieldDelegate {}

    impl FieldDelegate {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, n: &NSNotification) {
            if let Some(id) = field_id_of(n) {
                report_field(id);
            }
        }

        #[unsafe(method(controlTextDidBeginEditing:))]
        fn control_text_did_begin(&self, _n: &NSNotification) {
            style_field_editors();
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_text_did_end(&self, n: &NSNotification) {
            if let Some(id) = field_id_of(n) {
                with_widget(|w| w.field_blurred(id));
                after_event();
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FCAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            startup(MainThreadMarker::from(self));
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            show_existing();
            true
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            single_instance::cleanup(&data_dir());
        }
    }

    impl AppDelegate {
        #[unsafe(method(menuShow:))]
        fn menu_show(&self, _sender: Option<&AnyObject>) {
            toggle_overlay();
        }

        #[unsafe(method(menuQuit:))]
        fn menu_quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(MainThreadMarker::from(self)).terminate(None);
        }
    }
);

// ── Startup ──────────────────────────────────────────────────────────────

pub fn run() {
    let dir = data_dir();
    if !single_instance::acquire(&dir, || on_main(show_existing)) {
        return; // Another copy was asked to show itself.
    }
    let mtm = MainThreadMarker::new().expect("must start on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate: Retained<AppDelegate> = unsafe { msg_send![AppDelegate::alloc(mtm), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    drop(delegate);
}

fn load_settings(dir: &std::path::Path) -> (Settings, bool) {
    if let Some(s) = Settings::load(dir) {
        return (s, true);
    }
    // First native launch: import the web UI's localStorage, if any.
    let items = crate::migrate::read_webkit_local_storage();
    let had_old = !items.is_empty();
    let s = Settings::from_local_storage(&items);
    s.save(dir);
    (s, had_old)
}

fn startup(mtm: MainThreadMarker) {
    system::register_fonts();
    install_main_menu(mtm);
    let dir = data_dir();
    let _ = now(); // start the clock
    let (settings, returning) = load_settings(&dir);
    let prefs = franconvert_core::load_prefs(&dir);
    let widget = Widget::new(WidgetInit {
        engine: TransliterationEngine::new(),
        data_dir: dir.clone(),
        settings,
        prefs,
        is_mac: true,
        system_dark: system::is_dark(mtm),
        system_accent: system::accent_hex(),
        autostart: system::autostart_enabled(),
        now: 0.0,
    });
    let height = widget.window_height() as f64;

    // ── Widget window ──
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W as f64, height));
    let overlay = make_window(mtm, frame);
    overlay.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    let effect = make_backdrop(mtm, frame);
    overlay.setContentView(Some(&effect));
    let canvas = Canvas::new(mtm, frame, 0);
    canvas.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable);
    effect.addSubview(&canvas);

    let field_delegate: Retained<FieldDelegate> = unsafe { msg_send![FieldDelegate::alloc(mtm), init] };
    let input = make_field(mtm, &field_delegate);
    input.setAccessibilityLabel(Some(&NSString::from_str("Type Franco Arabic")));
    if let Some(cell) = input.cell() {
        cell.setAccessibilityLabel(Some(&NSString::from_str("Type Franco Arabic")));
    }
    let hex = make_field(mtm, &field_delegate);
    hex.setAccessibilityLabel(Some(&NSString::from_str("Hex colour")));
    hex.setHidden(true);
    canvas.addSubview(&input);
    canvas.addSubview(&hex);
    restore_position(&overlay, &dir);
    overlay.center_if_unplaced();

    // ── Tray ──
    let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
    if let Some(button) = status_item.button(mtm) {
        let png = include_bytes!("../../icons/tray-icon@2x.png");
        let data = NSData::with_bytes(png);
        if let Some(img) = NSImage::initWithData(NSImage::alloc(), &data) {
            img.setSize(NSSize::new(16.0, 12.0));
            img.setTemplate(true);
            button.setImage(Some(&img));
        }
        button.setToolTip(Some(&NSString::from_str("Franconvert — Franco Arabic Keyboard")));
    }
    let menu = NSMenu::new(mtm);
    let delegate = NSApplication::sharedApplication(mtm).delegate().unwrap();
    for (title, action) in [("Show Franconvert", sel!(menuShow:)), ("Quit", sel!(menuQuit:))] {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), Some(action), &NSString::new())
        };
        unsafe { item.setTarget(Some(&*(Retained::as_ptr(&delegate) as *const AnyObject))) };
        menu.addItem(&item);
    }
    status_item.setMenu(Some(&menu));

    // ── Global shortcut ──
    let hotkeys = GlobalHotKeyManager::new().expect("hotkey manager");
    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state == HotKeyState::Pressed {
            on_main(toggle_overlay);
        }
    }));

    let mut keep: Vec<Retained<AnyObject>> = Vec::new();
    keep.push(install_key_monitor());
    keep.extend(observe_appearance());

    SHELL.with(|s| {
        *s.borrow_mut() = Some(Shell {
            mtm,
            overlay,
            canvas,
            input,
            hex,
            _field_delegate: field_delegate,
            onboarding: None,
            _status_item: status_item,
            hotkeys,
            hotkey: None,
            frame_timer: None,
            wake_timer: None,
            prev_pid: 0,
            data_dir: dir.clone(),
            _keep: keep,
        })
    });
    WIDGET.with(|w| *w.borrow_mut() = Some(widget));

    let shortcut = with_widget(|w| w.shortcut()).unwrap();
    if !register_shortcut(&shortcut) {
        let default = with_widget(|w| w.default_shortcut().to_string()).unwrap();
        register_shortcut(&default);
    }
    with_widget(|w| w.maybe_show_changelog(CHANGELOG, returning));
    sync_fields();

    // First launch: welcome window.
    if !dir.join("onboarding_shown").exists() {
        show_onboarding(mtm);
    }
    // Check for updates at launch (quietly).
    if std::env::var_os("FRANCONVERT_NO_UPDATE_CHECK").is_none() {
        run_command(Command::CheckForUpdates { manual: false });
    }
    #[cfg(feature = "drive")]
    drive::start();
    schedule();
}

fn make_window(mtm: MainThreadMarker, frame: NSRect) -> Retained<KeyWindow> {
    let w: Retained<KeyWindow> = unsafe {
        msg_send![
            KeyWindow::alloc(mtm),
            initWithContentRect: frame,
            styleMask: NSWindowStyleMask::Borderless,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    w.setOpaque(false);
    w.setBackgroundColor(Some(&NSColor::clearColor()));
    w.setHasShadow(true);
    unsafe { w.setReleasedWhenClosed(false) };
    w.setMovableByWindowBackground(false);
    w
}

/// The frosted backdrop: FullScreenUI on macOS 26 (rendered as Liquid Glass),
/// HudWindow before — the materials the Tauri build used.
fn make_backdrop(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSVisualEffectView> {
    let v = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
    v.setMaterial(if system::macos_major() >= 26 { NSVisualEffectMaterial::FullScreenUI } else { NSVisualEffectMaterial::HUDWindow });
    v.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    v.setState(NSVisualEffectState::Active);
    v.setWantsLayer(true);
    if let Some(layer) = v.layer() {
        layer.setCornerRadius(12.0);
        layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
        layer.setMasksToBounds(true);
    }
    v
}

fn make_field(mtm: MainThreadMarker, delegate: &FieldDelegate) -> Retained<NSTextField> {
    let f = NSTextField::initWithFrame(NSTextField::alloc(mtm), NSRect::default());
    f.setBezeled(false);
    f.setBordered(false);
    f.setDrawsBackground(false);
    f.setFocusRingType(NSFocusRingType::None);
    f.setEditable(true);
    f.setSelectable(true);
    f.setAutomaticTextCompletionEnabled(false);
    if let Some(cell) = f.cell() {
        cell.setScrollable(true);
        cell.setWraps(false);
        cell.setUsesSingleLineMode(true);
    }
    unsafe { f.setDelegate(Some(ProtocolObject::from_ref(delegate))) };
    f
}

/// An Edit menu so Cmd+C/V/X/A/Z work in the text fields (an accessory app
/// has no menu bar, but key equivalents still route through the main menu).
fn install_main_menu(mtm: MainThreadMarker) {
    let main = NSMenu::new(mtm);
    let edit_item = NSMenuItem::new(mtm);
    let edit = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    for (title, action, key) in [
        ("Undo", sel!(undo:), "z"),
        ("Redo", sel!(redo:), "Z"),
        ("Cut", sel!(cut:), "x"),
        ("Copy", sel!(copy:), "c"),
        ("Paste", sel!(paste:), "v"),
        ("Select All", sel!(selectAll:), "a"),
    ] {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), Some(action), &NSString::from_str(key))
        };
        edit.addItem(&item);
    }
    edit_item.setSubmenu(Some(&edit));
    main.addItem(&edit_item);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&main));
}

trait CenterIfUnplaced {
    fn center_if_unplaced(&self);
}
impl CenterIfUnplaced for KeyWindow {
    fn center_if_unplaced(&self) {
        if self.frame().origin.x == 0.0 && self.frame().origin.y == 0.0 {
            self.center();
        }
    }
}

// ── Window position (window_pos.json, shared with the Tauri build) ───────

fn restore_position(w: &KeyWindow, dir: &std::path::Path) {
    let Ok(data) = fs::read_to_string(dir.join("window_pos.json")) else { return };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    let Some(screen) = objc2_app_kit::NSScreen::screens(w.mtm()).firstObject() else { return };
    let primary_h = screen.frame().size.height;
    // Native builds store logical top-left ("lx"/"ly"); Tauri stored
    // physical pixels ("x"/"y") from the primary screen's top-left.
    let (x, y) = match (v.get("lx").and_then(|n| n.as_f64()), v.get("ly").and_then(|n| n.as_f64())) {
        (Some(x), Some(y)) => (x, y),
        _ => {
            let scale = screen.backingScaleFactor();
            match (v.get("x").and_then(|n| n.as_f64()), v.get("y").and_then(|n| n.as_f64())) {
                (Some(x), Some(y)) => (x / scale, y / scale),
                _ => return,
            }
        }
    };
    let h = w.frame().size.height;
    let origin = NSPoint::new(x, primary_h - y - h);
    let visible = objc2_app_kit::NSScreen::screens(w.mtm())
        .iter()
        .any(|s| {
            let f = s.frame();
            origin.x >= f.origin.x && origin.x < f.origin.x + f.size.width && origin.y + h > f.origin.y && origin.y < f.origin.y + f.size.height
        });
    if visible {
        w.setFrameOrigin(origin);
    }
}

fn save_position() {
    with_shell(|s| {
        let f = s.overlay.frame();
        let Some(screen) = objc2_app_kit::NSScreen::screens(s.mtm).firstObject() else { return };
        let primary_h = screen.frame().size.height;
        let top = primary_h - (f.origin.y + f.size.height);
        let json = format!("{{\"lx\":{},\"ly\":{}}}", f.origin.x.round(), top.round());
        let _ = fs::write(s.data_dir.join("window_pos.json"), json);
    });
}

// ── Show / hide ──────────────────────────────────────────────────────────

fn capture_prev_focus() {
    let pid = system::frontmost_pid();
    if pid > 0 && pid != std::process::id() as i32 {
        with_shell(|s| s.prev_pid = pid);
    }
}

fn activate_self(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

fn overlay_visible() -> bool {
    with_shell(|s| s.overlay.isVisible()).unwrap_or(false)
}

fn toggle_overlay() {
    // The welcome window celebrates the shortcut, then closes itself.
    let onboarding_open = with_shell(|s| s.onboarding.is_some()).unwrap_or(false);
    if onboarding_open {
        ONBOARD.with(|o| {
            if let Some(o) = o.borrow_mut().as_mut() {
                o.shortcut_used();
            }
        });
        schedule();
    }
    if overlay_visible() {
        hide_overlay();
        return;
    }
    capture_prev_focus();
    with_widget(|w| w.shown());
    show_overlay_window();
    after_event();
}

/// Reopening (Finder, a second launch) reveals the widget without clearing it.
fn show_existing() {
    let ob = with_shell(|s| s.onboarding.as_ref().map(|(w, _)| w.clone())).flatten();
    if let Some(w) = ob {
        w.makeKeyAndOrderFront(None);
        return;
    }
    if !overlay_visible() {
        capture_prev_focus();
    }
    show_overlay_window();
    after_event();
}

fn show_overlay_window() {
    with_shell(|s| {
        activate_self(s.mtm);
        s.overlay.makeKeyAndOrderFront(None);
        s.overlay.makeFirstResponder(Some(&s.input));
        s.overlay.invalidateShadow();
    });
}

fn hide_overlay() {
    save_position();
    with_shell(|s| s.overlay.orderOut(None));
}

/// Hides the widget, gives focus back to the previous app and pastes `text`,
/// then restores whatever text the clipboard held before.
fn paste_and_hide(text: String) {
    let prev_pid = with_shell(|s| std::mem::take(&mut s.prev_pid)).unwrap_or(0);
    let saved = system::clipboard_text();
    let ours = system::set_clipboard_text(&text);
    hide_overlay();
    system::activate_pid(prev_pid);
    std::thread::spawn(move || {
        let own = std::process::id() as i32;
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(750) {
            let front = main_sync(system::frontmost_pid);
            let ready = if prev_pid > 0 { front == prev_pid } else { front > 0 && front != own };
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        std::thread::sleep(Duration::from_millis(60));
        system::send_paste_keystroke();
        // Give the target app time to read the clipboard, then restore it
        // unless something else was copied meanwhile.
        if let Some(previous) = saved {
            std::thread::sleep(Duration::from_millis(600));
            on_main(move || {
                if system::clipboard_change_count() == ours {
                    system::set_clipboard_text(&previous);
                }
            });
        }
    });
}

fn main_sync<T: Send + 'static>(f: fn() -> T) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    on_main(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_millis(200)).unwrap_or_else(|_| f())
}

// ── Event plumbing ───────────────────────────────────────────────────────

/// After any event: run the widget's commands, sync native fields, redraw,
/// and (re)arm timers.
fn after_event() {
    sync_fields();
    for _ in 0..4 {
        let cmds = with_widget(|w| w.take_commands()).unwrap_or_default();
        if cmds.is_empty() {
            break;
        }
        for c in cmds {
            run_command(c);
        }
    }
    sync_fields();
    apply_cursor();
    with_shell(|s| s.canvas.setNeedsDisplay(true));
    schedule();
}

fn run_command(c: Command) {
    match c {
        Command::Hide => hide_overlay(),
        Command::Paste(text) => paste_and_hide(text),
        Command::StartDrag => {
            with_shell(|s| {
                if let Some(e) = NSApplication::sharedApplication(s.mtm).currentEvent() {
                    s.overlay.performWindowDragWithEvent(&e);
                }
            });
            save_position();
        }
        Command::SetField { field, text, select_all } => with_shell(|s| {
            let f = if field == FieldId::Input { &s.input } else { &s.hex };
            f.setStringValue(&NSString::from_str(&text));
            if let Some(ed) = f.currentEditor() {
                let n = NSString::from_str(&text).length();
                ed.setSelectedRange(if select_all { NSRange::new(0, n) } else { NSRange::new(n, 0) });
            }
        })
        .unwrap_or(()),
        Command::FocusField(field) => with_shell(|s| {
            let f = if field == FieldId::Input { &s.input } else { &s.hex };
            let window_editor = s.overlay.firstResponder();
            let already = f.currentEditor().is_some_and(|ed| {
                window_editor.is_some_and(|r| std::ptr::eq(Retained::as_ptr(&r).cast::<u8>(), Retained::as_ptr(&ed).cast()))
            });
            if !already {
                s.overlay.makeFirstResponder(Some(f));
                if let Some(ed) = f.currentEditor() {
                    let n = f.stringValue().length();
                    ed.setSelectedRange(NSRange::new(n, 0));
                }
            }
        })
        .unwrap_or(()),
        Command::ResizeWindow(h) => with_shell(|s| {
            let mut f = s.overlay.frame();
            let top = f.origin.y + f.size.height;
            f.size.height = h as f64;
            f.origin.y = top - h as f64;
            s.overlay.setFrame_display(f, true);
            s.overlay.invalidateShadow();
        })
        .unwrap_or(()),
        Command::RegisterShortcut(sc) => {
            let ok = register_shortcut(&sc);
            with_widget(|w| w.shortcut_result(&sc, ok));
        }
        Command::SetAutostart(on) => {
            system::set_autostart(on);
            let now_on = system::autostart_enabled();
            with_widget(|w| w.set_autostart_state(now_on));
        }
        Command::CheckForUpdates { manual } => {
            std::thread::spawn(move || {
                let e = match updater::check(franconvert_core::VERSION) {
                    Ok(r) => {
                        if let Some(r) = &r {
                            *PENDING_RELEASE.lock().unwrap() = Some(r.clone());
                        }
                        UpdateEvent::Checked { manual, available: r.map(|r| r.version) }
                    }
                    Err(_) => UpdateEvent::CheckFailed { manual },
                };
                on_main(move || {
                    with_widget(|w| w.update_event(e));
                    after_event();
                });
            });
        }
        Command::InstallUpdate => {
            let Some(release) = PENDING_RELEASE.lock().unwrap().clone() else { return };
            std::thread::spawn(move || {
                let result = updater::download(&release, |p| {
                    on_main(move || {
                        with_widget(|w| w.update_event(UpdateEvent::Progress(p)));
                        after_event();
                    })
                })
                .and_then(|bytes| match system::bundle_path() {
                    Some(b) => updater::install(&bytes, &b),
                    None => Err("not running from an app bundle".into()),
                });
                on_main(move || {
                    with_widget(|w| w.update_event(if result.is_ok() { UpdateEvent::Installed } else { UpdateEvent::InstallFailed }));
                    after_event();
                });
            });
        }
        Command::Relaunch => relaunch(),
        Command::Announce(text) => access::announce(&text),
        Command::ResetAppData => {
            let dir = data_dir();
            for f in ["user_prefs.json", "onboarding_shown", franconvert_core::settings::SETTINGS_FILE] {
                let _ = fs::remove_file(dir.join(f));
            }
            relaunch();
        }
    }
}

static PENDING_RELEASE: std::sync::Mutex<Option<updater::Release>> = std::sync::Mutex::new(None);

fn relaunch() {
    let target = system::bundle_path().unwrap_or_else(|| std::env::current_exe().unwrap_or_default());
    single_instance::cleanup(&data_dir());
    let _ = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.5; open -n \"$0\"")
        .arg(&target)
        .spawn();
    with_shell(|s| NSApplication::sharedApplication(s.mtm).terminate(None));
}

fn register_shortcut(combo: &str) -> bool {
    let Some(hk) = shortcut::parse(combo) else { return false };
    with_shell(|s| {
        if let Some(old) = s.hotkey.take() {
            let _ = s.hotkeys.unregister(old);
        }
        match s.hotkeys.register(hk) {
            Ok(()) => {
                s.hotkey = Some(hk);
                true
            }
            Err(_) => false,
        }
    })
    .unwrap_or(false)
}

// ── Fields ───────────────────────────────────────────────────────────────

fn field_id_of(n: &NSNotification) -> Option<FieldId> {
    let obj = n.object()?;
    with_shell(|s| {
        if std::ptr::eq(Retained::as_ptr(&obj).cast::<u8>(), Retained::as_ptr(&s.input).cast()) {
            Some(FieldId::Input)
        } else if std::ptr::eq(Retained::as_ptr(&obj).cast::<u8>(), Retained::as_ptr(&s.hex).cast()) {
            Some(FieldId::Hex)
        } else {
            None
        }
    })
    .flatten()
}

/// UTF-16 range → char offsets.
fn char_range(s: &str, r: NSRange) -> (usize, usize) {
    let mut units = 0usize;
    let (mut a, mut b) = (None, None);
    for (i, c) in s.chars().enumerate() {
        if a.is_none() && units >= r.location {
            a = Some(i);
        }
        if b.is_none() && units >= r.location + r.length {
            b = Some(i);
        }
        units += c.len_utf16();
    }
    let n = s.chars().count();
    (a.unwrap_or(n), b.unwrap_or(n))
}

fn report_field(id: FieldId) {
    let state = with_shell(|s| {
        let f = if id == FieldId::Input { &s.input } else { &s.hex };
        let text = f.stringValue().to_string();
        let sel = f.currentEditor().map(|e| char_range(&text, e.selectedRange())).unwrap_or_else(|| {
            let n = text.chars().count();
            (n, n)
        });
        (text, sel)
    });
    if let Some((text, sel)) = state {
        with_widget(|w| w.field_changed(id, &text, sel));
        after_event();
    }
}

fn style_field_editors() {
    with_shell(|s| {
        for f in [&s.input, &s.hex] {
            if let Some(ed) = f.currentEditor() {
                if let Ok(tv) = ed.downcast::<NSTextView>() {
                    let caret = with_widget_ro(|w| w.theme().accent);
                    if let Some(c) = caret {
                        tv.setInsertionPointColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(c.r as f64, c.g as f64, c.b as f64, 1.0)));
                        let sel = NSColor::colorWithSRGBRed_green_blue_alpha(c.r as f64, c.g as f64, c.b as f64, 0.4);
                        let attrs = objc2_foundation::NSDictionary::<NSString, AnyObject>::from_retained_objects(
                            &[unsafe { objc2_app_kit::NSBackgroundColorAttributeName }],
                            &[Retained::into_super(Retained::into_super(sel))],
                        );
                        unsafe { tv.setSelectedTextAttributes(&attrs) };
                    }
                    tv.setAutomaticQuoteSubstitutionEnabled(false);
                    tv.setAutomaticDashSubstitutionEnabled(false);
                    tv.setAutomaticTextReplacementEnabled(false);
                    tv.setAutomaticSpellingCorrectionEnabled(false);
                    tv.setContinuousSpellCheckingEnabled(false);
                    tv.setGrammarCheckingEnabled(false);
                    tv.setAutomaticTextCompletionEnabled(false);
                    let supported: bool = unsafe { msg_send![&*tv, respondsToSelector: sel!(setWritingToolsBehavior:)] };
                    if supported {
                        let _: () = unsafe { msg_send![&*tv, setWritingToolsBehavior: -1isize] };
                    }
                }
            }
        }
    });
}

fn with_widget_ro<R>(f: impl FnOnce(&Widget) -> R) -> Option<R> {
    WIDGET.with(|w| w.try_borrow().ok().and_then(|w| w.as_ref().map(f)))
}

/// Positions and styles the native text fields over the canvas.
fn sync_fields() {
    let Some(specs) = with_widget_ro(|w| (w.fields(), w.settings.dyslexia_font)) else { return };
    let (specs, dys) = specs;
    with_shell(|s| {
        for spec in specs.iter() {
            let f = if spec.id == FieldId::Input { &s.input } else { &s.hex };
            let st = spec.style;
            let font = painter_font(&st, dys);
            let asc = font.ascender();
            let desc = -font.descender();
            // Match CSS: a 20px line box vertically centred in the input.
            let lh = if spec.id == FieldId::Input { if st.size > 17.0 { 24.0 } else { 20.0 } } else { spec.rect.h as f64 };
            let top = spec.rect.y as f64 + (spec.rect.h as f64 - lh) / 2.0 + (lh - (asc + desc)) / 2.0;
            let h = asc + desc + font.leading() + 2.0;
            let pad_x = if spec.id == FieldId::Hex { 8.0 } else { 0.0 };
            f.setFrame(NSRect::new(NSPoint::new(spec.rect.x as f64 - 2.0 + pad_x, top - 1.0), NSSize::new(spec.rect.w as f64 + 4.0 - 2.0 * pad_x, h)));
            f.setFont(Some(&font));
            let c = st.color;
            f.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(c.r as f64, c.g as f64, c.b as f64, c.a as f64)));
            f.setHidden(!spec.visible);
            f.setEnabled(spec.enabled);
        }
        if !specs.iter().any(|f| f.id == FieldId::Hex) {
            s.hex.setHidden(true);
        }
    });
    style_field_editors();
}

fn painter_font(st: &franconvert_core::ui::theme::TextStyle, dys: bool) -> Retained<NSFont> {
    painter::font(st, dys)
}

fn apply_cursor() {
    let c = with_widget_ro(|w| w.cursor()).unwrap_or(Cursor::Arrow);
    let cur = match c {
        Cursor::Arrow => NSCursor::arrowCursor(),
        Cursor::Pointer => NSCursor::pointingHandCursor(),
        Cursor::Grab => NSCursor::openHandCursor(),
        Cursor::Crosshair => NSCursor::crosshairCursor(),
    };
    cur.set();
}

// ── Keyboard ─────────────────────────────────────────────────────────────

fn mods_of(e: &NSEvent) -> Mods {
    let f = e.modifierFlags();
    Mods {
        shift: f.contains(NSEventModifierFlags::Shift),
        ctrl: f.contains(NSEventModifierFlags::Control),
        meta: f.contains(NSEventModifierFlags::Command),
        alt: f.contains(NSEventModifierFlags::Option),
    }
}

fn key_of(e: &NSEvent, mods: Mods) -> Option<Key> {
    Some(match e.keyCode() {
        126 => Key::Up,
        125 => Key::Down,
        123 => Key::Left,
        124 => Key::Right,
        49 => Key::Space,
        48 => Key::Tab,
        36 | 76 => Key::Enter,
        53 => Key::Escape,
        51 => Key::Backspace,
        _ if mods.meta || mods.ctrl => {
            let c = e.charactersIgnoringModifiers()?.to_string().to_lowercase().chars().next()?;
            Key::Command(c)
        }
        _ => return None,
    })
}

/// Name used in shortcut strings ("A", "5", "F3", "Space", "Escape").
fn key_name(e: &NSEvent) -> Option<String> {
    let code = e.keyCode();
    let f_keys = [(122, 1), (120, 2), (99, 3), (118, 4), (96, 5), (97, 6), (98, 7), (100, 8), (101, 9), (109, 10), (103, 11), (111, 12)];
    if let Some((_, n)) = f_keys.iter().find(|(c, _)| *c == code) {
        return Some(format!("F{n}"));
    }
    match code {
        49 => return Some("Space".into()),
        53 => return Some("Escape".into()),
        _ => {}
    }
    let c = e.charactersIgnoringModifiers()?.to_string().chars().next()?;
    c.is_ascii_alphanumeric().then(|| c.to_ascii_uppercase().to_string())
}

fn install_key_monitor() -> Retained<AnyObject> {
    let mask = NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged | NSEventMask::LeftMouseUp;
    let block = RcBlock::new(|e: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let ev = unsafe { e.as_ref() };
        if handle_event(ev) { std::ptr::null_mut() } else { e.as_ptr() }
    });
    unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }.expect("key monitor")
}

/// Returns true to swallow the event.
fn handle_event(e: &NSEvent) -> bool {
    let ty = e.r#type();
    let window = e.window(MainThreadMarker::new().unwrap());
    let in_onboarding = with_shell(|s| {
        s.onboarding.as_ref().is_some_and(|(w, _)| window.as_ref().is_some_and(|ew| std::ptr::eq(Retained::as_ptr(ew).cast::<u8>(), Retained::as_ptr(w).cast())))
    })
    .unwrap_or(false);
    if in_onboarding {
        let f = e.modifierFlags();
        let is_a = matches!(ty, NSEventType::KeyDown) && e.keyCode() == 0;
        ONBOARD.with(|o| {
            if let Some(o) = o.borrow_mut().as_mut() {
                if ty == NSEventType::KeyDown && e.keyCode() == 53 {
                    o.dismissed = true;
                }
                o.keys(f.contains(NSEventModifierFlags::Command), f.contains(NSEventModifierFlags::Shift), is_a);
            }
        });
        onboarding_tick();
        return ty == NSEventType::KeyDown;
    }
    let in_overlay = with_shell(|s| window.as_ref().is_some_and(|ew| std::ptr::eq(Retained::as_ptr(ew).cast::<u8>(), Retained::as_ptr(&s.overlay).cast())))
        .unwrap_or(false);
    if !in_overlay {
        return false;
    }
    let mods = mods_of(e);
    let recording = with_widget_ro(|w| w.is_recording()).unwrap_or(false);
    match ty {
        NSEventType::FlagsChanged if recording => {
            let mut names = Vec::new();
            if mods.ctrl {
                names.push("Ctrl");
            }
            if mods.shift {
                names.push("Shift");
            }
            if mods.alt {
                names.push("Alt");
            }
            if mods.meta {
                names.push("Cmd");
            }
            with_widget(|w| w.record_modifiers(names));
            after_event();
            false
        }
        NSEventType::KeyDown if recording => {
            if let Some(name) = key_name(e) {
                with_widget(|w| w.record_key(&name, mods));
                after_event();
            }
            true
        }
        NSEventType::KeyDown => {
            let Some(k) = key_of(e, mods) else { return false };
            let handled = with_widget(|w| w.key(k, mods)).unwrap_or(false);
            after_event();
            handled
        }
        NSEventType::KeyUp | NSEventType::LeftMouseUp => {
            // Caret / selection may have moved.
            if with_shell(|s| s.input.currentEditor().is_some()).unwrap_or(false) {
                on_main(|| report_field(FieldId::Input));
            }
            false
        }
        _ => false,
    }
}

// ── Appearance changes ───────────────────────────────────────────────────

fn observe_appearance() -> Vec<Retained<AnyObject>> {
    let refresh = || {
        on_main(|| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let dark = system::is_dark(mtm);
            let accent = system::accent_hex();
            with_widget(|w| w.system_appearance(dark, accent));
            ONBOARD.with(|o| {
                if let Some(o) = o.borrow_mut().as_mut() {
                    o.dark = dark;
                }
            });
            with_shell(|s| {
                if let Some((_, c)) = &s.onboarding {
                    c.setNeedsDisplay(true);
                }
            });
            after_event();
        });
    };
    let mut out = Vec::new();
    let block = RcBlock::new(move |_n: std::ptr::NonNull<NSNotification>| refresh());
    let dist = NSDistributedNotificationCenter::defaultCenter();
    for name in ["AppleInterfaceThemeChangedNotification", "AppleColorPreferencesChangedNotification", "AppleAquaColorVariantChanged"] {
        let o = unsafe { dist.addObserverForName_object_queue_usingBlock(Some(&NSString::from_str(name)), None, None, &block) };
        out.push(unsafe { Retained::cast_unchecked::<AnyObject>(o) });
    }
    let center = NSNotificationCenter::defaultCenter();
    let o = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(objc2_app_kit::NSSystemColorsDidChangeNotification), None, None, &block)
    };
    out.push(unsafe { Retained::cast_unchecked::<AnyObject>(o) });
    out
}

// ── Timers ───────────────────────────────────────────────────────────────

/// Keeps a 60 Hz timer while something animates; otherwise sleeps until the
/// widget's next timer.
fn schedule() {
    let t = now();
    with_widget(|w| w.tick(t));
    let (animating, wake) = with_widget_ro(|w| (w.animating(), w.next_wakeup())).unwrap_or((false, None));
    let onboard_anim = ONBOARD.with(|o| o.borrow().as_ref().is_some_and(|o| o.animating()));
    with_shell(|s| {
        if animating || onboard_anim {
            if s.frame_timer.is_none() {
                let block = RcBlock::new(|_t: std::ptr::NonNull<NSTimer>| frame());
                s.frame_timer = Some(unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0 / 60.0, true, &block) });
            }
        } else if let Some(timer) = s.frame_timer.take() {
            timer.invalidate();
        }
        if let Some(timer) = s.wake_timer.take() {
            timer.invalidate();
        }
        if let Some(secs) = wake {
            let block = RcBlock::new(|_t: std::ptr::NonNull<NSTimer>| {
                with_shell(|s| s.wake_timer = None);
                frame();
            });
            s.wake_timer = Some(unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(secs.max(0.01), false, &block) });
        }
    });
}

fn frame() {
    let cmds_pending = {
        let t = now();
        with_widget(|w| w.tick(t));
        true
    };
    if cmds_pending {
        let cmds = with_widget(|w| w.take_commands()).unwrap_or_default();
        for c in cmds {
            run_command(c);
        }
    }
    onboarding_tick();
    with_shell(|s| s.canvas.setNeedsDisplay(true));
    schedule_light();
}

/// Like `schedule` but without re-ticking (avoids recursion from `frame`).
fn schedule_light() {
    let (animating, _) = with_widget_ro(|w| (w.animating(), w.next_wakeup())).unwrap_or((false, None));
    let onboard_anim = ONBOARD.with(|o| o.borrow().as_ref().is_some_and(|o| o.animating()));
    if !animating && !onboard_anim {
        schedule();
    }
}

// ── Onboarding ───────────────────────────────────────────────────────────

fn show_onboarding(mtm: MainThreadMarker) {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(onboarding::W as f64, onboarding::H as f64));
    let win = make_window(mtm, frame);
    win.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    let effect = make_backdrop(mtm, frame);
    win.setContentView(Some(&effect));
    let canvas = Canvas::new(mtm, frame, 1);
    effect.addSubview(&canvas);
    win.center();
    let dark = system::is_dark(mtm);
    ONBOARD.with(|o| *o.borrow_mut() = Some(Onboarding::new(dark, true, &system::accent_hex(), now())));
    activate_self(mtm);
    win.makeKeyAndOrderFront(None);
    with_shell(|s| s.onboarding = Some((win, canvas)));
}

fn onboarding_tick() {
    let t = now();
    let done = ONBOARD.with(|o| o.borrow_mut().as_mut().map(|o| o.tick(t))).unwrap_or(false);
    with_shell(|s| {
        if let Some((w, c)) = &s.onboarding {
            if done {
                let _ = fs::write(s.data_dir.join("onboarding_shown"), "1");
                w.orderOut(None);
            } else {
                c.setNeedsDisplay(true);
            }
        }
        if done {
            s.onboarding = None;
        }
    });
    if done {
        ONBOARD.with(|o| *o.borrow_mut() = None);
    }
}
