//! `Painter` on Direct2D (shapes, layers, shadows) and DirectWrite (text).
//! The bundled fonts are compiled into the executable and loaded into a
//! private font collection, so nothing is installed system-wide.

use franconvert_core::ui::geom::{Color, Point, Rect};
use franconvert_core::ui::paint::{Affine, Painter, Shadow};
use franconvert_core::ui::path::{Cmd, Path};
use franconvert_core::ui::theme::{Family, TextStyle};
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use windows::core::{Interface, HSTRING};
use windows::core::BOOL;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows_numerics::Matrix3x2;

const FONTS: &[&[u8]] = &[
    include_bytes!("../../ui/fonts/Manrope-VariableFont_wght.ttf"),
    include_bytes!("../../ui/fonts/Rubik-Regular.ttf"),
    include_bytes!("../../ui/fonts/Rubik-Medium.ttf"),
    include_bytes!("../../ui/fonts/Rubik-SemiBold.ttf"),
    include_bytes!("../../ui/fonts/Rubik-Bold.ttf"),
    include_bytes!("../../ui/fonts/AtkinsonHyperlegible-Regular.ttf"),
    include_bytes!("../../ui/fonts/AtkinsonHyperlegible-Bold.ttf"),
];

// ── Factories and text ───────────────────────────────────────────────────

type StyleKey = (u8, u16, u32, u32, bool);

fn style_key(st: &TextStyle, dyslexia: bool) -> StyleKey {
    let fam = match st.family {
        Family::Latin => 0,
        Family::Arabic => 1,
        Family::Mono => 2,
    };
    (fam, st.weight, st.size.to_bits(), st.tracking.to_bits(), dyslexia && st.family == Family::Latin)
}

pub struct Laid {
    pub layout: IDWriteTextLayout,
    pub width: f32,
    /// Distance from the layout's top to the first line's baseline.
    pub baseline: f32,
}

struct Text {
    dwrite: IDWriteFactory,
    bundled: Option<IDWriteFontCollection>,
    system: Option<IDWriteFontCollection>,
    formats: HashMap<StyleKey, IDWriteTextFormat>,
    metrics: HashMap<StyleKey, (f32, f32, f32)>,
    layouts: HashMap<(String, StyleKey), std::rc::Rc<Laid>>,
}

thread_local! {
    static D2D: ID2D1Factory1 = unsafe {
        D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&D2D1_FACTORY_OPTIONS::default()))
            .expect("Direct2D factory")
    };
    static TEXT: RefCell<Text> = RefCell::new(Text::new());
}

pub fn d2d() -> ID2D1Factory1 {
    D2D.with(|f| f.clone())
}

impl Text {
    fn new() -> Text {
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.expect("DirectWrite factory");
        let bundled = load_bundled(&dwrite);
        let mut system = None;
        unsafe {
            let _ = dwrite.GetSystemFontCollection(&mut system, false);
        }
        Text { dwrite, bundled, system, formats: HashMap::new(), metrics: HashMap::new(), layouts: HashMap::new() }
    }

    /// Family name, collection and weight for a style.
    fn face(&self, st: &TextStyle, dyslexia: bool) -> (&'static str, Option<IDWriteFontCollection>, u16) {
        match st.family {
            Family::Latin if dyslexia => ("Atkinson Hyperlegible", self.bundled.clone(), if st.weight >= 600 { 700 } else { 400 }),
            Family::Latin => ("Manrope", self.bundled.clone(), st.weight),
            Family::Arabic => {
                let w = match st.weight {
                    w if w >= 700 => 700,
                    w if w >= 600 => 600,
                    w if w >= 500 => 500,
                    _ => 400,
                };
                ("Rubik", self.bundled.clone(), w)
            }
            Family::Mono => ("Courier New", self.system.clone(), st.weight),
        }
    }

    fn format(&mut self, st: &TextStyle, dyslexia: bool) -> Option<IDWriteTextFormat> {
        let key = style_key(st, dyslexia);
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        let (family, coll, weight) = self.face(st, dyslexia);
        let f = unsafe {
            self.dwrite.CreateTextFormat(
                &HSTRING::from(family),
                coll.as_ref(),
                DWRITE_FONT_WEIGHT(weight as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                st.size.max(1.0),
                &HSTRING::from("en-us"),
            )
        }
        .ok()?;
        unsafe {
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        }
        self.formats.insert(key, f.clone());
        Some(f)
    }

    /// Ascent, descent and line gap in DIPs.
    fn font_metrics(&mut self, st: &TextStyle, dyslexia: bool) -> (f32, f32, f32) {
        let key = style_key(st, dyslexia);
        if let Some(m) = self.metrics.get(&key) {
            return *m;
        }
        let (family, coll, weight) = self.face(st, dyslexia);
        let m = (|| unsafe {
            let coll = coll?;
            let mut index = 0u32;
            let mut exists = BOOL(0);
            coll.FindFamilyName(&HSTRING::from(family), &mut index, &mut exists).ok()?;
            if !exists.as_bool() {
                return None;
            }
            let fam = coll.GetFontFamily(index).ok()?;
            let font = fam.GetFirstMatchingFont(DWRITE_FONT_WEIGHT(weight as i32), DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL).ok()?;
            let mut fm = DWRITE_FONT_METRICS::default();
            font.GetMetrics(&mut fm);
            let s = st.size / fm.designUnitsPerEm.max(1) as f32;
            Some((fm.ascent as f32 * s, fm.descent as f32 * s, fm.lineGap as f32 * s))
        })()
        .unwrap_or((st.size * 0.95, st.size * 0.25, 0.0));
        self.metrics.insert(key, m);
        m
    }

    fn layout(&mut self, s: &str, st: &TextStyle, dyslexia: bool) -> Option<std::rc::Rc<Laid>> {
        let key = (s.to_string(), style_key(st, dyslexia));
        if let Some(l) = self.layouts.get(&key) {
            return Some(l.clone());
        }
        let format = self.format(st, dyslexia)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        let layout = unsafe { self.dwrite.CreateTextLayout(&wide, &format, 100_000.0, 10_000.0) }.ok()?;
        unsafe {
            if st.tracking != 0.0 {
                if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                    let range = DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 };
                    let _ = l1.SetCharacterSpacing(0.0, st.tracking * st.size, 0.0, range);
                }
            }
            let mut tm = DWRITE_TEXT_METRICS::default();
            let _ = layout.GetMetrics(&mut tm);
            let width = tm.widthIncludingTrailingWhitespace;
            // Like AppKit's natural direction: a line that starts with Arabic
            // runs right to left. Shrink the box to the text so it still
            // starts at the requested x.
            let _ = layout.SetMaxWidth(width.max(0.01));
            if first_strong_is_rtl(s) {
                let _ = layout.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);
            }
            let mut lines = [DWRITE_LINE_METRICS::default(); 1];
            let mut count = 0u32;
            let _ = layout.GetLineMetrics(Some(&mut lines), &mut count);
            let baseline = if count > 0 { lines[0].baseline } else { st.size };
            if self.layouts.len() > 4000 {
                self.layouts.clear();
            }
            let laid = std::rc::Rc::new(Laid { layout, width, baseline });
            self.layouts.insert(key, laid.clone());
            Some(laid)
        }
    }
}

fn first_strong_is_rtl(s: &str) -> bool {
    for c in s.chars() {
        let u = c as u32;
        if (0x0590..=0x08FF).contains(&u) || (0xFB1D..=0xFDFF).contains(&u) || (0xFE70..=0xFEFF).contains(&u) {
            return true;
        }
        if c.is_alphabetic() {
            return false;
        }
    }
    false
}

fn load_bundled(dwrite: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    unsafe {
        let f5: IDWriteFactory5 = dwrite.cast().ok()?;
        let loader = f5.CreateInMemoryFontFileLoader().ok()?;
        f5.RegisterFontFileLoader(&loader).ok()?;
        let builder = f5.CreateFontSetBuilder().ok()?;
        for data in FONTS {
            let file = loader.CreateInMemoryFontFileReference(&f5, data.as_ptr().cast(), data.len() as u32, None).ok()?;
            builder.AddFontFile(&file).ok()?;
        }
        let set = builder.CreateFontSet().ok()?;
        let coll = f5.CreateFontCollectionFromFontSet(&set).ok()?;
        coll.cast().ok()
    }
}

/// A shaped single line, cached.
pub fn layout(s: &str, st: &TextStyle, dyslexia: bool) -> Option<std::rc::Rc<Laid>> {
    TEXT.with(|t| t.borrow_mut().layout(s, st, dyslexia))
}

pub fn font_metrics(st: &TextStyle, dyslexia: bool) -> (f32, f32, f32) {
    TEXT.with(|t| t.borrow_mut().font_metrics(st, dyslexia))
}

/// x offset of UTF-16 position `pos` within `laid`.
pub fn caret_x(laid: &Laid, pos: u32) -> f32 {
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    unsafe {
        let _ = laid.layout.HitTestTextPosition(pos, false, &mut x, &mut y, &mut m);
    }
    x
}

/// UTF-16 position nearest to `x` within `laid`.
pub fn hit_index(laid: &Laid, x: f32) -> u32 {
    let mut trailing = BOOL(0);
    let mut inside = BOOL(0);
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    unsafe {
        let _ = laid.layout.HitTestPoint(x, 1.0, &mut trailing, &mut inside, &mut m);
    }
    m.textPosition + if trailing.as_bool() { m.length } else { 0 }
}

// ── Geometry helpers ─────────────────────────────────────────────────────

pub fn color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w.max(0.0), bottom: r.y + r.h.max(0.0) }
}

fn rounded(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    let rad = radius.max(0.0).min(r.w.min(r.h).max(0.0) / 2.0);
    D2D1_ROUNDED_RECT { rect: rect_f(r), radiusX: rad, radiusY: rad }
}

fn pt(x: f32, y: f32) -> windows_numerics::Vector2 {
    windows_numerics::Vector2 { X: x, Y: y }
}

fn matrix(t: Affine) -> Matrix3x2 {
    Matrix3x2 { M11: t[0], M12: t[1], M21: t[2], M22: t[3], M31: t[4], M32: t[5] }
}

/// `a` then `b` (Direct2D multiplies row vectors on the left).
fn mul(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}

const INFINITE: D2D_RECT_F = D2D_RECT_F { left: -1.0e7, top: -1.0e7, right: 1.0e7, bottom: 1.0e7 };

// ── Painter ──────────────────────────────────────────────────────────────

/// Cached drop-shadow images, keyed by shape and shadow.
pub type ShadowCache = HashMap<(u32, u32, u32, u32, [u32; 4]), ID2D1Image>;

enum Layer {
    Clip,
    Layer,
    Transform(Matrix3x2),
}

pub struct D2dPainter<'a> {
    rt: ID2D1RenderTarget,
    dc: Option<ID2D1DeviceContext>,
    brush: ID2D1SolidColorBrush,
    dyslexia: bool,
    stack: Vec<Layer>,
    shadows: Option<&'a mut ShadowCache>,
}

impl<'a> D2dPainter<'a> {
    /// Call between `BeginDraw` and `EndDraw`.
    pub fn new(rt: &ID2D1RenderTarget, dyslexia: bool, shadows: Option<&'a mut ShadowCache>) -> windows::core::Result<D2dPainter<'a>> {
        let brush = unsafe { rt.CreateSolidColorBrush(&color(Color::BLACK), None) }?;
        unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
        Ok(D2dPainter { rt: rt.clone(), dc: rt.cast().ok(), brush, dyslexia, stack: Vec::new(), shadows })
    }

    fn brush(&self, c: Color) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&color(c)) };
        &self.brush
    }

    fn transform(&self) -> Matrix3x2 {
        let mut m = Matrix3x2::default();
        unsafe { self.rt.GetTransform(&mut m) };
        m
    }

    fn push_layer(&mut self, mask: Option<ID2D1Geometry>, opacity: f32, opacity_brush: Option<ID2D1Brush>) {
        let Some(dc) = &self.dc else {
            self.stack.push(Layer::Transform(self.transform()));
            return;
        };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: INFINITE,
            geometricMask: ManuallyDrop::new(mask),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity,
            opacityBrush: ManuallyDrop::new(opacity_brush),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        unsafe {
            dc.PushLayer(&params, None);
            ManuallyDrop::drop(&mut params.geometricMask);
            ManuallyDrop::drop(&mut params.opacityBrush);
        }
        self.stack.push(Layer::Layer);
    }

    fn gradient_brush(&self, from: Point, to: Point, stops: &[(f32, Color)]) -> Option<ID2D1LinearGradientBrush> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|(o, c)| D2D1_GRADIENT_STOP { position: *o, color: color(*c) }).collect();
        unsafe {
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).ok()?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(from.x, from.y), endPoint: pt(to.x, to.y) };
            self.rt.CreateLinearGradientBrush(&props, None, &coll).ok()
        }
    }

    /// The shadow of a rounded rect, blurred once and reused.
    fn shadow_image(&mut self, w: f32, h: f32, radius: f32, s: &Shadow, pad: f32) -> Option<ID2D1Image> {
        let key = (w.to_bits(), h.to_bits(), radius.to_bits(), s.blur.to_bits(), [s.color.r.to_bits(), s.color.g.to_bits(), s.color.b.to_bits(), s.color.a.to_bits()]);
        if let Some(img) = self.shadows.as_ref().and_then(|c| c.get(&key)) {
            return Some(img.clone());
        }
        let dc = self.dc.clone()?;
        unsafe {
            let size = D2D_SIZE_F { width: w + 2.0 * pad, height: h + 2.0 * pad };
            let bmp_rt = self.rt.CreateCompatibleRenderTarget(Some(&size), None, None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE).ok()?;
            bmp_rt.BeginDraw();
            bmp_rt.Clear(Some(&color(Color::BLACK.with_alpha(0.0))));
            let b = bmp_rt.CreateSolidColorBrush(&color(Color::BLACK), None).ok()?;
            bmp_rt.FillRoundedRectangle(&rounded(Rect::new(pad, pad, w, h), radius), &b);
            bmp_rt.EndDraw(None, None).ok()?;
            let bitmap = bmp_rt.GetBitmap().ok()?;
            let effect = dc.CreateEffect(&CLSID_D2D1Shadow).ok()?;
            effect.SetInput(0, &bitmap, true);
            let sigma = (s.blur / 2.0).max(0.0);
            effect.SetValue(D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &sigma.to_le_bytes()).ok()?;
            let mut rgba = Vec::with_capacity(16);
            for v in [s.color.r, s.color.g, s.color.b, s.color.a] {
                rgba.extend_from_slice(&v.to_le_bytes());
            }
            effect.SetValue(D2D1_SHADOW_PROP_COLOR.0 as u32, D2D1_PROPERTY_TYPE_VECTOR4, &rgba).ok()?;
            let img = effect.GetOutput().ok()?;
            if let Some(c) = self.shadows.as_mut() {
                if c.len() > 64 {
                    c.clear();
                }
                c.insert(key, img.clone());
            }
            Some(img)
        }
    }
}

impl Drop for D2dPainter<'_> {
    fn drop(&mut self) {
        while !self.stack.is_empty() {
            self.pop();
        }
    }
}

impl Painter for D2dPainter<'_> {
    fn fill(&mut self, r: Rect, radius: f32, c: Color) {
        if c.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let b = self.brush(c).clone();
        unsafe {
            if radius <= 0.0 {
                self.rt.FillRectangle(&rect_f(r), &b);
            } else {
                self.rt.FillRoundedRectangle(&rounded(r, radius), &b);
            }
        }
    }

    fn stroke(&mut self, r: Rect, radius: f32, width: f32, c: Color) {
        if c.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let inset = r.inset(width / 2.0, width / 2.0);
        let b = self.brush(c).clone();
        unsafe { self.rt.DrawRoundedRectangle(&rounded(inset, (radius - width / 2.0).max(0.0)), &b, width, None) };
    }

    fn fill_shadowed(&mut self, r: Rect, radius: f32, c: Color, shadow: Shadow) {
        if shadow.color.a > 0.0 && r.w > 0.0 && r.h > 0.0 {
            let pad = (shadow.blur * 1.5).ceil() + 2.0;
            if let (Some(img), Some(dc)) = (self.shadow_image(r.w, r.h, radius, &shadow, pad), self.dc.clone()) {
                let at = pt(r.x - pad + shadow.dx, r.y - pad + shadow.dy);
                unsafe { dc.DrawImage(&img, Some(&at), None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER) };
            }
        }
        self.fill(r, radius, c);
    }

    fn fill_ellipse(&mut self, r: Rect, c: Color) {
        if c.a <= 0.0 {
            return;
        }
        let e = D2D1_ELLIPSE { point: pt(r.x + r.w / 2.0, r.y + r.h / 2.0), radiusX: r.w / 2.0, radiusY: r.h / 2.0 };
        let b = self.brush(c).clone();
        unsafe { self.rt.FillEllipse(&e, &b) };
    }

    fn stroke_ellipse(&mut self, r: Rect, width: f32, c: Color) {
        if c.a <= 0.0 {
            return;
        }
        let r = r.inset(width / 2.0, width / 2.0);
        let e = D2D1_ELLIPSE { point: pt(r.x + r.w / 2.0, r.y + r.h / 2.0), radiusX: r.w / 2.0, radiusY: r.h / 2.0 };
        let b = self.brush(c).clone();
        unsafe { self.rt.DrawEllipse(&e, &b, width, None) };
    }

    fn fill_path(&mut self, path: &Path, t: Affine, c: Color) {
        if c.a <= 0.0 {
            return;
        }
        let tp = |p: &Point| pt(t[0] * p.x + t[2] * p.y + t[4], t[1] * p.x + t[3] * p.y + t[5]);
        let geom = (|| unsafe {
            let geom = d2d().CreatePathGeometry().ok()?;
            let sink = geom.Open().ok()?;
            // Even-odd matches the glyph outlines (counters in the logo, icons).
            sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
            let mut open = false;
            for cmd in &path.cmds {
                match cmd {
                    Cmd::Move(a) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        }
                        sink.BeginFigure(tp(a), D2D1_FIGURE_BEGIN_FILLED);
                        open = true;
                    }
                    Cmd::Line(a) if open => sink.AddLine(tp(a)),
                    Cmd::Quad(c1, e) if open => sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: tp(c1), point2: tp(e) }),
                    Cmd::Cubic(c1, c2, e) if open => sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: tp(c1), point2: tp(c2), point3: tp(e) }),
                    Cmd::Close if open => {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                    _ => {}
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            sink.Close().ok()?;
            Some(geom)
        })();
        if let Some(geom) = geom {
            let b = self.brush(c).clone();
            unsafe { self.rt.FillGeometry(&geom, &b, None) };
        }
    }

    fn gradient(&mut self, r: Rect, radius: f32, from: Point, to: Point, stops: &[(f32, Color)]) {
        let Some(b) = self.gradient_brush(from, to, stops) else { return };
        unsafe {
            if radius <= 0.0 {
                self.rt.FillRectangle(&rect_f(r), &b);
            } else {
                self.rt.FillRoundedRectangle(&rounded(r, radius), &b);
            }
        }
    }

    fn text(&mut self, s: &str, style: &TextStyle, x: f32, top: f32, line_height: f32) {
        if s.is_empty() || style.color.a <= 0.0 {
            return;
        }
        let (asc, desc, _) = font_metrics(style, self.dyslexia);
        let baseline = top + (line_height - (asc + desc)) / 2.0 + asc;
        let Some(laid) = layout(s, style, self.dyslexia) else { return };
        let b = self.brush(style.color).clone();
        unsafe {
            self.rt.DrawTextLayout(pt(x, baseline - laid.baseline), &laid.layout, &b, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
        }
    }

    fn measure(&mut self, s: &str, style: &TextStyle) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        layout(s, style, self.dyslexia).map(|l| l.width).unwrap_or(0.0)
    }

    fn normal_line_height(&mut self, style: &TextStyle) -> f32 {
        let (a, d, l) = font_metrics(style, self.dyslexia);
        a + d + l
    }

    fn metrics(&mut self, style: &TextStyle) -> (f32, f32) {
        let (a, d, _) = font_metrics(style, self.dyslexia);
        (a, d)
    }

    fn push_clip(&mut self, r: Rect, radius: f32) {
        let m = self.transform();
        let axis_aligned = m.M12 == 0.0 && m.M21 == 0.0;
        if radius <= 0.0 && axis_aligned {
            unsafe { self.rt.PushAxisAlignedClip(&rect_f(r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
            self.stack.push(Layer::Clip);
            return;
        }
        let geom: Option<ID2D1Geometry> = unsafe { d2d().CreateRoundedRectangleGeometry(&rounded(r, radius)) }.ok().and_then(|g| g.cast().ok());
        self.push_layer(geom, 1.0, None);
    }

    fn push_alpha(&mut self, a: f32) {
        self.push_layer(None, a.clamp(0.0, 1.0), None);
    }

    fn push_transform(&mut self, t: Affine) {
        let old = self.transform();
        unsafe { self.rt.SetTransform(&mul(&matrix(t), &old)) };
        self.stack.push(Layer::Transform(old));
    }

    fn push_fade(&mut self, r: Rect, top: f32, bottom: f32) {
        // Fade to transparent at the edges (mask-image: linear-gradient).
        let h = r.h.max(1.0);
        let opaque = Color::BLACK;
        let clear = Color::BLACK.with_alpha(0.0);
        let mut stops = Vec::new();
        if top > 0.0 {
            stops.push((0.0, clear));
            stops.push(((top / h).min(1.0), opaque));
        } else {
            stops.push((0.0, opaque));
        }
        if bottom > 0.0 {
            stops.push(((1.0 - bottom / h).max(0.0), opaque));
            stops.push((1.0, clear));
        } else {
            stops.push((1.0, opaque));
        }
        let brush: Option<ID2D1Brush> = self
            .gradient_brush(Point { x: r.x, y: r.y }, Point { x: r.x, y: r.y + h }, &stops)
            .and_then(|b| b.cast().ok());
        self.push_layer(None, 1.0, brush);
    }

    fn pop(&mut self) {
        match self.stack.pop() {
            Some(Layer::Clip) => unsafe { self.rt.PopAxisAlignedClip() },
            Some(Layer::Layer) => {
                if let Some(dc) = &self.dc {
                    unsafe { dc.PopLayer() };
                }
            }
            Some(Layer::Transform(m)) => unsafe { self.rt.SetTransform(&m) },
            None => {}
        }
    }
}

/// The identity transform.
pub fn identity() -> Matrix3x2 {
    Matrix3x2::identity()
}
