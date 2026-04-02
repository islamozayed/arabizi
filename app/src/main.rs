#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arabizi_engine::TransliterationEngine;
use enigo::{Enigo, Key, Keyboard, Settings, Direction};
use serde::Serialize;
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
}

/// Per-word candidates for the frontend to render inline selection
#[derive(Serialize, Clone)]
struct WordResult {
    input: String,
    candidates: Vec<String>,
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
            let mut candidates = state.engine.transliterate_word(w);
            // Always provide the rule-based result as a fallback candidate
            let rule_based = state.engine.transliterate(w);
            for r in rule_based {
                if !candidates.contains(&r) {
                    candidates.push(r);
                }
            }
            WordResult {
                input: w.to_string(),
                candidates,
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
        }))
        .setup(|app| {
            // Apply Mica/Acrylic blur effect on Windows
            if let Some(window) = app.get_webview_window("overlay") {
                #[cfg(target_os = "windows")]
                {
                    use window_vibrancy::apply_mica;
                    let _ = apply_mica(&window, Some(true)); // dark mode
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
                    if let TrayIconEvent::Click { .. } = event {
                        toggle_overlay(tray.app_handle());
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
        .invoke_handler(tauri::generate_handler![transliterate, paste_from_clipboard])
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
