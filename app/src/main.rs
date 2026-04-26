#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arabizi_engine::{TransliterationEngine, UserPreferences};
use enigo::{Enigo, Key, Keyboard, Settings, Direction};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{
    Emitter,
    Manager,
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
            // Emoticons bypass Arabic transliteration entirely — return emojis as candidates
            if let Some(emojis) = state.engine.lookup_emoticon(w) {
                return WordResult {
                    input: w.to_string(),
                    candidates: emojis,
                    emojis: vec![],
                    selected: 0,
                };
            }
            let candidates = state.engine.transliterate_word_ranked(w, Some(&state.prefs));
            let emojis = state.engine.lookup_emojis(&candidates);
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

    // macOS: check for a LaunchAgent plist at ~/Library/LaunchAgents/com.arabizi.app.plist
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
    <string>com.arabizi.app</string>
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
        .map(|home| PathBuf::from(home).join("Library/LaunchAgents/com.arabizi.app.plist"))
}

// ── Clipboard paste ───────────────────────────────────────────────────────────

#[tauri::command]
fn paste_from_clipboard() {
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(200));
        if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
            // macOS uses Cmd (Meta) + V; all other platforms use Ctrl + V
            #[cfg(target_os = "macos")]
            {
                let _ = enigo.key(Key::Meta, Direction::Press);
                let _ = enigo.key(Key::Unicode('v'), Direction::Click);
                let _ = enigo.key(Key::Meta, Direction::Release);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = enigo.key(Key::Control, Direction::Press);
                let _ = enigo.key(Key::Unicode('v'), Direction::Click);
                let _ = enigo.key(Key::Control, Direction::Release);
            }
        }
    });
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
        }
    }

    if let Some(window) = app.get_webview_window("overlay") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit("focus-input", ());
        }
    }
}
