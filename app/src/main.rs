#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arabizi_engine::{TransliterationEngine, UserPreferences};
#[cfg(not(target_os = "macos"))]
use enigo::{Enigo, Key, Keyboard, Settings, Direction};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{
    Emitter,
    Manager,
    PhysicalPosition,
    WindowEvent,
    menu::{Menu, MenuItem},
    tray::TrayIconEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

struct AppState {
    engine: TransliterationEngine,
    prefs: UserPreferences,
    prefs_path: PathBuf,
}

/// Per-word candidates for the frontend to render inline selection
#[derive(Serialize, Clone)]
struct WordResult {
    input: String,
    candidates: Vec<String>,
    emojis: Vec<String>,
    selected: usize,
}

#[derive(Serialize, Clone)]
struct TransliterateResult {
    combined: String,
    words: Vec<WordResult>,
}

// ── Transliteration ───────────────────────────────────────────────────────────

/// Maps an ASCII punctuation character to its Arabic RTL equivalent.
fn to_arabic_punct(c: char) -> char {
    match c {
        '?' => '؟',
        ',' => '،',
        ';' => '؛',
        c   => c,
    }
}

/// Strips leading and trailing ASCII punctuation from a word token,
/// returning `(arabic_prefix, core_word, arabic_suffix)`.
/// The stripped punctuation is mapped to its Arabic equivalent so that
/// e.g. "ya3ni?" splits into ("", "ya3ni", "؟").
fn extract_punctuation(word: &str) -> (String, String, String) {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    let lead_n  = chars.iter().take_while(|&&c| c.is_ascii_punctuation()).count();
    let trail_n = chars.iter().rev()
        .take_while(|&&c| c.is_ascii_punctuation())
        .count()
        .min(n.saturating_sub(lead_n));   // never overlap with leading run
    let prefix: String = chars[..lead_n].iter().map(|&c| to_arabic_punct(c)).collect();
    let core:   String = chars[lead_n..n - trail_n].iter().collect();
    let suffix: String = chars[n - trail_n..].iter().map(|&c| to_arabic_punct(c)).collect();
    (prefix, core, suffix)
}

#[tauri::command]
fn transliterate(state: tauri::State<'_, Mutex<AppState>>, input: String) -> TransliterateResult {
    let state = state.lock().unwrap();
    let input_lower = input.trim().to_lowercase();

    if input_lower.is_empty() {
        return TransliterateResult {
            combined: String::new(),
            words: vec![],
        };
    }

    let parts: Vec<&str> = input_lower.split_whitespace().collect();
    let words: Vec<WordResult> = parts
        .iter()
        .map(|w| {
            // Emoticons are composed of punctuation — check before stripping.
            if let Some(emojis) = state.engine.lookup_emoticon(w) {
                return WordResult {
                    input: w.to_string(),
                    candidates: emojis,
                    emojis: vec![],
                    selected: 0,
                };
            }

            // Strip leading/trailing punctuation and map to Arabic equivalents.
            let (prefix, core, suffix) = extract_punctuation(w);

            // Pure punctuation token (e.g. a standalone "?").
            if core.is_empty() {
                return WordResult {
                    input: w.to_string(),
                    candidates: vec![format!("{}{}", prefix, suffix)],
                    emojis: vec![],
                    selected: 0,
                };
            }

            // Transliterate the bare word, then reattach Arabic punctuation.
            let candidates_raw = state.engine.transliterate_word_ranked(&core, Some(&state.prefs));
            let emojis = state.engine.lookup_emojis(&candidates_raw);
            let candidates = candidates_raw
                .into_iter()
                .map(|c| format!("{}{}{}", prefix, c, suffix))
                .collect();
            WordResult {
                input: w.to_string(),
                candidates,
                emojis,
                selected: 0,
            }
        })
        .collect();

    let combined = words
        .iter()
        .map(|w| w.candidates.first().map(|s| s.as_str()).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ");

    TransliterateResult { combined, words }
}

#[tauri::command]
fn record_selection(state: tauri::State<'_, Mutex<AppState>>, input: String, arabic: String) {
    let mut state = state.lock().unwrap();
    state.prefs.record(&input, &arabic);
    let json = state.prefs.to_json();
    let path = state.prefs_path.clone();
    drop(state);
    let _ = fs::write(path, json);
}

// ── Accent color ──────────────────────────────────────────────────────────────

#[tauri::command]
fn get_accent_color() -> String {
    // Windows: read from DWM registry key (ABGR u32)
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(dwm) = hkcu.open_subkey("SOFTWARE\\Microsoft\\Windows\\DWM") {
            if let Ok(color) = dwm.get_value::<u32, _>("AccentColor") {
                let r = color & 0xFF;
                let g = (color >> 8) & 0xFF;
                let b = (color >> 16) & 0xFF;
                return format!("#{:02x}{:02x}{:02x}", r, g, b);
            }
        }
    }
    // macOS: read NSColor.controlAccentColor, convert to sRGB, return as hex.
    #[cfg(target_os = "macos")]
    {
        use std::ffi::c_void;
        type Id  = *mut c_void;
        type Sel = *const c_void;

        unsafe extern "C" {
            fn objc_getClass(name: *const u8) -> Id;
            fn sel_registerName(name: *const u8) -> Sel;
            #[link_name = "objc_msgSend"] fn msg_id(obj: Id, sel: Sel) -> Id;
            #[link_name = "objc_msgSend"] fn msg_id_id(obj: Id, sel: Sel, a: Id) -> Id;
            #[link_name = "objc_msgSend"] fn msg_f64(obj: Id, sel: Sel) -> f64;
        }

        macro_rules! sel { ($s:literal) => { sel_registerName(concat!($s, "\0").as_ptr()) }; }
        macro_rules! cls { ($s:literal) => { objc_getClass(concat!($s, "\0").as_ptr()) }; }

        unsafe {
            let accent = msg_id(cls!("NSColor"), sel!("controlAccentColor"));
            if accent.is_null() { return "#58A1AC".to_string(); }

            let srgb_space = msg_id(cls!("NSColorSpace"), sel!("sRGBColorSpace"));
            let rgb = msg_id_id(accent, sel!("colorUsingColorSpace:"), srgb_space);
            if rgb.is_null() { return "#58A1AC".to_string(); }

            let r = (msg_f64(rgb, sel!("redComponent"))   * 255.0).round() as u8;
            let g = (msg_f64(rgb, sel!("greenComponent")) * 255.0).round() as u8;
            let b = (msg_f64(rgb, sel!("blueComponent"))  * 255.0).round() as u8;
            return format!("#{:02x}{:02x}{:02x}", r, g, b);
        }
    }
    "#58A1AC".to_string()
}

// ── Backdrop / vibrancy ───────────────────────────────────────────────────────

/// Returns (major, minor) of the running macOS version by parsing `sw_vers`.
#[cfg(target_os = "macos")]
fn macos_version() -> (u32, u32) {
    use std::process::Command;
    let Ok(out) = Command::new("sw_vers").arg("-productVersion").output() else {
        return (0, 0);
    };
    let s = String::from_utf8_lossy(&out.stdout);
    let mut parts = s.trim().splitn(3, '.');
    let major = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0u32);
    let minor = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0u32);
    (major, minor)
}

/// Applies the appropriate macOS window backdrop:
/// - macOS 26+ (Tahoe): FullScreenUI material — theme-adaptive translucency that
///   the OS renders with the Liquid Glass aesthetic on Tahoe+.
/// - macOS ≤ 25: HudWindow (frosted dark glass, Ventura/Sonoma/Sequoia).
/// Also rounds the NSVisualEffectView's CALayer to match the CSS --radius.
#[cfg(target_os = "macos")]
fn apply_macos_backdrop(window: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

    let (major, _) = macos_version();

    if major >= 26 {
        let _ = apply_vibrancy(window, NSVisualEffectMaterial::FullScreenUI, None, None);
    } else {
        let _ = apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, None);
    }

    apply_macos_corner_radius(window, 12.0);
}

/// Rounds the NSVisualEffectView's CALayer so the blur itself is clipped to
/// the same radius as the CSS `--radius` variable.  Uses raw `objc_msgSend`
/// so no extra crate dependency is required.
#[cfg(target_os = "macos")]
fn apply_macos_corner_radius(window: &tauri::WebviewWindow, radius: f64) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;

    type Id = *mut c_void;
    type Sel = *const c_void;

    // Three typed wrappers over the same symbol so the compiler generates the
    // right calling-convention for each argument list.
    unsafe extern "C" {
        #[link_name = "objc_msgSend"]
        fn msg_id(obj: Id, sel: Sel) -> Id;
        #[link_name = "objc_msgSend"]
        fn msg_void_bool(obj: Id, sel: Sel, arg: u8);
        #[link_name = "objc_msgSend"]
        fn msg_void_f64(obj: Id, sel: Sel, arg: f64);
        fn sel_registerName(name: *const u8) -> Sel;
    }

    macro_rules! sel {
        ($s:literal) => {
            sel_registerName(concat!($s, "\0").as_ptr())
        };
    }

    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    let ns_view = h.ns_view.as_ptr();

    unsafe {
        // NSView → NSWindow → contentView (NSVisualEffectView) → CALayer
        let ns_window = msg_id(ns_view, sel!("window"));
        if ns_window.is_null() { return; }
        let content_view = msg_id(ns_window, sel!("contentView"));
        if content_view.is_null() { return; }
        msg_void_bool(content_view, sel!("setWantsLayer:"), 1);
        let layer = msg_id(content_view, sel!("layer"));
        if layer.is_null() { return; }
        msg_void_f64(layer, sel!("setCornerRadius:"), radius);
        msg_void_bool(layer, sel!("setMasksToBounds:"), 1);
    }
}

#[tauri::command]
fn apply_theme(app: tauri::AppHandle, dark: bool) {
    // Windows: re-apply acrylic with the matching tint color
    #[cfg(target_os = "windows")]
    {
        use window_vibrancy::apply_acrylic;
        if let Some(window) = app.get_webview_window("overlay") {
            let color = if dark { (20u8, 20u8, 20u8, 238u8) } else { (245u8, 245u8, 245u8, 238u8) };
            let _ = apply_acrylic(&window, Some(color));
        }
    }
    // macOS: vibrancy adapts automatically to dark/light — no action needed.
    // The JS layer still drives the CSS theme variables for text/icon colours.
    #[cfg(target_os = "macos")]
    let _ = (app, dark); // suppress unused-variable warnings
}

// ── Global shortcut helpers ───────────────────────────────────────────────────

fn parse_shortcut_str(s: &str) -> Result<Shortcut, String> {
    let mut modifiers = Modifiers::empty();
    let mut code: Option<Code> = None;
    for part in s.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift"            => modifiers |= Modifiers::SHIFT,
            "alt"              => modifiers |= Modifiers::ALT,
            "meta" | "super" | "win" | "cmd" => modifiers |= Modifiers::SUPER,
            key => { code = Some(str_to_code(key).ok_or_else(|| format!("Unknown key: {key}"))?); }
        }
    }
    let code = code.ok_or_else(|| "No key code specified".to_string())?;
    Ok(Shortcut::new(if modifiers.is_empty() { None } else { Some(modifiers) }, code))
}

fn str_to_code(s: &str) -> Option<Code> {
    match s {
        "a" => Some(Code::KeyA), "b" => Some(Code::KeyB), "c" => Some(Code::KeyC),
        "d" => Some(Code::KeyD), "e" => Some(Code::KeyE), "f" => Some(Code::KeyF),
        "g" => Some(Code::KeyG), "h" => Some(Code::KeyH), "i" => Some(Code::KeyI),
        "j" => Some(Code::KeyJ), "k" => Some(Code::KeyK), "l" => Some(Code::KeyL),
        "m" => Some(Code::KeyM), "n" => Some(Code::KeyN), "o" => Some(Code::KeyO),
        "p" => Some(Code::KeyP), "q" => Some(Code::KeyQ), "r" => Some(Code::KeyR),
        "s" => Some(Code::KeyS), "t" => Some(Code::KeyT), "u" => Some(Code::KeyU),
        "v" => Some(Code::KeyV), "w" => Some(Code::KeyW), "x" => Some(Code::KeyX),
        "y" => Some(Code::KeyY), "z" => Some(Code::KeyZ),
        "0" => Some(Code::Digit0), "1" => Some(Code::Digit1), "2" => Some(Code::Digit2),
        "3" => Some(Code::Digit3), "4" => Some(Code::Digit4), "5" => Some(Code::Digit5),
        "6" => Some(Code::Digit6), "7" => Some(Code::Digit7), "8" => Some(Code::Digit8),
        "9" => Some(Code::Digit9),
        "space" => Some(Code::Space),
        "f1"  => Some(Code::F1),  "f2"  => Some(Code::F2),  "f3"  => Some(Code::F3),
        "f4"  => Some(Code::F4),  "f5"  => Some(Code::F5),  "f6"  => Some(Code::F6),
        "f7"  => Some(Code::F7),  "f8"  => Some(Code::F8),  "f9"  => Some(Code::F9),
        "f10" => Some(Code::F10), "f11" => Some(Code::F11), "f12" => Some(Code::F12),
        _ => None,
    }
}

#[tauri::command]
fn update_shortcut(app: tauri::AppHandle, shortcut_str: String) -> Result<(), String> {
    let shortcut = parse_shortcut_str(&shortcut_str)?;
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())?;
    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_overlay(&app_handle);
            }
        })
        .map_err(|e| e.to_string())
}

// ── Autostart ─────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_autostart() -> bool {
    // Windows: check HKCU Run registry key
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let exe = std::env::current_exe().unwrap_or_default();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run") {
            if let Ok(val) = run_key.get_value::<String, _>("Franconvert") {
                return val.to_lowercase() == exe.to_string_lossy().to_lowercase();
            }
        }
    }

    // macOS: check for a LaunchAgent plist at ~/Library/LaunchAgents/com.franconvert.app.plist
    #[cfg(target_os = "macos")]
    {
        if let Some(plist_path) = launchagent_path() {
            return plist_path.exists();
        }
    }

    false
}

#[tauri::command]
fn set_autostart(enabled: bool) -> bool {
    // Windows: write/delete HKCU Run registry value
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
        use winreg::RegKey;
        let exe = std::env::current_exe().unwrap_or_default();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey_with_flags(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
            KEY_ALL_ACCESS,
        ) {
            if enabled {
                let _ = run_key.set_value("Franconvert", &exe.to_string_lossy().into_owned());
            } else {
                let _ = run_key.delete_value("Franconvert");
            }
            return true;
        }
    }

    // macOS: write/delete a LaunchAgent plist
    #[cfg(target_os = "macos")]
    {
        if let Some(plist_path) = launchagent_path() {
            if enabled {
                let exe = std::env::current_exe().unwrap_or_default();
                let exe_str = exe.to_string_lossy();
                let plist = format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.franconvert.app</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_str}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>"#
                );
                if let Some(parent) = plist_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                return fs::write(&plist_path, plist).is_ok();
            } else {
                return fs::remove_file(&plist_path).is_ok() || !plist_path.exists();
            }
        }
    }

    false
}

/// Returns the path to the LaunchAgent plist for this app on macOS.
#[cfg(target_os = "macos")]
fn launchagent_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join("Library/LaunchAgents/com.franconvert.app.plist"))
}

// ── macOS frontmost-app tracking ──────────────────────────────────────────────
//
// As an Accessory app, calling `window.show() + set_focus()` makes us the
// active app, which means `[NSApp hide:]` alone does not reliably restore
// focus to the *specific* app the user was typing into. Instead we capture
// the frontmost app's PID *before* we steal focus, then explicitly
// reactivate it before sending the paste keystroke.

#[cfg(target_os = "macos")]
static PREV_APP_PID: AtomicI32 = AtomicI32::new(0);

#[cfg(target_os = "macos")]
mod mac_focus {
    use std::ffi::c_void;

    type Id = *mut c_void;
    type Sel = *const c_void;

    unsafe extern "C" {
        fn objc_getClass(name: *const u8) -> Id;
        fn sel_registerName(name: *const u8) -> Sel;
        #[link_name = "objc_msgSend"]
        fn msg_id(obj: Id, sel: Sel) -> Id;
        #[link_name = "objc_msgSend"]
        fn msg_id_i32(obj: Id, sel: Sel, a: i32) -> Id;
        #[link_name = "objc_msgSend"]
        fn msg_bool_u64(obj: Id, sel: Sel, a: u64) -> u8;
        #[link_name = "objc_msgSend"]
        fn msg_i32(obj: Id, sel: Sel) -> i32;
    }

    macro_rules! sel { ($s:literal) => { unsafe { sel_registerName(concat!($s, "\0").as_ptr()) } }; }
    macro_rules! cls { ($s:literal) => { unsafe { objc_getClass(concat!($s, "\0").as_ptr()) } }; }

    /// Returns the PID of the currently frontmost application, or 0.
    pub fn frontmost_pid() -> i32 {
        unsafe {
            let workspace = msg_id(cls!("NSWorkspace"), sel!("sharedWorkspace"));
            if workspace.is_null() { return 0; }
            let app = msg_id(workspace, sel!("frontmostApplication"));
            if app.is_null() { return 0; }
            msg_i32(app, sel!("processIdentifier"))
        }
    }

    /// Activate the application with the given PID, ignoring other apps.
    /// Returns true if activation was attempted successfully.
    pub fn activate_pid(pid: i32) -> bool {
        if pid <= 0 { return false; }
        unsafe {
            let app_class = cls!("NSRunningApplication");
            let app = msg_id_i32(app_class, sel!("runningApplicationWithProcessIdentifier:"), pid);
            if app.is_null() { return false; }
            // NSApplicationActivateIgnoringOtherApps = 1 << 1 = 2
            let result = msg_bool_u64(app, sel!("activateWithOptions:"), 2);
            result != 0
        }
    }
}

/// Remember which app was frontmost so we can return focus to it after paste.
/// Call this *before* showing the overlay window.
fn capture_prev_focus() {
    #[cfg(target_os = "macos")]
    {
        let pid = mac_focus::frontmost_pid();
        // Don't overwrite with our own PID if we somehow re-enter this path
        // while already active — that would lose the real previous app.
        if pid > 0 && pid != std::process::id() as i32 {
            PREV_APP_PID.store(pid, Ordering::SeqCst);
        }
    }
}

// ── Clipboard paste ───────────────────────────────────────────────────────────

#[tauri::command]
fn paste_from_clipboard() {
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(200));
        send_paste_keystroke();
    });
}

#[cfg(not(target_os = "macos"))]
fn send_paste_keystroke() {
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.key(Key::Control, Direction::Press);
        let _ = enigo.key(Key::Unicode('v'), Direction::Click);
        let _ = enigo.key(Key::Control, Direction::Release);
    }
}

/// macOS paste, posted via Core Graphics with the raw virtual keycode for V.
///
/// We bypass enigo's `Key::Unicode('v')` path because on macOS 26+ it calls
/// `TSMGetInputSourceProperty`, which asserts it must run on the main thread
/// (`dispatch_assert_queue`) and otherwise traps the process — silently
/// killing it with no Rust panic. `CGEventPost` is thread-safe and does not
/// touch TextInputSources, so this is safe to call from any thread.
#[cfg(target_os = "macos")]
fn send_paste_keystroke() {
    use std::ffi::c_void;

    type CGEventRef = *mut c_void;
    type CGEventSourceRef = *mut c_void;

    const KCGEVENT_SOURCE_STATE_HID_SYSTEM: i32 = 1;
    const KCG_HID_EVENT_TAP: u32 = 0;
    const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;
    const KVK_ANSI_V: u16 = 0x09;

    unsafe extern "C" {
        fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: u16,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventPost(tap: u32, event: CGEventRef);
        fn CFRelease(obj: *const c_void);
    }

    unsafe {
        let source = CGEventSourceCreate(KCGEVENT_SOURCE_STATE_HID_SYSTEM);
        // A null source is allowed and just means "synthesize from scratch".

        let key_down = CGEventCreateKeyboardEvent(source, KVK_ANSI_V, true);
        let key_up = CGEventCreateKeyboardEvent(source, KVK_ANSI_V, false);
        if key_down.is_null() || key_up.is_null() {
            if !key_down.is_null() { CFRelease(key_down); }
            if !key_up.is_null() { CFRelease(key_up); }
            if !source.is_null() { CFRelease(source); }
            return;
        }

        CGEventSetFlags(key_down, KCG_EVENT_FLAG_MASK_COMMAND);
        CGEventSetFlags(key_up, KCG_EVENT_FLAG_MASK_COMMAND);

        CGEventPost(KCG_HID_EVENT_TAP, key_down);
        CGEventPost(KCG_HID_EVENT_TAP, key_up);

        CFRelease(key_down);
        CFRelease(key_up);
        if !source.is_null() { CFRelease(source); }
    }
}

/// Hide the overlay, return focus to the app that was frontmost before we
/// opened, and send Cmd+V (or Ctrl+V) so the clipboard contents land in that
/// app's text field.
///
/// On macOS, the Accessory activation policy means `window.hide()` alone does
/// not deactivate us, and even `[NSApp hide:]` does not reliably re-focus the
/// *specific* app the user was typing into. So we explicitly reactivate the
/// PID captured in `capture_prev_focus()` and then synthesize the paste.
#[tauri::command]
fn hide_and_paste(app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let prev_pid = PREV_APP_PID.swap(0, Ordering::SeqCst);

    // AppKit (NSWindow ops, NSRunningApplication.activateWithOptions) must
    // run on the main thread. Tauri command handlers run on a worker thread,
    // so we hop to the main thread before touching any of it.
    //
    // Note: we deliberately do NOT call `app.hide()` (NSApp.hide:) here —
    // when combined with window.hide() in the same tick it has caused
    // crashes in Tauri 2. Activating the previous app via
    // NSRunningApplication is sufficient to swap focus on macOS.
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_for_main.get_webview_window("overlay") {
            let _ = window.hide();
        }
        #[cfg(target_os = "macos")]
        {
            mac_focus::activate_pid(prev_pid);
        }
    });

    // The keystroke synthesis itself uses CGEventPost (via enigo), which is
    // thread-safe, so we sleep + send from a background thread to avoid
    // blocking the main thread / IPC reply.
    thread::spawn(move || {
        // Give AppKit time to actually make the target app key/active before
        // we synthesize the paste.
        #[cfg(target_os = "macos")]
        thread::sleep(Duration::from_millis(160));
        #[cfg(not(target_os = "macos"))]
        thread::sleep(Duration::from_millis(80));

        send_paste_keystroke();
        let _ = app;
    });
}

// ── Window position persistence ───────────────────────────────────────────────

/// Read a saved overlay position from disk, returning (x, y) physical pixels.
fn load_window_position(path: &PathBuf) -> Option<(i32, i32)> {
    let data = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    let x = v.get("x")?.as_i64()? as i32;
    let y = v.get("y")?.as_i64()? as i32;
    Some((x, y))
}

/// Returns true if the position lies inside any connected monitor's bounds.
/// Prevents restoring the overlay onto a now-disconnected display.
fn position_visible(window: &tauri::WebviewWindow, x: i32, y: i32) -> bool {
    let Ok(monitors) = window.available_monitors() else { return false };
    monitors.iter().any(|m| {
        let pos = m.position();
        let size = m.size();
        x >= pos.x
            && y >= pos.y
            && x < pos.x + size.width as i32
            && y < pos.y + size.height as i32
    })
}

// ── Onboarding ────────────────────────────────────────────────────────────────

/// Called from the onboarding window when the user dismisses it.
/// Writes a flag file so the window never shows again, then closes the window.
#[tauri::command]
fn mark_onboarding_shown(app: tauri::AppHandle) {
    if let Ok(app_data) = app.path().app_data_dir() {
        let _ = fs::write(app_data.join("onboarding_shown"), "1");
    }
    if let Some(window) = app.get_webview_window("onboarding") {
        let _ = window.close();
    }
}

// ── Tray icon (Windows — macOS uses iconAsTemplate, OS handles light/dark) ────

/// Returns true if Windows is currently using a light system theme.
#[cfg(target_os = "windows")]
fn is_light_theme() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
    ) {
        if let Ok(val) = key.get_value::<u32, _>("SystemUsesLightTheme") {
            return val == 1;
        }
    }
    false
}

/// Swaps the tray icon between white (dark-mode) and black (light-mode) variants.
/// Only needed on Windows; macOS iconAsTemplate handles it automatically.
#[cfg(target_os = "windows")]
fn set_tray_icon(app: &tauri::AppHandle, light: bool) {
    use image::ImageReader;
    use std::io::Cursor;
    use tauri::image::Image;

    let bytes: &[u8] = if light {
        include_bytes!("../icons/tray-icon-black.png")
    } else {
        include_bytes!("../icons/tray-icon.png")
    };

    let Ok(img) = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .and_then(|r| r.decode().map_err(std::io::Error::other))
    else {
        return;
    };

    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    let icon = Image::new_owned(rgba.into_raw(), w, h);

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(icon));
    }
}

// ── Windows helpers ───────────────────────────────────────────────────────────

/// Asks DWM to round the window corners (Windows 11+).
/// Silently does nothing on Windows 10 where the attribute is unsupported.
/// Uses GetAncestor(GA_ROOT) to ensure we target the top-level HWND —
/// Tauri can expose the WebView2 child HWND which DWM ignores for this attribute.
#[cfg(target_os = "windows")]
fn apply_rounded_corners(window: &tauri::WebviewWindow) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            dw_attribute: u32,
            pv_attribute: *const c_void,
            cb_attribute: u32,
        ) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAncestor(hwnd: isize, ga_flags: u32) -> isize;
    }

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;
    const GA_ROOT: u32 = 2;

    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    let hwnd = h.hwnd.get();

    // Walk up to the top-level window; fall back to the original handle if
    // GetAncestor returns NULL (already a root, or error).
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let target = if root != 0 { root } else { hwnd };

    unsafe {
        DwmSetWindowAttribute(
            target,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &DWMWCP_ROUND as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(Mutex::new(AppState {
            engine: TransliterationEngine::new(),
            prefs: UserPreferences::new(),
            prefs_path: PathBuf::new(),
        }))
        .setup(|app| {
            // Hide dock icon — run as a tray-only agent like Raycast
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Load user preferences
            let app_data = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
            let _ = fs::create_dir_all(&app_data);
            let prefs_path = app_data.join("user_prefs.json");
            let prefs = if prefs_path.exists() {
                let data = fs::read_to_string(&prefs_path).unwrap_or_default();
                UserPreferences::from_json(&data)
            } else {
                UserPreferences::new()
            };
            {
                let state_mutex = app.state::<Mutex<AppState>>();
                let mut state = state_mutex.lock().unwrap();
                state.prefs = prefs;
                state.prefs_path = prefs_path;
            }

            // Apply platform backdrop
            #[cfg(target_os = "windows")]
            {
                use window_vibrancy::apply_acrylic;
                if let Some(window) = app.get_webview_window("overlay") {
                    let _ = apply_acrylic(&window, Some((20u8, 20u8, 20u8, 238u8)));
                    apply_rounded_corners(&window);
                }
            }
            #[cfg(target_os = "macos")]
            {
                if let Some(window) = app.get_webview_window("overlay") {
                    apply_macos_backdrop(&window);
                }
            }

            // Restore the user's last overlay position (and persist any future
            // moves). Falls back to the centered position from tauri.conf.json
            // if no saved position exists or it lies off-screen.
            let window_pos_path = app_data.join("window_pos.json");
            if let Some(window) = app.get_webview_window("overlay") {
                if let Some((x, y)) = load_window_position(&window_pos_path) {
                    if position_visible(&window, x, y) {
                        let _ = window.set_position(PhysicalPosition::new(x, y));
                    }
                }
                let save_path = window_pos_path.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Moved(p) = event {
                        let json = format!("{{\"x\":{},\"y\":{}}}", p.x, p.y);
                        let _ = fs::write(&save_path, json);
                    }
                });
            }

            // Show onboarding on first launch
            let onboarding_flag = app_data.join("onboarding_shown");
            if !onboarding_flag.exists() {
                let ob = tauri::WebviewWindowBuilder::new(
                    app,
                    "onboarding",
                    tauri::WebviewUrl::App("onboarding.html".into()),
                )
                .title("Welcome to Franconvert")
                .inner_size(480.0, 380.0)
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .center()
                .skip_taskbar(true)
                .shadow(true)
                .transparent(true)
                .build()?;

                #[cfg(target_os = "windows")]
                {
                    use window_vibrancy::apply_acrylic;
                    let tint = if is_light_theme() {
                        (245u8, 245u8, 245u8, 238u8)
                    } else {
                        (20u8, 20u8, 20u8, 238u8)
                    };
                    let _ = apply_acrylic(&ob, Some(tint));
                    apply_rounded_corners(&ob);
                }
                #[cfg(target_os = "macos")]
                apply_macos_backdrop(&ob);
            }

            // Build tray menu
            let show = MenuItem::with_id(app, "show", "Show Franconvert", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            if let Some(tray) = app.tray_by_id("main") {
                tray.set_menu(Some(menu))?;
                tray.on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => toggle_overlay(app),
                    "quit" => app.exit(0),
                    _ => {}
                });
                tray.on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button, button_state, .. } = event {
                        if button == tauri::tray::MouseButton::Left
                            && button_state == tauri::tray::MouseButtonState::Up
                        {
                            toggle_overlay(tray.app_handle());
                        }
                    }
                });
            }

            // Windows: set initial tray icon for current theme, poll for changes.
            // macOS: iconAsTemplate=true means the OS handles light/dark automatically.
            #[cfg(target_os = "windows")]
            {
                let initial_light = is_light_theme();
                set_tray_icon(app.handle(), initial_light);

                let app_handle = app.handle().clone();
                thread::spawn(move || {
                    let mut last_light = initial_light;
                    loop {
                        thread::sleep(Duration::from_secs(2));
                        let current_light = is_light_theme();
                        if current_light != last_light {
                            last_light = current_light;
                            set_tray_icon(&app_handle, current_light);
                        }
                    }
                });
            }

            // Register global shortcut
            #[cfg(target_os = "macos")]
            let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyA);
            #[cfg(not(target_os = "macos"))]
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyA);
            let app_handle = app.handle().clone();
            app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    toggle_overlay(&app_handle);
                }
            })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            transliterate,
            paste_from_clipboard,
            hide_and_paste,
            record_selection,
            apply_theme,
            get_accent_color,
            update_shortcut,
            get_autostart,
            set_autostart,
            mark_onboarding_shown,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn toggle_overlay(app: &tauri::AppHandle) {
    // If the onboarding window is still open, signal it that the shortcut was used
    // so it can play its success animation and close itself.
    if let Some(ob) = app.get_webview_window("onboarding") {
        if ob.is_visible().unwrap_or(false) {
            let _ = ob.emit("shortcut-used", ());
            // Guarantee the onboarding window dismisses even if its JS
            // setTimeout is throttled while the window is backgrounded
            // behind the overlay (common on macOS/Windows).
            let app_handle = app.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(1000));
                if let Ok(app_data) = app_handle.path().app_data_dir() {
                    let _ = fs::write(app_data.join("onboarding_shown"), "1");
                }
                if let Some(w) = app_handle.get_webview_window("onboarding") {
                    let _ = w.close();
                }
            });
        }
    }

    if let Some(window) = app.get_webview_window("overlay") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            // Snapshot whoever's frontmost *before* we steal focus, so we can
            // hand focus back to them when the user commits a paste.
            capture_prev_focus();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit("focus-input", ());
        }
    }
}
