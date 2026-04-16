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
        use window_vibrancy::apply_mica;
        if let Some(window) = app.get_webview_window("overlay") {
            let _ = apply_mica(&window, Some(dark));
        }
    }
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

            // Apply Mica with system default — JS will call apply_theme to match user preference
            if let Some(window) = app.get_webview_window("overlay") {
                #[cfg(target_os = "windows")]
                {
                    use window_vibrancy::apply_mica;
                    let _ = apply_mica(&window, None);
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
        .invoke_handler(tauri::generate_handler![transliterate, paste_from_clipboard, record_selection, apply_theme, get_accent_color])
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
