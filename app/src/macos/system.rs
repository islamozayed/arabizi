//! macOS system services: accent colour, appearance, focus hand-off, paste,
//! login item, fonts.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationOptions, NSColor, NSColorSpace,
    NSPasteboard, NSPasteboardTypeString, NSRunningApplication, NSWorkspace,
};
use objc2_foundation::{NSArray, NSString, NSURL};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_ACCENT: &str = "#58A1AC";

/// The system accent colour as `#rrggbb`.
pub fn accent_hex() -> String {
    let accent = NSColor::controlAccentColor();
    let Some(rgb) = accent.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
        return DEFAULT_ACCENT.into();
    };
    let c = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(rgb.redComponent()), c(rgb.greenComponent()), c(rgb.blueComponent()))
}

pub fn is_dark(mtm: MainThreadMarker) -> bool {
    let app = NSApplication::sharedApplication(mtm);
    let appearance = app.effectiveAppearance();
    let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    let best = appearance.bestMatchFromAppearancesWithNames(&names);
    best.is_some_and(|n| unsafe { &*n == NSAppearanceNameDarkAqua })
}

pub fn macos_major() -> isize {
    objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersion().majorVersion
}

// ── Focus hand-off ───────────────────────────────────────────────────────

/// PID of the frontmost application, or 0.
pub fn frontmost_pid() -> i32 {
    NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()).unwrap_or(0)
}

pub fn activate_pid(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    match NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        #[allow(deprecated)]
        Some(app) => app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps),
        None => false,
    }
}

/// Posts Cmd+V through Core Graphics with the raw virtual keycode for V.
/// (enigo's Unicode path calls TSM APIs that trap off the main thread on
/// macOS 26; CGEventPost is thread-safe.)
pub fn send_paste_keystroke() {
    use std::ffi::c_void;
    type Ref = *mut c_void;
    unsafe extern "C" {
        fn CGEventSourceCreate(state_id: i32) -> Ref;
        fn CGEventCreateKeyboardEvent(source: Ref, virtual_key: u16, key_down: bool) -> Ref;
        fn CGEventSetFlags(event: Ref, flags: u64);
        fn CGEventPost(tap: u32, event: Ref);
        fn CFRelease(obj: *const c_void);
    }
    const HID_SYSTEM_STATE: i32 = 1;
    const HID_EVENT_TAP: u32 = 0;
    const FLAG_COMMAND: u64 = 1 << 20;
    const KEY_V: u16 = 0x09;
    unsafe {
        let source = CGEventSourceCreate(HID_SYSTEM_STATE);
        let down = CGEventCreateKeyboardEvent(source, KEY_V, true);
        let up = CGEventCreateKeyboardEvent(source, KEY_V, false);
        if !down.is_null() && !up.is_null() {
            CGEventSetFlags(down, FLAG_COMMAND);
            CGEventSetFlags(up, FLAG_COMMAND);
            CGEventPost(HID_EVENT_TAP, down);
            CGEventPost(HID_EVENT_TAP, up);
        }
        for r in [down, up, source] {
            if !r.is_null() {
                CFRelease(r);
            }
        }
    }
}

// ── Clipboard ────────────────────────────────────────────────────────────

pub fn clipboard_text() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    unsafe { pb.stringForType(NSPasteboardTypeString) }.map(|s| s.to_string())
}

/// Writes text and returns the pasteboard's change count afterwards.
pub fn set_clipboard_text(text: &str) -> isize {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    unsafe { pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) };
    pb.changeCount()
}

pub fn clipboard_change_count() -> isize {
    NSPasteboard::generalPasteboard().changeCount()
}

// ── Login item (LaunchAgent, as in the Tauri build) ──────────────────────

fn launch_agent_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/LaunchAgents/com.franconvert.app.plist"))
}

pub fn autostart_enabled() -> bool {
    launch_agent_path().is_some_and(|p| p.exists())
}

pub fn set_autostart(enabled: bool) -> bool {
    let Some(path) = launch_agent_path() else { return false };
    if enabled {
        let exe = std::env::current_exe().unwrap_or_default();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.franconvert.app</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>"#,
            exe.to_string_lossy()
        );
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&path, plist).is_ok()
    } else {
        fs::remove_file(&path).is_ok() || !path.exists()
    }
}

// ── Fonts ────────────────────────────────────────────────────────────────

/// Folder holding the bundled fonts: `Contents/Resources/fonts` in the app
/// bundle, or the source tree when running with `cargo run`.
pub fn resources_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
            let r = contents.join("Resources");
            if r.join("fonts").exists() {
                return r;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui")
}

pub fn register_fonts() {
    unsafe extern "C" {
        fn CTFontManagerRegisterFontsForURL(url: *const std::ffi::c_void, scope: u32, error: *mut *mut std::ffi::c_void) -> bool;
    }
    const SCOPE_PROCESS: u32 = 1;
    let dir = resources_dir().join("fonts");
    let Ok(entries) = fs::read_dir(&dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "ttf" || x == "otf") {
            let url: Retained<NSURL> = NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()));
            unsafe {
                CTFontManagerRegisterFontsForURL(Retained::as_ptr(&url).cast(), SCOPE_PROCESS, std::ptr::null_mut());
            }
        }
    }
}

/// Path of the running `.app` bundle, if we're in one.
pub fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    app.extension().is_some_and(|e| e == "app").then(|| app.to_path_buf())
}
