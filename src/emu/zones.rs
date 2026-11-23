//! Dead zones and response curves for the virtual pad. Pure maths, no I/O.
//!
//! A stick goes through, in order:
//! 1. the raw position is clamped to the unit circle (the corners of the byte square are outside it);
//! 2. the **inner dead zone** is radial: everything within `inner` of the centre reads as zero, and the travel
//!    after it is stretched back to the full range (so there is no jump when the stick leaves the zone);
//! 3. the **outer dead zone** makes the stick reach full deflection before the physical edge (`1 - outer`);
//! 4. the **response curve** reshapes the magnitude;
//! 5. the **anti dead zone** lifts the smallest output to `anti`, to jump over a game's own inner dead zone.
//!
//! The direction is never changed, only the length of the vector, so a diagonal stays a diagonal.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Curve {
    #[default]
    Linear,
    /// Finer near the centre, steeper at the edge (`t^1.6`).
    Smooth,
    /// Quicker near the centre (`t^0.65`).
    Aggressive,
    /// Two points the curve passes through.
    Custom,
}

impl Curve {
    pub const ALL: [Curve; 4] = [Curve::Linear, Curve::Smooth, Curve::Aggressive, Curve::Custom];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StickCfg {
    /// 0..0.9, share of the travel that reads as zero.
    pub inner: f32,
    /// 0..0.5, share of the travel at the edge that reads as full.
    pub outer: f32,
    /// 0..0.5, the smallest output once the stick leaves the inner zone.
    pub anti: f32,
    pub curve: Curve,
    /// The two points of the custom curve, input and output both 0..1, `p1.x <= p2.x`.
    pub p1: (f32, f32),
    pub p2: (f32, f32),
}

impl Default for StickCfg {
    fn default() -> Self {
        Self { inner: 0.05, outer: 0.0, anti: 0.0, curve: Curve::Linear, p1: (0.33, 0.33), p2: (0.66, 0.66) }
    }
}

fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

impl StickCfg {
    /// Brings every field into its range (a hand-edited or old config must never produce NaN or a division by zero).
    pub fn sanitize(&mut self) {
        self.inner = finite(self.inner, 0.05).clamp(0.0, 0.9);
        self.outer = finite(self.outer, 0.0).clamp(0.0, 0.5);
        if self.inner + self.outer > 0.98 {
            self.outer = (0.98 - self.inner).max(0.0);
        }
        self.anti = finite(self.anti, 0.0).clamp(0.0, 0.5);
        self.p1 = (finite(self.p1.0, 0.33).clamp(0.0, 1.0), finite(self.p1.1, 0.33).clamp(0.0, 1.0));
        self.p2 = (finite(self.p2.0, 0.66).clamp(0.0, 1.0), finite(self.p2.1, 0.66).clamp(0.0, 1.0));
        if self.p2.0 < self.p1.0 {
            std::mem::swap(&mut self.p1, &mut self.p2);
        }
    }

    /// The curve at `t` in 0..1.
    pub fn curve_at(&self, t: f32) -> f32 {
        let t = finite(t, 0.0).clamp(0.0, 1.0);
        match self.curve {
            Curve::Linear => t,
            Curve::Smooth => t.powf(1.6),
            Curve::Aggressive => t.powf(0.65),
            Curve::Custom => {
                // Through (0,0), p1, p2, (1,1), never going down.
                let y1 = self.p1.1;
                let y2 = self.p2.1.max(y1);
                let pts = [(0.0f32, 0.0f32), (self.p1.0, y1), (self.p2.0.max(self.p1.0), y2), (1.0, 1.0)];
                let mut out = 1.0f32;
                for w in pts.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    if t <= b.0 {
                        out = if b.0 - a.0 < 1e-6 { b.1 } else { a.1 + (b.1 - a.1) * ((t - a.0) / (b.0 - a.0)) };
                        break;
                    }
                }
                out.clamp(0.0, 1.0)
            }
        }
    }

    /// The whole chain for a magnitude (0..1 of the physical travel). Returns the output magnitude 0..1.
    pub fn magnitude(&self, m: f32) -> f32 {
        let m = finite(m, 0.0).clamp(0.0, 1.0);
        let full = 1.0 - self.outer;
        let span = (full - self.inner).max(1e-4);
        if m <= self.inner {
            return 0.0;
        }
        let t = ((m - self.inner) / span).clamp(0.0, 1.0);
        let c = self.curve_at(t);
        if c <= 0.0 && self.anti <= 0.0 {
            return 0.0;
        }
        (self.anti + (1.0 - self.anti) * c).clamp(0.0, 1.0)
    }

    /// Processes a stick position (each axis -1..1, Y up) and returns the position the game should see.
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = (finite(x, 0.0), finite(y, 0.0));
        let m = (x * x + y * y).sqrt();
        if !m.is_finite() {
            return (0.0, 0.0);
        }
        if m < 1e-6 {
            return (0.0, 0.0);
        }
        let out = self.magnitude(m.min(1.0));
        // Direction from the raw vector; the length is only ever the processed magnitude (no corner overshoot).
        (x / m * out, y / m * out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TrigCfg {
    /// 0..0.9: the share of the pull that reads as zero.
    pub inner: f32,
    /// 0..0.5: the share at the end of the pull that reads as full.
    pub outer: f32,
}

impl TrigCfg {
    pub fn sanitize(&mut self) {
        self.inner = finite(self.inner, 0.0).clamp(0.0, 0.9);
        self.outer = finite(self.outer, 0.0).clamp(0.0, 0.5);
        if self.inner + self.outer > 0.98 {
            self.outer = (0.98 - self.inner).max(0.0);
        }
    }

    /// 0..1 in, 0..1 out.
    pub fn level(&self, v: f32) -> f32 {
        let v = finite(v, 0.0).clamp(0.0, 1.0);
        if v <= self.inner {
            return 0.0;
        }
        ((v - self.inner) / (1.0 - self.outer - self.inner).max(1e-4)).clamp(0.0, 1.0)
    }

    pub fn apply(&self, raw: u8) -> u8 {
        (self.level(raw as f32 / 255.0) * 255.0).round() as u8
    }
}

/// A -1..1 axis as the XInput 16-bit value (symmetric, so the centre is exactly 0).
pub fn to_axis(v: f32) -> i16 {
    (finite(v, 0.0).clamp(-1.0, 1.0) * 32767.0).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(inner: f32, outer: f32, anti: f32, curve: Curve) -> StickCfg {
        let mut c = StickCfg { inner, outer, anti, curve, ..Default::default() };
        c.sanitize();
        c
    }

    #[test]
    fn inside_the_inner_zone_is_exactly_zero_and_the_edge_is_one() {
        let c = cfg(0.1, 0.0, 0.0, Curve::Linear);
        assert_eq!(c.apply(0.0, 0.0), (0.0, 0.0));
        assert_eq!(c.apply(0.05, 0.05), (0.0, 0.0));
        assert_eq!(c.apply(0.1, 0.0), (0.0, 0.0), "exactly on the edge of the zone is still zero");
        let (x, y) = c.apply(1.0, 0.0);
        assert!((x - 1.0).abs() < 1e-6 && y == 0.0);
        let (x, _) = c.apply(0.55, 0.0);
        assert!((x - 0.5).abs() < 1e-5, "the travel after the zone is stretched back to 0..1");
    }

    #[test]
    fn the_zone_is_radial_not_per_axis() {
        let c = cfg(0.2, 0.0, 0.0, Curve::Linear);
        // Each axis is under 0.2, but the vector (0.15, 0.15) has length 0.21: a per-axis zone would zero it.
        let (x, y) = c.apply(0.15, 0.15);
        assert!(x > 0.0 && y > 0.0, "{x} {y}");
        assert!((x - y).abs() < 1e-6, "the direction is kept");
        let (x2, y2) = c.apply(0.1, 0.1);
        assert_eq!((x2, y2), (0.0, 0.0), "magnitude 0.14 is inside the 0.2 zone");
    }

    #[test]
    fn corners_of_the_byte_square_never_exceed_the_unit_circle() {
        let c = cfg(0.05, 0.0, 0.0, Curve::Linear);
        let (x, y) = c.apply(-1.0, 1.0);
        assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-5);
        assert!(x < 0.0 && y > 0.0);
    }

    #[test]
    fn outer_zone_reaches_full_early() {
        let c = cfg(0.0, 0.1, 0.0, Curve::Linear);
        let (x, _) = c.apply(0.9, 0.0);
        assert!((x - 1.0).abs() < 1e-5);
        let (x, _) = c.apply(0.45, 0.0);
        assert!((x - 0.5).abs() < 1e-5);
        assert!((c.apply(0.95, 0.0).0 - 1.0).abs() < 1e-6, "past the outer edge stays at one");
    }

    #[test]
    fn anti_dead_zone_lifts_the_first_output() {
        let c = cfg(0.1, 0.0, 0.2, Curve::Linear);
        assert_eq!(c.apply(0.1, 0.0).0, 0.0);
        let (x, _) = c.apply(0.1001, 0.0);
        assert!((0.2..0.21).contains(&x), "{x}");
        assert!((c.apply(1.0, 0.0).0 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn curves_keep_the_ends_and_order_the_middle() {
        let lin = cfg(0.0, 0.0, 0.0, Curve::Linear);
        let smooth = cfg(0.0, 0.0, 0.0, Curve::Smooth);
        let aggr = cfg(0.0, 0.0, 0.0, Curve::Aggressive);
        for c in [lin, smooth, aggr] {
            assert_eq!(c.curve_at(0.0), 0.0);
            assert!((c.curve_at(1.0) - 1.0).abs() < 1e-6);
        }
        assert!(smooth.curve_at(0.5) < 0.5 && aggr.curve_at(0.5) > 0.5);
        for c in [lin, smooth, aggr] {
            let mut prev = 0.0;
            for i in 0..=100 {
                let v = c.curve_at(i as f32 / 100.0);
                assert!(v >= prev - 1e-6);
                prev = v;
            }
        }
    }

    #[test]
    fn custom_curve_passes_through_its_points() {
        let mut c = StickCfg { curve: Curve::Custom, p1: (0.25, 0.1), p2: (0.75, 0.9), ..Default::default() };
        c.sanitize();
        assert!((c.curve_at(0.25) - 0.1).abs() < 1e-6);
        assert!((c.curve_at(0.75) - 0.9).abs() < 1e-6);
        assert!((c.curve_at(0.5) - 0.5).abs() < 1e-6);
        assert_eq!(c.curve_at(0.0), 0.0);
        assert!((c.curve_at(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn custom_curve_edge_cases_never_go_down_or_blow_up() {
        // Points in the wrong order are swapped, equal x values do not divide by zero, a dip is flattened.
        let mut c = StickCfg { curve: Curve::Custom, p1: (0.8, 0.2), p2: (0.2, 0.9), ..Default::default() };
        c.sanitize();
        assert!(c.p1.0 <= c.p2.0);
        let mut c2 = StickCfg { curve: Curve::Custom, p1: (0.5, 0.7), p2: (0.5, 0.3), ..Default::default() };
        c2.sanitize();
        for c in [c, c2] {
            let mut prev = 0.0;
            for i in 0..=200 {
                let v = c.curve_at(i as f32 / 200.0);
                assert!(v.is_finite() && (0.0..=1.0).contains(&v));
                assert!(v >= prev - 1e-6, "{v} < {prev}");
                prev = v;
            }
        }
    }

    #[test]
    fn nan_inf_and_huge_inputs_are_safe() {
        let c = cfg(0.05, 0.0, 0.0, Curve::Smooth);
        for (x, y) in [(f32::NAN, 0.0), (f32::INFINITY, 1.0), (1e30, -1e30), (-5.0, 5.0)] {
            let (a, b) = c.apply(x, y);
            assert!(a.is_finite() && b.is_finite() && (a * a + b * b).sqrt() <= 1.0 + 1e-5, "{x} {y} -> {a} {b}");
        }
        let mut bad = StickCfg { inner: f32::NAN, outer: 7.0, anti: -3.0, p1: (f32::NAN, 9.0), p2: (-1.0, f32::INFINITY), ..Default::default() };
        bad.sanitize();
        assert!(bad.inner.is_finite() && bad.outer <= 0.5 && bad.anti == 0.0);
        assert!(bad.apply(0.7, 0.2).0.is_finite());
    }

    #[test]
    fn inner_plus_outer_never_close_the_range() {
        let mut c = StickCfg { inner: 0.9, outer: 0.5, ..Default::default() };
        c.sanitize();
        assert!(c.inner + c.outer <= 0.98 + 1e-6);
        assert!(c.apply(1.0, 0.0).0 > 0.0);
    }

    #[test]
    fn triggers_inner_and_outer() {
        let t = TrigCfg { inner: 0.1, outer: 0.1 };
        assert_eq!(t.apply(0), 0);
        assert_eq!(t.apply(25), 0, "25/255 is under 10 percent");
        assert_eq!(t.apply(255), 255);
        assert_eq!(t.apply(230), 255, "past 90 percent is full");
        let mid = t.apply(128);
        assert!((mid as i32 - 128).abs() <= 2, "{mid}");
        let d = TrigCfg::default();
        for v in 0..=255u8 {
            assert_eq!(d.apply(v), v, "no zones: unchanged");
        }
        let mut bad = TrigCfg { inner: 2.0, outer: f32::NAN };
        bad.sanitize();
        assert!(bad.inner <= 0.9 && bad.outer.is_finite());
    }

    #[test]
    fn axis_conversion_is_symmetric() {
        assert_eq!(to_axis(0.0), 0);
        assert_eq!(to_axis(1.0), 32767);
        assert_eq!(to_axis(-1.0), -32767);
        assert_eq!(to_axis(2.0), 32767);
        assert_eq!(to_axis(f32::NAN), 0);
    }
}
