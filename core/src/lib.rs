//! Platform-independent logic for the Franconvert widget. Each platform UI
//! (AppKit on macOS, and later Win32 and GTK) renders this state natively.

pub mod color;
pub mod picker;
pub mod session;
pub mod settings;
pub mod text;
pub mod translit;
pub mod ui;

pub use arabizi_engine::{LetterSlot, TransliterationEngine, UserPreferences};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Bundle identifier; also names the data folder Tauri used, so learned
/// words and the onboarding flag carry over from the web-based releases.
pub const APP_ID: &str = "com.franconvert.app";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Per-user data folder (same place the Tauri releases used).
/// `FRANCONVERT_DATA_DIR` overrides it (tests, side-by-side dev builds).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FRANCONVERT_DATA_DIR") {
        let dir = PathBuf::from(dir);
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support").join(APP_ID)
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join(APP_ID)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join(APP_ID)
    };
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Write via a temp file + rename so a crash mid-write can't leave a
/// truncated file. Serialized so two quick writes can't interleave.
pub fn write_atomic(path: &Path, contents: &str) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, contents).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

pub fn load_prefs(dir: &Path) -> UserPreferences {
    fs::read_to_string(dir.join("user_prefs.json"))
        .map(|d| UserPreferences::from_json(&d))
        .unwrap_or_default()
}

pub fn save_prefs(dir: &Path, prefs: &UserPreferences) {
    write_atomic(&dir.join("user_prefs.json"), &prefs.to_json());
}
