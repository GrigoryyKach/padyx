//! A fake pad with a scripted motion, for screenshots, UI checks and tests with no hardware.
//!
//! The demo goes through the real pipeline: [`DemoIo`] produces genuine USB input reports (built by
//! [`usb_report_from`]), so the parser, the statistics and the writer are all exercised. Output reports are
//! swallowed.
//!
//! The script repeats every 40 seconds:
//! - 0..8 s: sticks at rest with a small offset and noise (for the drift test);
//! - 8..20 s: both sticks rotate along the edge, slowly growing and shrinking, with a little gyro;
//! - 20..32 s: the 17 buttons light up one after another, the triggers follow slow waves;
//! - 32..40 s: two fingers move over the touchpad.
//!
//! `PADYX_DEMO_T=<seconds>` starts the script at that time and `PADYX_DEMO_HOLD=1` freezes it there, so a
//! screenshot shows the same frame every time. `PADYX_DEMO_POSE=1` holds one fixed pose instead (several buttons,
//! both sticks pushed, both triggers pulled, a finger on the touchpad), for the Overview screenshots.

use super::hub::{IoError, PadIo};
#[cfg(test)]
use super::output::crc32;
use super::state::*;
use std::time::{Duration, Instant};

pub const SCRIPT_SECS: f32 = 40.0;

fn to_byte(v: f32) -> u8 {
    (127.5 + v * 127.5).round().clamp(0.0, 255.0) as u8
}

/// A cheap deterministic noise in -1..1.
fn noise(t: f32, seed: f32) -> f32 {
    ((t * 91.7 + seed * 13.3).sin() * 43758.545).fract() * 2.0 - 1.0
}

/// The pad state at script time `t` (seconds).
pub fn demo_state(t: f32) -> PadState {
    let t = t.rem_euclid(SCRIPT_SECS);
    let mut s = PadState { conn: Conn::Usb, battery: Some(78), charge: Charge::Discharging, ..Default::default() };
    let (mut lx, mut ly, mut rx, mut ry) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    if t < 8.0 {
        // At rest with a small offset (about 5 percent on the left stick) and a touch of noise.
        lx = 0.050 + 0.004 * noise(t, 1.0);
        ly = -0.020 + 0.004 * noise(t, 2.0);
        rx = -0.006 + 0.003 * noise(t, 3.0);
        ry = 0.004 + 0.003 * noise(t, 4.0);
    } else if t < 20.0 {
        let u = t - 8.0;
        let ramp = (u / 1.0).min(1.0);
        let a = u * 1.7;
        // A gate that is a little longer on the diagonals, like a real one.
        let r = (0.94 + 0.05 * (2.0 * a).sin().powi(2)) * ramp;
        lx = a.cos() * r;
        ly = a.sin() * r;
        let b = -u * 2.1;
        let q = (0.90 + 0.04 * (2.0 * b).sin().powi(2)) * ramp;
        rx = b.cos() * q;
        ry = b.sin() * q;
        s.gyro = [30.0 * (u * 2.0).sin(), 45.0 * (u * 1.3).cos(), 20.0 * (u * 1.7).sin()];
    } else if t < 32.0 {
        let u = t - 20.0;
        lx = 0.25 * (u * 0.9).sin();
        ly = 0.18 * (u * 0.7).cos();
        rx = 0.2 * (u * 0.6).cos();
        ry = -0.3 * (u * 0.8).sin();
        let i = (u / 0.7) as usize;
        if let Some(b) = Btn::CHECKLIST.get(i) {
            s.set(*b, true);
        }
        s.l2 = (127.5 + 127.5 * (u * 0.8).sin()) as u8;
        s.r2 = (127.5 + 127.5 * (u * 0.55 + 1.0).sin()) as u8;
        s.set(Btn::L2, s.l2 > 240);
        s.set(Btn::R2, s.r2 > 240);
    } else {
        let u = t - 32.0;
        s.touch[0] = Finger { active: true, id: 1, x: (960.0 + 700.0 * (u * 0.9).sin()) as u16, y: (540.0 + 380.0 * (u * 1.3).cos()) as u16 };
        if u > 2.0 {
            s.touch[1] = Finger { active: true, id: 2, x: (1300.0 + 400.0 * (u * 0.7).cos()) as u16, y: (400.0 + 250.0 * (u * 1.1).sin()) as u16 };
        }
        s.set(Btn::Touchpad, (u as u32) % 4 == 3);
        s.gyro = [10.0 * (u * 1.5).sin(), 0.0, 0.0];
    }
    s.lx = to_byte(lx);
    s.ly = to_byte(-ly);
    s.rx = to_byte(rx);
    s.ry = to_byte(-ry);
    // A tilt that changes slowly.
    let tilt = 0.25 * (t * 0.35).sin();
    let roll = 0.2 * (t * 0.27).cos();
    s.accel = [roll.sin(), tilt.cos() * roll.cos(), tilt.sin()];
    s
}

/// A fixed pose with several controls active at once.
pub fn pose_state() -> PadState {
    let mut s = PadState { conn: Conn::Usb, battery: Some(78), charge: Charge::Discharging, ..Default::default() };
    for b in [Btn::Up, Btn::Left, Btn::Cross, Btn::Triangle, Btn::L1, Btn::Create, Btn::Ps, Btn::L3] {
        s.set(b, true);
    }
    s.lx = to_byte(0.75);
    s.ly = to_byte(-0.45);
    s.rx = to_byte(-0.55);
    s.ry = to_byte(0.65);
    s.l2 = 150;
    s.r2 = 255;
    s.set(Btn::R2, true);
    s.touch[0] = Finger { active: true, id: 1, x: 700, y: 430 };
    s.touch[1] = Finger { active: true, id: 2, x: 1350, y: 620 };
    s
}

fn put16(r: &mut [u8], o: usize, v: i16) {
    r[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn fill_common(r: &mut [u8], b: usize, s: &PadState, counter: u8) {
    r[b] = s.lx;
    r[b + 1] = s.ly;
    r[b + 2] = s.rx;
    r[b + 3] = s.ry;
    r[b + 4] = s.l2;
    r[b + 5] = s.r2;
    r[b + 6] = counter;
    let hat = match (s.pressed(Btn::Up), s.pressed(Btn::Right), s.pressed(Btn::Down), s.pressed(Btn::Left)) {
        (true, false, false, false) => 0,
        (true, true, false, false) => 1,
        (false, true, false, false) => 2,
        (false, true, true, false) => 3,
        (false, false, true, false) => 4,
        (false, false, true, true) => 5,
        (false, false, false, true) => 6,
        (true, false, false, true) => 7,
        _ => 8,
    };
    let mut b0 = hat;
    for (btn, bit) in [(Btn::Square, 0x10), (Btn::Cross, 0x20), (Btn::Circle, 0x40), (Btn::Triangle, 0x80)] {
        if s.pressed(btn) {
            b0 |= bit;
        }
    }
    r[b + 7] = b0;
    let mut b1 = 0u8;
    for (i, btn) in [Btn::L1, Btn::R1, Btn::L2, Btn::R2, Btn::Create, Btn::Options, Btn::L3, Btn::R3].into_iter().enumerate() {
        if s.pressed(btn) {
            b1 |= 1 << i;
        }
    }
    r[b + 8] = b1;
    let mut b2 = 0u8;
    for (i, btn) in [Btn::Ps, Btn::Touchpad, Btn::Mute].into_iter().enumerate() {
        if s.pressed(btn) {
            b2 |= 1 << i;
        }
    }
    r[b + 9] = b2;
    for i in 0..3 {
        put16(r, b + 15 + 2 * i, (s.gyro[i] * 32768.0 / 2000.0) as i16);
        put16(r, b + 21 + 2 * i, (s.accel[i] * 8192.0) as i16);
    }
    r[b + 27..b + 31].copy_from_slice(&(s.counter).to_le_bytes());
    for (k, f) in s.touch.iter().enumerate() {
        let o = b + 32 + 4 * k;
        r[o] = if f.active { f.id & 0x7F } else { 0x80 };
        r[o + 1] = (f.x & 0xFF) as u8;
        r[o + 2] = ((f.x >> 8) & 0x0F) as u8 | (((f.y & 0x0F) as u8) << 4);
        r[o + 3] = (f.y >> 4) as u8;
    }
    let level = (s.battery.unwrap_or(0) as u32 / 10).min(10) as u8;
    r[b + 52] = match s.charge {
        Charge::Discharging => level,
        Charge::Charging => level | 0x10,
        Charge::Full => 0x20,
        Charge::Error => 0xA0,
    };
}

/// A genuine USB input report (64 bytes) for `s`.
pub fn usb_report_from(s: &PadState) -> Vec<u8> {
    let mut r = vec![0u8; 64];
    r[0] = 0x01;
    fill_common(&mut r, 1, s, 0);
    r
}

/// A genuine Bluetooth full report (78 bytes) for `s`, with the CRC (used by the tests).
#[cfg(test)]
pub fn bt_report_from(s: &PadState) -> Vec<u8> {
    let mut r = vec![0u8; 78];
    r[0] = 0x31;
    r[1] = 0x01;
    fill_common(&mut r, 2, s, 0);
    let crc = crc32(&[0xA1], &r[..74]);
    r[74..78].copy_from_slice(&crc.to_le_bytes());
    r
}

/// The fake device: about 250 reports per second.
pub struct DemoIo {
    start: Instant,
    t0: f32,
    hold: bool,
    pose: bool,
    next: Instant,
    counter: u32,
}

impl DemoIo {
    pub fn new() -> Self {
        let t0 = std::env::var("PADYX_DEMO_T").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let hold = std::env::var("PADYX_DEMO_HOLD").is_ok_and(|v| v == "1");
        let pose = std::env::var("PADYX_DEMO_POSE").is_ok_and(|v| v == "1");
        let now = Instant::now();
        Self { start: now, t0, hold, pose, next: now, counter: 0 }
    }
}

impl Default for DemoIo {
    fn default() -> Self {
        Self::new()
    }
}

impl PadIo for DemoIo {
    fn read(&mut self, buf: &mut [u8], _timeout_ms: i32) -> Result<usize, IoError> {
        // 4 ms per report, paced against a clock so the rate does not drift.
        let now = Instant::now();
        if self.next > now {
            std::thread::sleep(self.next - now);
        }
        self.next += Duration::from_millis(4);
        let t = if self.hold { self.t0 } else { self.t0 + self.start.elapsed().as_secs_f32() };
        let mut s = if self.pose { pose_state() } else { demo_state(t) };
        self.counter = self.counter.wrapping_add(1);
        s.counter = self.counter;
        let r = usb_report_from(&s);
        buf[..r.len()].copy_from_slice(&r);
        Ok(r.len())
    }

    fn write(&mut self, _data: &[u8]) -> Result<(), IoError> {
        Ok(())
    }

    fn feature(&mut self, _id: u8, _len: usize) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::parse::{parse_dualsense, Calib};

    #[test]
    fn the_pose_round_trips_through_the_parser() {
        let s = pose_state();
        let p = parse_dualsense(&usb_report_from(&s), &Calib::default()).unwrap();
        assert_eq!((p.l2, p.r2, p.buttons, p.touch), (s.l2, s.r2, s.buttons, s.touch));
        assert!(p.pressed(Btn::Cross) && p.pressed(Btn::L3) && p.pressed(Btn::R2));
    }

    #[test]
    fn the_script_round_trips_through_the_parser() {
        let mut checked = 0;
        for i in 0..400 {
            let t = i as f32 * 0.1;
            let s = demo_state(t);
            let r = usb_report_from(&s);
            let p = parse_dualsense(&r, &Calib::default()).unwrap();
            assert_eq!((p.lx, p.ly, p.rx, p.ry, p.l2, p.r2), (s.lx, s.ly, s.rx, s.ry, s.l2, s.r2), "t={t}");
            assert_eq!(p.buttons, s.buttons, "t={t}");
            assert_eq!(p.touch, s.touch, "t={t}");
            assert_eq!(p.battery, Some(75));
            assert!((p.accel[1] - s.accel[1]).abs() < 0.001);
            checked += 1;
        }
        assert_eq!(checked, 400);
    }

    #[test]
    fn script_phases_are_what_the_docs_say() {
        // Rest: both sticks near the centre.
        let s = demo_state(3.0);
        assert!((s.lx as i32 - 128).abs() < 8 && (s.ly as i32 - 128).abs() < 8);
        // Edge rotation: the left stick is far from the centre.
        let s = demo_state(12.0);
        let (x, y) = s.left();
        assert!((x * x + y * y).sqrt() > 0.4);
        // Buttons phase: exactly one checklist button at a time.
        let s = demo_state(20.1);
        assert_eq!(Btn::CHECKLIST.iter().filter(|b| s.pressed(**b)).count(), 1);
        // Touchpad phase: a finger.
        assert!(demo_state(35.0).touch[0].active);
        // The script wraps.
        assert_eq!(demo_state(3.0).lx, demo_state(43.0).lx);
    }

    #[test]
    fn bt_report_has_a_valid_input_crc() {
        let r = bt_report_from(&demo_state(5.0));
        assert_eq!(u32::from_le_bytes(r[74..78].try_into().unwrap()), crc32(&[0xA1], &r[..74]));
    }
}
