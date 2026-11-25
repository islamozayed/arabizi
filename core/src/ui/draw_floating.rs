//! Things that float above the panels: the letter picker, tips, toast,
//! update banner, colour picker and the "What's new" sheet.

use super::draw_main::{kbd, KbdLabel};
use super::geom::{Color, Point, Rect};
use super::icons;
use super::paint::{self, Painter, Shadow};
use super::theme::TextStyle;
use super::widget::*;
use crate::color;
use crate::session::Session;

const EDGE: f32 = 8.0;

impl Widget {
    pub(super) fn paint_floating(&mut self, p: &mut dyn Painter) {
        // The banner sits lowest; tips and the picker float above it.
        self.paint_banner(p);
        if !self.settings_open {
            self.paint_picker(p);
            self.paint_cmdg_tip(p);
            self.paint_shortcuts_tip(p);
        }
        self.paint_toast(p);
        if self.settings_open && self.color_picker.is_some() {
            self.paint_color_picker(p);
        }
        self.paint_changelog(p);
    }

    /// The popover surface: a soft ambient + key shadow, a hairline border
    /// and a faint top highlight. `lift` scales the shadow (0.6 small tips,
    /// 1.0 menus, 1.6 dialogs).
    fn glass_lifted(&self, p: &mut dyn Painter, r: Rect, radius: f32, lift: f32) {
        let t = &self.theme;
        let k = if t.dark { 1.0 } else { 0.55 };
        // Key shadow first (large, soft), then a tight contact shadow.
        p.fill_shadowed(r, radius, t.overlay_glass_bg, Shadow { dx: 0.0, dy: 10.0 * lift, blur: 28.0 * lift, color: Color::rgba(0, 0, 0, 0.26 * k) });
        p.fill_shadowed(r, radius, t.overlay_glass_bg, Shadow { dx: 0.0, dy: 1.0, blur: 3.0, color: Color::rgba(0, 0, 0, 0.18 * k) });
        p.push_clip(r, radius);
        let hl = if t.dark { 0.07 } else { 0.6 };
        p.fill(Rect::new(r.x, r.y + 1.0, r.w, 1.0), 0.0, Color::rgba(255, 255, 255, hl));
        p.pop();
        p.stroke(r, radius, 1.0, t.overlay_glass_border);
    }

    fn glass(&self, p: &mut dyn Painter, r: Rect, radius: f32) {
        self.glass_lifted(p, r, radius, 1.0);
    }

    // ── Letter / tashkeel picker ─────────────────────────────────────────

    fn paint_picker(&mut self, p: &mut dyn Painter) {
        let Some(picker) = self.session.picker.clone() else { return };
        let slots = self.session.letter_slots.clone();
        if slots.is_empty() || picker.slot_indices.is_empty() {
            return;
        }
        let t = self.theme.clone();
        let has_choices = picker.has_choices(&slots);
        let chip_st = t.arabic(16.0, t.text_primary);
        let mut latin_st = t.latin(400, t.font_size_min, t.text_tertiary);
        latin_st.tracking = 0.04;
        let label = picker.label(&slots);

        // Items in reading order (right to left): label, chips, "no alternatives".
        enum Item {
            Label,
            Chip(usize),
            Empty,
        }
        let mut items: Vec<(Item, f32)> = vec![(Item::Label, p.measure(&label, &latin_st) + 10.0)];
        for (i, c) in picker.chips.iter().enumerate() {
            let w = (p.measure(&Session::display(&c.display), &chip_st) + 16.0).max(24.0);
            items.push((Item::Chip(i), w));
        }
        if !has_choices {
            items.push((Item::Empty, p.measure("no alternatives", &latin_st) + 12.0));
        }

        // Wrap into rows so the picker never runs past the window edge.
        let win_h = self.window_height();
        let max_inner = WINDOW_W - 2.0 * EDGE - 10.0;
        let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
        let mut row_w = 0.0f32;
        for (i, (_, w)) in items.iter().enumerate() {
            let need = if row_w == 0.0 { *w } else { row_w + 4.0 + w };
            if row_w > 0.0 && need > max_inner {
                rows.push(vec![i]);
                row_w = *w;
            } else {
                rows.last_mut().unwrap().push(i);
                row_w = need;
            }
        }
        let row_widths: Vec<f32> = rows
            .iter()
            .map(|r| r.iter().map(|&i| items[i].1).sum::<f32>() + 4.0 * (r.len() as f32 - 1.0))
            .collect();
        let inner_w = row_widths.iter().cloned().fold(0.0, f32::max);
        let chip_h = 26.0;
        let w = inner_w + 10.0;
        let h = rows.len() as f32 * chip_h + (rows.len() as f32 - 1.0) * 4.0 + 10.0;

        // Hang under the selected letters in the input, centred on them, so
        // the picker stays anchored to what it edits. Kept inside the window.
        let (sx0, sx1) = self.selected_letters_span(p, &picker.slot_indices);
        let left = ((sx0 + sx1) / 2.0 - w / 2.0).max(EDGE).min(WINDOW_W - w - EDGE).max(EDGE);
        let top = (INPUT_RECT.bottom() + 6.0).min(win_h - EDGE - h).max(EDGE);
        let r = Rect::new(left, top, w, h);
        self.glass(p, r, 10.0);
        self.hits.push((r, Target::Block));

        for (ri, row_items) in rows.iter().enumerate() {
            let y = r.y + 5.0 + ri as f32 * (chip_h + 4.0);
            let mut right = r.right() - 5.0;
            for &i in row_items {
                let (item, iw) = &items[i];
                let cell = Rect::new(right - iw, y, *iw, chip_h);
                match item {
                    Item::Label => {
                        let lh = t.font_size_min;
                        p.text(&label, &latin_st, cell.x + 4.0, cell.center().y - lh / 2.0, lh);
                        let what = if picker.mode == crate::picker::Mode::Tashkeel { "Tashkeel for" } else { "Letters for" };
                        self.ax_text.push((cell, format!("{what} {label}{}", if has_choices { "" } else { ", no alternatives" })));
                    }
                    Item::Chip(ci) => {
                        let selected = has_choices && *ci == picker.alt_idx;
                        if !has_choices {
                            p.push_alpha(0.45);
                        }
                        let color = if selected {
                            p.fill(cell.outset(1.0), 7.0, t.accent_fill);
                            t.on_accent
                        } else {
                            if has_choices && self.hovered(Target::PickerChip(*ci)) {
                                p.fill(cell, 6.0, t.item_hover);
                            }
                            t.text_primary
                        };
                        let st = TextStyle { color, ..chip_st };
                        let text = Session::display(&picker.chips[*ci].display);
                        paint::text_center(p, &text, &st, cell.center().x, cell.center().y - 8.0, 16.0);
                        if !has_choices {
                            p.pop();
                        } else {
                            self.hits.push((cell, Target::PickerChip(*ci)));
                        }
                    }
                    Item::Empty => {
                        let lh = t.font_size_min;
                        p.text("no alternatives", &latin_st, cell.x + 6.0, cell.center().y - lh / 2.0, lh);
                    }
                }
                right -= iw + 4.0;
            }
        }
    }

    /// Horizontal extent (window x) of the given letter slots as drawn in
    /// the input field.
    fn selected_letters_span(&self, p: &mut dyn Painter, slot_indices: &[usize]) -> (f32, f32) {
        let t = &self.theme;
        let st = t.latin(500, t.input_size, t.text_primary);
        let text: Vec<char> = self.field.text.chars().collect();
        let leading = text.iter().take_while(|c| c.is_whitespace()).count();
        let slots = &self.session.letter_slots;
        let start = slot_indices.iter().filter_map(|&i| slots.get(i)).map(|s| s.pos).min().unwrap_or(0) + leading;
        let end = slot_indices.iter().filter_map(|&i| slots.get(i)).map(|s| s.pos + s.pattern.chars().count()).max().unwrap_or(0) + leading;
        let width_to = |p: &mut dyn Painter, n: usize| p.measure(&text[..n.min(text.len())].iter().collect::<String>(), &st);
        let x0 = INPUT_RECT.x + width_to(p, start);
        let x1 = INPUT_RECT.x + width_to(p, end);
        // A long line scrolls inside the field; keep the anchor in view.
        let max = INPUT_RECT.right();
        (x0.min(max), x1.min(max))
    }

    // ── Cmd+G tip ────────────────────────────────────────────────────────

    fn paint_cmdg_tip(&mut self, p: &mut dyn Painter) {
        let Some(anchor) = self.cmdg_tip else { return };
        let a = self.cmdg_anim.value(self.now);
        if a <= 0.0 {
            if !self.cmdg_anim.running(self.now) {
                self.cmdg_tip = None;
            }
            return;
        }
        let t = self.theme.clone();
        let st = t.latin(400, t.fs_caption, t.text_primary);
        let kst = t.latin(500, t.fs_caption, t.text_primary);
        let key = if self.is_mac { "⌘" } else { "Ctrl" };
        let kw = |p: &mut dyn Painter, s: &str| p.measure(s, &kst) + 12.0;
        let w_text = p.measure("Press ", &st) + kw(p, key) + p.measure("+", &st) + kw(p, "G") + p.measure(" to swap a letter", &st);
        let x_w = p.measure("×", &t.latin(400, t.font_size_min, t.text_primary));
        let w = 20.0 + w_text + 8.0 + 2.0 + x_w + 2.0;
        let lh = t.fs_caption * 1.3;
        let h = 12.0 + (t.fs_caption + 8.0).max(lh) + 2.0;
        let mut left = anchor.x - w - 8.0;
        if left < EDGE {
            left = anchor.right() + 8.0;
        }
        let top = (anchor.y + (anchor.h - h) / 2.0).clamp(EDGE, self.window_height() - h - EDGE);
        let r = Rect::new(left, top + (1.0 - a) * 2.0, w, h);
        p.push_alpha(a);
        self.glass_lifted(p, r, 10.0, 0.6);
        let cy = r.center().y;
        let mut x = r.x + 11.0;
        p.text("Press ", &st, x, cy - lh / 2.0, lh);
        self.ax_text.push((r, format!("Tip: press {}+G to swap a letter", if self.is_mac { "Command" } else { "Control" })));
        x += p.measure("Press ", &st);
        x += kbd(p, x, cy, KbdLabel::Text(if self.is_mac { "⌘" } else { "Ctrl" }), &kst, t.kbd_bg, 6.0, t.fs_caption + 8.0, 0.0);
        p.text("+", &st, x, cy - lh / 2.0, lh);
        x += p.measure("+", &st);
        x += kbd(p, x, cy, KbdLabel::Text("G"), &kst, t.kbd_bg, 6.0, t.fs_caption + 8.0, 0.0);
        p.text(" to swap a letter", &st, x, cy - lh / 2.0, lh);
        x += p.measure(" to swap a letter", &st) + 8.0 + 2.0;
        let close = Rect::new(x - 2.0, r.y, x_w + 6.0, r.h);
        let xa = if self.hovered(Target::CmdgTipDismiss) { 1.0 } else { 0.55 };
        let xst = t.latin(400, t.font_size_min, t.text_primary.with_alpha_mul(xa));
        p.text("×", &xst, x, cy - t.font_size_min * 0.65, t.font_size_min * 1.3);
        p.pop();
        self.hits.push((r, Target::CmdgTip));
        self.hits.push((close, Target::CmdgTipDismiss));
    }

    // ── Shortcuts tip (text selected in the input) ───────────────────────

    fn paint_shortcuts_tip(&mut self, p: &mut dyn Painter) {
        let a = self.shortcuts_anim.value(self.now);
        if a <= 0.0 {
            return;
        }
        let t = self.theme.clone();
        let label = t.latin(400, t.font_size_min, t.text_secondary);
        let kst = t.latin(500, t.fs_caption, t.text_primary);
        let m = if self.is_mac { KbdLabel::Icon(&icons::MS_KEYBOARD_COMMAND_KEY) } else { KbdLabel::Text("Ctrl") };
        let kw = |p: &mut dyn Painter, k: KbdLabel| match k {
            KbdLabel::Text(s) => p.measure(s, &kst) + 16.0,
            KbdLabel::Icon(_) => t.font_size_min + 16.0,
        };
        let keys_w = kw(p, m) + 4.0 + kw(p, KbdLabel::Text("G"));
        let labels_w = p.measure("Swap a letter", &label).max(p.measure("Add tashkeel", &label));
        let w = 28.0 + keys_w + 10.0 + labels_w + 10.0 + 24.0 + 2.0;
        let h = 20.0 + 24.0 * 2.0 + 8.0 + 2.0;
        let anchor = if self.session.is_typing() {
            Rect::new(0.0, HEADER_H, WINDOW_W, CONTENT_BOTTOM - HEADER_H)
        } else {
            Rect::new(0.0, 0.0, WINDOW_W, self.window_height() - 56.0)
        };
        let top = (anchor.bottom() - h - 8.0).clamp(EDGE, self.window_height() - h - EDGE);
        let r = Rect::new(anchor.center().x - w / 2.0, top + (1.0 - a) * 4.0, w, h);
        p.push_alpha(a);
        // `.shortcuts-tip` uses the glass fill with a softer shadow.
        self.glass_lifted(p, r, 10.0, 0.8);
        let lh = t.font_size_min * 1.4;
        let m_name = if self.is_mac { "Command" } else { "Control" };
        self.ax_text.push((r, format!("Shortcuts: {m_name} G swaps a letter, {m_name} T adds tashkeel")));
        for (i, (k, text)) in [(KbdLabel::Text("G"), "Swap a letter"), (KbdLabel::Text("T"), "Add tashkeel")].into_iter().enumerate() {
            let cy = r.y + 11.0 + 12.0 + i as f32 * 32.0;
            let mut x = r.x + 15.0;
            x += kbd(p, x, cy, m, &kst, t.kbd_bg, 8.0, 24.0, t.font_size_min) + 4.0;
            kbd(p, x, cy, k, &kst, t.kbd_bg, 8.0, 24.0, t.font_size_min);
            p.text(text, &label, r.x + 15.0 + keys_w + 10.0, cy - lh / 2.0, lh);
        }
        for (i, (icon, target)) in [(&icons::MS_CLOSE, Target::ShortcutsTipDismiss), (&icons::MS_OPEN_IN_NEW, Target::ShortcutsTipMore)]
            .into_iter()
            .enumerate()
        {
            let b = Rect::new(r.right() - 15.0 - 24.0, r.y + 11.0 + i as f32 * 32.0, 24.0, 24.0);
            let hovered = self.hovered(target);
            if hovered {
                p.fill(b, 6.0, t.item_hover);
            }
            let c = t.text_secondary.with_alpha_mul(if hovered { 1.0 } else { 0.7 });
            paint::glyph(p, icon, b.x + 4.0, b.y + 4.0, 16.0, c);
            self.hits.push((b, target));
        }
        p.pop();
    }

    // ── Toast ────────────────────────────────────────────────────────────

    fn paint_toast(&mut self, p: &mut dyn Painter) {
        let Some(toast) = &self.toast else { return };
        let a = toast.anim.value(self.now);
        if a <= 0.0 {
            return;
        }
        let parts = toast.html.clone();
        let spoken: String = parts.iter().map(|p| match p { ToastPart::Text(s) | ToastPart::Kbd(s) => *s }).collect::<String>().replace('⌫', "Delete");
        let t = self.theme.clone();
        let st = t.latin(400, t.font_size_min, t.text_secondary);
        let kst = t.latin(500, t.fs_caption, t.icon_fill);
        let mut w = 24.0;
        for part in &parts {
            w += match part {
                ToastPart::Text(s) => p.measure(s, &st),
                ToastPart::Kbd(s) => (p.measure(s, &kst) + 12.0).max(24.0) + 4.0,
            };
        }
        let h = 12.0 + 24.0;
        let r = Rect::new(WINDOW_W / 2.0 - w / 2.0, self.window_height() - 56.0 - h + (1.0 - a) * 8.0, w, h);
        p.push_alpha(a);
        self.glass_lifted(p, r, 8.0, 0.6);
        self.ax_text.push((r, spoken));
        let cy = r.center().y;
        let mut x = r.x + 12.0;
        let lh = t.font_size_min * 1.3;
        for part in &parts {
            match part {
                ToastPart::Text(s) => {
                    p.text(s, &st, x, cy - lh / 2.0, lh);
                    x += p.measure(s, &st);
                }
                ToastPart::Kbd(s) => {
                    let kw = (p.measure(s, &kst) + 12.0).max(24.0);
                    let k = Rect::new(x + 2.0, cy - 12.0, kw, 24.0);
                    p.fill(k, 5.0, t.kbd_bg);
                    paint::text_center(p, s, &kst, k.center().x, cy - t.fs_caption / 2.0, t.fs_caption);
                    x += kw + 4.0;
                }
            }
        }
        p.pop();
    }

    // ── Update banner ────────────────────────────────────────────────────

    fn paint_banner(&mut self, p: &mut dyn Painter) {
        let a = self.banner.value(self.now);
        if a <= 0.0 {
            return;
        }
        let t = self.theme.clone();
        let (sub, later, action, progress): (String, Option<&str>, &str, Option<Option<f32>>) = match &self.update {
            UpdateState::Available { version } => (format!("Version {version} is available"), Some("Later"), "Install", None),
            UpdateState::Downloading { version, progress } => (format!("Downloading v{version}…"), Some("Later"), "Install", Some(*progress)),
            UpdateState::Installed { version } => (format!("Installed v{version} — restart to apply"), Some("Later"), "Restart", None),
            UpdateState::Failed => ("Update failed — try again later".into(), Some("Later"), "Dismiss", None),
            UpdateState::Idle => return,
        };
        let busy = matches!(self.update, UpdateState::Downloading { .. });
        let lh = t.font_size_min * 1.3;
        let h = 20.0 + lh * 2.0 + 1.0;
        let full = Rect::new(8.0, self.window_height() - 8.0 - h, WINDOW_W - 16.0, h);
        let r = full.translate(0.0, (1.0 - a) * 1.2 * h);
        p.push_alpha(a.min(1.0));
        self.glass_lifted(p, r, 10.0, 0.8);
        self.hits.push((r, Target::Block));
        self.ax_text.push((Rect::new(r.x, r.y, r.w * 0.5, r.h), format!("Update available. {sub}")));

        let btn_st = |c: Color| t.latin(600, t.font_size_min, c);
        let mut right = r.right() - 12.0;
        let mut btn = |p: &mut dyn Painter, label: &str, primary: bool, target: Target, hits: &mut Vec<(Rect, Target)>, hovered: bool| -> Rect {
            let st = btn_st(if primary { t.on_accent } else { t.text_primary });
            let w = p.measure(label, &st) + 22.0;
            let b = Rect::new(right - w, r.center().y - (lh + 12.0) / 2.0, w, lh + 12.0);
            if busy {
                p.push_alpha(0.6);
            }
            if primary {
                let fill = if hovered && !busy { t.accent_fill.lerp(Color::WHITE, 0.1) } else { t.accent_fill };
                p.fill(b, 6.0, fill);
                p.stroke(b, 6.0, 1.0, t.accent_fill);
            } else {
                if hovered && !busy {
                    p.fill(b, 6.0, t.item_hover);
                }
                p.stroke(b, 6.0, 1.0, t.outline.with_alpha_mul(0.55));
            }
            p.text(label, &st, b.x + 11.0, b.y + 6.0, lh);
            if busy {
                p.pop();
            } else {
                hits.push((b, target));
            }
            right = b.x - 10.0;
            b
        };
        let ha = self.hovered(Target::BannerAction);
        let hl = self.hovered(Target::BannerLater);
        let mut hits = Vec::new();
        btn(p, action, true, Target::BannerAction, &mut hits, ha);
        if let Some(l) = later {
            btn(p, l, false, Target::BannerLater, &mut hits, hl);
        }
        self.hits.extend(hits);
        let text_right = right;
        let x = r.x + 12.0;
        match progress {
            Some(prog) => {
                // Text column then a flexible progress bar.
                let title = t.latin(600, t.font_size_min, t.text_primary);
                let tw = p.measure(&sub, &t.latin(400, t.font_size_min, t.text_secondary)).max(p.measure("Update available", &title));
                p.text("Update available", &title, x, r.y + 10.0, lh);
                p.text(&sub, &t.latin(400, t.font_size_min, t.text_secondary), x, r.y + 10.0 + lh + 1.0, lh);
                let bar = Rect::new(x + tw + 10.0, r.center().y - 2.0, (text_right - (x + tw + 10.0)).max(20.0), 4.0);
                p.fill(bar, 2.0, t.kbd_bg);
                let fw = bar.w * prog.unwrap_or(0.0).clamp(0.0, 1.0);
                if fw > 0.0 {
                    p.push_clip(bar, 2.0);
                    p.fill(Rect::new(bar.x, bar.y, fw, 4.0), 0.0, t.accent);
                    p.pop();
                }
            }
            None => {
                p.text("Update available", &t.latin(600, t.font_size_min, t.text_primary), x, r.y + 10.0, lh);
                p.text(&sub, &t.latin(400, t.font_size_min, t.text_secondary), x, r.y + 10.0 + lh + 1.0, lh);
            }
        }
        p.pop();
    }

    // ── Colour picker ────────────────────────────────────────────────────

    pub(super) fn hex_rect(&self) -> Rect {
        let (panel, _) = self.color_picker_layout();
        Rect::new(panel.x + 13.0 + 26.0 + 8.0, panel.y + 13.0 + 120.0 + 8.0 + 12.0 + 8.0, 224.0 - 26.0 - 26.0 - 8.0, 26.0)
    }

    fn color_picker_layout(&self) -> (Rect, bool) {
        let recent = !self.settings.recent_colors.is_empty();
        let label_h = self.theme.font_size_min * 1.37;
        let h = 2.0 + 24.0 + 120.0 + 8.0 + 12.0 + 8.0 + 26.0 + if recent { 8.0 + label_h + 5.0 + 18.0 } else { 0.0 };
        let w = 224.0;
        let wh = self.window_height();
        (Rect::new(WINDOW_W / 2.0 - w / 2.0, wh / 2.0 - h / 2.0, w, h), recent)
    }

    fn paint_color_picker(&mut self, p: &mut dyn Painter) {
        let t = self.theme.clone();
        let wh = self.window_height();
        let backdrop = Rect::new(0.0, 0.0, WINDOW_W, wh);
        p.fill(backdrop, t.radius, Color::rgba(0, 0, 0, 0.4));
        self.hits.push((backdrop, Target::CpBackdrop));
        let (panel, recent) = self.color_picker_layout();
        self.glass_lifted(p, panel, 10.0, 1.3);
        self.hits.push((panel, Target::CpPanel));
        let cp = self.color_picker.as_ref().unwrap();
        let (h, s, v) = (cp.h, cp.s, cp.v);

        let x = panel.x + 13.0;
        let w = panel.w - 26.0;
        let grad = Rect::new(x, panel.y + 13.0, w, 120.0);
        p.fill(grad, 6.0, Color::from_rgb(color::hsv_to_rgb(h, 100, 100)));
        p.gradient(grad, 4.0, Point { x: grad.x, y: grad.y }, Point { x: grad.right(), y: grad.y }, &[(0.0, Color::WHITE), (1.0, Color::WHITE.with_alpha(0.0))]);
        p.gradient(grad, 4.0, Point { x: grad.x, y: grad.y }, Point { x: grad.x, y: grad.bottom() }, &[(0.0, Color::BLACK.with_alpha(0.0)), (1.0, Color::BLACK)]);
        let hp = Point { x: grad.x + grad.w * s as f32 / 100.0, y: grad.y + grad.h * (1.0 - v as f32 / 100.0) };
        let handle = Rect::new(hp.x - 6.0, hp.y - 6.0, 12.0, 12.0);
        p.push_clip(grad, 4.0);
        p.stroke_ellipse(handle.outset(1.0), 1.0, Color::rgba(0, 0, 0, 0.5));
        p.stroke_ellipse(handle, 2.0, Color::WHITE);
        p.pop();

        let hue = Rect::new(x, grad.bottom() + 8.0, w, 12.0);
        let stops: Vec<(f32, Color)> = [(0.0, "#f00"), (0.17, "#ff0"), (0.33, "#0f0"), (0.5, "#0ff"), (0.67, "#00f"), (0.83, "#f0f"), (1.0, "#f00")]
            .iter()
            .map(|(o, c)| {
                let c = c.trim_start_matches('#');
                let d = |i: usize| u8::from_str_radix(&c[i..i + 1].repeat(2), 16).unwrap();
                (*o, Color::rgb(d(0), d(1), d(2)))
            })
            .collect();
        p.gradient(hue, 6.0, Point { x: hue.x, y: hue.y }, Point { x: hue.right(), y: hue.y }, &stops);
        let tx = hue.x + hue.w * h as f32 / 360.0;
        let thumb = Rect::new(tx - 7.0, hue.center().y - 7.0, 14.0, 14.0);
        p.stroke_ellipse(thumb.outset(1.0), 1.0, Color::rgba(0, 0, 0, 0.4));
        p.stroke_ellipse(thumb, 2.0, Color::WHITE);

        let row_y = hue.bottom() + 8.0;
        let prev = Rect::new(x, row_y, 26.0, 26.0);
        p.fill_ellipse(prev, Color::from_rgb(color::hsv_to_rgb(h, s, v)));
        p.stroke_ellipse(prev, 2.0, t.outline.with_alpha_mul(0.6));
        let hex = self.hex_rect();
        p.fill(hex, 6.0, t.kbd_bg);
        p.stroke(hex, 6.0, 1.0, t.outline.with_alpha_mul(0.6));

        if let Some(cp) = &mut self.color_picker {
            cp.gradient = grad;
            cp.hue = hue;
        }
        self.hits.push((grad, Target::CpGradient));
        self.hits.push((hue.outset(2.0), Target::CpHue));

        if recent {
            let mut st = t.latin(600, t.font_size_min, t.text_tertiary);
            st.tracking = 0.5 / t.font_size_min;
            let lh = p.normal_line_height(&st);
            let ly = row_y + 26.0 + 8.0;
            p.text("RECENT", &st, x, ly, lh);
            let colors = self.settings.recent_colors.clone();
            for (i, c) in colors.iter().enumerate() {
                let r = Rect::new(x + i as f32 * 23.0, ly + lh + 5.0, 18.0, 18.0);
                let hovered = self.hovered(Target::CpRecent(i));
                let scale = if hovered { 1.2 } else { 1.0 };
                let cc = r.center();
                p.push_transform([scale, 0.0, 0.0, scale, cc.x * (1.0 - scale), cc.y * (1.0 - scale)]);
                p.fill_ellipse(r, Color::hex(c));
                if hovered {
                    p.stroke_ellipse(r.outset(2.0), 2.0, t.text_primary);
                }
                p.pop();
                self.hits.push((r, Target::CpRecent(i)));
            }
        }
    }

    // ── What's new ───────────────────────────────────────────────────────

    fn paint_changelog(&mut self, p: &mut dyn Painter) {
        let Some(cl) = &self.changelog else { return };
        let (version_line, title, items) = (cl.version_line.clone(), cl.title.clone(), cl.items.clone());
        let t = self.theme.clone();
        let wh = self.window_height();
        let backdrop = Rect::new(0.0, 0.0, WINDOW_W, wh);
        p.fill(backdrop, t.radius, Color::rgba(0, 0, 0, 0.45));
        self.hits.push((backdrop, Target::Block));

        let w = 380.0f32.min(WINDOW_W - 32.0);
        let inner_w = w - 40.0 - 2.0;
        let body = t.latin(400, t.fs_caption, t.text_primary);
        let bold = t.latin(700, t.fs_caption, t.text_primary);
        let blh = t.fs_caption * 1.5;
        // Lay out the body: optional bold title, then bulleted items.
        let mut lines: Vec<(f32, Vec<(String, bool)>)> = Vec::new();
        if let Some(title) = &title {
            for l in wrap_rich(p, &[(title.clone(), true)], &body, &bold, inner_w) {
                lines.push((0.0, l));
            }
            lines.push((0.0, vec![]));
        }
        for item in &items {
            let runs = parse_inline_html(item);
            for (i, l) in wrap_rich(p, &runs, &body, &bold, inner_w - 18.0).into_iter().enumerate() {
                lines.push((if i == 0 { -1.0 } else { 18.0 }, l));
            }
        }
        let h2 = t.latin(600, t.fs_title, t.text_primary);
        let h2_lh = t.fs_title * 1.25;
        let max_h = wh * 0.8;
        let fixed = 2.0 + 18.0 + h2_lh + 10.0 + t.fs_caption + 10.0 + 10.0 + 6.0 + t.fs_caption + 12.0 + 14.0;
        let body_h = (lines.len() as f32 * blh).min(max_h - fixed);
        let h = fixed + body_h;
        let r = Rect::new(WINDOW_W / 2.0 - w / 2.0, wh / 2.0 - h / 2.0, w, h);
        self.glass_lifted(p, r, 12.0, 1.6);
        self.hits.push((r, Target::ChangelogPanel));
        let x = r.x + 21.0;
        let mut y = r.y + 19.0;
        p.text("What's new", &h2, x, y, h2_lh);
        let mut spoken = format!("What's new, {version_line}. ");
        if let Some(t) = &title {
            spoken.push_str(t);
            spoken.push_str(". ");
        }
        for item in &items {
            let plain: String = parse_inline_html(item).into_iter().map(|(s, _)| s).collect();
            spoken.push_str(&plain);
            spoken.push(' ');
        }
        self.ax_text.push((Rect::new(x, y, inner_w, h - 60.0), spoken));
        y += h2_lh + 10.0;
        p.text(&version_line, &t.latin(400, t.fs_caption, t.text_secondary), x, y, t.fs_caption);
        y += t.fs_caption + 10.0;
        let body_rect = Rect::new(x, y, inner_w, body_h);
        let sc = &mut self.scroll_changelog;
        sc.viewport = body_rect;
        sc.content = lines.len() as f32 * blh;
        sc.clamp();
        let (ft, fb) = sc.fades();
        y -= sc.offset;
        p.push_clip(body_rect, 0.0);
        p.push_fade(body_rect, if ft { 24.0 } else { 0.0 }, if fb { 24.0 } else { 0.0 });
        for (indent, runs) in &lines {
            let mut lx = x + indent.max(0.0);
            if *indent < 0.0 {
                p.text("•", &body, x + 4.0, y, blh);
                lx = x + 18.0;
            }
            for (s, b) in runs {
                let st = if *b { &bold } else { &body };
                p.text(s, st, lx, y, blh);
                lx += p.measure(s, st);
            }
            y += blh;
        }
        p.pop();
        p.pop();
        y = r.y + 19.0 + h2_lh + 10.0 + t.fs_caption + 10.0 + body_h + 10.0 + 6.0;
        let st = t.latin(600, t.fs_caption, t.on_accent);
        let bw = p.measure("Got it", &st) + 28.0;
        let b = Rect::new(r.right() - 21.0 - bw, y, bw, t.fs_caption + 12.0);
        p.fill(b, 6.0, t.accent_fill);
        p.text("Got it", &st, b.x + 14.0, b.y + 6.0, t.fs_caption);
        self.hits.push((b, Target::ChangelogClose));
    }
}

/// Splits the changelog's tiny HTML subset (`<strong>`, `<em>`) into runs of
/// (text, bold). Other tags are dropped and entities decoded.
pub fn parse_inline_html(s: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut bold = false;
    let mut cur = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        cur.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('>') else { break };
        let tag = rest[i + 1..i + j].trim().to_ascii_lowercase();
        let b = match tag.as_str() {
            "strong" | "b" => Some(true),
            "/strong" | "/b" => Some(false),
            _ => None,
        };
        if let Some(b) = b {
            if !cur.is_empty() {
                out.push((decode_entities(&cur), bold));
                cur.clear();
            }
            bold = b;
        }
        rest = &rest[i + j + 1..];
    }
    cur.push_str(rest);
    if !cur.is_empty() {
        out.push((decode_entities(&cur), bold));
    }
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

/// Word-wraps styled runs into lines of runs.
fn wrap_rich(p: &mut dyn Painter, runs: &[(String, bool)], body: &TextStyle, bold: &TextStyle, max: f32) -> Vec<Vec<(String, bool)>> {
    let mut lines: Vec<Vec<(String, bool)>> = vec![Vec::new()];
    let mut width = 0.0f32;
    for (text, b) in runs {
        let st = if *b { bold } else { body };
        for (i, word) in text.split(' ').enumerate() {
            let piece = if i == 0 { word.to_string() } else { format!(" {word}") };
            let w = p.measure(&piece, st);
            if width > 0.0 && width + w > max && i > 0 {
                lines.push(Vec::new());
                width = 0.0;
                let w2 = p.measure(word, st);
                lines.last_mut().unwrap().push((word.to_string(), *b));
                width += w2;
                continue;
            }
            let line = lines.last_mut().unwrap();
            match line.last_mut() {
                Some((s, lb)) if *lb == *b => s.push_str(&piece),
                _ => line.push((piece, *b)),
            }
            width += w;
        }
    }
    lines
}
