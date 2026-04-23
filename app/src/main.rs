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
    // Persist to disk (best-effort)
    let json = state.prefs.to_json();
    let path = state.prefs_path.clone();
    drop(state);
    let _ = fs::write(path, json);
}

#[tauri::command]
fn get_accent_color() -> String {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(dwm) = hkcu.open_subkey("SOFTWARE\\Microsoft\\Windows\\DWM") {
            if let Ok(color) = dwm.get_value::<u32, _>("AccentColor") {
                // ABGR format: bits 0-7 = R, 8-15 = G, 16-23 = B
                let r = color & 0xFF;
                let g = (color >> 8) & 0xFF;
                let b = (color >> 16) & 0xFF;
                return format!("#{:02x}{:02x}{:02x}", r, g, b);
            }
        }
    }
    "#58A1AC".to_string()
}


#[tauri::command]
fn apply_theme(app: tauri::AppHandle, dark: bool) {
    #[cfg(target_os = "windows")]
    {
        use window_vibrancy::apply_acrylic;
        if let Some(window) = app.get_webview_window("overlay") {
            // #121212 @ 84% for dark, #F5F5F5 @ 84% for light
            let color = if dark { (20u8, 20u8, 20u8, 238u8) } else { (245u8, 245u8, 245u8, 238u8) };
            let _ = apply_acrylic(&window, Some(color));
        }
    }
}

fn parse_shortcut_str(s: &str) -> Result<Shortcut, String> {
    let mut modifiers = Modifiers::empty();
    let mut code: Option<Code> = None;
    for part in s.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" => modifiers |= Modifiers::ALT,
            "meta" | "super" | "win" => modifiers |= Modifiers::SUPER,
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
        "f1" => Some(Code::F1), "f2" => Some(Code::F2), "f3" => Some(Code::F3),
        "f4" => Some(Code::F4), "f5" => Some(Code::F5), "f6" => Some(Code::F6),
        "f7" => Some(Code::F7), "f8" => Some(Code::F8), "f9" => Some(Code::F9),
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

#[tauri::command]
fn get_autostart() -> bool {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let exe = std::env::current_exe().unwrap_or_default();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run") {
            if let Ok(val) = run_key.get_value::<String, _>("Arabizi") {
                return val.to_lowercase() == exe.to_string_lossy().to_lowercase();
            }
        }
    }
    false
}

#[tauri::command]
fn set_autostart(enabled: bool) -> bool {
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
                let _ = run_key.set_value("Arabizi", &exe.to_string_lossy().into_owned());
            } else {
                let _ = run_key.delete_value("Arabizi");
            }
            return true;
        }
    }
    false
}

#[tauri::command]
fn paste_from_clipboard() {
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(200));
        if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
            let _ = enigo.key(Key::Control, Direction::Press);
            let _ = enigo.key(Key::Unicode('v'), Direction::Click);
            let _ = enigo.key(Key::Control, Direction::Release);
        }
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(Mutex::new(AppState {
            engine: TransliterationEngine::new(),
            prefs: UserPreferences::new(), // replaced in setup
            prefs_path: PathBuf::new(),    // replaced in setup
        }))
        .setup(|app| {
            // Load user preferences from app data directory
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

            // Apply acrylic backdrop — re-applied on theme change via apply_theme command
            #[cfg(target_os = "windows")]
            {
                use window_vibrancy::apply_acrylic;
                if let Some(window) = app.get_webview_window("overlay") {
                    let _ = apply_acrylic(&window, Some((20u8, 20u8, 20u8, 238u8)));
                }
            }

            // Build tray menu
            let show = MenuItem::with_id(app, "show", "Show Arabizi", true, None::<&str>)?;
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
                        // Only toggle on left-click press, not right-click (which opens the menu)
                        if button == tauri::tray::MouseButton::Left
                            && button_state == tauri::tray::MouseButtonState::Up
                        {
                            toggle_overlay(tray.app_handle());
                        }
                    }
                });
            }

            // Register global shortcut: Ctrl+Shift+A — press only
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyA);
            let app_handle = app.handle().clone();
            app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    toggle_overlay(&app_handle);
                }
            })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![transliterate, paste_from_clipboard, record_selection, apply_theme, get_accent_color, update_shortcut, get_autostart, set_autostart])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn toggle_overlay(app: &tauri::AppHandle) {
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
