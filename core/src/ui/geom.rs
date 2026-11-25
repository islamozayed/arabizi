//! Geometry and colour types for the view layer (points = CSS px).

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }
    pub fn inset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w - 2.0 * dx, self.h - 2.0 * dy)
    }
    /// Grows the rect on every side (CSS `outline` sits outside the box).
    pub fn outset(&self, d: f32) -> Rect {
        self.inset(-d, -d)
    }
    pub fn center(&self) -> Point {
        Point { x: self.x + self.w / 2.0, y: self.y + self.h / 2.0 }
    }
    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
        Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::rgba(r, g, b, 1.0)
    }
    pub fn hex(s: &str) -> Color {
        let c = crate::color::Rgb::from_hex(s).unwrap_or(crate::color::Rgb(0x58, 0xA1, 0xAC));
        Color::rgb(c.0, c.1, c.2)
    }
    pub fn from_rgb(c: crate::color::Rgb) -> Color {
        Color::rgb(c.0, c.1, c.2)
    }
    /// `color-mix(in srgb, self p%, transparent)`.
    pub fn with_alpha_mul(self, p: f32) -> Color {
        Color { a: self.a * p, ..self }
    }
    pub fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Color { r: l(self.r, other.r), g: l(self.g, other.g), b: l(self.b, other.b), a: l(self.a, other.a) }
    }
}
