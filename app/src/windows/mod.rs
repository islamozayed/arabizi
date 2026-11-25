//! The Windows app: a tray app with a borderless, acrylic widget window.
//! Drawing goes through Direct2D/DirectWrite (`painter`); behaviour lives in
//! `franconvert_core::ui::Widget`.
//!
//! The text fields are drawn by the widget's painter too, so their text,
//! caret and selection match the rest of the UI on the translucent backdrop
//! (GDI-drawn controls lose their alpha there). Each field is backed by a
//! real EDIT control kept out of sight, which owns the text, caret, undo,
//! clipboard and keyboard layout handling.

mod access;
mod painter;
mod surface;
mod system;

use crate::{shortcut, single_instance, updater};
use franconvert_core::session::{Key, Mods};
use franconvert_core::settings::Settings;
use franconvert_core::ui::geom::{Color, Point, Rect};
use franconvert_core::ui::onboarding::{self, Onboarding};
use franconvert_core::ui::paint::Painter;
use franconvert_core::ui::{Command, Cursor, FieldId, FieldSpec, UpdateEvent, Widget, WidgetInit, WINDOW_W};
use franconvert_core::{data_dir, TransliterationEngine};
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use painter::D2dPainter;
use surface::Surface;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};
use windows::core::{w, Interface, BOOL, HSTRING, PCWSTR};
use windows::Win32::UI::Controls::{EM_GETSEL, EM_REPLACESEL, EM_SETSEL, MARGINS, WM_MOUSELEAVE};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const CHANGELOG: &str = include_str!("../../ui/CHANGELOG.json");

const WM_APP_CALL: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 2;
const WM_APP_TOGGLE: u32 = WM_APP + 3;
const TIMER_FRAME: usize = 1;
const TIMER_WAKE: usize = 2;
const TIMER_CARET: usize = 3;
const MENU_SHOW: usize = 1;
const MENU_QUIT: usize = 2;
const TRAY_ID: u32 = 1;
const CLASS: PCWSTR = w!("FranconvertWidget");
const NIN_KEYSELECT_: u32 = NIN_SELECT | 1;

// ── Global state (main thread only) ──────────────────────────────────────

#[derive(Clone, Copy)]
struct Handles {
    overlay: HWND,
    input: HWND,
    hex: HWND,
}

impl Handles {
    fn edit(&self, id: FieldId) -> HWND {
        if id == FieldId::Input { self.input } else { self.hex }
    }
    fn field_of(&self, hwnd: HWND) -> Option<FieldId> {
        if hwnd == self.input {
            Some(FieldId::Input)
        } else if hwnd == self.hex {
            Some(FieldId::Hex)
        } else {
            None
        }
    }
}

/// Small bits of shell state; borrowed briefly, never across a Win32 call
/// that could re-enter the window procedure.
#[derive(Default)]
struct Ui {
    data_dir: PathBuf,
    /// Horizontal scroll of each field's text.
    scroll: HashMap<FieldId, f32>,
    /// A field being drag-selected: (field, anchor).
    field_drag: Option<(FieldId, u32)>,
    /// Moving the window: cursor offset from its top-left (px).
    window_drag: Option<(i32, i32)>,
    /// (time, x, y, count) of the last mouse press, for double clicks.
    last_click: (f64, i32, i32, u32),
    mouse_tracked: bool,
    swallow_char: bool,
    /// Ignore EN_CHANGE while the shell itself sets a field's text.
    suppress_change: bool,
    caret_on: bool,
    frame_timer: bool,
    dark_applied: Option<bool>,
    backdrop: bool,
    tray_icon: Option<HICON>,
    tray_light: bool,
    taskbar_created: u32,
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
    static ONBOARD: RefCell<Option<Onboarding>> = const { RefCell::new(None) };
    static HANDLES: Cell<Option<Handles>> = const { Cell::new(None) };
    static ONBOARD_HWND: Cell<Option<HWND>> = const { Cell::new(None) };
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
    static HOTKEYS: RefCell<Option<(GlobalHotKeyManager, Option<HotKey>)>> = const { RefCell::new(None) };
    static SURFACES: RefCell<HashMap<isize, Surface>> = RefCell::new(HashMap::new());
    /// The app that had focus before the widget showed (paste goes there).
    static PREV: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    /// The last foreground window that wasn't ours or the taskbar.
    static LAST_EXTERNAL: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static START: Instant = Instant::now();
}

/// The widget window, for posting to the main thread from other threads.
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

fn with_widget<R>(f: impl FnOnce(&mut Widget) -> R) -> Option<R> {
    WIDGET.with(|w| w.try_borrow_mut().ok().and_then(|mut w| w.as_mut().map(f)))
}

fn with_widget_ro<R>(f: impl FnOnce(&Widget) -> R) -> Option<R> {
    WIDGET.with(|w| w.try_borrow().ok().and_then(|w| w.as_ref().map(f)))
}

fn ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    UI.with(|u| f(&mut u.borrow_mut()))
}

fn handles() -> Option<Handles> {
    HANDLES.with(|h| h.get())
}

fn now() -> f64 {
    START.with(|s| s.elapsed().as_secs_f64())
}

/// Runs `f` on the main thread (from any thread).
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    let hwnd = MAIN_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return;
    }
    let boxed: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(f));
    let ptr = Box::into_raw(boxed);
    if unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_CALL, WPARAM(0), LPARAM(ptr as isize)) }.is_err() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

fn lo_word(v: isize) -> i32 {
    (v & 0xffff) as i16 as i32
}

fn hi_word(v: isize) -> i32 {
    ((v >> 16) & 0xffff) as i16 as i32
}

fn scale(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn is_ours(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == std::process::id()
}

// ── Startup ──────────────────────────────────────────────────────────────

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let dir = data_dir();
    if !single_instance::acquire(&dir, || on_main(show_existing)) {
        return; // Another copy was asked to show itself.
    }
    startup(dir);
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    single_instance::cleanup(&data_dir());
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

fn startup(dir: PathBuf) {
    let _ = now(); // start the clock
    let (settings, returning) = load_settings(&dir);
    let prefs = franconvert_core::load_prefs(&dir);
    let widget = Widget::new(WidgetInit {
        engine: TransliterationEngine::new(),
        data_dir: dir.clone(),
        settings,
        prefs,
        is_mac: false,
        system_dark: system::is_dark(),
        system_accent: system::accent_hex(),
        autostart: system::autostart_enabled(),
        now: 0.0,
    });
    let height = widget.window_height();
    let backdrop = system::build_number() >= 22621;
    ui(|u| {
        u.data_dir = dir.clone();
        u.backdrop = backdrop;
        u.caret_on = true;
        u.taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    });

    // ── Widget window ──
    register_class();
    let overlay = make_window(WINDOW_W, height, w!("Franconvert"));
    MAIN_HWND.store(overlay.0 as isize, Ordering::Release);
    let input = make_edit(overlay, 1);
    let hex = make_edit(overlay, 2);
    access::name_field(input, w!("Franco text"));
    access::name_field(hex, w!("Hex colour"));
    HANDLES.with(|h| h.set(Some(Handles { overlay, input, hex })));
    WIDGET.with(|w| *w.borrow_mut() = Some(widget));
    restore_position(overlay, &dir);
    apply_window_theme(overlay);

    // ── Tray ──
    add_tray_icon(overlay);

    // ── Global shortcut ──
    match GlobalHotKeyManager::new() {
        Ok(m) => HOTKEYS.with(|h| *h.borrow_mut() = Some((m, None))),
        Err(e) => eprintln!("global shortcuts unavailable: {e}"),
    }
    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state == HotKeyState::Pressed {
            let hwnd = MAIN_HWND.load(Ordering::Acquire);
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
            }
        }
    }));

    // Remember the last app the user was in, so a tray click (which makes
    // the taskbar the foreground window) still pastes into that app.
    unsafe {
        let _: HWINEVENTHOOK = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(foreground_changed), 0, 0, WINEVENT_OUTOFCONTEXT);
    }
    note_external(system::foreground());

    let shortcut = with_widget(|w| w.shortcut()).unwrap();
    if !register_shortcut(&shortcut) {
        let default = with_widget(|w| w.default_shortcut().to_string()).unwrap();
        register_shortcut(&default);
    }
    with_widget(|w| w.maybe_show_changelog(CHANGELOG, returning));
    sync_fields();

    // First launch: welcome window.
    if !dir.join("onboarding_shown").exists() {
        show_onboarding();
    }
    // Check for updates at launch (quietly).
    if std::env::var_os("FRANCONVERT_NO_UPDATE_CHECK").is_none() {
        run_command(Command::CheckForUpdates { manual: false });
    }
    if std::env::var_os("FRANCONVERT_SHOW").is_some() {
        toggle_overlay();
    }
    schedule();
}

fn register_class() {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let icon = LoadImageW(Some(hinstance.into()), PCWSTR(1 as _), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE | LR_SHARED)
            .map(|h| HICON(h.0))
            .unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: icon,
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

/// A hidden, borderless, topmost tool window `w`×`h` DIPs, with the system
/// acrylic backdrop (Windows 11), rounded corners and the system shadow.
///
/// Windows only draws a system backdrop behind windows that have a frame (a
/// bare popup gets a flat fallback colour instead), so the window keeps a
/// resizing frame and folds it into the client area (`WM_NCCALCSIZE`);
/// `WM_NCHITTEST` and `WM_WINDOWPOSCHANGING` keep it from being resized.
fn make_window(w: f32, h: f32, title: PCWSTR) -> HWND {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP,
            CLASS,
            title,
            WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
            0,
            0,
            w as i32,
            h as i32,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .expect("create window");
        let s = scale(hwnd);
        let _ = SetWindowPos(hwnd, None, 0, 0, (w * s).round() as i32, (h * s).round() as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, (&corner as *const DWM_WINDOW_CORNER_PREFERENCE).cast(), 4);
        if ui(|u| u.backdrop) {
            let backdrop = DWMSBT_TRANSIENTWINDOW;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, (&backdrop as *const DWM_SYSTEMBACKDROP_TYPE).cast(), 4);
        }
        // Re-run WM_NCCALCSIZE so the frame folds into the client area.
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        hwnd
    }
}

/// An EDIT control parked outside the window's client area: it owns the
/// text and keyboard handling, and the widget draws it.
fn make_edit(parent: HWND, id: usize) -> HWND {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            -4000,
            -4000,
            600,
            40,
            Some(parent),
            Some(HMENU(id as *mut _)),
            Some(hinstance.into()),
            None,
        )
        .expect("create edit");
        let prev = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as usize as isize);
        SetWindowLongPtrW(edit, GWLP_USERDATA, prev);
        edit
    }
}

/// Dark mode tints the acrylic; keep it in step with the widget's theme.
fn apply_window_theme(hwnd: HWND) {
    let dark = with_widget_ro(|w| w.is_dark()).unwrap_or(false);
    let is_overlay = handles().is_some_and(|h| h.overlay == hwnd);
    if is_overlay && ui(|u| u.dark_applied == Some(dark)) {
        return;
    }
    if is_overlay {
        ui(|u| u.dark_applied = Some(dark));
    }
    let v = BOOL::from(dark);
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&v as *const BOOL).cast(), 4);
    }
}

// ── Window position (window_pos.json, physical px like the Tauri build) ──

fn work_area(hmon: HMONITOR) -> RECT {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(hmon, &mut mi);
    }
    mi.rcWork
}

fn restore_position(hwnd: HWND, dir: &std::path::Path) {
    let saved = fs::read_to_string(dir.join("window_pos.json"))
        .ok()
        .and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok())
        .and_then(|v| Some((v.get("x")?.as_f64()? as i32, v.get("y")?.as_f64()? as i32)));
    let on_screen = saved.filter(|&(x, y)| unsafe { !MonitorFromPoint(POINT { x: x + 20, y: y + 20 }, MONITOR_DEFAULTTONULL).is_invalid() });
    let (x, y) = match on_screen {
        Some(p) => p,
        None => {
            // Centred, a little above the middle (like NSWindow.center).
            let wa = work_area(unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) });
            let mut r = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut r);
            }
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            (wa.left + (wa.right - wa.left - w) / 2, wa.top + (wa.bottom - wa.top - h) / 3)
        }
    };
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    fit_window(hwnd);
}

/// Sizes the window for its monitor's DPI, keeping the top-left corner.
fn fit_window(hwnd: HWND) {
    let Some((w, h)) = intended_size(hwnd) else { return };
    unsafe {
        let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// The window's size in px for its current DPI.
fn intended_size(hwnd: HWND) -> Option<(i32, i32)> {
    let (w, h) = if is_onboarding(hwnd) {
        (onboarding::W, onboarding::H)
    } else if handles().is_some_and(|h| h.overlay == hwnd) {
        (WINDOW_W, with_widget_ro(|w| w.window_height())?)
    } else {
        return None;
    };
    let s = scale(hwnd);
    Some(((w * s).round() as i32, (h * s).round() as i32))
}

/// The resizing frame is only there for the backdrop: undo any resize from
/// outside (Snap, Win+arrow keys).
fn keep_size(hwnd: HWND, lp: LPARAM) {
    let pos = unsafe { &mut *(lp.0 as *mut WINDOWPOS) };
    if pos.flags.contains(SWP_NOSIZE) {
        return;
    }
    if let Some((w, h)) = intended_size(hwnd) {
        pos.cx = w;
        pos.cy = h;
    }
}

fn save_position() {
    let Some(h) = handles() else { return };
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(h.overlay, &mut r);
    }
    let dir = ui(|u| u.data_dir.clone());
    let _ = fs::write(dir.join("window_pos.json"), format!("{{\"x\":{},\"y\":{}}}", r.left, r.top));
}

// ── Show / hide ──────────────────────────────────────────────────────────

/// A window the user works in (not ours, not the taskbar or tray overflow).
fn is_app_window(hwnd: HWND) -> bool {
    if hwnd.is_invalid() || is_ours(hwnd) {
        return false;
    }
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    let class = String::from_utf16_lossy(&buf[..n]);
    !matches!(
        class.as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow" | "TopLevelWindowForOverflowXamlIsland" | "Progman" | "WorkerW"
    )
}

fn note_external(hwnd: HWND) {
    if is_app_window(hwnd) {
        LAST_EXTERNAL.with(|l| l.set(hwnd));
    }
}

unsafe extern "system" fn foreground_changed(_hook: HWINEVENTHOOK, _event: u32, hwnd: HWND, _obj: i32, _child: i32, _thread: u32, _time: u32) {
    note_external(hwnd);
}

fn capture_prev_focus() {
    let fg = system::foreground();
    let target = if is_app_window(fg) { fg } else { LAST_EXTERNAL.with(|l| l.get()) };
    if !target.is_invalid() {
        PREV.with(|p| p.set(target));
    }
}

fn overlay_visible() -> bool {
    handles().is_some_and(|h| unsafe { IsWindowVisible(h.overlay) }.as_bool())
}

fn toggle_overlay() {
    // The welcome window celebrates the shortcut, then closes itself.
    if ONBOARD_HWND.with(|o| o.get()).is_some() {
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

/// Reopening (a second launch) reveals the widget without clearing it.
fn show_existing() {
    if let Some(w) = ONBOARD_HWND.with(|o| o.get()) {
        unsafe {
            let _ = ShowWindow(w, SW_SHOW);
            let _ = SetForegroundWindow(w);
        }
        return;
    }
    if !overlay_visible() {
        capture_prev_focus();
    }
    show_overlay_window();
    after_event();
}

fn show_overlay_window() {
    let Some(h) = handles() else { return };
    fit_window(h.overlay);
    unsafe {
        let _ = ShowWindow(h.overlay, SW_SHOW);
        let _ = SetWindowPos(h.overlay, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        if !SetForegroundWindow(h.overlay).as_bool() {
            // Windows only lets the foreground app hand focus over; a
            // synthetic key press counts as user input and unlocks it.
            let alt = [system_key(VK_MENU, false), system_key(VK_MENU, true)];
            SendInput(&alt, std::mem::size_of::<INPUT>() as i32);
            let _ = SetForegroundWindow(h.overlay);
        }
    }
    focus_default();
    invalidate(h.overlay);
}

fn system_key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
    }
}

/// Focus the input when it's usable, otherwise the window itself.
fn focus_default() {
    let Some(h) = handles() else { return };
    let input_ok = with_widget_ro(|w| w.fields().iter().any(|f| f.id == FieldId::Input && f.visible && f.enabled)).unwrap_or(false);
    unsafe {
        let _ = SetFocus(Some(if input_ok { h.input } else { h.overlay }));
    }
}

fn hide_overlay() {
    save_position();
    if let Some(h) = handles() {
        unsafe {
            let _ = ShowWindow(h.overlay, SW_HIDE);
        }
    }
}

/// Hides the widget, gives focus back to the previous app and pastes `text`,
/// then restores whatever text the clipboard held before.
fn paste_and_hide(text: String) {
    let prev = PREV.with(|p| p.replace(HWND::default()));
    let saved = system::clipboard_text();
    let ours = system::set_clipboard_text(&text);
    hide_overlay();
    system::activate(prev);
    let prev_raw = prev.0 as isize;
    std::thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(750) {
            let front = system::foreground();
            let ready = if prev_raw != 0 { front.0 as isize == prev_raw } else { !front.is_invalid() && !is_ours(front) };
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
                if system::clipboard_sequence() == ours {
                    system::set_clipboard_text(&previous);
                }
            });
        }
    });
}

// ── Event plumbing ───────────────────────────────────────────────────────

/// After any event: run the widget's commands, sync fields, redraw, and
/// (re)arm timers.
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
    if let Some(h) = handles() {
        apply_window_theme(h.overlay);
        invalidate(h.overlay);
    }
    schedule();
}

fn run_command(c: Command) {
    match c {
        Command::Hide => hide_overlay(),
        Command::Paste(text) => paste_and_hide(text),
        Command::StartDrag => {
            let Some(h) = handles() else { return };
            let mut cur = POINT::default();
            let mut r = RECT::default();
            unsafe {
                let _ = GetCursorPos(&mut cur);
                let _ = GetWindowRect(h.overlay, &mut r);
                SetCapture(h.overlay);
            }
            ui(|u| u.window_drag = Some((cur.x - r.left, cur.y - r.top)));
        }
        Command::SetField { field, text, select_all } => {
            let Some(h) = handles() else { return };
            let edit = h.edit(field);
            ui(|u| u.suppress_change = true);
            unsafe {
                let _ = SetWindowTextW(edit, &HSTRING::from(text.as_str()));
                let n = text.encode_utf16().count();
                let (a, b) = if select_all { (0, n) } else { (n, n) };
                SendMessageW(edit, EM_SETSEL, Some(WPARAM(a)), Some(LPARAM(b as isize)));
            }
            ui(|u| {
                u.suppress_change = false;
                u.scroll.insert(field, 0.0);
            });
            reset_caret();
        }
        Command::FocusField(field) => {
            let Some(h) = handles() else { return };
            let edit = h.edit(field);
            unsafe {
                if GetFocus() != edit {
                    let _ = SetFocus(Some(edit));
                    let n = GetWindowTextLengthW(edit).max(0) as usize;
                    SendMessageW(edit, EM_SETSEL, Some(WPARAM(n)), Some(LPARAM(n as isize)));
                }
            }
            reset_caret();
        }
        Command::ResizeWindow(_) => {
            if let Some(h) = handles() {
                fit_window(h.overlay);
            }
        }
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
                .and_then(|bytes| updater::stage(&bytes, &release));
                on_main(move || {
                    if let Ok(path) = &result {
                        *STAGED_INSTALLER.lock().unwrap() = Some(path.clone());
                    }
                    with_widget(|w| w.update_event(if result.is_ok() { UpdateEvent::Installed } else { UpdateEvent::InstallFailed }));
                    after_event();
                });
            });
        }
        Command::Relaunch => relaunch(),
        Command::Announce(text) => {
            if let Some(h) = handles() {
                access::announce(h.overlay, &text);
            }
        }
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
static STAGED_INSTALLER: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Restarts the app, running a downloaded update's installer first.
fn relaunch() {
    let exe = std::env::current_exe().unwrap_or_default();
    single_instance::cleanup(&data_dir());
    let staged = STAGED_INSTALLER.lock().unwrap().take();
    match staged {
        Some(installer) => updater::run_installer(&installer, &exe),
        None => {
            let _ = std::process::Command::new(&exe).env("FRANCONVERT_WAIT_PID", std::process::id().to_string()).spawn();
        }
    }
    quit();
}

fn quit() {
    if let Some(h) = handles() {
        access::shutdown(h.overlay);
        remove_tray_icon(h.overlay);
        hide_overlay();
    }
    unsafe { PostQuitMessage(0) };
}

fn register_shortcut(combo: &str) -> bool {
    let Some(hk) = shortcut::parse(combo) else { return false };
    HOTKEYS.with(|h| {
        let mut h = h.borrow_mut();
        let Some((manager, current)) = h.as_mut() else { return false };
        if let Some(old) = current.take() {
            let _ = manager.unregister(old);
        }
        match manager.register(hk) {
            Ok(()) => {
                *current = Some(hk);
                true
            }
            Err(_) => false,
        }
    })
}

// ── Tray ─────────────────────────────────────────────────────────────────

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    nid
}

fn tray_icon() -> Option<HICON> {
    let light = system::taskbar_light();
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) * 2;
    let icon = system::logo_icon(size, light);
    ui(|u| {
        if let Some(old) = u.tray_icon.take() {
            unsafe {
                let _ = DestroyIcon(old);
            }
        }
        u.tray_icon = icon;
        u.tray_light = light;
    });
    icon
}

fn add_tray_icon(hwnd: HWND) {
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_APP_TRAY;
    nid.hIcon = tray_icon().unwrap_or_default();
    let tip: Vec<u16> = "Franconvert — Franco Arabic Keyboard".encode_utf16().collect();
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

fn refresh_tray_icon(hwnd: HWND) {
    if ui(|u| u.tray_light) == system::taskbar_light() {
        return;
    }
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_ICON;
    nid.hIcon = tray_icon().unwrap_or_default();
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn remove_tray_icon(hwnd: HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, MENU_SHOW, w!("Show Franconvert"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RIGHTALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match cmd.0 as usize {
            MENU_SHOW => {
                if !overlay_visible() {
                    toggle_overlay();
                }
            }
            MENU_QUIT => quit(),
            _ => {}
        }
    }
}

// ── Fields ───────────────────────────────────────────────────────────────

fn edit_text(edit: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(edit, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..got])
    }
}

/// Selection as UTF-16 offsets.
fn edit_sel(edit: HWND) -> (u32, u32) {
    let (mut a, mut b) = (0u32, 0u32);
    unsafe {
        SendMessageW(edit, EM_GETSEL, Some(WPARAM(&mut a as *mut u32 as usize)), Some(LPARAM(&mut b as *mut u32 as isize)));
    }
    (a, b)
}

fn set_sel(edit: HWND, a: u32, b: u32) {
    unsafe {
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(a as usize)), Some(LPARAM(b as isize)));
    }
}

/// UTF-16 offset → char offset.
fn char_index(s: &str, units: u32) -> usize {
    let mut u = 0u32;
    for (i, c) in s.chars().enumerate() {
        if u >= units {
            return i;
        }
        u += c.len_utf16() as u32;
    }
    s.chars().count()
}

fn report_field(id: FieldId) {
    if ui(|u| u.suppress_change) {
        return;
    }
    let Some(h) = handles() else { return };
    let edit = h.edit(id);
    let text = edit_text(edit);
    let (a, b) = edit_sel(edit);
    let sel = (char_index(&text, a), char_index(&text, b));
    reset_caret();
    with_widget(|w| w.field_changed(id, &text, sel));
    after_event();
}

/// Keeps focus off fields that are hidden or disabled.
fn sync_fields() {
    let Some(h) = handles() else { return };
    let Some(specs) = with_widget_ro(|w| w.fields()) else { return };
    let focus = unsafe { GetFocus() };
    for id in [FieldId::Input, FieldId::Hex] {
        let edit = h.edit(id);
        let shown = specs.iter().any(|s| s.id == id && s.visible);
        let usable = specs.iter().any(|s| s.id == id && s.visible && s.enabled);
        unsafe {
            let _ = EnableWindow(edit, usable);
            // Hidden fields stay out of screen readers' way.
            let _ = ShowWindow(edit, if shown { SW_SHOWNA } else { SW_HIDE });
        }
        if !usable && focus == edit {
            unsafe {
                let _ = SetFocus(Some(h.overlay));
            }
        }
    }
}

fn reset_caret() {
    ui(|u| u.caret_on = true);
    if let Some(h) = handles() {
        let blink = unsafe { GetCaretBlinkTime() };
        unsafe {
            if blink == u32::MAX || blink == 0 {
                let _ = KillTimer(Some(h.overlay), TIMER_CARET);
            } else {
                SetTimer(Some(h.overlay), TIMER_CARET, blink, None);
            }
        }
    }
}

/// Where a field's text sits: (left x, line top, line height).
fn field_geometry(spec: &FieldSpec) -> (f32, f32, f32) {
    let pad = if spec.id == FieldId::Hex { 8.0 } else { 0.0 };
    // Match CSS: a 20px line box vertically centred in the input.
    let lh = if spec.id == FieldId::Input { if spec.style.size > 17.0 { 24.0 } else { 20.0 } } else { spec.rect.h };
    (spec.rect.x + pad, spec.rect.y + (spec.rect.h - lh) / 2.0, lh)
}

fn field_text_rect(spec: &FieldSpec) -> Rect {
    let pad = if spec.id == FieldId::Hex { 8.0 } else { 0.0 };
    Rect::new(spec.rect.x + pad, spec.rect.y, (spec.rect.w - 2.0 * pad).max(1.0), spec.rect.h)
}

/// Draws a field's text, selection and caret (after the widget painted).
fn paint_field(p: &mut D2dPainter, spec: &FieldSpec, edit: HWND, dyslexia: bool, focused: bool) {
    if !spec.visible {
        return;
    }
    let text = edit_text(edit);
    let (a, b) = edit_sel(edit);
    let st = spec.style;
    let (x0, top, lh) = field_geometry(spec);
    let clip = field_text_rect(spec);
    let laid = if text.is_empty() { None } else { painter::layout(&text, &st, dyslexia) };
    let x_of = |pos: u32| laid.as_ref().map(|l| painter::caret_x(l, pos)).unwrap_or(0.0);
    let width = laid.as_ref().map(|l| l.width).unwrap_or(0.0);

    // Scroll so the caret stays in view.
    let caret = x_of(b);
    let avail = clip.w - 2.0;
    let scroll = ui(|u| {
        let s = u.scroll.entry(spec.id).or_insert(0.0);
        if caret - *s > avail {
            *s = caret - avail;
        }
        if caret - *s < 0.0 {
            *s = caret;
        }
        if width - *s < avail {
            *s = (width - avail).max(0.0);
        }
        *s
    });

    let (asc, desc, _) = painter::font_metrics(&st, dyslexia);
    let baseline = top + (lh - (asc + desc)) / 2.0 + asc;
    let band_top = baseline - asc;
    let band_h = asc + desc;

    p.push_clip(clip.outset(1.0), 0.0);
    if focused && a != b {
        let (s0, s1) = (x_of(a.min(b)), x_of(a.max(b)));
        let (l, r) = (s0.min(s1), s0.max(s1));
        p.fill(Rect::new(x0 + l - scroll, band_top, r - l, band_h), 0.0, spec.caret.with_alpha(0.4));
    }
    if laid.is_some() {
        p.text(&text, &st, x0 - scroll, top, lh);
    }
    if focused && a == b && spec.enabled && ui(|u| u.caret_on) {
        let w = if st.size > 17.0 { 2.0 } else { 1.5 };
        p.fill(Rect::new((x0 + caret - scroll - w / 2.0).max(clip.x), band_top, w, band_h), 0.0, spec.caret);
    }
    p.pop();
}

fn field_at(pt: Point) -> Option<FieldSpec> {
    let specs = with_widget_ro(|w| w.fields())?;
    specs.into_iter().rev().find(|s| s.visible && s.enabled && s.rect.contains(pt))
}

fn field_index_at(spec: &FieldSpec, edit: HWND, x: f32) -> u32 {
    let text = edit_text(edit);
    if text.is_empty() {
        return 0;
    }
    let dys = with_widget_ro(|w| w.settings.dyslexia_font).unwrap_or(false);
    let Some(laid) = painter::layout(&text, &spec.style, dys) else { return 0 };
    let (x0, _, _) = field_geometry(spec);
    let scroll = ui(|u| u.scroll.get(&spec.id).copied().unwrap_or(0.0));
    painter::hit_index(&laid, x - x0 + scroll)
}

/// The word around UTF-16 offset `pos`.
fn word_bounds(text: &str, pos: u32) -> (u32, u32) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let is_word = |u: u16| char::from_u32(u as u32).is_some_and(|c| c.is_alphanumeric() || c == '\'') || u >= 0xD800;
    let pos = (pos as usize).min(units.len());
    let mut a = pos;
    while a > 0 && is_word(units[a - 1]) {
        a -= 1;
    }
    let mut b = pos;
    while b < units.len() && is_word(units[b]) {
        b += 1;
    }
    (a as u32, b as u32)
}

fn delete_word_back(edit: HWND) {
    let text = edit_text(edit);
    let (a, b) = edit_sel(edit);
    if a != b {
        unsafe {
            SendMessageW(edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(w!("").as_ptr() as isize)));
        }
        return;
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut i = (b as usize).min(units.len());
    while i > 0 && units[i - 1] == b' ' as u16 {
        i -= 1;
    }
    while i > 0 && units[i - 1] != b' ' as u16 {
        i -= 1;
    }
    set_sel(edit, i as u32, b);
    unsafe {
        SendMessageW(edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(w!("").as_ptr() as isize)));
    }
}

// ── Keyboard ─────────────────────────────────────────────────────────────

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

fn mods_now() -> Mods {
    Mods { shift: down(VK_SHIFT), ctrl: down(VK_CONTROL), meta: down(VK_LWIN) || down(VK_RWIN), alt: down(VK_MENU) }
}

fn is_modifier(vk: u16) -> bool {
    matches!(
        VIRTUAL_KEY(vk),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN
    )
}

fn key_of(vk: u16, mods: Mods) -> Option<Key> {
    Some(match VIRTUAL_KEY(vk) {
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_SPACE => Key::Space,
        VK_TAB => Key::Tab,
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Escape,
        VK_BACK => Key::Backspace,
        _ if (mods.ctrl || mods.meta) && !mods.alt => {
            let c = char::from_u32(unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_CHAR) } & 0xffff)?.to_ascii_lowercase();
            if !c.is_ascii_graphic() {
                return None;
            }
            Key::Command(c)
        }
        _ => return None,
    })
}

/// Name used in shortcut strings ("A", "5", "F3", "Space", "Escape").
fn key_name(vk: u16) -> Option<String> {
    let k = VIRTUAL_KEY(vk);
    if (VK_F1.0..=VK_F12.0).contains(&vk) {
        return Some(format!("F{}", vk - VK_F1.0 + 1));
    }
    match k {
        VK_SPACE => return Some("Space".into()),
        VK_ESCAPE => return Some("Escape".into()),
        _ => {}
    }
    let c = char::from_u32(vk as u32)?;
    c.is_ascii_alphanumeric().then(|| c.to_ascii_uppercase().to_string())
}

fn modifier_names(m: Mods) -> Vec<&'static str> {
    let mut names = Vec::new();
    if m.ctrl {
        names.push("Ctrl");
    }
    if m.shift {
        names.push("Shift");
    }
    if m.alt {
        names.push("Alt");
    }
    if m.meta {
        names.push("Meta");
    }
    names
}

/// A key went down in the widget. Returns true to swallow it.
fn key_down(vk: u16) -> bool {
    let mods = mods_now();
    if with_widget_ro(|w| w.is_recording()).unwrap_or(false) {
        if is_modifier(vk) {
            with_widget(|w| w.record_modifiers(modifier_names(mods)));
        } else if let Some(name) = key_name(vk) {
            with_widget(|w| w.record_key(&name, mods));
        }
        after_event();
        return true;
    }
    let Some(k) = key_of(vk, mods) else { return false };
    let handled = with_widget(|w| w.key(k, mods)).unwrap_or(false);
    after_event();
    handled
}

fn key_up(vk: u16) {
    if is_modifier(vk) && with_widget_ro(|w| w.is_recording()).unwrap_or(false) {
        let mods = mods_now();
        with_widget(|w| w.record_modifiers(modifier_names(mods)));
        after_event();
    }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let prev: WNDPROC = unsafe { std::mem::transmute(GetWindowLongPtrW(hwnd, GWLP_USERDATA)) };
    let id = handles().and_then(|h| h.field_of(hwnd));
    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wp.0 as u16;
            ui(|u| u.swallow_char = false);
            if key_down(vk) {
                ui(|u| u.swallow_char = true);
                return LRESULT(0);
            }
            let m = mods_now();
            if m.ctrl && !m.alt && vk == VK_A.0 {
                set_sel(hwnd, 0, u32::MAX);
                ui(|u| u.swallow_char = true);
                if let Some(id) = id {
                    report_field(id);
                }
                return LRESULT(0);
            }
            if m.ctrl && vk == VK_BACK.0 {
                delete_word_back(hwnd);
                ui(|u| u.swallow_char = true);
                return LRESULT(0);
            }
            let r = unsafe { CallWindowProcW(prev, hwnd, msg, wp, lp) };
            // Arrows, Home/End move the caret without changing the text.
            if let Some(id) = id {
                report_field(id);
            }
            return r;
        }
        WM_CHAR | WM_SYSCHAR => {
            if ui(|u| std::mem::take(&mut u.swallow_char)) {
                return LRESULT(0);
            }
            // Keys a one-line field would beep at.
            if matches!(wp.0 as u32, 0x09 | 0x0A | 0x0D | 0x1B | 0x7F) {
                return LRESULT(0);
            }
        }
        WM_KEYUP | WM_SYSKEYUP => key_up(wp.0 as u16),
        // The canvas handles the mouse for the field.
        WM_NCHITTEST => return LRESULT(HTTRANSPARENT as isize),
        WM_SETFOCUS | WM_KILLFOCUS => {
            reset_caret();
            if let Some(h) = handles() {
                invalidate(h.overlay);
            }
        }
        _ => {}
    }
    unsafe { CallWindowProcW(prev, hwnd, msg, wp, lp) }
}

// ── Window procedure ─────────────────────────────────────────────────────

fn client_point(hwnd: HWND, lp: LPARAM) -> Point {
    let s = scale(hwnd);
    Point { x: lo_word(lp.0) as f32 / s, y: hi_word(lp.0) as f32 / s }
}

/// Counts consecutive clicks (Windows only reports doubles natively).
fn click_count(lp: LPARAM) -> u32 {
    let (x, y) = (lo_word(lp.0), hi_word(lp.0));
    let t = now();
    let max_gap = unsafe { GetDoubleClickTime() } as f64 / 1000.0;
    let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
    ui(|u| {
        let (lt, lx, ly, n) = u.last_click;
        let count = if t - lt <= max_gap && (x - lx).abs() <= cx / 2 && (y - ly).abs() <= cy / 2 { n + 1 } else { 1 };
        u.last_click = (t, x, y, count);
        count
    })
}

fn is_onboarding(hwnd: HWND) -> bool {
    ONBOARD_HWND.with(|o| o.get()) == Some(hwnd)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if is_onboarding(hwnd) {
        if let Some(r) = onboarding_proc(hwnd, msg, wp, lp) {
            return r;
        }
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    match msg {
        WM_APP_CALL => {
            let f = unsafe { Box::from_raw(lp.0 as *mut Box<dyn FnOnce() + Send>) };
            f();
            return LRESULT(0);
        }
        WM_APP_TOGGLE => {
            toggle_overlay();
            return LRESULT(0);
        }
        WM_APP_TRAY => {
            match lo_word(lp.0) as u32 {
                NIN_SELECT | NIN_KEYSELECT_ => {
                    if overlay_visible() {
                        hide_overlay();
                    } else {
                        toggle_overlay();
                    }
                }
                WM_CONTEXTMENU => tray_menu(hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        WM_GETOBJECT if !is_onboarding(hwnd) => {
            if let Some(r) = access::get_object(hwnd, wp, lp) {
                return r;
            }
        }
        m if m != 0 && m == ui(|u| u.taskbar_created) => {
            // Explorer restarted: put the tray icon back.
            add_tray_icon(hwnd);
            return LRESULT(0);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            render(hwnd);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_SIZE => {
            resize_surface(hwnd);
            return LRESULT(0);
        }
        WM_DPICHANGED => {
            let r = unsafe { &*(lp.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(hwnd, None, r.left, r.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            SURFACES.with(|s| s.borrow_mut().remove(&(hwnd.0 as isize)));
            fit_window(hwnd);
            invalidate(hwnd);
            return LRESULT(0);
        }
        WM_NCHITTEST => return LRESULT(HTCLIENT as isize),
        WM_NCCALCSIZE if wp.0 != 0 => return LRESULT(0),
        WM_WINDOWPOSCHANGING => {
            keep_size(hwnd, lp);
            return LRESULT(0);
        }
        WM_ACTIVATE => {
            if (wp.0 & 0xffff) as u32 != WA_INACTIVE {
                focus_default();
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            hide_overlay();
            return LRESULT(0);
        }
        WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED | WM_SYSCOLORCHANGE => {
            let dark = system::is_dark();
            let accent = system::accent_hex();
            with_widget(|w| w.system_appearance(dark, accent));
            ONBOARD.with(|o| {
                if let Some(o) = o.borrow_mut().as_mut() {
                    o.dark = dark;
                }
            });
            if let Some(ob) = ONBOARD_HWND.with(|o| o.get()) {
                apply_window_theme(ob);
                invalidate(ob);
            }
            refresh_tray_icon(hwnd);
            after_event();
        }
        WM_TIMER => {
            match wp.0 {
                TIMER_FRAME => frame(),
                TIMER_WAKE => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER_WAKE);
                    }
                    frame();
                }
                TIMER_CARET => {
                    ui(|u| u.caret_on = !u.caret_on);
                    invalidate(hwnd);
                }
                _ => {}
            }
            return LRESULT(0);
        }
        WM_COMMAND => {
            let code = ((wp.0 >> 16) & 0xffff) as u32;
            let from = HWND(lp.0 as *mut _);
            if let Some(id) = handles().and_then(|h| h.field_of(from)) {
                match code {
                    EN_CHANGE => report_field(id),
                    EN_KILLFOCUS => {
                        with_widget(|w| w.field_blurred(id));
                        after_event();
                    }
                    _ => {}
                }
            }
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            if lo_word(lp.0) as u32 == HTCLIENT {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                let s = scale(hwnd);
                let p = Point { x: pt.x as f32 / s, y: pt.y as f32 / s };
                let id = if field_at(p).is_some() {
                    IDC_IBEAM
                } else {
                    match with_widget_ro(|w| w.cursor()).unwrap_or(Cursor::Arrow) {
                        Cursor::Arrow => IDC_ARROW,
                        Cursor::Pointer => IDC_HAND,
                        Cursor::Grab => IDC_SIZEALL,
                        Cursor::Crosshair => IDC_CROSS,
                    }
                };
                unsafe {
                    SetCursor(LoadCursorW(None, id).ok());
                }
                return LRESULT(1);
            }
        }
        WM_LBUTTONDOWN => {
            let p = client_point(hwnd, lp);
            let clicks = click_count(lp);
            unsafe { SetCapture(hwnd) };
            if let (Some(spec), Some(h)) = (field_at(p), handles()) {
                let edit = h.edit(spec.id);
                unsafe {
                    if GetFocus() != edit {
                        let _ = SetFocus(Some(edit));
                    }
                }
                let idx = field_index_at(&spec, edit, p.x);
                let (a, b) = edit_sel(edit);
                let anchor = if down(VK_SHIFT) { if idx < b { b.max(a) } else { a.min(b) } } else { idx };
                match clicks {
                    2 => {
                        let (wa, wb) = word_bounds(&edit_text(edit), idx);
                        set_sel(edit, wa, wb);
                    }
                    n if n >= 3 => set_sel(edit, 0, u32::MAX),
                    _ => set_sel(edit, anchor, idx),
                }
                ui(|u| u.field_drag = if clicks == 1 { Some((spec.id, anchor)) } else { None });
                report_field(spec.id);
                return LRESULT(0);
            }
            with_widget(|w| w.mouse_down(p, clicks));
            after_event();
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            if !ui(|u| std::mem::replace(&mut u.mouse_tracked, true)) {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
            }
            if let Some((dx, dy)) = ui(|u| u.window_drag) {
                let mut cur = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut cur);
                    let _ = SetWindowPos(hwnd, None, cur.x - dx, cur.y - dy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
                return LRESULT(0);
            }
            let p = client_point(hwnd, lp);
            if let Some((id, anchor)) = ui(|u| u.field_drag) {
                if let (Some(h), Some(spec)) = (handles(), with_widget_ro(|w| w.fields()).and_then(|f| f.into_iter().find(|s| s.id == id))) {
                    let edit = h.edit(id);
                    let idx = field_index_at(&spec, edit, p.x);
                    set_sel(edit, anchor, idx);
                    reset_caret();
                    invalidate(hwnd);
                }
                return LRESULT(0);
            }
            let dragging = wp.0 & 0x0001 != 0 && unsafe { GetCapture() } == hwnd;
            if dragging {
                with_widget(|w| w.mouse_dragged(p));
            } else {
                with_widget(|w| w.mouse_moved(p));
            }
            after_event();
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            if ui(|u| u.window_drag.take()).is_some() {
                save_position();
                return LRESULT(0);
            }
            if let Some((id, _)) = ui(|u| u.field_drag.take()) {
                report_field(id);
                return LRESULT(0);
            }
            let p = client_point(hwnd, lp);
            let clicks = ui(|u| u.last_click.3);
            with_widget(|w| w.mouse_up(p, clicks));
            after_event();
            return LRESULT(0);
        }
        WM_CAPTURECHANGED => {
            ui(|u| {
                u.window_drag = None;
                u.field_drag = None;
            });
        }
        WM_MOUSELEAVE => {
            ui(|u| u.mouse_tracked = false);
            with_widget(|w| w.mouse_exited());
            after_event();
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let mut pt = POINT { x: lo_word(lp.0), y: hi_word(lp.0) };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            let s = scale(hwnd);
            let p = Point { x: pt.x as f32 / s, y: pt.y as f32 / s };
            let delta = hi_word(wp.0 as isize) as f32;
            with_widget(|w| w.scroll(p, -delta / 120.0 * 40.0));
            after_event();
            return LRESULT(0);
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if key_down(wp.0 as u16) || msg == WM_KEYDOWN {
                return LRESULT(0);
            }
        }
        WM_KEYUP | WM_SYSKEYUP => key_up(wp.0 as u16),
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

// ── Rendering ────────────────────────────────────────────────────────────

fn client_px(hwnd: HWND) -> (u32, u32) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32)
}

fn resize_surface(hwnd: HWND) {
    let (w, h) = client_px(hwnd);
    SURFACES.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(surface) = s.get_mut(&(hwnd.0 as isize)) {
            if surface.resize(w, h).is_err() {
                s.remove(&(hwnd.0 as isize));
            }
        }
    });
    invalidate(hwnd);
}

fn render(hwnd: HWND) {
    let key = hwnd.0 as isize;
    let onboarding = is_onboarding(hwnd);
    // Gather what the fields need before borrowing the surface.
    let fields: Vec<(FieldSpec, HWND, bool)> = if onboarding {
        Vec::new()
    } else {
        let focus = unsafe { GetFocus() };
        let h = handles();
        with_widget_ro(|w| w.fields())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| {
                let edit = h?.edit(s.id);
                Some((s, edit, focus == edit))
            })
            .collect()
    };
    place_edits(hwnd, &fields);
    let dark = if onboarding { ONBOARD.with(|o| o.borrow().as_ref().is_some_and(|o| o.dark)) } else { with_widget_ro(|w| w.is_dark()).unwrap_or(false) };
    let dyslexia = !onboarding && with_widget_ro(|w| w.settings.dyslexia_font).unwrap_or(false);
    let backdrop = ui(|u| u.backdrop);
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
    let (pw, ph) = client_px(hwnd);
    let t = now();

    SURFACES.with(|surfaces| {
        let mut surfaces = surfaces.borrow_mut();
        if !surfaces.contains_key(&key) {
            match Surface::new(hwnd, pw, ph) {
                Ok(s) => {
                    surfaces.insert(key, s);
                }
                Err(e) => {
                    eprintln!("surface: {e}");
                    return;
                }
            }
        }
        let surface = surfaces.get_mut(&key).unwrap();
        if surface.resize(pw, ph).is_err() || surface.begin(dpi).is_err() {
            surfaces.remove(&key);
            surface::reset_device();
            return;
        }
        let base: ID2D1RenderTarget = match surface.dc.cast() {
            Ok(b) => b,
            Err(_) => return,
        };
        unsafe {
            base.SetTransform(&painter::identity());
            base.Clear(Some(&painter::color(Color::BLACK.with_alpha(0.0))));
        }
        if let Ok(mut p) = D2dPainter::new(&base, dyslexia, Some(&mut surface.shadows)) {
            if !backdrop {
                // No system acrylic (Windows 10): an opaque Mica-like base.
                let c = if dark { Color::hex("#202020") } else { Color::hex("#f3f3f3") };
                p.fill(Rect::new(0.0, 0.0, 10_000.0, 10_000.0), 0.0, c);
            }
            if onboarding {
                ONBOARD.with(|o| {
                    if let Some(o) = o.borrow_mut().as_mut() {
                        o.paint(&mut p);
                    }
                });
            } else {
                with_widget(|w| w.paint(&mut p, t));
                for (spec, edit, focused) in &fields {
                    paint_field(&mut p, spec, *edit, dyslexia, *focused);
                }
            }
        }
        if let Err(e) = surface.end() {
            eprintln!("present: {e}");
            surfaces.clear();
            surface::reset_device();
            invalidate(hwnd);
        }
    });
    if !onboarding {
        access::refresh(hwnd);
    }
}

/// Keeps each EDIT control under the field it backs, so screen readers find
/// its text and caret where the field is drawn. The canvas is composed above
/// child windows, and mouse input passes through to the canvas.
fn place_edits(hwnd: HWND, fields: &[(FieldSpec, HWND, bool)]) {
    let s = scale(hwnd);
    for (spec, edit, _) in fields {
        let r = field_text_rect(spec);
        let (x, y, w, h) = if spec.visible {
            ((r.x * s).round() as i32, (r.y * s).round() as i32, (r.w * s).round() as i32, (r.h * s).round() as i32)
        } else {
            (-4000, -4000, 600, 40)
        };
        let mut cur = RECT::default();
        unsafe {
            let _ = GetWindowRect(*edit, &mut cur);
            let mut pt = [POINT { x: cur.left, y: cur.top }];
            MapWindowPoints(None, Some(hwnd), &mut pt);
            if (pt[0].x, pt[0].y, cur.right - cur.left, cur.bottom - cur.top) != (x, y, w, h) {
                let _ = SetWindowPos(*edit, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW);
            }
        }
    }
}

// ── Timers ───────────────────────────────────────────────────────────────

/// Keeps a ~60 Hz timer while something animates; otherwise sleeps until
/// the widget's next timer.
fn schedule() {
    let Some(h) = handles() else { return };
    let t = now();
    with_widget(|w| w.tick(t));
    let (animating, wake) = with_widget_ro(|w| (w.animating(), w.next_wakeup())).unwrap_or((false, None));
    let onboard_anim = ONBOARD.with(|o| o.borrow().as_ref().is_some_and(|o| o.animating()));
    let want_frame = animating || onboard_anim;
    let had_frame = ui(|u| std::mem::replace(&mut u.frame_timer, want_frame));
    unsafe {
        if want_frame && !had_frame {
            SetTimer(Some(h.overlay), TIMER_FRAME, 15, None);
        } else if !want_frame && had_frame {
            let _ = KillTimer(Some(h.overlay), TIMER_FRAME);
        }
        let _ = KillTimer(Some(h.overlay), TIMER_WAKE);
        if let Some(secs) = wake {
            SetTimer(Some(h.overlay), TIMER_WAKE, ((secs.max(0.01)) * 1000.0).ceil() as u32, None);
        }
    }
}

fn frame() {
    let t = now();
    with_widget(|w| w.tick(t));
    let cmds = with_widget(|w| w.take_commands()).unwrap_or_default();
    for c in cmds {
        run_command(c);
    }
    onboarding_tick();
    if let Some(h) = handles() {
        invalidate(h.overlay);
    }
    let animating = with_widget_ro(|w| w.animating()).unwrap_or(false);
    let onboard_anim = ONBOARD.with(|o| o.borrow().as_ref().is_some_and(|o| o.animating()));
    if !animating && !onboard_anim {
        schedule();
    }
}

// ── Onboarding ───────────────────────────────────────────────────────────

fn show_onboarding() {
    let hwnd = make_window(onboarding::W, onboarding::H, w!("Welcome to Franconvert"));
    ONBOARD_HWND.with(|o| o.set(Some(hwnd)));
    let dark = system::is_dark();
    ONBOARD.with(|o| *o.borrow_mut() = Some(Onboarding::new(dark, false, &system::accent_hex(), now())));
    apply_window_theme(hwnd);
    unsafe {
        let wa = work_area(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY));
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let _ = SetWindowPos(hwnd, None, wa.left + (wa.right - wa.left - w) / 2, wa.top + (wa.bottom - wa.top - h) / 3, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
}

fn onboarding_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            render(hwnd);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
        }
        WM_ERASEBKGND => return Some(LRESULT(1)),
        WM_SIZE => resize_surface(hwnd),
        WM_DPICHANGED => {
            let r = unsafe { &*(lp.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            SURFACES.with(|s| s.borrow_mut().remove(&(hwnd.0 as isize)));
        }
        WM_NCHITTEST => return Some(LRESULT(HTCLIENT as isize)),
        WM_NCCALCSIZE if wp.0 != 0 => return Some(LRESULT(0)),
        WM_WINDOWPOSCHANGING => keep_size(hwnd, lp),
        WM_CLOSE => {
            ONBOARD.with(|o| {
                if let Some(o) = o.borrow_mut().as_mut() {
                    o.dismissed = true;
                }
            });
            onboarding_tick();
        }
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let vk = wp.0 as u16;
            ONBOARD.with(|o| {
                if let Some(o) = o.borrow_mut().as_mut() {
                    if is_down && vk == VK_ESCAPE.0 {
                        o.dismissed = true;
                    }
                    o.keys(down(VK_CONTROL), down(VK_SHIFT), is_down && vk == VK_A.0);
                }
            });
            onboarding_tick();
            schedule();
        }
        WM_MOUSEMOVE => {
            let p = client_point(hwnd, lp);
            ONBOARD.with(|o| {
                if let Some(o) = o.borrow_mut().as_mut() {
                    o.mouse_moved(p.x, p.y);
                }
            });
            invalidate(hwnd);
        }
        WM_LBUTTONDOWN => {
            let p = client_point(hwnd, lp);
            ONBOARD.with(|o| {
                if let Some(o) = o.borrow_mut().as_mut() {
                    o.click(p.x, p.y);
                }
            });
            onboarding_tick();
            schedule();
        }
        _ => return None,
    }
    Some(LRESULT(0))
}

fn onboarding_tick() {
    let Some(hwnd) = ONBOARD_HWND.with(|o| o.get()) else { return };
    let t = now();
    let done = ONBOARD.with(|o| o.borrow_mut().as_mut().map(|o| o.tick(t))).unwrap_or(false);
    if done {
        let dir = ui(|u| u.data_dir.clone());
        let _ = fs::write(dir.join("onboarding_shown"), "1");
        ONBOARD_HWND.with(|o| o.set(None));
        ONBOARD.with(|o| *o.borrow_mut() = None);
        SURFACES.with(|s| s.borrow_mut().remove(&(hwnd.0 as isize)));
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    } else {
        invalidate(hwnd);
    }
}
