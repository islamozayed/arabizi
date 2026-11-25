//! `Painter` on CoreGraphics (shapes) and AppKit string drawing (text, which
//! is CoreText underneath — the same engine WebKit used for the old UI).

use franconvert_core::ui::geom::{Color, Point, Rect};
use franconvert_core::ui::paint::{Affine, Painter, Shadow};
use franconvert_core::ui::path::{Cmd, Path};
use franconvert_core::ui::theme::{Family, TextStyle};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType};
use objc2_app_kit::{
    NSFont, NSFontDescriptor, NSFontVariationAttribute, NSFontAttributeName, NSAttributedStringNSExtendedStringDrawing, NSAttributedStringNSStringDrawing, NSForegroundColorAttributeName,
    NSKernAttributeName, NSColor, NSStringDrawingOptions,
};
use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBlendMode, CGColor, CGColorSpace, CGContext, CGGradient, CGGradientDrawingOptions, CGMutablePath};
use objc2_foundation::{NSAttributedString, NSDictionary, NSMutableDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

/// Font family names after registration (see `fonts::register`).
pub fn font(style: &TextStyle, dyslexia: bool) -> Retained<NSFont> {
    thread_local! {
        static CACHE: RefCell<HashMap<(u8, u16, u32, bool), Retained<NSFont>>> = RefCell::new(HashMap::new());
    }
    let fam = match style.family {
        Family::Latin => 0,
        Family::Arabic => 1,
        Family::Mono => 2,
    };
    let key = (fam, style.weight, style.size.to_bits(), dyslexia);
    CACHE.with(|c| {
        if let Some(f) = c.borrow().get(&key) {
            return f.clone();
        }
        let size = style.size as CGFloat;
        let font = unsafe {
            match style.family {
                Family::Latin if dyslexia => {
                    let name = if style.weight >= 600 { "AtkinsonHyperlegible-Bold" } else { "AtkinsonHyperlegible-Regular" };
                    NSFont::fontWithName_size(&NSString::from_str(name), size)
                }
                Family::Latin => {
                    // Manrope is a variable font: pick the weight on the wght axis.
                    NSFont::fontWithName_size(&NSString::from_str("Manrope-Regular"), size)
                        .or_else(|| NSFont::fontWithName_size(&NSString::from_str("Manrope"), size))
                        .map(|base| {
                            let wght: u32 = u32::from_be_bytes(*b"wght");
                            let variation = NSDictionary::<NSNumber, NSNumber>::from_retained_objects(
                                &[&*NSNumber::new_u32(wght)],
                                &[NSNumber::new_f64(style.weight as f64)],
                            );
                            let attrs = NSDictionary::<NSString, AnyObject>::from_retained_objects(
                                &[NSFontVariationAttribute],
                                &[Retained::into_super(Retained::into_super(variation))],
                            );
                            let desc = base.fontDescriptor().fontDescriptorByAddingAttributes(&attrs);
                            NSFont::fontWithDescriptor_size(&desc, size).unwrap_or(base)
                        })
                }
                Family::Arabic => {
                    let name = match style.weight {
                        w if w >= 700 => "Rubik-Bold",
                        w if w >= 600 => "Rubik-SemiBold",
                        w if w >= 500 => "Rubik-Medium",
                        _ => "Rubik-Regular",
                    };
                    NSFont::fontWithName_size(&NSString::from_str(name), size)
                }
                Family::Mono => NSFont::fontWithName_size(&NSString::from_str("CourierNewPSMT"), size),
            }
        }
        .unwrap_or_else(|| NSFont::systemFontOfSize(size));
        let _ = NSFontDescriptor::class();
        c.borrow_mut().insert(key, font.clone());
        font
    })
}

fn ns_color(c: Color) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c.r as f64, c.g as f64, c.b as f64, c.a as f64)
}

fn cg_color(c: Color) -> objc2_core_foundation::CFRetained<CGColor> {
    CGColor::new_srgb(c.r as f64, c.g as f64, c.b as f64, c.a as f64)
}

fn rect(r: Rect) -> CGRect {
    CGRect::new(CGPoint::new(r.x as f64, r.y as f64), CGSize::new(r.w.max(0.0) as f64, r.h.max(0.0) as f64))
}

enum Layer {
    State,
    Alpha,
    Fade { r: Rect, top: f32, bottom: f32 },
}

pub struct MacPainter<'a> {
    cg: &'a CGContext,
    dyslexia: bool,
    stack: Vec<Layer>,
}

impl<'a> MacPainter<'a> {
    pub fn new(cg: &'a CGContext, dyslexia: bool) -> MacPainter<'a> {
        CGContext::set_should_smooth_fonts(Some(cg), false);
        MacPainter { cg, dyslexia, stack: Vec::new() }
    }

    fn ctx(&self) -> Option<&CGContext> {
        Some(self.cg)
    }

    fn rounded(&self, r: Rect, radius: f32) -> objc2_core_foundation::CFRetained<CGMutablePath> {
        let path = CGMutablePath::new();
        let rad = radius.max(0.0) as f64;
        unsafe {
            if rad <= 0.0 {
                CGMutablePath::add_rect(Some(&path), std::ptr::null(), rect(r));
                return path;
            }
            // Pills (toggles, capsule buttons) keep fully round ends.
            let half = (r.w.min(r.h) / 2.0) as f64;
            if rad >= half - 0.5 {
                CGMutablePath::add_rounded_rect(Some(&path), std::ptr::null(), rect(r), half, half);
                return path;
            }
            // macOS-style continuous corners: each corner eases in over
            // ~1.5× the radius instead of switching abruptly to a circle.
            let (x, y, w, h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
            let s = (rad * 1.528).min(w / 2.0).min(h / 2.0);
            let k = 0.18 * s; // control-point inset (smaller = rounder)
            let m = Some(&*path);
            let n = std::ptr::null();
            CGMutablePath::move_to_point(m, n, x + s, y);
            CGMutablePath::add_line_to_point(m, n, x + w - s, y);
            CGMutablePath::add_curve_to_point(m, n, x + w - k, y, x + w, y + k, x + w, y + s);
            CGMutablePath::add_line_to_point(m, n, x + w, y + h - s);
            CGMutablePath::add_curve_to_point(m, n, x + w, y + h - k, x + w - k, y + h, x + w - s, y + h);
            CGMutablePath::add_line_to_point(m, n, x + s, y + h);
            CGMutablePath::add_curve_to_point(m, n, x + k, y + h, x, y + h - k, x, y + h - s);
            CGMutablePath::add_line_to_point(m, n, x, y + s);
            CGMutablePath::add_curve_to_point(m, n, x, y + k, x + k, y, x + s, y);
            CGMutablePath::close_subpath(m);
        }
        path
    }

    fn attributed(&self, s: &str, style: &TextStyle) -> Retained<NSAttributedString> {
        let font = font(style, self.dyslexia);
        let color = ns_color(style.color);
        let dict = NSMutableDictionary::<NSString, AnyObject>::new();
        unsafe {
            dict.setObject_forKey(&font, ProtocolKey::key(NSFontAttributeName));
            dict.setObject_forKey(&color, ProtocolKey::key(NSForegroundColorAttributeName));
            if style.tracking != 0.0 {
                let kern = NSNumber::new_f64((style.tracking * style.size) as f64);
                dict.setObject_forKey(&kern, ProtocolKey::key(NSKernAttributeName));
            }
            NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &NSString::from_str(s), Some(&dict))
        }
    }

    fn font_metrics(&self, style: &TextStyle) -> (f32, f32, f32) {
        let f = font(style, self.dyslexia);
        (f.ascender() as f32, -f.descender() as f32, f.leading() as f32)
    }
}

/// `setObject:forKey:` wants `&ProtocolObject<dyn NSCopying>`; the string
/// constants already are NSStrings.
struct ProtocolKey;
impl ProtocolKey {
    fn key(k: &NSString) -> &objc2::runtime::ProtocolObject<dyn objc2_foundation::NSCopying> {
        objc2::runtime::ProtocolObject::from_ref(k)
    }
}

impl Painter for MacPainter<'_> {
    fn fill(&mut self, r: Rect, radius: f32, color: Color) {
        if color.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let path = self.rounded(r, radius);
        CGContext::set_fill_color_with_color(self.ctx(), Some(&cg_color(color)));
        CGContext::add_path(self.ctx(), Some(&path));
        CGContext::fill_path(self.ctx());
    }

    fn stroke(&mut self, r: Rect, radius: f32, width: f32, color: Color) {
        if color.a <= 0.0 {
            return;
        }
        let inset = r.inset(width / 2.0, width / 2.0);
        let path = self.rounded(inset, (radius - width / 2.0).max(0.0));
        CGContext::set_stroke_color_with_color(self.ctx(), Some(&cg_color(color)));
        CGContext::set_line_width(self.ctx(), width as f64);
        CGContext::add_path(self.ctx(), Some(&path));
        CGContext::stroke_path(self.ctx());
    }

    fn fill_shadowed(&mut self, r: Rect, radius: f32, color: Color, shadow: Shadow) {
        CGContext::save_g_state(self.ctx());
        // CG shadow offsets ignore the flipped CTM: positive y points up.
        CGContext::set_shadow_with_color(
            self.ctx(),
            CGSize::new(shadow.dx as f64, -shadow.dy as f64),
            shadow.blur as f64,
            Some(&cg_color(shadow.color)),
        );
        self.fill(r, radius, color);
        CGContext::restore_g_state(self.ctx());
    }

    fn fill_ellipse(&mut self, r: Rect, color: Color) {
        CGContext::set_fill_color_with_color(self.ctx(), Some(&cg_color(color)));
        CGContext::fill_ellipse_in_rect(self.ctx(), rect(r));
    }

    fn stroke_ellipse(&mut self, r: Rect, width: f32, color: Color) {
        CGContext::set_stroke_color_with_color(self.ctx(), Some(&cg_color(color)));
        CGContext::set_line_width(self.ctx(), width as f64);
        CGContext::stroke_ellipse_in_rect(self.ctx(), rect(r.inset(width / 2.0, width / 2.0)));
    }

    fn fill_path(&mut self, path: &Path, t: Affine, color: Color) {
        let tp = |p: &Point| CGPoint::new((t[0] * p.x + t[2] * p.y + t[4]) as f64, (t[1] * p.x + t[3] * p.y + t[5]) as f64);
        let cg = CGMutablePath::new();
        let m = Some(&*cg);
        unsafe {
            for c in &path.cmds {
                match c {
                    Cmd::Move(a) => {
                        let a = tp(a);
                        CGMutablePath::move_to_point(m, std::ptr::null(), a.x, a.y)
                    }
                    Cmd::Line(a) => {
                        let a = tp(a);
                        CGMutablePath::add_line_to_point(m, std::ptr::null(), a.x, a.y)
                    }
                    Cmd::Quad(c1, e) => {
                        let (c1, e) = (tp(c1), tp(e));
                        CGMutablePath::add_quad_curve_to_point(m, std::ptr::null(), c1.x, c1.y, e.x, e.y)
                    }
                    Cmd::Cubic(c1, c2, e) => {
                        let (c1, c2, e) = (tp(c1), tp(c2), tp(e));
                        CGMutablePath::add_curve_to_point(m, std::ptr::null(), c1.x, c1.y, c2.x, c2.y, e.x, e.y)
                    }
                    Cmd::Close => CGMutablePath::close_subpath(m),
                }
            }
        }
        CGContext::set_fill_color_with_color(self.ctx(), Some(&cg_color(color)));
        CGContext::add_path(self.ctx(), Some(&cg));
        // Even-odd matches the glyph outlines (counters in the logo, icons).
        CGContext::eo_fill_path(self.ctx());
    }

    fn gradient(&mut self, r: Rect, radius: f32, from: Point, to: Point, stops: &[(f32, Color)]) {
        let space = unsafe { CGColorSpace::with_name(Some(objc2_core_graphics::kCGColorSpaceSRGB)) };
        let mut comps: Vec<CGFloat> = Vec::new();
        let mut locs: Vec<CGFloat> = Vec::new();
        for (o, c) in stops {
            comps.extend([c.r as f64, c.g as f64, c.b as f64, c.a as f64]);
            locs.push(*o as f64);
        }
        let g = unsafe { CGGradient::with_color_components(space.as_deref(), comps.as_ptr(), locs.as_ptr(), stops.len()) };
        CGContext::save_g_state(self.ctx());
        let path = self.rounded(r, radius);
        CGContext::add_path(self.ctx(), Some(&path));
        CGContext::clip(self.ctx());
        CGContext::draw_linear_gradient(
            self.ctx(),
            g.as_deref(),
            CGPoint::new(from.x as f64, from.y as f64),
            CGPoint::new(to.x as f64, to.y as f64),
            CGGradientDrawingOptions::empty(),
        );
        CGContext::restore_g_state(self.ctx());
    }

    fn text(&mut self, s: &str, style: &TextStyle, x: f32, top: f32, line_height: f32) {
        if s.is_empty() {
            return;
        }
        let (asc, desc, _) = self.font_metrics(style);
        let baseline = top + (line_height - (asc + desc)) / 2.0 + asc;
        let a = self.attributed(s, style);
        a.drawWithRect_options_context(
            NSRect::new(NSPoint::new(x as f64, baseline as f64), NSSize::new(10000.0, 0.0)),
            NSStringDrawingOptions::empty(),
            None,
        );
    }

    fn measure(&mut self, s: &str, style: &TextStyle) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        self.attributed(s, style).size().width as f32
    }

    fn metrics(&mut self, style: &TextStyle) -> (f32, f32) {
        let (a, d, _) = self.font_metrics(style);
        (a, d)
    }

    fn normal_line_height(&mut self, style: &TextStyle) -> f32 {
        let (a, d, l) = self.font_metrics(style);
        a + d + l
    }

    fn push_clip(&mut self, r: Rect, radius: f32) {
        CGContext::save_g_state(self.ctx());
        let path = self.rounded(r, radius);
        CGContext::add_path(self.ctx(), Some(&path));
        CGContext::clip(self.ctx());
        self.stack.push(Layer::State);
    }

    fn push_alpha(&mut self, a: f32) {
        CGContext::save_g_state(self.ctx());
        CGContext::set_alpha(self.ctx(), a.clamp(0.0, 1.0) as f64);
        unsafe { CGContext::begin_transparency_layer(self.ctx(), None) };
        self.stack.push(Layer::Alpha);
    }

    fn push_transform(&mut self, t: Affine) {
        CGContext::save_g_state(self.ctx());
        CGContext::concat_ctm(
            self.ctx(),
            CGAffineTransform { a: t[0] as f64, b: t[1] as f64, c: t[2] as f64, d: t[3] as f64, tx: t[4] as f64, ty: t[5] as f64 },
        );
        self.stack.push(Layer::State);
    }

    fn push_fade(&mut self, r: Rect, top: f32, bottom: f32) {
        CGContext::save_g_state(self.ctx());
        unsafe { CGContext::begin_transparency_layer(self.ctx(), None) };
        self.stack.push(Layer::Fade { r, top, bottom });
    }

    fn pop(&mut self) {
        match self.stack.pop() {
            Some(Layer::State) => CGContext::restore_g_state(self.ctx()),
            Some(Layer::Alpha) => {
                CGContext::end_transparency_layer(self.ctx());
                CGContext::restore_g_state(self.ctx());
            }
            Some(Layer::Fade { r, top, bottom }) => {
                // Erase towards the faded edges (mask-image: linear-gradient).
                CGContext::set_blend_mode(self.ctx(), CGBlendMode::DestinationOut);
                let opaque = Color::BLACK;
                let clear = Color::BLACK.with_alpha(0.0);
                if top > 0.0 {
                    let band = Rect::new(r.x, r.y, r.w, top);
                    self.gradient(band, 0.0, Point { x: r.x, y: r.y }, Point { x: r.x, y: r.y + top }, &[(0.0, opaque), (1.0, clear)]);
                }
                if bottom > 0.0 {
                    let band = Rect::new(r.x, r.bottom() - bottom, r.w, bottom);
                    self.gradient(band, 0.0, Point { x: r.x, y: r.bottom() - bottom }, Point { x: r.x, y: r.bottom() }, &[(0.0, clear), (1.0, opaque)]);
                }
                CGContext::end_transparency_layer(self.ctx());
                CGContext::restore_g_state(self.ctx());
            }
            None => {}
        }
    }
}
