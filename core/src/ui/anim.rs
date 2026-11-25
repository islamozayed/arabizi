//! Time-based transitions matching the CSS ones.

/// A value easing from `from` to `to` over `dur` seconds starting at `start`.
#[derive(Debug, Clone, Copy)]
pub struct Anim {
    from: f32,
    to: f32,
    start: f64,
    dur: f64,
    ease: Ease,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ease {
    /// CSS `ease`.
    Ease,
    /// `cubic-bezier(0.16, 1, 0.3, 1)` (settings button reveal, onboarding).
    OutExpo,
}

impl Anim {
    pub fn fixed(v: f32) -> Anim {
        Anim { from: v, to: v, start: 0.0, dur: 0.0, ease: Ease::Ease }
    }

    /// Re-targets the animation from wherever it is now.
    pub fn set(&mut self, to: f32, now: f64, dur: f64) {
        if (self.to - to).abs() < f32::EPSILON {
            return;
        }
        self.from = self.value(now);
        self.to = to;
        self.start = now;
        self.dur = dur;
    }

    pub fn with_ease(mut self, ease: Ease) -> Anim {
        self.ease = ease;
        self
    }

    pub fn start_from(&mut self, from: f32, to: f32, now: f64, dur: f64) {
        self.from = from;
        self.to = to;
        self.start = now;
        self.dur = dur;
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn value(&self, now: f64) -> f32 {
        if self.dur <= 0.0 || now >= self.start + self.dur {
            return self.to;
        }
        if now <= self.start {
            return self.from;
        }
        let t = ((now - self.start) / self.dur) as f32;
        let e = match self.ease {
            Ease::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
            Ease::OutExpo => cubic_bezier(0.16, 1.0, 0.3, 1.0, t),
        };
        self.from + (self.to - self.from) * e
    }

    pub fn running(&self, now: f64) -> bool {
        self.dur > 0.0 && now < self.start + self.dur
    }
}

/// Solves a CSS cubic-bezier timing function at progress `x`.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let bez = |t: f32, a: f32, b: f32| {
        let mt = 1.0 - t;
        3.0 * mt * mt * t * a + 3.0 * mt * t * t * b + t * t * t
    };
    // Binary search t for x.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if bez(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2.0, y1, y2)
}
