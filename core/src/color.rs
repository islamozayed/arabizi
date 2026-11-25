//! Accent colour maths, ported from the web UI so colours match exactly.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn from_hex(hex: &str) -> Option<Rgb> {
        let h = hex.trim().strip_prefix('#')?;
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(p(0)?, p(2)?, p(4)?))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

pub const DEFAULT_ACCENT: &str = "#58A1AC";
pub const ACCENT_PRESETS: [&str; 6] = ["#E6A734", "#7CCB74", "#00CED0", "#8BAEFF", "#F28DCE", "#FF8C91"];
pub const PRESET_NAMES: [&str; 6] = ["Amber", "Green", "Teal", "Blue", "Pink", "Coral"];

/// Fill + text colour for anything with text on the accent, keeping at least
/// 4.5:1 contrast: mid-tone accents get slightly darkened behind white text,
/// bright ones keep their colour and get dark text.
pub fn accent_text_colors(c: Rgb) -> (Rgb, Rgb) {
    fn lin(c: f64) -> f64 {
        let c = c / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    let lum = |r: f64, g: f64, b: f64| 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    let mut t = 0.0f64;
    while t <= 0.25 + 1e-9 {
        let m = [c.0 as f64 * (1.0 - t), c.1 as f64 * (1.0 - t), c.2 as f64 * (1.0 - t)];
        if 1.05 / (lum(m[0], m[1], m[2]) + 0.05) >= 4.5 {
            let r = |v: f64| v.round() as u8;
            return (Rgb(r(m[0]), r(m[1]), r(m[2])), Rgb(255, 255, 255));
        }
        t += 0.02;
    }
    (c, Rgb(0x14, 0x14, 0x14))
}

/// Relative brightness used to pick the "+" glyph colour on a custom swatch.
pub fn simple_luminance(c: Rgb) -> f64 {
    (c.0 as f64 * 0.2126 + c.1 as f64 * 0.7152 + c.2 as f64 * 0.0722) / 255.0
}

/// HSV with h in 0..=360, s and v in 0..=100 (integers, like the web picker).
pub fn hex_to_hsv(c: Rgb) -> (i32, i32, i32) {
    let (r, g, b) = (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = 0.0;
    if d != 0.0 {
        h = if max == r {
            ((g - b) / d + if g < b { 6.0 } else { 0.0 }) * 60.0
        } else if max == g {
            ((b - r) / d + 2.0) * 60.0
        } else {
            ((r - g) / d + 4.0) * 60.0
        };
    }
    let s = if max != 0.0 { (d / max * 100.0).round() } else { 0.0 };
    (h.round() as i32, s as i32, (max * 100.0).round() as i32)
}

pub fn hsv_to_rgb(h: i32, s: i32, v: i32) -> Rgb {
    let (h, s, v) = (h as f64, s as f64 / 100.0, v as f64 / 100.0);
    let f = |n: f64| {
        let k = (n + h / 60.0) % 6.0;
        v - v * s * k.min(4.0 - k).min(1.0).max(0.0)
    };
    let c = |x: f64| (x * 255.0).round() as u8;
    Rgb(c(f(5.0)), c(f(3.0)), c(f(1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_accent_fill_matches_css() {
        // --accent-fill in the stylesheet was computed by the same function.
        let (fill, on) = accent_text_colors(Rgb::from_hex(DEFAULT_ACCENT).unwrap());
        assert_eq!(fill.to_hex(), "#457e86");
        assert_eq!(on, Rgb(255, 255, 255));
    }

    #[test]
    fn hsv_round_trip() {
        for hex in ACCENT_PRESETS {
            let c = Rgb::from_hex(hex).unwrap();
            let (h, s, v) = hex_to_hsv(c);
            let back = hsv_to_rgb(h, s, v);
            assert!((back.0 as i32 - c.0 as i32).abs() <= 3, "{hex}");
        }
    }
}
