//! User-visible settings. The web UI kept these in localStorage under
//! `arabizi-*` keys; the native apps keep them in `settings.json` next to
//! `user_prefs.json`, and import the old keys once on first launch.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CmdgTipState {
    pub perm_dismissed: bool,
    pub seen: bool,
    /// Unix seconds when the tip was last shown.
    pub last_shown: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Starter {
    pub ar: String,
    #[serde(default)]
    pub input: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    /// `None` follows the system accent colour.
    pub accent: Option<String>,
    pub recent_colors: Vec<String>,
    /// Launch shortcut such as "Cmd+Shift+A"; `None` is the platform default.
    pub shortcut: Option<String>,
    pub compact: bool,
    pub large_text: bool,
    pub high_contrast: bool,
    pub dyslexia_font: bool,
    pub tutorial_seen: bool,
    pub ctrl_backspace_hint_shown: bool,
    pub cmdg_tip: CmdgTipState,
    pub shortcuts_tip_dismissed: bool,
    pub last_seen_version: Option<String>,
    pub starters: Option<Vec<Starter>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            accent: None,
            recent_colors: Vec::new(),
            shortcut: None,
            compact: false,
            large_text: false,
            high_contrast: false,
            dyslexia_font: false,
            tutorial_seen: false,
            ctrl_backspace_hint_shown: false,
            cmdg_tip: CmdgTipState::default(),
            shortcuts_tip_dismissed: false,
            last_seen_version: None,
            starters: None,
        }
    }
}

pub const SETTINGS_FILE: &str = "settings.json";

impl Settings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(SETTINGS_FILE)
    }

    pub fn load(data_dir: &Path) -> Option<Settings> {
        let data = fs::read_to_string(Self::path(data_dir)).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self, data_dir: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            crate::write_atomic(&Self::path(data_dir), &json);
        }
    }

    /// True when the user had any state at all in the old web UI — used to
    /// decide whether an upgrade should show the "What's new" sheet.
    pub fn is_returning_user(&self) -> bool {
        *self != Settings::default()
    }

    /// Import the web UI's localStorage (`arabizi-*` keys).
    pub fn from_local_storage(items: &HashMap<String, String>) -> Settings {
        let get = |k: &str| items.get(k).map(|s| s.as_str());
        let flag = |k: &str| get(k) == Some("true");
        let mut s = Settings::default();
        s.theme = match get("arabizi-theme") {
            Some("light") => Theme::Light,
            Some("dark") => Theme::Dark,
            _ => Theme::System,
        };
        s.accent = get("arabizi-accent")
            .filter(|h| crate::color::Rgb::from_hex(h).is_some())
            .map(str::to_string);
        s.recent_colors = get("arabizi-recent-colors")
            .and_then(|j| serde_json::from_str::<Vec<String>>(j).ok())
            .unwrap_or_default();
        s.shortcut = get("arabizi-shortcut").map(str::to_string);
        s.compact = flag("arabizi-compact");
        s.large_text = flag("arabizi-large-text");
        s.high_contrast = flag("arabizi-high-contrast");
        s.dyslexia_font = flag("arabizi-dyslexia-font");
        s.tutorial_seen = flag("arabizi-tutorial-seen");
        s.ctrl_backspace_hint_shown = flag("arabizi-ctrlbs-hint-v2");
        if let Some(v) = get("arabizi-tip-cmdg-v2").and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok()) {
            s.cmdg_tip.perm_dismissed = v.get("permDismissed").and_then(|b| b.as_bool()).unwrap_or(false);
            s.cmdg_tip.seen = v.get("seen").and_then(|b| b.as_bool()).unwrap_or(false);
            s.cmdg_tip.last_shown = v.get("lastShown").and_then(|d| d.as_str()).and_then(parse_iso8601);
        }
        s.shortcuts_tip_dismissed = get("arabizi-tip-shortcuts-v1")
            .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
            .and_then(|v| v.get("dismissed").and_then(|b| b.as_bool()))
            .unwrap_or(false);
        s.last_seen_version = get("arabizi-last-seen-version").map(str::to_string);
        s.starters = get("arabizi-starters").and_then(|j| serde_json::from_str(j).ok());
        s
    }
}

/// Parses the `Date.toISOString()` format (`2026-11-25T10:20:30.123Z`) into
/// Unix seconds.
pub fn parse_iso8601(s: &str) -> Option<i64> {
    let (date, time) = s.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':');
    let (hh, mm) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    let ss = t.next().and_then(|v| v.split('.').next()).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Decodes a localStorage value as WebKit stores it (UTF-16LE bytes).
pub fn decode_utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_local_storage() {
        let mut m = HashMap::new();
        m.insert("arabizi-theme".into(), "dark".into());
        m.insert("arabizi-compact".into(), "true".into());
        m.insert("arabizi-accent".into(), "#E6A734".into());
        m.insert("arabizi-tip-cmdg-v2".into(), r#"{"seen":true,"lastShown":"2026-11-25T10:00:00.000Z"}"#.into());
        let s = Settings::from_local_storage(&m);
        assert_eq!(s.theme, Theme::Dark);
        assert!(s.compact);
        assert_eq!(s.accent.as_deref(), Some("#E6A734"));
        assert!(s.cmdg_tip.seen);
        assert_eq!(s.cmdg_tip.last_shown, Some(1795600800));
    }

    #[test]
    fn utf16() {
        assert_eq!(decode_utf16le(&[0x73, 0, 0x79, 0]), "sy");
    }
}
