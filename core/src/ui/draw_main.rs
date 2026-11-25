//! Header, landing (quick start), typing panels (suggestions + preview) and
//! footer. Numbers are the stylesheet's.

use super::geom::{Color, Rect};
use super::icons;
use super::paint::{self, Painter};
use super::theme::{Family, TextStyle};
use super::widget::*;
use crate::session::{PreviewWord, Session};

pub(super) const PAD: f32 = 12.0;

impl Widget {
    pub(super) fn paint_main(&mut self, p: &mut dyn Painter) {
        let h = self.window_height();
        let t = self.theme.clone();
        // Every surface (header, panels, footer) has the same translucent fill.
        p.fill(Rect::new(0.0, 0.0, WINDOW_W, h), t.radius, t.bg_surface);

        self.paint_header(p);
        let content = Rect::new(0.0, HEADER_H, WINDOW_W, CONTENT_BOTTOM - HEADER_H);
        if self.settings_open || self.session.is_typing() {
            self.session.selected_starter = None;
        }
        if self.settings_open {
            self.paint_settings(p, content);
        } else if self.session.is_typing() {
            self.paint_typing(p, content);
        } else {
            self.paint_landing(p, content);
        }
        if !self.settings.compact {
            self.paint_footer(p, Rect::new(0.0, CONTENT_BOTTOM, WINDOW_W, FOOTER_H));
        }
        // Separator lines between header / content / footer (the CSS outlines).
        p.fill(Rect::new(0.0, HEADER_H - 1.0, WINDOW_W, 1.0), 0.0, t.separator);
        if !self.settings.compact {
            p.fill(Rect::new(0.0, CONTENT_BOTTOM - 1.0, WINDOW_W, 1.0), 0.0, t.separator);
        }
    }

    fn paint_header(&mut self, p: &mut dyn Painter) {
        let t = self.theme.clone();
        let header = Rect::new(0.0, 0.0, WINDOW_W, HEADER_H);
        self.hits.push((header, Target::Drag));
        let cy = HEADER_H / 2.0;

        if self.settings_open {
            // Back button: 18px icon in 4px padding.
            let back = Rect::new(PAD, cy - 13.0, 26.0, 26.0);
            if self.hovered(Target::Back) {
                p.fill(back, 6.0, t.item_hover);
            }
            paint::icon(p, &icons::BACK, back.inset(4.0, 4.0), t.icon_fill);
            self.hits.push((back, Target::Back));
            let title = t.latin(600, t.fs_title, t.text_primary);
            let lh = p.normal_line_height(&title);
            p.text("Settings", &title, back.right() + 10.0, cy - lh / 2.0, lh);
            return;
        }

        // Sit the logo on the input text's baseline (both glyph strokes end at
        // the logo's bottom edge), not merely centred in the header.
        let lw = 24.0;
        let input_style = TextStyle { family: Family::Latin, weight: 500, size: t.input_size, color: t.text_primary, tracking: 0.0 };
        let (asc, desc) = p.metrics(&input_style);
        let line = if t.input_size > 17.0 { 24.0 } else { 20.0 };
        let baseline = 16.0 + (28.0 - line) / 2.0 + (line - (asc + desc)) / 2.0 + asc;
        paint::logo(p, PAD, baseline - paint::logo_height(lw), lw, t.icon_fill_muted);

        // Input area: the native text field sits here (see `fields`). The
        // first-run hint is drawn beneath it while the field is empty.
        let input = Rect::new(52.0, 16.0, WINDOW_W - 52.0 - PAD - 20.0 - 16.0, 28.0);
        self.hits.push((input, Target::Block));
        if !self.settings.tutorial_seen && self.field.text.is_empty() {
            let hint = t.latin(400, t.hint_size, t.text_secondary);
            let msg = "Try typing in franco, something like";
            p.text(msg, &hint, input.x, cy - 10.0, 20.0);
            let x = input.x + p.measure(msg, &hint) + 6.0;
            let ex = t.latin(500, t.hint_size, t.text_primary);
            let w = p.measure(session::HINT_EXAMPLE, &ex) + 20.0;
            let chip = Rect::new(x + 2.0, cy - 13.0, w, 26.0);
            let hovered = self.hovered(Target::HintExample);
            let bg = t.accent.with_alpha(if hovered { 0.34 } else { 0.22 });
            p.fill(chip, 13.0, bg);
            p.stroke(chip, 13.0, 1.0, t.accent.with_alpha(if hovered { 0.45 } else { 0.28 }));
            p.text(session::HINT_EXAMPLE, &ex, chip.x + 10.0, chip.y + 3.0, 20.0);
            self.hits.push((chip, Target::HintExample));
        }

        // Settings gear (hidden during the first-run hint, then slides in).
        if self.settings.tutorial_seen {
            let (alpha, dx) = match &self.settings_reveal {
                Some(a) => {
                    let v = a.value(self.now);
                    (v, (1.0 - v) * 14.0)
                }
                None => (1.0, 0.0),
            };
            let gear = Rect::new(WINDOW_W - PAD - 20.0 + dx, cy - 10.0, 20.0, 20.0);
            let color = if self.hovered(Target::SettingsBtn) { t.accent } else { t.icon_fill };
            p.push_alpha(alpha);
            paint::icon(p, &icons::GEAR, gear, color);
            p.pop();
            self.hits.push((gear, Target::SettingsBtn));
        }
    }

    // ── Landing ───────────────────────────────────────────────────────────

    fn paint_landing(&mut self, p: &mut dyn Painter, area: Rect) {
        let t = self.theme.clone();
        self.hits.push((area, Target::Drag));
        let inner = area.inset(PAD, PAD);
        self.scroll_tips.viewport = area;
        let off = self.scroll_tips.offset;
        let (ft, fb) = self.scroll_tips.fades();
        p.push_clip(area, 0.0);
        p.push_fade(area, if ft { 24.0 } else { 0.0 }, if fb { 24.0 } else { 0.0 });

        let mut y = inner.y + 8.0 - off;
        let mut label = t.latin(600, t.font_size_min, t.text_secondary);
        label.tracking = 0.06;
        let lh = p.normal_line_height(&label);
        p.text("QUICK START", &label, inner.x, y, lh);
        self.ax_text.push((Rect::new(inner.x, y, 120.0, lh), "Quick start, heading".into()));
        y += lh + 4.0 + 4.0;

        // Two right-to-left columns, 4px row gap / 6px column gap.
        let col_w = (inner.w - 6.0) / 2.0;
        let ar_size = t.fs_ar_list;
        let row_h = 8.0 + 24.0f32.max(ar_size * 4.0 / 3.0).max(24.0);
        let starters = self.session.starters.clone();
        for (i, s) in starters.iter().enumerate() {
            let col = i % 2;
            let row = i / 2;
            let x = if col == 0 { inner.x + col_w + 6.0 } else { inner.x };
            let chip = Rect::new(x, y + row as f32 * (row_h + 4.0), col_w, row_h);
            let selected = self.session.selected_starter == Some(i);
            if selected {
                p.fill(chip.outset(1.0), 7.0, t.accent_fill);
            }
            let ar_color = if selected { t.on_accent } else { t.text_primary };
            let ar = t.arabic(ar_size, ar_color);
            paint::text_right(p, &Session::display(&s.ar), &ar, chip.right() - 10.0, chip.y + 4.0, 24.0);
            if selected {
                self.paint_starter_kbd(p, chip);
            }
            self.hits.push((chip, Target::Starter(i)));
        }
        let rows = starters.len().div_ceil(2) as f32;
        let content = (y + off) - area.y + rows * (row_h + 4.0) - 4.0 + PAD;
        self.scroll_tips.content = content;
        p.pop();
        p.pop();
    }

    /// "↵ paste · Tab new" shown on the selected starter chip.
    fn paint_starter_kbd(&self, p: &mut dyn Painter, chip: Rect) {
        let t = &self.theme;
        let kbd_style = t.latin(500, t.fs_caption, t.on_accent);
        let txt = TextStyle { tracking: 0.2 / t.font_size_min, ..t.latin(400, t.font_size_min, t.on_accent) };
        let kbd_bg = t.on_accent.with_alpha_mul(0.18);
        let mut x = chip.x + 10.0;
        let cy = chip.center().y;
        let kbd = |p: &mut dyn Painter, x: f32, label: &str| -> f32 {
            let w = p.measure(label, &kbd_style) + 12.0;
            let h = t.fs_caption + 6.0;
            p.fill(Rect::new(x, cy - h / 2.0, w, h), 4.0, kbd_bg);
            p.text(label, &kbd_style, x + 6.0, cy - t.fs_caption / 2.0, t.fs_caption);
            w
        };
        let word = |p: &mut dyn Painter, x: f32, s: &str, st: &TextStyle| -> f32 {
            p.text(s, st, x, cy - t.font_size_min / 2.0, t.font_size_min);
            p.measure(s, st)
        };
        x += kbd(p, x, "↵") + 4.0;
        x += word(p, x, "paste", &txt) + 4.0;
        let sep = TextStyle { color: t.on_accent.with_alpha_mul(0.55), ..txt };
        x += 3.0;
        x += word(p, x, "·", &sep) + 3.0 + 4.0;
        x += kbd(p, x, "Tab") + 4.0;
        word(p, x, "new", &txt);
    }

    // ── Typing ────────────────────────────────────────────────────────────

    fn paint_typing(&mut self, p: &mut dyn Painter, area: Rect) {
        let t = self.theme.clone();
        self.hits.push((area, Target::Drag));
        let sugg = Rect::new(area.x, area.y, 142.0, area.h);
        let prev = Rect::new(area.x + 142.0, area.y, area.w - 142.0, area.h);
        p.fill(Rect::new(sugg.right() - 1.0, area.y, 1.0, area.h), 0.0, t.separator);

        let label = t.latin(400, t.font_size_min, t.text_secondary);
        let llh = t.font_size_min * 1.3;
        p.text("Suggestions", &label, sugg.x + PAD, sugg.y + PAD, llh);
        p.text("Preview", &label, prev.x + PAD, prev.y + PAD, llh);
        let list_top = sugg.y + PAD + llh + 10.0;

        self.paint_suggestions(p, Rect::new(sugg.x + PAD, list_top, sugg.w - 2.0 * PAD, sugg.bottom() - PAD - list_top));
        self.paint_preview(p, Rect::new(prev.x + PAD, list_top, prev.w - 2.0 * PAD, prev.bottom() - PAD - list_top));
    }

    fn paint_suggestions(&mut self, p: &mut dyn Painter, list: Rect) {
        let t = self.theme.clone();
        let s = &self.session;
        let line = t.suggestion_line;
        let row_h = line + 8.0;
        let emoji_after = s.emoji_row_after();
        let candidates = s.candidates.clone();
        let emojis = s.emojis.clone();
        let (selected, emoji_mode, sel_emoji, lock) = (s.selected, s.emoji_mode, s.selected_emoji, s.show_lock());

        // Lay rows out first so the selected one can be scrolled into view.
        let mut rows: Vec<(Rect, Option<usize>)> = Vec::new(); // None = emoji row
        let mut y = 0.0;
        for i in 0..candidates.len() {
            rows.push((Rect::new(list.x, y, list.w, row_h), Some(i)));
            y += row_h + 4.0;
            if emoji_after == Some(i + 1) {
                rows.push((Rect::new(list.x, y, list.w, 32.0), None));
                y += 32.0 + 4.0;
            }
        }
        if emoji_after == Some(0) {
            rows.push((Rect::new(list.x, y, list.w, 32.0), None));
            y += 32.0 + 4.0;
        }
        let content = (y - 4.0).max(0.0);
        let sc = &mut self.scroll_suggestions;
        sc.viewport = list;
        sc.content = content;
        // scrollIntoView({ block: 'nearest' }) for the selection.
        let sel_row = rows.iter().find(|(_, i)| if emoji_mode { i.is_none() } else { *i == Some(selected) }).map(|(r, _)| *r);
        if let Some(r) = sel_row {
            if r.y < sc.offset {
                sc.offset = r.y;
            } else if r.bottom() > sc.offset + list.h {
                sc.offset = r.bottom() - list.h;
            }
        }
        sc.clamp();
        let off = sc.offset;

        p.push_clip(Rect::new(list.x - 2.0, list.y - 2.0, list.w + 4.0, list.h + 4.0), 0.0);
        self.suggestion_rows.clear();
        for (r, idx) in rows {
            let r = r.translate(0.0, list.y - off);
            match idx {
                Some(i) => {
                    let is_sel = !emoji_mode && i == selected;
                    if is_sel {
                        p.fill(r.outset(1.0), 7.0, t.accent_fill);
                    } else if self.hovered(Target::Suggestion(i)) {
                        p.fill(r, 6.0, t.item_hover);
                    }
                    let color = if is_sel { t.on_accent } else { t.text_primary };
                    let st = t.arabic(t.fs_ar_list, color);
                    let mut max_w = r.w - 8.0;
                    let right = r.right() - 4.0;
                    if lock && i == 0 {
                        max_w -= 14.0 + 6.0;
                    }
                    let text = paint::ellipsize(p, &Session::display(&candidates[i]), &st, max_w);
                    paint::text_right(p, &text, &st, right, r.y + 4.0, line);
                    if lock && i == 0 {
                        let c = if is_sel { t.on_accent } else { t.text_tertiary };
                        paint::icon(p, &icons::LOCK, Rect::new(r.x + 4.0, r.center().y - 7.0, 14.0, 14.0), c);
                    }
                    self.suggestion_rows.push(r);
                    self.hits.push((r, Target::Suggestion(i)));
                }
                None => {
                    // Emoji chips, right-aligned, 32×32 with 4px gaps.
                    let n = emojis.len() as f32;
                    let start = r.right() - n * 32.0 - (n - 1.0).max(0.0) * 4.0;
                    for (j, e) in emojis.iter().enumerate() {
                        let cell = Rect::new(start + j as f32 * 36.0, r.y, 32.0, 32.0);
                        if emoji_mode && j == sel_emoji {
                            p.fill(cell.outset(1.0), 7.0, t.accent_fill);
                        } else if self.hovered(Target::Emoji(j)) {
                            p.fill(cell, 6.0, t.item_hover);
                        }
                        let st = TextStyle { family: Family::Latin, weight: 400, size: 18.0, color: t.text_primary, tracking: 0.0 };
                        let w = p.measure(e, &st);
                        let lh = p.normal_line_height(&st);
                        p.text(e, &st, cell.center().x - w / 2.0, cell.center().y - lh / 2.0, lh);
                        self.hits.push((cell, Target::Emoji(j)));
                    }
                }
            }
        }
        p.pop();
    }

    fn paint_preview(&mut self, p: &mut dyn Painter, area: Rect) {
        let t = self.theme.clone();
        let words = self.session.preview();
        let st = t.arabic(t.preview_size, t.text_primary);
        let text_area = area.inset(4.0, 4.0);
        let space = p.measure(" ", &st);
        let pitch = t.preview_line + 8.0;

        // Right-to-left flow of inline blocks (padding 4px 6px).
        let mut lines: Vec<Vec<(f32, PreviewWord)>> = vec![Vec::new()];
        let mut used = 0.0f32;
        for w in words {
            let text = match &w {
                PreviewWord::Committed { text, .. } | PreviewWord::Editing { text, .. } | PreviewWord::Current { text } => text.clone(),
            };
            let bw = p.measure(&text, &st) + 12.0;
            let need = if used == 0.0 { bw } else { used + space + bw };
            if used > 0.0 && need > text_area.w {
                lines.push(Vec::new());
                used = bw;
            } else {
                used = need;
            }
            lines.last_mut().unwrap().push((bw, w));
        }
        let content = lines.len() as f32 * pitch + 8.0;
        let sc = &mut self.scroll_preview;
        sc.viewport = area;
        sc.content = content;
        if self.preview_stick_bottom {
            sc.offset = sc.max();
        }
        sc.clamp();
        let off = sc.offset;
        let (ft, fb) = sc.fades();

        p.push_clip(area, 0.0);
        p.push_fade(area, if ft { 24.0 } else { 0.0 }, if fb { 24.0 } else { 0.0 });
        for (li, line) in lines.iter().enumerate() {
            let top = text_area.y + li as f32 * pitch - off;
            let mut right = text_area.right();
            for (bw, w) in line {
                let r = Rect::new(right - bw, top, *bw, pitch);
                let (text, idx, editing) = match w {
                    PreviewWord::Committed { idx, text } => (text, Some(*idx), false),
                    PreviewWord::Editing { idx, text } => (text, Some(*idx), true),
                    PreviewWord::Current { text } => (text, None, false),
                };
                if let Some(i) = idx {
                    if editing {
                        p.fill(r, 8.0, t.accent_translucent);
                    } else if self.hovered(Target::PreviewWord(i)) {
                        p.fill(r, 8.0, t.item_hover);
                        paint::outline(p, r, 8.0, t.outline);
                    }
                    if r.bottom() > area.y && r.y < area.bottom() {
                        self.hits.push((r, Target::PreviewWord(i)));
                    }
                }
                p.text(text, &st, r.x + 6.0, r.y + 4.0, t.preview_line);
                right -= bw + space;
            }
        }
        p.pop();
        p.pop();
    }

    // ── Footer ────────────────────────────────────────────────────────────

    fn paint_footer(&mut self, p: &mut dyn Painter, area: Rect) {
        let t = self.theme.clone();
        self.hits.push((area, Target::Drag));
        let label = t.latin(400, t.font_size_min, t.text_tertiary);
        let kbd_text = t.latin(500, t.fs_caption, t.icon_fill);
        let y = area.y + PAD;
        let mut x = area.x + PAD;
        enum K {
            Icon(&'static icons::IconDef),
            Text(&'static str),
        }
        let items: Vec<(Vec<K>, &str)> = if self.settings_open {
            vec![(vec![K::Text("Esc")], "Back")]
        } else {
            vec![
                (vec![K::Icon(&icons::ARROW_UP), K::Icon(&icons::ARROW_DOWN)], "Select Word"),
                (vec![K::Text("Space")], "Commit"),
                (vec![K::Text("Tab")], "Skip"),
                (vec![K::Icon(&icons::ENTER)], "Paste"),
            ]
        };
        for (keys, text) in items {
            for (i, k) in keys.iter().enumerate() {
                let w = match k {
                    K::Icon(_) => 28.0,
                    K::Text(s) => (p.measure(s, &kbd_text) + 12.0).max(24.0),
                };
                let r = Rect::new(x, y, w, 24.0);
                p.fill(r, 5.0, t.kbd_bg);
                match k {
                    K::Icon(def) => paint::icon(p, def, Rect::new(x + 6.0, y + 4.0, 16.0, 16.0), t.icon_fill),
                    K::Text(s) => paint::text_center(p, s, &kbd_text, r.center().x, y + 12.0 - t.fs_caption / 2.0, t.fs_caption),
                }
                x += w + if i + 1 < keys.len() { 3.0 } else { 0.0 };
            }
            x += 4.0;
            let lh = t.font_size_min * 1.3;
            p.text(text, &label, x, y + 12.0 - lh / 2.0, lh);
            x += p.measure(text, &label) + 12.0;
        }
    }
}

use crate::session;

/// Draws a `<kbd>` chip with text or an icon and returns its width.
pub(super) fn kbd(p: &mut dyn Painter, x: f32, cy: f32, label: KbdLabel, st: &TextStyle, bg: Color, pad_x: f32, min_h: f32, icon_size: f32) -> f32 {
    let w = match label {
        KbdLabel::Text(s) => p.measure(s, st) + 2.0 * pad_x,
        KbdLabel::Icon(_) => icon_size + 2.0 * pad_x,
    };
    let r = Rect::new(x, cy - min_h / 2.0, w, min_h);
    p.fill(r, 5.0, bg);
    match label {
        KbdLabel::Text(s) => p.text(s, st, x + pad_x, cy - st.size / 2.0, st.size),
        KbdLabel::Icon(def) => paint::glyph(p, def, x + pad_x, cy - icon_size / 2.0, icon_size, st.color),
    }
    w
}

#[derive(Clone, Copy)]
pub(super) enum KbdLabel {
    Text(&'static str),
    Icon(&'static icons::IconDef),
}
