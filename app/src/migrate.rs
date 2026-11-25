//! One-time import of the web UI's settings from the web view's
//! localStorage (WebKit on macOS, WebView2 on Windows).

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

/// Reads `arabizi-*` keys from the Tauri build's WebView2 localStorage
/// (Windows). Chromium keeps it in LevelDB; a store this small lives in the
/// write-ahead log, which this replays.
#[cfg(windows)]
pub fn read_webkit_local_storage() -> HashMap<String, String> {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return HashMap::new() };
    let dir = std::path::PathBuf::from(local).join(franconvert_core::APP_ID).join("EBWebView/Default/Local Storage/leveldb");
    let Ok(entries) = std::fs::read_dir(&dir) else { return HashMap::new() };
    let mut logs: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "log")).collect();
    logs.sort();
    let mut out = HashMap::new();
    for log in logs {
        if let Ok(bytes) = std::fs::read(&log) {
            replay_leveldb_log(&bytes, &mut out);
        }
    }
    out
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn read_webkit_local_storage() -> HashMap<String, String> {
    HashMap::new()
}

/// Applies the puts and deletes in a LevelDB log to `out`, keeping
/// Chromium localStorage keys that start with `arabizi-`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn replay_leveldb_log(log: &[u8], out: &mut HashMap<String, String>) {
    const BLOCK: usize = 32 * 1024;
    // Reassemble records (FULL = 1, FIRST = 2, MIDDLE = 3, LAST = 4).
    let mut batches = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut pos = 0;
    while pos + 7 <= log.len() {
        let left_in_block = BLOCK - pos % BLOCK;
        if left_in_block < 7 {
            pos += left_in_block;
            continue;
        }
        let len = u16::from_le_bytes([log[pos + 4], log[pos + 5]]) as usize;
        let kind = log[pos + 6];
        let start = pos + 7;
        let Some(data) = log.get(start..start + len) else { break };
        match kind {
            1 => batches.push(data.to_vec()),
            2 => pending = data.to_vec(),
            3 => pending.extend_from_slice(data),
            4 => {
                pending.extend_from_slice(data);
                batches.push(std::mem::take(&mut pending));
            }
            _ => {}
        }
        pos = start + len;
    }
    for batch in batches {
        apply_batch(&batch, out);
    }
}

fn varint(b: &[u8], pos: &mut usize) -> Option<usize> {
    let mut v = 0usize;
    for shift in (0..35).step_by(7) {
        let byte = *b.get(*pos)?;
        *pos += 1;
        v |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn apply_batch(b: &[u8], out: &mut HashMap<String, String>) -> Option<()> {
    // Sequence number (8 bytes) and entry count (4).
    let count = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    let mut pos = 12;
    for _ in 0..count {
        let tag = *b.get(pos)?;
        pos += 1;
        let klen = varint(b, &mut pos)?;
        let key = b.get(pos..pos + klen)?;
        pos += klen;
        let value = if tag == 1 {
            let vlen = varint(b, &mut pos)?;
            let v = b.get(pos..pos + vlen)?;
            pos += vlen;
            Some(v)
        } else {
            None
        };
        // Chromium: "_" + origin + "\0" + (1 = Latin-1 | 0 = UTF-16LE) + key.
        let Some(sep) = key.iter().position(|&c| c == 0) else { continue };
        if key.first() != Some(&b'_') {
            continue;
        }
        let Some(name) = decode_chromium_string(&key[sep + 1..]) else { continue };
        if !name.starts_with("arabizi-") {
            continue;
        }
        match value.and_then(decode_chromium_string) {
            Some(v) => out.insert(name, v),
            None => out.remove(&name),
        };
    }
    Some(())
}

/// A Chromium localStorage string: a format byte, then Latin-1 or UTF-16LE.
fn decode_chromium_string(b: &[u8]) -> Option<String> {
    match b.first()? {
        1 => Some(b[1..].iter().map(|&c| c as char).collect()),
        0 => Some(franconvert_core::settings::decode_utf16le(&b[1..])),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(key: &str, value: &str) -> Vec<u8> {
        let mut k = b"_https://tauri.localhost\0\x01".to_vec();
        k.extend_from_slice(key.as_bytes());
        let mut v = vec![1u8];
        v.extend_from_slice(value.as_bytes());
        let mut e = vec![1u8, k.len() as u8];
        e.extend_from_slice(&k);
        e.push(v.len() as u8);
        e.extend_from_slice(&v);
        e
    }

    fn record(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut batch = vec![0u8; 8];
        batch.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for e in entries {
            batch.extend_from_slice(e);
        }
        let mut r = vec![0u8; 4];
        r.extend_from_slice(&(batch.len() as u16).to_le_bytes());
        r.push(1);
        r.extend_from_slice(&batch);
        r
    }

    #[test]
    fn replays_puts_and_later_overrides() {
        let mut log = record(&[put("arabizi-theme", "light"), put("other-app", "x")]);
        log.extend(record(&[put("arabizi-theme", "dark"), put("arabizi-compact", "true")]));
        let mut out = HashMap::new();
        replay_leveldb_log(&log, &mut out);
        assert_eq!(out.get("arabizi-theme").map(String::as_str), Some("dark"));
        assert_eq!(out.get("arabizi-compact").map(String::as_str), Some("true"));
        assert!(!out.contains_key("other-app"));
    }

    /// Prints what an installed Tauri build left behind:
    /// `cargo test -p franconvert -- --ignored --nocapture installed`.
    #[test]
    #[ignore]
    fn installed_webview_storage() {
        let mut items: Vec<_> = read_webkit_local_storage().into_iter().collect();
        items.sort();
        for (k, v) in items {
            println!("{k} = {v}");
        }
    }
}
