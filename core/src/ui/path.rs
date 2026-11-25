//! A small SVG path-data parser so icons can be drawn by every platform's
//! native vector API. Supports M L H V C S Q T A Z (absolute and relative).

use super::geom::Point;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Quad(Point, Point),
    Close,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    pub cmds: Vec<Cmd>,
}

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

impl Path {
    pub fn parse(d: &str) -> Path {
        let mut cmds = Vec::new();
        let toks = tokenize(d);
        let mut i = 0;
        let mut cur = p(0.0, 0.0);
        let mut start = cur;
        let mut last_ctrl: Option<Point> = None;
        let mut last_quad: Option<Point> = None;
        let mut op = 'M';
        while i < toks.len() {
            if let Tok::Op(c) = toks[i] {
                op = c;
                i += 1;
                if c == 'Z' || c == 'z' {
                    cmds.push(Cmd::Close);
                    cur = start;
                    last_ctrl = None;
                    last_quad = None;
                    continue;
                }
            }
            let rel = op.is_ascii_lowercase();
            let mut num = || -> f32 {
                let v = match toks.get(i) {
                    Some(Tok::Num(n)) => *n,
                    _ => 0.0,
                };
                i += 1;
                v
            };
            let base = if rel { cur } else { p(0.0, 0.0) };
            match op.to_ascii_uppercase() {
                'M' => {
                    let (x, y) = (num(), num());
                    cur = p(base.x + x, base.y + y);
                    start = cur;
                    cmds.push(Cmd::Move(cur));
                    // Subsequent pairs are implicit line-tos.
                    op = if rel { 'l' } else { 'L' };
                    last_ctrl = None;
                    last_quad = None;
                }
                'L' => {
                    let (x, y) = (num(), num());
                    cur = p(base.x + x, base.y + y);
                    cmds.push(Cmd::Line(cur));
                    last_ctrl = None;
                    last_quad = None;
                }
                'H' => {
                    let x = num();
                    cur = p(if rel { cur.x + x } else { x }, cur.y);
                    cmds.push(Cmd::Line(cur));
                    last_ctrl = None;
                    last_quad = None;
                }
                'V' => {
                    let y = num();
                    cur = p(cur.x, if rel { cur.y + y } else { y });
                    cmds.push(Cmd::Line(cur));
                    last_ctrl = None;
                    last_quad = None;
                }
                'C' => {
                    let c1 = p(base.x + num(), base.y + num());
                    let c2 = p(base.x + num(), base.y + num());
                    let e = p(base.x + num(), base.y + num());
                    cmds.push(Cmd::Cubic(c1, c2, e));
                    last_ctrl = Some(c2);
                    last_quad = None;
                    cur = e;
                }
                'S' => {
                    let c1 = last_ctrl.map(|c| p(2.0 * cur.x - c.x, 2.0 * cur.y - c.y)).unwrap_or(cur);
                    let c2 = p(base.x + num(), base.y + num());
                    let e = p(base.x + num(), base.y + num());
                    cmds.push(Cmd::Cubic(c1, c2, e));
                    last_ctrl = Some(c2);
                    last_quad = None;
                    cur = e;
                }
                'Q' => {
                    let c = p(base.x + num(), base.y + num());
                    let e = p(base.x + num(), base.y + num());
                    cmds.push(Cmd::Quad(c, e));
                    last_quad = Some(c);
                    last_ctrl = None;
                    cur = e;
                }
                'T' => {
                    let c = last_quad.map(|c| p(2.0 * cur.x - c.x, 2.0 * cur.y - c.y)).unwrap_or(cur);
                    let e = p(base.x + num(), base.y + num());
                    cmds.push(Cmd::Quad(c, e));
                    last_quad = Some(c);
                    last_ctrl = None;
                    cur = e;
                }
                'A' => {
                    let (rx, ry, rot, large, sweep) = (num(), num(), num(), num(), num());
                    let e = p(base.x + num(), base.y + num());
                    arc_to_cubics(&mut cmds, cur, e, rx, ry, rot, large != 0.0, sweep != 0.0);
                    cur = e;
                    last_ctrl = None;
                    last_quad = None;
                }
                _ => {
                    i += 1;
                }
            }
        }
        Path { cmds }
    }
}

#[derive(Debug, Clone, Copy)]
enum Tok {
    Op(char),
    Num(f32),
}

fn tokenize(d: &str) -> Vec<Tok> {
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    // Arc flags may be written without separators ("a16,16,0,0,0" is fine, but
    // "a1 1 0 011 1" packs flags) — track position within an arc argument list.
    let mut arc_arg: Option<usize> = None;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Tok::Op(c));
            arc_arg = if c == 'A' || c == 'a' { Some(0) } else { None };
            i += 1;
            continue;
        }
        if c == ',' || c.is_whitespace() {
            i += 1;
            continue;
        }
        if let Some(n) = arc_arg {
            if n % 7 == 3 || n % 7 == 4 {
                out.push(Tok::Num(if c == '1' { 1.0 } else { 0.0 }));
                i += 1;
                arc_arg = Some(n + 1);
                continue;
            }
        }
        let start = i;
        if b[i] == b'-' || b[i] == b'+' {
            i += 1;
        }
        let mut seen_dot = false;
        while i < b.len() {
            let ch = b[i];
            if ch.is_ascii_digit() {
                i += 1;
            } else if ch == b'.' && !seen_dot {
                seen_dot = true;
                i += 1;
            } else if (ch == b'e' || ch == b'E') && i + 1 < b.len() {
                i += 1;
                if b[i] == b'-' || b[i] == b'+' {
                    i += 1;
                }
            } else {
                break;
            }
        }
        if i == start {
            i += 1;
            continue;
        }
        out.push(Tok::Num(d[start..i].parse().unwrap_or(0.0)));
        if let Some(n) = arc_arg {
            arc_arg = Some(n + 1);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(out: &mut Vec<Cmd>, from: Point, to: Point, rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool) {
    let (mut rx, mut ry) = (rx.abs() as f64, ry.abs() as f64);
    if rx == 0.0 || ry == 0.0 {
        out.push(Cmd::Line(to));
        return;
    }
    let phi = (rot_deg as f64).to_radians();
    let (cos, sin) = (phi.cos(), phi.sin());
    let (x1, y1, x2, y2) = (from.x as f64, from.y as f64, to.x as f64, to.y as f64);
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos * dx + sin * dy;
    let y1p = -sin * dx + cos * dy;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = (num / den).max(0.0).sqrt();
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cos * cxp - sin * cyp + (x1 + x2) / 2.0;
    let cy = sin * cxp + cos * cyp + (y1 + y2) / 2.0;
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }
    let segs = (dtheta.abs() / (std::f64::consts::FRAC_PI_2)).ceil().max(1.0) as usize;
    let delta = dtheta / segs as f64;
    let t = 4.0 / 3.0 * (delta / 4.0).tan();
    let mut th = theta1;
    let pt = |a: f64| {
        let (ca, sa) = (a.cos(), a.sin());
        (cx + rx * ca * cos - ry * sa * sin, cy + rx * ca * sin + ry * sa * cos)
    };
    let der = |a: f64| {
        let (ca, sa) = (a.cos(), a.sin());
        (-rx * sa * cos - ry * ca * sin, -rx * sa * sin + ry * ca * cos)
    };
    for _ in 0..segs {
        let th2 = th + delta;
        let (p1x, p1y) = pt(th);
        let (p2x, p2y) = pt(th2);
        let (d1x, d1y) = der(th);
        let (d2x, d2y) = der(th2);
        out.push(Cmd::Cubic(
            p((p1x + t * d1x) as f32, (p1y + t * d1y) as f32),
            p((p2x - t * d2x) as f32, (p2y - t * d2y) as f32),
            p(p2x as f32, p2y as f32),
        ));
        th = th2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_material_and_phosphor_paths() {
        let gear = Path::parse("m370-80-16-128q-13-5-24.5-12T307-235l-119 50Z");
        assert!(matches!(gear.cmds[0], Cmd::Move(Point { x: 370.0, y: -80.0 })));
        assert!(matches!(gear.cmds[1], Cmd::Line(Point { x: 354.0, y: -208.0 })));
        let lock = Path::parse("M96,56a32,32,0,0,1,64,0V80H96Z");
        assert!(lock.cmds.iter().any(|c| matches!(c, Cmd::Cubic(..))));
        let packed = Path::parse("M10 10a5 5 0 1110 0z");
        assert!(packed.cmds.len() >= 3);
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;
    use crate::ui::icons;

    #[test]
    fn inline_svg_icons_have_sane_bounds() {
        for (name, def) in [("gear", &icons::GEAR), ("back", &icons::BACK), ("plus", &icons::PLUS), ("up", &icons::ARROW_UP), ("enter", &icons::ENTER), ("tab", &icons::MS_KEYBOARD_TAB)] {
            let p = Path::parse(def.d);
            let b = p.bounds();
            assert!(b.w > 200.0 && b.h > 200.0, "{name} parsed to {b:?}");
            assert!(b.x >= -1.0 && b.right() <= 961.0 && b.y >= -961.0 && b.bottom() <= 1.0, "{name} {b:?}");
        }
    }
}
