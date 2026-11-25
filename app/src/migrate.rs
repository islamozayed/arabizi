//! One-time import of the web UI's settings from WebKit's localStorage.

use std::collections::HashMap;

/// Reads `arabizi-*` keys from the Tauri build's WebKit localStorage
/// (macOS). Values are UTF-16LE blobs; `sqlite3` ships with macOS.
#[cfg(target_os = "macos")]
pub fn read_webkit_local_storage() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(home) = std::env::var_os("HOME") else { return out };
    let root = std::path::PathBuf::from(home).join("Library/WebKit").join(franconvert_core::APP_ID).join("WebsiteData/Default");
    let Ok(origins) = std::fs::read_dir(&root) else { return out };
    for o in origins.flatten() {
        let Ok(inner) = std::fs::read_dir(o.path()) else { continue };
        for i in inner.flatten() {
            let db = i.path().join("LocalStorage/localstorage.sqlite3");
            if !db.exists() {
                continue;
            }
            let Ok(res) = std::process::Command::new("/usr/bin/sqlite3")
                .arg("-readonly")
                .arg(&db)
                .arg("select key, hex(value) from ItemTable where key like 'arabizi-%';")
                .output()
            else {
                continue;
            };
            for line in String::from_utf8_lossy(&res.stdout).lines() {
                let Some((k, hex)) = line.split_once('|') else { continue };
                let bytes: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()).collect();
                out.insert(k.to_string(), franconvert_core::settings::decode_utf16le(&bytes));
            }
        }
    }
    out
}

#[cfg(not(target_os = "macos"))]
pub fn read_webkit_local_storage() -> HashMap<String, String> {
    HashMap::new()
}
