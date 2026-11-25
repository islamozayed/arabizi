//! One running copy per user. A second launch asks the first to show its
//! window and exits (the Tauri build used a plugin for the same thing).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Unix socket paths are capped at 104 bytes, so the socket lives in the
/// temp folder under a name derived from the data folder.
fn socket_path(dir: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    std::env::temp_dir().join(format!("franconvert-{:016x}.sock", h.finish()))
}

/// Returns false if another copy is running (it has been asked to show
/// itself). Otherwise listens for later launches and calls `on_show`.
#[cfg(unix)]
pub fn acquire(dir: &Path, on_show: impl Fn() + Send + 'static) -> bool {
    use std::os::unix::net::{UnixListener, UnixStream};
    let path = socket_path(dir);
    if let Ok(mut s) = UnixStream::connect(&path) {
        let _ = s.write_all(b"show");
        return false;
    }
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return true };
    std::thread::spawn(move || {
        for mut conn in listener.incoming().flatten() {
            let mut buf = [0u8; 16];
            let _ = conn.read(&mut buf);
            on_show();
        }
    });
    true
}

#[cfg(not(unix))]
pub fn acquire(_dir: &Path, _on_show: impl Fn() + Send + 'static) -> bool {
    true
}

pub fn cleanup(dir: &Path) {
    let _ = std::fs::remove_file(socket_path(dir));
}
