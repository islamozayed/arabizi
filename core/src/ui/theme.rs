//! Design tokens: the CSS custom properties of the web UI, resolved for the
//! current theme, contrast, text size, font and accent.

use super::geom::Color;
use crate::color::{accent_text_colors, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Manrope, or Atkinson Hyperlegible in dyslexia mode.
    Latin,
    /// Rubik (Arabic text).
    Arabic,
    /// Courier New (hex input).
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub family: Family,
    pub weight: u16,
    pub size: f32,
    pub color: Color,
    /// Letter spacing in em.
    pub tracking: f32,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub dark: bool,
    pub dyslexia: bool,
    pub large: bool,
    pub bg_surface: Color,
    pub outline: Color,
    pub separator: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub hint_example_bg: Color,
    pub hint_example_outline: Color,
    pub kbd_bg: Color,
    pub icon_fill: Color,
    pub icon_fill_muted: Color,
    pub item_hover: Color,
    pub mapping_bg: Color,
    pub overlay_glass_bg: Color,
    pub overlay_glass_border: Color,
    pub accent: Color,
    pub accent_translucent: Color,
    pub accent_fill: Color,
    pub on_accent: Color,
    pub radius: f32,
    // Type scale.
    pub font_size_min: f32,
    pub fs_caption: f32,
    pub fs_body: f32,
    pub fs_title: f32,
    pub fs_ar_list: f32,
    pub input_size: f32,
    pub hint_size: f32,
    pub preview_size: f32,
    pub preview_line: f32,
    pub suggestion_line: f32,
}

/// 11pt in CSS px.
pub const PT11: f32 = 11.0 * 96.0 / 72.0;
pub const PT13: f32 = 13.0 * 96.0 / 72.0;

impl Theme {
    pub fn new(dark: bool, high_contrast: bool, large: bool, dyslexia: bool, accent_hex: &str) -> Theme {
        let rgb = Rgb::from_hex(accent_hex).unwrap_or(Rgb(0x58, 0xA1, 0xAC));
        let accent = Color::from_rgb(rgb);
        let (fill, on) = accent_text_colors(rgb);
        let (mut outline, mut tp, mut ts, mut tt, mut kbd) = if dark {
            (Color::hex("#717171"), Color::WHITE, Color::rgba(255, 255, 255, 0.7), Color::rgba(255, 255, 255, 0.55), Color::rgba(255, 255, 255, 0.1))
        } else {
            (Color::hex("#c8c8c8"), Color::hex("#1a1a1a"), Color::rgba(0, 0, 0, 0.68), Color::rgba(0, 0, 0, 0.56), Color::rgba(0, 0, 0, 0.06))
        };
        if high_contrast {
            if dark {
                tp = Color::WHITE;
                ts = Color::rgba(255, 255, 255, 0.88);
                tt = Color::rgba(255, 255, 255, 0.65);
                outline = Color::hex("#a0a0a0");
                kbd = Color::rgba(255, 255, 255, 0.18);
            } else {
                tp = Color::BLACK;
                ts = Color::rgba(0, 0, 0, 0.82);
                tt = Color::rgba(0, 0, 0, 0.62);
                outline = Color::hex("#606060");
                kbd = Color::rgba(0, 0, 0, 0.12);
            }
        }
        let accent_translucent = accent.with_alpha(0.4);
        Theme {
            dark,
            dyslexia,
            large,
            bg_surface: if dark { Color::rgba(33, 33, 33, 0.6) } else { Color::rgba(245, 245, 245, 0.6) },
            outline,
            separator: outline.with_alpha_mul(0.25),
            text_primary: tp,
            text_secondary: ts,
            text_tertiary: tt,
            // applyAccentColor overrides --hint-example-bg with the accent at 0.4.
            hint_example_bg: accent_translucent,
            hint_example_outline: if dark { Color::rgba(255, 255, 255, 0.2) } else { Color::rgba(0, 0, 0, 0.1) },
            kbd_bg: kbd,
            icon_fill: if dark { Color::hex("#E3E3E3") } else { Color::hex("#333333") },
            icon_fill_muted: if dark { Color::hex("#9C9C9C") } else { Color::hex("#666666") },
            item_hover: if dark { Color::rgba(255, 255, 255, 0.06) } else { Color::rgba(0, 0, 0, 0.04) },
            mapping_bg: if dark { Color::rgba(255, 255, 255, 0.03) } else { Color::rgba(0, 0, 0, 0.04) },
            overlay_glass_bg: if dark { Color::rgba(28, 28, 30, 0.94) } else { Color::rgba(250, 250, 250, 0.96) },
            overlay_glass_border: if dark { Color::rgba(255, 255, 255, 0.10) } else { Color::rgba(0, 0, 0, 0.08) },
            accent,
            accent_translucent,
            accent_fill: Color::from_rgb(fill),
            on_accent: Color::from_rgb(on),
            // macOS window corner radius.
            radius: 12.0,
            font_size_min: PT11,
            fs_caption: if large { PT13 } else { PT11 },
            fs_body: if large { 19.0 } else { 16.0 },
            fs_title: if large { 21.0 } else { 18.0 },
            fs_ar_list: if large { 21.0 } else { 18.0 },
            input_size: if large { 19.0 } else { 16.0 },
            hint_size: if large { 18.0 } else { 16.0 },
            preview_size: if large { 38.0 } else { 32.0 },
            preview_line: if large { 48.0 } else { 40.0 },
            suggestion_line: if large { 28.0 } else { 24.0 },
        }
    }

    pub fn latin(&self, weight: u16, size: f32, color: Color) -> TextStyle {
        TextStyle { family: Family::Latin, weight, size, color, tracking: 0.0 }
    }

    pub fn arabic(&self, size: f32, color: Color) -> TextStyle {
        TextStyle { family: Family::Arabic, weight: 400, size, color, tracking: 0.0 }
    }
}
