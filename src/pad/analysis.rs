//! Stick drift and circularity tests on streams of samples. Positions are -1..1 (the stick at rest is 0).
//!
//! Thresholds (shown to the user next to the verdict):
//! - the resting offset is the distance of the average position from the centre, in percent of the full travel;
//! - jitter is the standard deviation of the position around its average, in percent;
//! - OK: offset under 3 % and jitter under 1 %; light drift: offset under 8 % and jitter under 3 %; drift: more.
//!
//! A circularity test collects the farthest reach of the stick in 72 angular sectors. The shape error is the
//! average difference between that reach and the mean reach, in percent of the mean. A pad's gate is not a
//! perfect circle, so a few percent is normal; the verdict thresholds are 6 % and 12 %.

pub const OFFSET_OK: f32 = 0.03;
pub const OFFSET_LIGHT: f32 = 0.08;
pub const JITTER_OK: f32 = 0.01;
pub const JITTER_LIGHT: f32 = 0.03;
pub const SECTORS: usize = 72;
pub const CIRC_OK: f32 = 6.0;
pub const CIRC_LIGHT: f32 = 12.0;
/// A circularity result needs this share of sectors visited.
pub const MIN_COVERAGE: f32 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Light,
    Bad,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drift {
    /// Average position.
    pub mean: (f32, f32),
    /// Distance of the average from the centre, 0..1.
    pub offset: f32,
    /// Standard deviation around the average, 0..1.
    pub jitter: f32,
    pub samples: usize,
    pub verdict: Verdict,
}

/// `None` with fewer than 10 samples.
pub fn drift(samples: &[(f32, f32)]) -> Option<Drift> {
    if samples.len() < 10 {
        return None;
    }
    let n = samples.len() as f32;
    let mx = samples.iter().map(|s| s.0).sum::<f32>() / n;
    let my = samples.iter().map(|s| s.1).sum::<f32>() / n;
    let var = samples.iter().map(|s| (s.0 - mx).powi(2) + (s.1 - my).powi(2)).sum::<f32>() / n;
    let offset = (mx * mx + my * my).sqrt();
    let jitter = var.sqrt();
    let verdict = if offset < OFFSET_OK && jitter < JITTER_OK {
        Verdict::Ok
    } else if offset < OFFSET_LIGHT && jitter < JITTER_LIGHT {
        Verdict::Light
    } else {
        Verdict::Bad
    };
    Some(Drift { mean: (mx, my), offset, jitter, samples: samples.len(), verdict })
}

/// Collects the reach of the stick by angle.
#[derive(Clone, Debug, PartialEq)]
pub struct Circularity {
    pub reach: [f32; SECTORS],
}

impl Default for Circularity {
    fn default() -> Self {
        Self { reach: [0.0; SECTORS] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircResult {
    /// Share of sectors that were reached (0..1).
    pub coverage: f32,
    /// Mean reach, 0..1 (1 is the full travel on an axis).
    pub mean_reach: f32,
    /// Average shape error in percent.
    pub error_pct: f32,
    pub verdict: Verdict,
    /// False when the stick did not go all the way round.
    pub complete: bool,
}

pub fn sector_of(x: f32, y: f32) -> usize {
    let a = y.atan2(x).rem_euclid(std::f32::consts::TAU);
    ((a / std::f32::consts::TAU * SECTORS as f32) as usize).min(SECTORS - 1)
}

impl Circularity {
    /// Takes a sample; only positions near the edge (beyond 30 % of the travel) count.
    pub fn add(&mut self, x: f32, y: f32) {
        let r = (x * x + y * y).sqrt();
        if r < 0.3 {
            return;
        }
        let s = sector_of(x, y);
        if r > self.reach[s] {
            self.reach[s] = r;
        }
    }

    pub fn coverage(&self) -> f32 {
        self.reach.iter().filter(|r| **r > 0.0).count() as f32 / SECTORS as f32
    }

    pub fn result(&self) -> CircResult {
        let hit: Vec<f32> = self.reach.iter().copied().filter(|r| *r > 0.0).collect();
        let coverage = self.coverage();
        if hit.is_empty() {
            return CircResult { coverage, mean_reach: 0.0, error_pct: 0.0, verdict: Verdict::Bad, complete: false };
        }
        let mean = hit.iter().sum::<f32>() / hit.len() as f32;
        let err = hit.iter().map(|r| (r - mean).abs()).sum::<f32>() / hit.len() as f32 / mean * 100.0;
        let verdict = if err < CIRC_OK {
            Verdict::Ok
        } else if err < CIRC_LIGHT {
            Verdict::Light
        } else {
            Verdict::Bad
        };
        CircResult { coverage, mean_reach: mean, error_pct: err, verdict, complete: coverage >= MIN_COVERAGE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(i: usize) -> f32 {
        ((i as f32 * 12.9898).sin() * 43758.545).fract() * 2.0 - 1.0
    }

    #[test]
    fn a_still_centred_stick_is_ok() {
        let s: Vec<(f32, f32)> = (0..300).map(|i| (0.002 * noise(i), 0.002 * noise(i + 999))).collect();
        let d = drift(&s).unwrap();
        assert_eq!(d.verdict, Verdict::Ok);
        assert!(d.offset < 0.01 && d.jitter < 0.005);
        assert_eq!(d.samples, 300);
    }

    #[test]
    fn an_offset_stick_is_light_drift_then_drift() {
        let light: Vec<(f32, f32)> = (0..100).map(|_| (0.05, 0.0)).collect();
        let d = drift(&light).unwrap();
        assert_eq!(d.verdict, Verdict::Light);
        assert!((d.offset - 0.05).abs() < 1e-6 && d.jitter < 1e-6);
        let bad: Vec<(f32, f32)> = (0..100).map(|_| (0.06, -0.08)).collect();
        assert_eq!(drift(&bad).unwrap().verdict, Verdict::Bad);
        // The boundaries: just under the OK threshold, just over it.
        assert_eq!(drift(&vec![(OFFSET_OK - 0.001, 0.0); 20]).unwrap().verdict, Verdict::Ok);
        assert_eq!(drift(&vec![(OFFSET_OK + 0.001, 0.0); 20]).unwrap().verdict, Verdict::Light);
        assert_eq!(drift(&vec![(OFFSET_LIGHT + 0.001, 0.0); 20]).unwrap().verdict, Verdict::Bad);
    }

    #[test]
    fn jitter_alone_can_fail_a_centred_stick() {
        let s: Vec<(f32, f32)> = (0..300).map(|i| (0.08 * noise(i), 0.08 * noise(i + 7))).collect();
        let d = drift(&s).unwrap();
        assert!(d.jitter > JITTER_LIGHT);
        assert_eq!(d.verdict, Verdict::Bad);
    }

    #[test]
    fn too_few_samples_give_no_result() {
        assert!(drift(&[]).is_none());
        assert!(drift(&[(0.0, 0.0); 9]).is_none());
        assert!(drift(&[(0.0, 0.0); 10]).is_some());
    }

    fn ring(r: impl Fn(f32) -> f32, steps: usize) -> Circularity {
        let mut c = Circularity::default();
        for i in 0..steps {
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            c.add(a.cos() * r(a), a.sin() * r(a));
        }
        c
    }

    #[test]
    fn a_perfect_circle_has_no_error() {
        let r = ring(|_| 0.95, 720).result();
        assert!(r.complete && r.coverage > 0.99);
        assert!(r.error_pct < 0.01 && r.verdict == Verdict::Ok);
        assert!((r.mean_reach - 0.95).abs() < 1e-4);
    }

    #[test]
    fn an_ellipse_is_flagged() {
        // 20 percent shorter on one axis.
        let r = ring(|a| 1.0 - 0.2 * a.sin().abs(), 720).result();
        assert!(r.complete);
        assert!(r.error_pct > CIRC_OK, "{}", r.error_pct);
        assert_ne!(r.verdict, Verdict::Ok);
        // A strong squash is plainly bad.
        let r = ring(|a| 1.0 - 0.5 * a.sin().abs(), 720).result();
        assert_eq!(r.verdict, Verdict::Bad);
    }

    #[test]
    fn a_square_gate_is_a_few_percent_error() {
        // The usual octagon-ish gate: reach grows toward the diagonals up to 1.08.
        let r = ring(|a| 1.0 + 0.08 * (2.0 * a).sin().abs().powi(2), 720).result();
        assert!(r.error_pct > 1.0 && r.error_pct < CIRC_OK, "{}", r.error_pct);
    }

    #[test]
    fn a_half_turn_is_incomplete() {
        let mut c = Circularity::default();
        for i in 0..360 {
            let a = i as f32 / 360.0 * std::f32::consts::PI;
            c.add(a.cos(), a.sin());
        }
        let r = c.result();
        assert!(!r.complete);
        assert!(r.coverage > 0.45 && r.coverage < 0.55);
    }

    #[test]
    fn samples_near_the_centre_are_ignored_and_sectors_wrap() {
        let mut c = Circularity::default();
        c.add(0.1, 0.1);
        assert_eq!(c.coverage(), 0.0);
        assert_eq!(c.result().verdict, Verdict::Bad);
        assert_eq!(sector_of(1.0, 0.0), 0);
        assert_eq!(sector_of(0.0, 1.0), SECTORS / 4);
        assert_eq!(sector_of(1.0, -1e-6), SECTORS - 1);
    }
}
