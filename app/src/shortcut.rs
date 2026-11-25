//! Launch-shortcut strings ("Cmd+Shift+A") ⇄ global-hotkey definitions.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

pub fn parse(s: &str) -> Option<HotKey> {
    let mut mods = Modifiers::empty();
    let mut code = None;
    for part in s.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            "meta" | "super" | "win" | "cmd" => mods |= Modifiers::META,
            key => code = Some(key_code(key)?),
        }
    }
    Some(HotKey::new(if mods.is_empty() { None } else { Some(mods) }, code?))
}

fn key_code(k: &str) -> Option<Code> {
    use Code::*;
    Some(match k {
        "a" => KeyA, "b" => KeyB, "c" => KeyC, "d" => KeyD, "e" => KeyE, "f" => KeyF, "g" => KeyG,
        "h" => KeyH, "i" => KeyI, "j" => KeyJ, "k" => KeyK, "l" => KeyL, "m" => KeyM, "n" => KeyN,
        "o" => KeyO, "p" => KeyP, "q" => KeyQ, "r" => KeyR, "s" => KeyS, "t" => KeyT, "u" => KeyU,
        "v" => KeyV, "w" => KeyW, "x" => KeyX, "y" => KeyY, "z" => KeyZ,
        "0" => Digit0, "1" => Digit1, "2" => Digit2, "3" => Digit3, "4" => Digit4,
        "5" => Digit5, "6" => Digit6, "7" => Digit7, "8" => Digit8, "9" => Digit9,
        "space" => Space,
        "f1" => F1, "f2" => F2, "f3" => F3, "f4" => F4, "f5" => F5, "f6" => F6,
        "f7" => F7, "f8" => F8, "f9" => F9, "f10" => F10, "f11" => F11, "f12" => F12,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_and_custom() {
        assert_eq!(parse("Cmd+Shift+A"), Some(HotKey::new(Some(Modifiers::META | Modifiers::SHIFT), Code::KeyA)));
        assert_eq!(parse("Ctrl+Shift+A"), Some(HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyA)));
        assert_eq!(parse("Cmd+F5"), Some(HotKey::new(Some(Modifiers::META), Code::F5)));
        assert_eq!(parse("Cmd+Shift+?"), None);
    }
}
