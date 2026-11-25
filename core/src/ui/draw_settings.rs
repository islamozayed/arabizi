//! The settings panel: sidebar navigation and the four pages.

use super::draw_main::{kbd, KbdLabel, PAD};
use super::geom::{Color, Rect};
use super::icons::{self, IconDef};
use super::paint::{self, Painter};
use super::theme::TextStyle;
use super::widget::*;
use crate::color::{self, Rgb, ACCENT_PRESETS};
use crate::settings::Theme as ThemePref;

const NAV_W: f32 = 156.0;

impl Widget {
    pub(super) fn paint_settings(&mut self, p: &mut dyn Painter, area: Rect) {
        let t = self.theme.clone();
        // Sidebar.
        p.fill(Rect::new(area.x + NAV_W - 1.0, area.y, 1.0, area.h), 0.0, t.separator);
        let mut y = area.y + PAD;
        let label = t.latin(500, t.fs_caption, t.text_secondary);
        let lh = t.fs_caption * 1.3;
        let btn_h = lh.max(18.0) + 10.0;
        for page in Page::ALL {
            let r = Rect::new(area.x + PAD, y, NAV_W - 2.0 * PAD, btn_h);
            let active = self.page == page;
            let hovered = self.hovered(Target::Nav(page));
            let color = if active {
                p.fill(r.outset(1.0), 7.0, t.accent_fill);
                t.on_accent
            } else if hovered {
                p.fill(r, 6.0, t.item_hover);
                t.text_primary
            } else {
                t.text_secondary
            };
            let icon: &IconDef = match page {
                Page::Appearance => &icons::NAV_APPEARANCE,
                Page::System => &icons::NAV_SYSTEM,
                Page::Accessibility => &icons::NAV_ACCESSIBILITY,
                Page::Shortcuts => &icons::NAV_SHORTCUTS,
            };
            paint::icon(p, icon, Rect::new(r.x + 8.0, r.center().y - 9.0, 18.0, 18.0), color);
            p.text(page.title(), &TextStyle { color, ..label }, r.x + 8.0 + 18.0 + 6.0, r.center().y - lh / 2.0, lh);
            self.hits.push((r, Target::Nav(page)));
            y += btn_h + 4.0;
        }

        // Page content (scrolls; fades at the edges with more content).
        let view = Rect::new(area.x + NAV_W, area.y, area.w - NAV_W, area.h);
        self.scroll_settings.viewport = view;
        let off = self.scroll_settings.offset;
        let (ft, fb) = self.scroll_settings.fades();
        p.push_clip(view, 0.0);
        p.push_fade(view, if ft { 24.0 } else { 0.0 }, if fb { 24.0 } else { 0.0 });
        let inner = Rect::new(view.x + PAD, view.y + PAD - off, view.w - 2.0 * PAD, 0.0);
        let end = match self.page {
            Page::Appearance => self.page_appearance(p, inner),
            Page::System => self.page_system(p, inner),
            Page::Accessibility => self.page_accessibility(p, inner),
            Page::Shortcuts => self.page_shortcuts(p, inner),
        };
        p.pop();
        p.pop();
        self.scroll_settings.content = end + off - view.y + PAD;
        self.scroll_settings.clamp();
    }

    /// Section label ("ACCENT"), returns the y below it.
    fn section_label(&mut self, p: &mut dyn Painter, text: &str, x: f32, y: f32) -> f32 {
        let t = &self.theme;
        let mut st = t.latin(600, t.font_size_min, t.text_secondary);
        st.tracking = 0.06;
        let lh = p.normal_line_height(&st);
        p.text(&text.to_uppercase(), &st, x, y, lh);
        self.ax_text.push((Rect::new(x, y, p.measure(&text.to_uppercase(), &st), lh), format!("{text}, heading")));
        y + lh
    }

    /// Divider between subsections: returns the y where the next one starts.
    fn subsection_gap(&self, p: &mut dyn Painter, x: f32, w: f32, y: f32) -> f32 {
        let y = y + 10.0;
        p.fill(Rect::new(x, y, w, 1.0), 0.0, self.theme.separator);
        y + 1.0 + 10.0
    }

    fn toggle_row(&mut self, p: &mut dyn Painter, label: &str, target: Target, x: f32, w: f32, y: f32) -> f32 {
        let t = self.theme.clone();
        let st = t.latin(400, t.fs_body, t.text_primary);
        let lh = t.fs_body * 1.3;
        let row_h = lh.max(20.0) + 4.0;
        p.text(label, &st, x, y + (row_h - lh) / 2.0, lh);
        let sw = Rect::new(x + w - 36.0, y + (row_h - 20.0) / 2.0, 36.0, 20.0);
        let v = self.toggle_value(target);
        let off_bg = t.kbd_bg;
        let bg = off_bg.lerp(t.accent, v);
        let border = t.outline.lerp(t.accent, v);
        p.fill(sw, 10.0, bg);
        p.stroke(sw, 10.0, 1.0, border);
        let knob = Rect::new(sw.x + 1.0 + 2.0 + 16.0 * v, sw.y + 1.0 + 2.0, 14.0, 14.0);
        p.fill_ellipse(knob, Color::WHITE);
        self.hits.push((sw, target));
        y + row_h
    }

    fn button(&mut self, p: &mut dyn Painter, label: &str, target: Target, right: f32, y: f32, style: ButtonStyle) -> Rect {
        let t = self.theme.clone();
        let (color, bg, border) = match style {
            ButtonStyle::Normal => (t.text_secondary, Color::TRANSPARENT, t.outline),
            ButtonStyle::Recording => (t.accent, t.accent_translucent, t.accent),
        };
        let st = t.latin(500, t.font_size_min, color);
        let lh = p.normal_line_height(&st);
        let w = p.measure(label, &st) + 20.0 + 2.0;
        let r = Rect::new(right - w, y, w, lh + 8.0 + 2.0);
        let hovered = self.hovered(target);
        p.fill(r, 6.0, if hovered && style == ButtonStyle::Normal { t.item_hover } else { bg });
        p.stroke(r, 6.0, 1.0, border);
        p.text(label, &st, r.x + 11.0, r.y + 5.0, lh);
        self.hits.push((r, target));
        r
    }

    // ── Appearance ───────────────────────────────────────────────────────

    fn page_appearance(&mut self, p: &mut dyn Painter, inner: Rect) -> f32 {
        let t = self.theme.clone();
        let x = inner.x;
        let mut y = self.section_label(p, "Accent", x, inner.y) + 6.0;

        // System button + swatches + custom.
        let sys_active = self.settings.accent.is_none();
        let st = t.latin(500, t.font_size_min, if sys_active { t.text_primary } else { t.text_secondary });
        let w = p.measure("System", &st) + 20.0 + 2.0;
        let sys = Rect::new(x, y, w, 28.0);
        let bg = if sys_active {
            t.accent_translucent
        } else if self.hovered(Target::AccentSystem) {
            t.item_hover
        } else {
            Color::TRANSPARENT
        };
        p.fill(sys, 14.0, bg);
        p.stroke(sys, 14.0, 1.0, if sys_active { t.accent } else { t.text_primary });
        let lh = p.normal_line_height(&st);
        p.text("System", &st, sys.x + 11.0, sys.center().y - lh / 2.0, lh);
        self.hits.push((sys, Target::AccentSystem));

        let current = self.settings.accent.clone();
        let mut sx = sys.right() + 5.0;
        for (i, hex) in ACCENT_PRESETS.iter().enumerate() {
            let r = Rect::new(sx, y, 28.0, 28.0);
            let active = current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(hex));
            let scale = if self.hovered(Target::AccentPreset(i)) { 1.1 } else { 1.0 };
            self.swatch(p, r, scale, Color::hex(hex), active);
            self.hits.push((r, Target::AccentPreset(i)));
            sx += 28.0 + 5.0;
        }
        // Custom colour button.
        let r = Rect::new(sx, y, 28.0, 28.0);
        let custom = current.as_deref().filter(|c| !ACCENT_PRESETS.iter().any(|p| p.eq_ignore_ascii_case(c)));
        let active = custom.is_some();
        let (fill, glyph) = match custom.and_then(Rgb::from_hex) {
            Some(rgb) => {
                let lum = color::simple_luminance(rgb);
                (Color::from_rgb(rgb), if lum > 0.5 { Color::rgba(0, 0, 0, 0.7) } else { Color::rgba(255, 255, 255, 0.8) })
            }
            None => (t.kbd_bg, if self.hovered(Target::AccentCustom) { t.text_primary } else { t.text_secondary }),
        };
        let scale = if self.hovered(Target::AccentCustom) { 1.1 } else { 1.0 };
        self.swatch(p, r, scale, fill, active);
        paint::icon(p, &icons::PLUS, Rect::new(r.center().x - 7.0, r.center().y - 7.0, 14.0, 14.0), glyph);
        self.hits.push((r, Target::AccentCustom));
        y += 28.0;

        y = self.subsection_gap(p, x, inner.w, y);
        y = self.section_label(p, "Theme", x, y) + 6.0;
        let opt_w = (inner.w - 12.0) / 3.0;
        let prev_h = opt_w * 9.0 / 16.0;
        let label_st = t.latin(400, t.font_size_min, t.text_secondary);
        let llh = t.font_size_min * 1.3;
        for (i, (val, name)) in [(ThemePref::Light, "Light"), (ThemePref::Dark, "Dark"), (ThemePref::System, "System")].into_iter().enumerate() {
            let ox = x + i as f32 * (opt_w + 6.0);
            let prev = Rect::new(ox, y, opt_w, prev_h);
            self.theme_preview(p, prev, val);
            // Radio + label, centred under the preview.
            let tw = p.measure(name, &label_st);
            let row_w = 10.0 + 4.0 + tw;
            let ly = prev.bottom() + 5.0;
            let rx = prev.center().x - row_w / 2.0;
            // Centre the dot on the label's lowercase letters, not its line box.
            let radio = Rect::new(rx, ly + (llh - 10.0) / 2.0 + 1.0, 10.0, 10.0);
            if self.settings.theme == val {
                p.fill_ellipse(radio, t.accent);
            } else {
                p.stroke_ellipse(radio, 1.0, t.outline);
            }
            p.text(name, &label_st, rx + 14.0, ly, llh);
            self.hits.push((Rect::new(ox, y, opt_w, prev_h + 5.0 + llh), Target::ThemeOption(val)));
        }
        y += prev_h + 5.0 + llh;

        y = self.subsection_gap(p, x, inner.w, y);
        self.toggle_row(p, "Compact mode", Target::CompactToggle, x, inner.w, y)
    }

    fn swatch(&self, p: &mut dyn Painter, r: Rect, scale: f32, fill: Color, active: bool) {
        let t = &self.theme;
        let c = r.center();
        p.push_transform([scale, 0.0, 0.0, scale, c.x * (1.0 - scale), c.y * (1.0 - scale)]);
        p.fill_ellipse(r, fill);
        p.stroke_ellipse(r, 1.0, t.outline);
        if active {
            // outline: 2px, offset 2px.
            p.stroke_ellipse(r.outset(4.0), 2.0, t.text_primary);
        }
        p.pop();
    }

    fn theme_preview(&self, p: &mut dyn Painter, r: Rect, which: ThemePref) {
        let t = &self.theme;
        let dark = Color::rgba(33, 33, 33, 0.9);
        let light = Color::rgba(245, 245, 245, 0.9);
        p.push_clip(r, 6.0);
        match which {
            ThemePref::Light => p.fill(r, 0.0, light),
            ThemePref::Dark => p.fill(r, 0.0, dark),
            ThemePref::System => {
                // One clean split down the middle, each half painted once so
                // the dark half is a single tone.
                let half = (r.w / 2.0).round();
                p.fill(Rect::new(r.x + half, r.y, r.w - half, r.h), 0.0, light);
                p.fill(Rect::new(r.x, r.y, half, r.h), 0.0, dark);
            }
        }
        let sep = t.separator;
        p.fill(Rect::new(r.x, r.y + 11.0, r.w, 1.0), 0.0, sep);
        // The "sidebar" stands for the Suggestions panel: 142 of 600 pt.
        let panel = (r.w * 142.0 / WINDOW_W).round();
        p.fill(Rect::new(r.x + panel - 1.0, r.y + 12.0, 1.0, r.h - 26.0), 0.0, sep);
        p.fill(Rect::new(r.x, r.bottom() - 14.0, r.w, 1.0), 0.0, sep);
        let key = t.outline.with_alpha_mul(0.15);
        let wide = t.outline.with_alpha_mul(0.12);
        let ky = r.bottom() - 7.0;
        p.fill(Rect::new(r.x + 4.0, ky - 4.0, 8.0, 8.0), 1.0, key);
        p.fill(Rect::new(r.x + 14.0, ky - 4.0, 8.0, 8.0), 1.0, key);
        p.fill(Rect::new(r.x + 24.0, ky - 2.5, 28.0, 5.0), 1.0, wide);
        p.pop();
        paint::outline(p, r.inset(1.0, 1.0), 5.0, t.outline.with_alpha_mul(0.6));
    }

    // ── System ───────────────────────────────────────────────────────────

    fn page_system(&mut self, p: &mut dyn Painter, inner: Rect) -> f32 {
        let t = self.theme.clone();
        let x = inner.x;
        let right = inner.right();
        let mut y = self.section_label(p, "Launch Shortcut", x, inner.y) + 6.0;

        // Current shortcut (or live keys while recording) + Change button.
        let recording = self.recording.clone();
        let btn = self.button(
            p,
            if recording.is_some() { "Press keys…" } else { "Change" },
            Target::RecordShortcut,
            right,
            y,
            if recording.is_some() { ButtonStyle::Recording } else { ButtonStyle::Normal },
        );
        let cy = btn.center().y;
        let kbd_st = t.latin(500, t.fs_caption, t.icon_fill);
        let hint = t.latin(400, t.font_size_min, t.text_tertiary);
        let parts: Option<Vec<String>> = match &recording {
            None => Some(self.shortcut().split('+').map(str::to_string).collect()),
            Some(mods) if self.recording_hint.is_some() => {
                let _ = mods;
                None
            }
            Some(mods) if mods.is_empty() => None,
            Some(mods) => Some(mods.iter().map(|m| m.to_string()).chain(std::iter::once("?".to_string())).collect()),
        };
        match parts {
            Some(parts) => {
                let mut kx = x;
                for (i, part) in parts.iter().enumerate() {
                    let w = (p.measure(part, &kbd_st) + 12.0).max(24.0);
                    let r = Rect::new(kx, cy - 12.0, w, 24.0);
                    p.fill(r, 5.0, t.kbd_bg);
                    paint::text_center(p, part, &kbd_st, r.center().x, cy - t.fs_caption / 2.0, t.fs_caption);
                    kx += w + 3.0;
                    if i + 1 < parts.len() {
                        let sep = TextStyle { color: t.text_tertiary, ..t.latin(400, t.font_size_min, t.text_tertiary) };
                        p.text("+", &sep, kx + 1.0, cy - t.font_size_min * 0.65, t.font_size_min * 1.3);
                        kx += p.measure("+", &sep) + 2.0 + 3.0;
                    }
                }
            }
            None => {
                let msg = self.recording_hint.clone().unwrap_or_else(|| "Hold modifier + key…".into());
                let lh = t.font_size_min * 1.3;
                p.text(&msg, &hint, x, cy - lh / 2.0, lh);
            }
        }
        y = btn.bottom();

        y = self.subsection_gap(p, x, inner.w, y);
        y = self.toggle_row(p, "Run on startup", Target::StartupToggle, x, inner.w, y);

        y = self.subsection_gap(p, x, inner.w, y);
        y = self.section_label(p, "About", x, y) + 6.0;
        let btn = self.button(p, "Check for updates", Target::CheckUpdates, right, y, ButtonStyle::Normal);
        let lh = t.font_size_min * 1.3;
        let version = format!("Franconvert v{}", self.version);
        p.text(&version, &hint, x, btn.center().y - lh / 2.0, lh);
        self.ax_text.push((Rect::new(x, btn.y, btn.x - x - 8.0, btn.h), version));
        y = btn.bottom() + 6.0;
        if !self.update_status.is_empty() {
            p.text(&self.update_status.clone(), &hint, x, y + 6.0, lh);
            self.ax_text.push((Rect::new(x, y + 6.0, inner.w, lh), self.update_status.clone()));
            y += 6.0 + lh;
        }

        y = self.subsection_gap(p, x, inner.w, y);
        y = self.section_label(p, "Reset", x, y) + 6.0;
        let label = if self.resetting {
            "Resetting…"
        } else if self.reset_armed_until.is_some() {
            "Click again to confirm"
        } else {
            "Reset app"
        };
        let btn = self.button(p, label, Target::ResetApp, right, y, ButtonStyle::Normal);
        // The description wraps in the space left of the button.
        let lines = paint::wrap(p, "Clear learned words, dismissed tips, and saved settings.", &hint, btn.x - 8.0 - x);
        let block = lines.len() as f32 * lh;
        self.ax_text.push((Rect::new(x, btn.y, btn.x - x - 8.0, btn.h), "Clear learned words, dismissed tips, and saved settings.".into()));
        let top = btn.center().y - block / 2.0;
        for (i, l) in lines.iter().enumerate() {
            p.text(l, &hint, x, top + i as f32 * lh, lh);
        }
        y = btn.bottom().max(top + block) + 6.0;
        if self.reset_armed_until.is_some() {
            p.text("This will erase learned words and saved settings.", &hint, x, y + 6.0, lh);
            y += 6.0 + lh;
        }
        y
    }

    // ── Accessibility ────────────────────────────────────────────────────

    fn page_accessibility(&mut self, p: &mut dyn Painter, inner: Rect) -> f32 {
        let x = inner.x;
        let mut y = self.toggle_row(p, "Larger text", Target::LargeText, x, inner.w, inner.y);
        y = self.subsection_gap(p, x, inner.w, y);
        y = self.toggle_row(p, "High contrast", Target::HighContrast, x, inner.w, y);
        y = self.subsection_gap(p, x, inner.w, y);
        self.toggle_row(p, "Dyslexia font", Target::Dyslexia, x, inner.w, y)
    }

    // ── Shortcuts ────────────────────────────────────────────────────────

    fn page_shortcuts(&mut self, p: &mut dyn Painter, inner: Rect) -> f32 {
        let t = self.theme.clone();
        let x = inner.x;
        let m = if self.is_mac { KbdLabel::Icon(&icons::MS_KEYBOARD_COMMAND_KEY) } else { KbdLabel::Text("Ctrl") };
        use KbdLabel::*;
        let sections: [(&str, Vec<(&str, Vec<KbdLabel>)>); 3] = [
            (
                "Editing",
                vec![
                    ("Swap a letter", vec![m, Text("G")]),
                    ("Add tashkeel (letter or whole word)", vec![m, Text("T")]),
                    ("Undo last picker change", vec![m, Text("Z")]),
                    ("Cycle picker chips", vec![Icon(&icons::MS_KEYBOARD_TAB)]),
                ],
            ),
            (
                "Suggestions",
                vec![
                    ("Move between candidates", vec![Icon(&icons::MS_KEYBOARD_ARROW_UP), Icon(&icons::MS_KEYBOARD_ARROW_DOWN)]),
                    ("Keep Latin (skip transliteration)", vec![Icon(&icons::MS_KEYBOARD_TAB)]),
                    ("Commit current word", vec![Icon(&icons::MS_SPACE_BAR)]),
                    ("Paste committed text", vec![Icon(&icons::MS_KEYBOARD_RETURN)]),
                ],
            ),
            (
                "Words",
                vec![
                    ("Edit previous committed word", vec![Icon(&icons::MS_BACKSPACE)]),
                    ("Delete previous committed word", vec![m, Icon(&icons::MS_BACKSPACE)]),
                ],
            ),
        ];
        let label = t.latin(400, t.fs_body, t.text_primary);
        let lh = t.fs_body * 1.3;
        let kbd_st = t.latin(400, t.font_size_min, t.text_primary);
        let mut y = inner.y;
        for (si, (title, rows)) in sections.iter().enumerate() {
            if si > 0 {
                y = self.subsection_gap(p, x, inner.w, y);
            }
            y = self.section_label(p, title, x, y) + 6.0;
            for (ri, (text, keys)) in rows.iter().enumerate() {
                if ri > 0 {
                    y += 6.0;
                }
                let row_h = lh.max(24.0) + 12.0;
                let r = Rect::new(x - 8.0, y, inner.w + 16.0, row_h);
                if self.mouse.is_some_and(|m| r.contains(m)) {
                    p.fill(r, 6.0, t.item_hover);
                }
                p.text(text, &label, x, r.center().y - lh / 2.0, lh);
                // Keys right-aligned, 4px apart.
                let widths: Vec<f32> = keys
                    .iter()
                    .map(|k| match k {
                        Text(s) => p.measure(s, &kbd_st) + 16.0,
                        Icon(_) => t.font_size_min + 16.0,
                    })
                    .collect();
                let total: f32 = widths.iter().sum::<f32>() + 4.0 * (keys.len() as f32 - 1.0);
                let mut kx = x + inner.w - total;
                for k in keys {
                    kx += kbd(p, kx, r.center().y, *k, &kbd_st, t.kbd_bg, 8.0, 24.0, t.font_size_min) + 4.0;
                }
                let spoken: Vec<&str> = keys
                    .iter()
                    .map(|k| match k {
                        Text(s) => *s,
                        Icon(d) if std::ptr::eq(*d, &icons::MS_KEYBOARD_COMMAND_KEY) => "Command",
                        Icon(d) if std::ptr::eq(*d, &icons::MS_KEYBOARD_TAB) => "Tab",
                        Icon(d) if std::ptr::eq(*d, &icons::MS_KEYBOARD_ARROW_UP) => "Up arrow",
                        Icon(d) if std::ptr::eq(*d, &icons::MS_KEYBOARD_ARROW_DOWN) => "Down arrow",
                        Icon(d) if std::ptr::eq(*d, &icons::MS_SPACE_BAR) => "Space",
                        Icon(d) if std::ptr::eq(*d, &icons::MS_KEYBOARD_RETURN) => "Return",
                        Icon(_) => "Delete",
                    })
                    .collect();
                self.ax_text.push((r, super::access::shortcut_row_label(text, &spoken)));
                self.hits.push((r, Target::ShortcutsRow));
                y += row_h;
            }
        }
        y
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ButtonStyle {
    Normal,
    Recording,
}
