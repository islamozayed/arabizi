//! One running copy per user. A second launch asks the first to show its
//! window and exits (the Tauri build used a plugin for the same thing).

#[cfg(unix)]
use std::io::Read;
use std::io::Write;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

/// Unix socket paths are capped at 104 bytes, so the socket lives in the
/// temp folder under a name derived from the data folder.
#[cfg(unix)]
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

/// Windows: a named mutex marks the running copy and a named pipe carries
/// the "show" request.
#[cfg(windows)]
pub fn acquire(dir: &Path, on_show: impl Fn() + Send + 'static) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, HANDLE};
    use windows::Win32::Storage::FileSystem::{ReadFile, PIPE_ACCESS_INBOUND};
    use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

    wait_for_previous();
    let id = instance_id(dir);
    let pipe = format!(r"\\.\pipe\franconvert-{id}");
    let mutex = unsafe { CreateMutexW(None, true, &HSTRING::from(format!(r"Local\franconvert-{id}"))) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        // Let the running copy take the foreground, then ask it to show.
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
        for _ in 0..40 {
            if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(&pipe) {
                let _ = f.write_all(b"show");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        return false;
    }
    // Held for the life of the process.
    std::mem::forget(mutex);
    std::thread::spawn(move || {
        let name = HSTRING::from(pipe.as_str());
        loop {
            let h: HANDLE = unsafe { CreateNamedPipeW(&name, PIPE_ACCESS_INBOUND, PIPE_TYPE_BYTE | PIPE_WAIT, PIPE_UNLIMITED_INSTANCES, 0, 64, 0, None) };
            if h.is_invalid() {
                return;
            }
            let connected = unsafe { ConnectNamedPipe(h, None) }.is_ok() || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            if connected {
                let mut buf = [0u8; 16];
                let mut n = 0u32;
                let _ = unsafe { ReadFile(h, Some(&mut buf), Some(&mut n), None) };
                on_show();
                let _ = unsafe { DisconnectNamedPipe(h) };
            }
            let _ = unsafe { CloseHandle(h) };
        }
    });
    true
}

#[cfg(windows)]
fn instance_id(dir: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.to_string_lossy().to_lowercase().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// A restart passes the old process id; wait for it to exit so the new
/// copy doesn't find it still running.
#[cfg(windows)]
fn wait_for_previous() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    let Some(pid) = std::env::var("FRANCONVERT_WAIT_PID").ok().and_then(|p| p.parse::<u32>().ok()) else { return };
    unsafe {
        std::env::remove_var("FRANCONVERT_WAIT_PID");
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, 5000);
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(not(any(unix, windows)))]
pub fn acquire(_dir: &Path, _on_show: impl Fn() + Send + 'static) -> bool {
    true
}

pub fn cleanup(dir: &Path) {
    #[cfg(unix)]
    let _ = std::fs::remove_file(socket_path(dir));
    #[cfg(not(unix))]
    let _ = dir;
}
