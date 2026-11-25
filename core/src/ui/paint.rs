//! The drawing interface each platform implements with its native 2D API
//! (CoreGraphics + CoreText on macOS), plus helpers built on it.

use super::geom::{Color, Point, Rect};
use super::icons::{IconDef, LOGO_PARTS, LOGO_VIEW_BOX};
use super::path::{Cmd, Path};
use super::theme::TextStyle;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub color: Color,
}

/// Affine transform `[a, b, c, d, e, f]` (x' = a·x + c·y + e, y' = b·x + d·y + f).
pub type Affine = [f32; 6];

pub trait Painter {
    /// Fills a rounded rectangle.
    fn fill(&mut self, r: Rect, radius: f32, color: Color);
    /// Strokes a rounded rectangle; the stroke lies inside `r` (CSS border).
    fn stroke(&mut self, r: Rect, radius: f32, width: f32, color: Color);
    /// Fills a rounded rectangle casting a drop shadow.
    fn fill_shadowed(&mut self, r: Rect, radius: f32, color: Color, shadow: Shadow);
    /// Fills an ellipse inscribed in `r`.
    fn fill_ellipse(&mut self, r: Rect, color: Color);
    fn stroke_ellipse(&mut self, r: Rect, width: f32, color: Color);
    /// Fills a path transformed by `t`.
    fn fill_path(&mut self, path: &Path, t: Affine, color: Color);
    /// Linear gradient from `from` to `to` across a rounded rect.
    fn gradient(&mut self, r: Rect, radius: f32, from: Point, to: Point, stops: &[(f32, Color)]);

    /// Draws one line of text whose CSS line box starts at (`x`, `top`) and is
    /// `line_height` tall; the glyphs sit where CSS half-leading puts them.
    fn text(&mut self, s: &str, style: &TextStyle, x: f32, top: f32, line_height: f32);
    /// Advance width of a single line of text.
    fn measure(&mut self, s: &str, style: &TextStyle) -> f32;
    /// CSS `line-height: normal` for this font.
    fn normal_line_height(&mut self, style: &TextStyle) -> f32;
    /// Font ascent and descent (both positive).
    fn metrics(&mut self, style: &TextStyle) -> (f32, f32);

    fn push_clip(&mut self, r: Rect, radius: f32);
    fn push_alpha(&mut self, a: f32);
    fn push_transform(&mut self, t: Affine);
    /// Fades content to transparent over `top` px at the top and `bottom` px
    /// at the bottom of `r` (scroll-edge fades).
    fn push_fade(&mut self, r: Rect, top: f32, bottom: f32);
    /// Undo the matching `push_*`.
    fn pop(&mut self);
}

// ── Helpers ──────────────────────────────────────────────────────────────

thread_local! {
    static PATHS: RefCell<HashMap<&'static str, Path>> = RefCell::new(HashMap::new());
}

fn with_path<R>(d: &'static str, f: impl FnOnce(&Path) -> R) -> R {
    PATHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let path = cache.entry(d).or_insert_with(|| Path::parse(d));
        f(path)
    })
}

/// Draws an icon scaled to fit `r` (like an `<svg>` with that box).
pub fn icon(p: &mut dyn Painter, def: &IconDef, r: Rect, color: Color) {
    let [vx, vy, vw, vh] = def.view_box;
    let s = (r.w / vw).min(r.h / vh);
    let ox = r.x + (r.w - vw * s) / 2.0;
    let oy = r.y + (r.h - vh * s) / 2.0;
    let t = [s, 0.0, 0.0, s, ox - vx * s, oy - vy * s];
    with_path(def.d, |path| p.fill_path(path, t, color));
}

/// A Material Symbols glyph drawn like the icon font: `size` px em box with
/// its top at `top`.
pub fn glyph(p: &mut dyn Painter, def: &IconDef, x: f32, top: f32, size: f32, color: Color) {
    icon(p, def, Rect::new(x, top, size, size), color);
}

/// The logo at `width` px wide (height follows the 180:136 aspect).
pub fn logo(p: &mut dyn Painter, x: f32, y: f32, width: f32, color: Color) {
    let [_, _, vw, _] = LOGO_VIEW_BOX;
    let s = width / vw;
    for (m, d) in LOGO_PARTS.iter() {
        // Compose the part's transform with the placement scale/translate.
        let t = [m[0] * s, m[1] * s, m[2] * s, m[3] * s, m[4] * s + x, m[5] * s + y];
        with_path(d, |path| p.fill_path(path, t, color));
    }
}

pub fn logo_height(width: f32) -> f32 {
    width * LOGO_VIEW_BOX[3] / LOGO_VIEW_BOX[2]
}

/// Draws text right-aligned to `right`, returning its left edge.
pub fn text_right(p: &mut dyn Painter, s: &str, st: &TextStyle, right: f32, top: f32, lh: f32) -> f32 {
    let w = p.measure(s, st);
    p.text(s, st, right - w, top, lh);
    right - w
}

pub fn text_center(p: &mut dyn Painter, s: &str, st: &TextStyle, cx: f32, top: f32, lh: f32) {
    let w = p.measure(s, st);
    p.text(s, st, cx - w / 2.0, top, lh);
}

/// Shortens `s` with a trailing ellipsis until it fits `max` px.
pub fn ellipsize(p: &mut dyn Painter, s: &str, st: &TextStyle, max: f32) -> String {
    if p.measure(s, st) <= max {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cand: String = chars[..mid].iter().collect::<String>() + "…";
        if p.measure(&cand, st) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// Greedy word wrap into lines no wider than `max`.
pub fn wrap(p: &mut dyn Painter, s: &str, st: &TextStyle, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && p.measure(&cand, st) > max {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = cand;
            }
        }
        lines.push(line);
    }
    lines
}

/// Strokes a 1px CSS outline just outside `r` (CSS `outline: 1px`).
pub fn outline(p: &mut dyn Painter, r: Rect, radius: f32, color: Color) {
    p.stroke(r.outset(1.0), if radius > 0.0 { radius + 1.0 } else { 0.0 }, 1.0, color);
}

impl Path {
    /// Bounding box of the control points (good enough for icons).
    pub fn bounds(&self) -> Rect {
        let mut min = Point { x: f32::MAX, y: f32::MAX };
        let mut max = Point { x: f32::MIN, y: f32::MIN };
        let mut add = |q: &Point| {
            min.x = min.x.min(q.x);
            min.y = min.y.min(q.y);
            max.x = max.x.max(q.x);
            max.y = max.y.max(q.y);
        };
        for c in &self.cmds {
            match c {
                Cmd::Move(a) | Cmd::Line(a) => add(a),
                Cmd::Quad(a, b) => {
                    add(a);
                    add(b)
                }
                Cmd::Cubic(a, b, c) => {
                    add(a);
                    add(b);
                    add(c)
                }
                Cmd::Close => {}
            }
        }
        Rect::new(min.x, min.y, max.x - min.x, max.y - min.y)
    }
}
