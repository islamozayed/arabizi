//! Windows system services: accent colour, theme, focus hand-off, clipboard,
//! paste, run-at-login, tray icon and screen-reader announcements.

use super::painter::{self, D2dPainter};
use franconvert_core::ui::geom::Color;
use franconvert_core::ui::paint;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const DEFAULT_ACCENT: &str = "#58A1AC";
const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
/// Value name the Tauri autostart plugin used, so the setting carries over.
const RUN_VALUE: PCWSTR = w!("Franconvert");

// ── Registry ─────────────────────────────────────────────────────────────

fn reg_dword(key: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key, value, RRF_RT_REG_DWORD, None, Some((&mut data as *mut u32).cast()), Some(&mut size))
    };
    (ok == ERROR_SUCCESS).then_some(data)
}

fn reg_string(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, None, Some(&mut size)) != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        if RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// Apps use the dark theme.
pub fn is_dark() -> bool {
    reg_dword(PERSONALIZE, w!("AppsUseLightTheme")) == Some(0)
}

/// The taskbar (and so the tray) is light.
pub fn taskbar_light() -> bool {
    reg_dword(PERSONALIZE, w!("SystemUsesLightTheme")) == Some(1)
}

/// The system accent colour as `#rrggbb` (the DWM accent, stored as ABGR).
pub fn accent_hex() -> String {
    match reg_dword(w!("Software\\Microsoft\\Windows\\DWM"), w!("AccentColor")) {
        Some(abgr) => format!("#{:02x}{:02x}{:02x}", abgr & 0xff, (abgr >> 8) & 0xff, (abgr >> 16) & 0xff),
        None => DEFAULT_ACCENT.into(),
    }
}

/// Windows build number (22000+ is Windows 11).
pub fn build_number() -> u32 {
    reg_string(HKEY_LOCAL_MACHINE, w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"), w!("CurrentBuildNumber"))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

// ── Run at login ─────────────────────────────────────────────────────────

pub fn autostart_enabled() -> bool {
    reg_string(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE).is_some()
}

pub fn set_autostart(enabled: bool) -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None) != ERROR_SUCCESS {
            return false;
        }
        let ok = if enabled {
            let exe = std::env::current_exe().unwrap_or_default();
            let cmd: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
            let bytes = std::slice::from_raw_parts(cmd.as_ptr().cast::<u8>(), cmd.len() * 2);
            RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes)) == ERROR_SUCCESS
        } else {
            let r = RegDeleteValueW(key, RUN_VALUE);
            r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
        };
        let _ = RegCloseKey(key);
        ok
    }
}

// ── Focus hand-off and paste ─────────────────────────────────────────────

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

pub fn activate(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// Sends Ctrl+V to the foreground window, first releasing any modifiers
/// still held (so it can't turn into Ctrl+Shift+V or Win+V).
pub fn send_paste_keystroke() {
    let mut inputs = Vec::new();
    for vk in [VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_MENU, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN] {
        if unsafe { GetAsyncKeyState(vk.0 as i32) } < 0 {
            inputs.push(key_input(vk, true));
        }
    }
    inputs.push(key_input(VK_CONTROL, false));
    inputs.push(key_input(VK_V, false));
    inputs.push(key_input(VK_V, true));
    inputs.push(key_input(VK_CONTROL, true));
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

// ── Clipboard ────────────────────────────────────────────────────────────

fn open_clipboard() -> bool {
    // Another app may hold it for a moment.
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

pub fn clipboard_text() -> Option<String> {
    if !open_clipboard() {
        return None;
    }
    let out = unsafe {
        (|| {
            let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        })()
    };
    unsafe {
        let _ = CloseClipboard();
    }
    out
}

/// Writes text and returns the clipboard sequence number afterwards.
pub fn set_clipboard_text(text: &str) -> u32 {
    if !open_clipboard() {
        return 0;
    }
    unsafe {
        let _ = EmptyClipboard();
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        if let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(g) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(g);
                if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(g.0))).is_err() {
                    let _ = GlobalFree(Some(g));
                }
            }
        }
        let _ = CloseClipboard();
        GetClipboardSequenceNumber()
    }
}

pub fn clipboard_sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

// ── Tray icon ────────────────────────────────────────────────────────────

/// Draws the logo into a square icon `size` px wide (white on a dark
/// taskbar, black on a light one).
pub fn logo_icon(size: i32, light_taskbar: bool) -> Option<HICON> {
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let screen = GetDC(None);
        let mut bits = std::ptr::null_mut();
        let color_bmp = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let mem = CreateCompatibleDC(Some(screen));
        ReleaseDC(None, screen);
        let old = SelectObject(mem, color_bmp.into());

        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let drawn = (|| {
            let rt = painter::d2d().CreateDCRenderTarget(&props).ok()?;
            let rc = RECT { left: 0, top: 0, right: size, bottom: size };
            rt.BindDC(mem, &rc).ok()?;
            rt.BeginDraw();
            rt.Clear(Some(&painter::color(Color::BLACK.with_alpha(0.0))));
            {
                let base: ID2D1RenderTarget = windows::core::Interface::cast(&rt).ok()?;
                let mut p = D2dPainter::new(&base, false, None).ok()?;
                let s = size as f32;
                // 180:136 logo, centred with a little margin.
                let w = s * 0.94;
                let h = paint::logo_height(w);
                let c = if light_taskbar { Color::BLACK } else { Color::WHITE };
                paint::logo(&mut p, (s - w) / 2.0, (s - h) / 2.0, w, c);
            }
            rt.EndDraw(None, None).ok()
        })();
        SelectObject(mem, old);
        let _ = DeleteDC(mem);
        if drawn.is_none() {
            let _ = DeleteObject(color_bmp.into());
            return None;
        }
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color_bmp };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(mask.into());
        let _ = DeleteObject(color_bmp.into());
        icon
    }
}

// ── Screen reader ────────────────────────────────────────────────────────
