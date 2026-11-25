//! The first-run welcome window (480×380): logo, greeting, and the launch
//! shortcut keys that light up as the user holds them.

use super::anim::{Anim, Ease};
use super::geom::{Color, Rect};
use super::paint::{self, Painter, Shadow};
use super::theme::{Family, TextStyle};

pub const W: f32 = 480.0;
pub const H: f32 = 380.0;

pub struct Onboarding {
    pub dark: bool,
    pub is_mac: bool,
    pub accent: Color,
    start: f64,
    now: f64,
    held_mod: bool,
    held_shift: bool,
    held_a: bool,
    success_at: Option<f64>,
    hover_button: bool,
    button: Rect,
    pub dismissed: bool,
}

/// Reveal delays (s) from the stylesheet, in drawing order.
const DELAYS: [f64; 7] = [0.08, 0.26, 0.46, 0.62, 0.76, 0.90, 1.05];
const BUTTON_DELAY: f64 = 1.22;

impl Onboarding {
    pub fn new(dark: bool, is_mac: bool, accent_hex: &str, now: f64) -> Onboarding {
        Onboarding {
            dark,
            is_mac,
            accent: Color::hex(accent_hex),
            start: now,
            now,
            held_mod: false,
            held_shift: false,
            held_a: false,
            success_at: None,
            hover_button: false,
            button: Rect::default(),
            dismissed: false,
        }
    }

    pub fn animating(&self) -> bool {
        self.now - self.start < BUTTON_DELAY + 0.6 || self.success_at.is_some()
    }

    /// Keys currently held (from flags-changed / key events in the window).
    pub fn keys(&mut self, modifier: bool, shift: bool, a: bool) {
        if self.success_at.is_some() {
            return;
        }
        self.held_mod = modifier;
        self.held_shift = shift;
        self.held_a = modifier && shift && a;
        if self.held_a {
            self.success_at = Some(self.now);
        }
    }

    /// The global shortcut fired (the OS may swallow the keys first).
    pub fn shortcut_used(&mut self) {
        if self.success_at.is_none() {
            self.held_mod = true;
            self.held_shift = true;
            self.held_a = true;
            self.success_at = Some(self.now + 0.13);
        }
    }

    /// Advances time; returns true once the window should close.
    pub fn tick(&mut self, now: f64) -> bool {
        self.now = now;
        if let Some(t) = self.success_at {
            if now >= t + 0.85 {
                self.dismissed = true;
            }
        }
        self.dismissed
    }

    pub fn mouse_moved(&mut self, x: f32, y: f32) {
        self.hover_button = self.button.contains(super::geom::Point { x, y });
    }

    pub fn click(&mut self, x: f32, y: f32) {
        if self.button.contains(super::geom::Point { x, y }) {
            self.dismissed = true;
        }
    }

    fn reveal(&self, delay: f64) -> (f32, f32) {
        let mut a = Anim::fixed(0.0).with_ease(Ease::OutExpo);
        a.start_from(0.0, 1.0, self.start + delay, 0.55);
        let v = a.value(self.now);
        (v, (1.0 - v) * 10.0)
    }

    pub fn paint(&mut self, p: &mut dyn Painter) {
        let dark = self.dark;
        let text = if dark { Color::WHITE } else { Color::hex("#1a1a1a") };
        let muted = if dark { Color::rgba(255, 255, 255, 0.6) } else { Color::rgba(0, 0, 0, 0.6) };
        let key_bg = if dark { Color::rgba(255, 255, 255, 0.1) } else { Color::rgba(0, 0, 0, 0.06) };
        let bg = if dark { Color::rgba(33, 33, 33, 0.6) } else { Color::rgba(245, 245, 245, 0.6) };
        let border = if dark { Color::rgba(113, 113, 113, 0.25) } else { Color::rgba(192, 192, 192, 0.25) };
        let btn_bg = if dark { Color::rgba(255, 255, 255, 0.08) } else { Color::rgba(0, 0, 0, 0.05) };
        let btn_hover = if dark { Color::rgba(255, 255, 255, 0.14) } else { Color::rgba(0, 0, 0, 0.09) };
        let btn_border = if dark { Color::rgba(113, 113, 113, 0.8) } else { Color::rgba(192, 192, 192, 0.9) };
        let st = |w: u16, size: f32, c: Color| TextStyle { family: Family::Latin, weight: w, size, color: c, tracking: 0.0 };

        let panel = Rect::new(0.0, 0.0, W, H);
        p.fill(panel, 8.0, bg);
        p.stroke(panel, 8.0, 1.0, border);

        // Column: logo+welcome (gap 8) | 42 | keys section (gap 12) | 42 | button.
        let logo_h = paint::logo_height(48.0);
        let welcome = st(600, 20.0, text);
        let instr = st(400, 16.0, text);
        let hint = st(400, 11.0 * 96.0 / 72.0, muted);
        let hint_lh = hint.size * 1.3;
        let btn_st = st(500, 11.0 * 96.0 / 72.0, text);
        let btn_lh = p.normal_line_height(&btn_st);
        let btn_h = btn_lh + 14.0 + 2.0;
        let keys_h = 44.0;
        let total = logo_h + 8.0 + 26.0 + 42.0 + 20.0 + 12.0 + keys_h + 12.0 + hint_lh + 42.0 + btn_h;
        // padding 20 top / 32 bottom, centred.
        let mut y = 20.0 + ((H - 52.0) - total) / 2.0;

        let (a, dy) = self.reveal(DELAYS[0]);
        p.push_alpha(a);
        let lc = if dark { Color::WHITE } else { Color::hex("#1a1a1a") };
        paint::logo(p, W / 2.0 - 24.0, y + dy, 48.0, lc);
        p.pop();
        y += logo_h + 8.0;

        let (a, dy) = self.reveal(DELAYS[1]);
        p.push_alpha(a);
        paint::text_center(p, "Welcome to Franconvert 👋", &welcome, W / 2.0, y + dy, 26.0);
        p.pop();
        y += 26.0 + 42.0;

        let (a, dy) = self.reveal(DELAYS[2]);
        p.push_alpha(a);
        paint::text_center(p, "Launch the widget by holding", &instr, W / 2.0, y + dy, 20.0);
        p.pop();
        y += 20.0 + 12.0;

        let key_st = st(500, 28.0, text);
        let labels = [if self.is_mac { "CMD" } else { "CTRL" }, "SHIFT", "A"];
        let held = [self.held_mod, self.held_shift, self.held_a];
        let widths: Vec<f32> = labels.iter().map(|l| p.measure(l, &key_st) + 28.0).collect();
        let row_w = widths.iter().sum::<f32>() + 16.0;
        let mut x = W / 2.0 - row_w / 2.0;
        let success = self.success_at.is_some_and(|t| self.now >= t);
        let green = Color::hex("#22c55e");
        for i in 0..3 {
            let (a, dy) = self.reveal(DELAYS[3 + i]);
            let r = Rect::new(x, y + dy, widths[i], keys_h);
            let (fill, color, scale, glow) = if success {
                (green, Color::WHITE, 1.02, Some(green.with_alpha(0.45)))
            } else if held[i] {
                (self.accent, Color::WHITE, 0.95, Some(self.accent.with_alpha(0.45)))
            } else {
                (key_bg, text, 1.0, None)
            };
            let c = r.center();
            p.push_alpha(a);
            p.push_transform([scale, 0.0, 0.0, scale, c.x * (1.0 - scale), c.y * (1.0 - scale)]);
            match glow {
                Some(g) => p.fill_shadowed(r, 4.0, fill, Shadow { dx: 0.0, dy: 2.0, blur: 16.0, color: g }),
                None => p.fill(r, 4.0, fill),
            }
            paint::text_center(p, labels[i], &TextStyle { color, ..key_st }, c.x, r.y + 2.0, 40.0);
            p.pop();
            p.pop();
            x += widths[i] + 8.0;
        }
        y += keys_h + 12.0;

        let (a, dy) = self.reveal(DELAYS[6]);
        p.push_alpha(a);
        paint::text_center(p, "You can change this later in settings", &hint, W / 2.0, y + dy, hint_lh);
        p.pop();
        y += hint_lh + 42.0;

        let (a, dy) = self.reveal(BUTTON_DELAY);
        let bw = p.measure("Got it", &btn_st) + 48.0 + 2.0;
        let b = Rect::new(W / 2.0 - bw / 2.0, y + dy, bw, btn_h);
        p.push_alpha(a);
        p.fill(b, 6.0, if self.hover_button { btn_hover } else { btn_bg });
        p.stroke(b, 6.0, 1.0, btn_border);
        paint::text_center(p, "Got it", &btn_st, W / 2.0, b.y + 8.0, btn_lh);
        p.pop();
        self.button = b;
    }
}
