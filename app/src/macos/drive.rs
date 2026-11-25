//! Test driver (feature `drive`): replays a script of UI steps against the
//! real window so screenshots can be compared with the old UI. Not built
//! into releases.
//!
//! `FRANCONVERT_DRIVE=<file>`: one step per line —
//!   show | hide | type <text> | key <up|down|left|right|space|tab|enter|escape|backspace|cmd+g|cmd+t|cmd+z|ctrl+backspace>
//!   | click <x> <y> | dblclick <x> <y> | move <x> <y> | select <start> <end> | wait <secs> | shot <png path> | quit

use super::*;

pub fn start() {
    let Ok(path) = std::env::var("FRANCONVERT_DRIVE") else { return };
    let Ok(script) = fs::read_to_string(&path) else { return };
    let steps: Vec<String> = script.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        for step in steps {
            let delay = if step.starts_with("wait ") { step[5..].parse::<f64>().unwrap_or(0.5) } else { 0.35 };
            if step.starts_with("shot ") {
                // The app has no Screen Recording permission; hand the
                // capture to the watcher script (`<script>.req` = "wid path").
                let out = step[5..].to_string();
                let id = main_sync(|| with_shell(|s| s.overlay.windowNumber()).unwrap_or(0));
                let req = format!("{path}.req");
                let _ = fs::write(&req, format!("{id} {out}"));
                let start = Instant::now();
                while std::path::Path::new(&req).exists() && start.elapsed() < Duration::from_secs(10) {
                    std::thread::sleep(Duration::from_millis(50));
                }
            } else if !step.starts_with("wait ") {
                let s = step.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                on_main(move || {
                    run_step(&s);
                    with_shell(|sh| sh.canvas.display());
                    let _ = tx.send(());
                });
                let _ = rx.recv_timeout(Duration::from_secs(5));
            }
            std::thread::sleep(Duration::from_secs_f64(delay));
        }
    });
}

fn run_step(step: &str) {
    let (cmd, arg) = step.split_once(' ').unwrap_or((step, ""));
    let nums: Vec<f32> = arg.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    let pt = || Point { x: nums[0], y: nums[1] };
    match cmd {
        "show" => toggle_overlay(),
        "hide" => hide_overlay(),
        "quit" => std::process::exit(0),
        "type" => {
            for c in arg.chars() {
                if c == ' ' && with_widget(|w| w.key(Key::Space, Mods::default())).unwrap_or(false) {
                    after_event();
                    continue;
                }
                with_shell(|s| {
                    let mut t = s.input.stringValue().to_string();
                    t.push(c);
                    s.input.setStringValue(&NSString::from_str(&t));
                    if let Some(ed) = s.input.currentEditor() {
                        let n = NSString::from_str(&t).length();
                        ed.setSelectedRange(NSRange::new(n, 0));
                    }
                });
                report_field(FieldId::Input);
            }
        }
        "select" => {
            with_shell(|s| {
                if let Some(ed) = s.input.currentEditor() {
                    ed.setSelectedRange(NSRange::new(nums[0] as usize, (nums[1] - nums[0]) as usize));
                }
            });
            report_field(FieldId::Input);
        }
        "key" => {
            let (mods, name) = match arg.rsplit_once('+') {
                Some((m, n)) => (
                    Mods { meta: m.contains("cmd"), ctrl: m.contains("ctrl"), shift: m.contains("shift"), alt: m.contains("alt") },
                    n,
                ),
                None => (Mods::default(), arg),
            };
            let key = match name {
                "up" => Key::Up,
                "down" => Key::Down,
                "left" => Key::Left,
                "right" => Key::Right,
                "space" => Key::Space,
                "tab" => Key::Tab,
                "enter" => Key::Enter,
                "escape" => Key::Escape,
                "backspace" => Key::Backspace,
                other => Key::Command(other.chars().next().unwrap_or('?')),
            };
            let handled = with_widget(|w| w.key(key, mods)).unwrap_or(false);
            if !handled {
                // Let the field do what it would have: caret moves / delete.
                with_shell(|s| {
                    if let Some(ed) = s.input.currentEditor() {
                        let r = ed.selectedRange();
                        let text = s.input.stringValue().to_string();
                        let n = NSString::from_str(&text).length();
                        match key {
                            Key::Left => ed.setSelectedRange(NSRange::new(r.location.saturating_sub(1), 0)),
                            Key::Right => ed.setSelectedRange(NSRange::new((r.location + 1).min(n), 0)),
                            Key::Backspace if !text.is_empty() => {
                                let mut t: Vec<char> = text.chars().collect();
                                t.pop();
                                let t: String = t.into_iter().collect();
                                s.input.setStringValue(&NSString::from_str(&t));
                            }
                            _ => {}
                        }
                    }
                });
                report_field(FieldId::Input);
            }
            after_event();
        }
        "click" | "dblclick" => {
            let clicks = if cmd == "dblclick" { 2 } else { 1 };
            let p = pt();
            with_widget(|w| {
                w.mouse_moved(p);
                w.mouse_down(p, 1);
                w.mouse_up(p, 1);
                if clicks == 2 {
                    w.mouse_down(p, 2);
                    w.mouse_up(p, 2);
                }
            });
            after_event();
        }
        "update" => {
            let v = arg.to_string();
            with_widget(|w| w.update_event(UpdateEvent::Checked { manual: false, available: Some(v) }));
            after_event();
        }
        "changelog" => {
            with_widget(|w| {
                w.settings.last_seen_version = Some("0.0.0".into());
                w.maybe_show_changelog(CHANGELOG, true);
            });
            after_event();
        }
        "axdump" => {
            // Walk the canvas's accessibility children like the system does.
            let mut out = String::new();
            with_shell(|s| unsafe {
                let kids: Option<Retained<objc2_foundation::NSArray<AnyObject>>> = msg_send![&*s.canvas, accessibilityChildren];
                for k in kids.map(|a| a.to_vec()).unwrap_or_default() {
                    let role: Option<Retained<NSString>> = msg_send![&*k, accessibilityRole];
                    let sub: Option<Retained<NSString>> = msg_send![&*k, accessibilitySubrole];
                    let label: Option<Retained<NSString>> = msg_send![&*k, accessibilityLabel];
                    let value: Option<Retained<AnyObject>> = msg_send![&*k, accessibilityValue];
                    let selected: bool = msg_send![&*k, isAccessibilitySelected];
                    let frame: NSRect = msg_send![&*k, accessibilityFrame];
                    let press: bool = msg_send![&*k, respondsToSelector: sel!(accessibilityPerformPress)];
                    out.push_str(&format!(
                        "{} {} \"{}\" value={} {}{} @({:.0},{:.0} {:.0}x{:.0})\n",
                        role.map(|r| r.to_string()).unwrap_or_default(),
                        sub.map(|r| r.to_string()).unwrap_or_default(),
                        label.map(|r| r.to_string()).unwrap_or_default(),
                        value.map(|v| { let d: Retained<NSString> = msg_send![&*v, description]; d.to_string() }).unwrap_or_default(),
                        if selected { "[selected] " } else { "" },
                        if press { "{press}" } else { "" },
                        frame.origin.x, frame.origin.y, frame.size.width, frame.size.height,
                    ));
                }
            });
            let _ = fs::write(arg, out);
        }
        "axpress" => {
            // Press the n-th accessibility child (VoiceOver's VO-Space).
            let canvas = with_shell(|s| s.canvas.clone());
            if let Some(canvas) = canvas {
                unsafe {
                    let kids: Option<Retained<objc2_foundation::NSArray<AnyObject>>> = msg_send![&*canvas, accessibilityChildren];
                    if let Some(k) = kids.and_then(|a| a.to_vec().get(nums[0] as usize).cloned()) {
                        let _: bool = msg_send![&*k, accessibilityPerformPress];
                    }
                }
            }
        }
        "move" => {
            let p = pt();
            with_widget(|w| w.mouse_moved(p));
            after_event();
        }
        _ => {}
    }
}
