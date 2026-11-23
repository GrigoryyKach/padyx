//! Input reports to [`PadState`], and the motion calibration.
//!
//! Layouts, written from the public descriptions of the reports (the Linux `hid-playstation` driver notes, the
//! SDL PS5 and PS4 HIDAPI drivers, DS4Windows). All offsets are into the buffer that `hidapi` returns, report id
//! included. `b` is the offset of the left stick X byte.
//!
//! DualSense, USB, report `0x01`, 64 bytes, `b = 1`; Bluetooth full report `0x31`, 78 bytes, `b = 2`
//! (one extra sequence byte), with a CRC32 in the last four bytes:
//!
//! | offset      | field |
//! |-------------|-------|
//! | `b`..`b+5`  | left X, left Y, right X, right Y, L2, R2 (0..255, sticks centred near 128) |
//! | `b+6`       | report counter |
//! | `b+7`       | low nibble: hat (0 up, clockwise to 7, 8 none); bit 4 square, 5 cross, 6 circle, 7 triangle |
//! | `b+8`       | bit 0 L1, 1 R1, 2 L2 click, 3 R2 click, 4 Create, 5 Options, 6 L3, 7 R3 |
//! | `b+9`       | bit 0 PS, 1 touchpad click, 2 Mute |
//! | `b+15`      | gyro x3, signed 16 bit little endian (pitch, yaw, roll) |
//! | `b+21`      | accelerometer x3, signed 16 bit |
//! | `b+27`      | sensor timestamp, u32 |
//! | `b+32`      | two touch points, 4 bytes each: bit 7 of the first byte set means "no finger", low 7 bits the id; then x (12 bit) and y (12 bit) packed in 3 bytes |
//! | `b+52`      | low nibble battery level (x10 percent), high nibble charge state (0 discharging, 1 charging, 2 full, other = error) |
//!
//! The reduced Bluetooth report `0x01` (about 10 bytes, before the pad is switched to the full report):
//! `1..=4` sticks, `5` hat and face buttons, `6` shoulder and stick buttons (same bits as `b+8`), `7` PS and
//! touchpad, `8` and `9` triggers.
//!
//! DualShock 4 (an optional extra, never tested on hardware): USB `0x01` and Bluetooth `0x11` use the same
//! stick, trigger and button bytes (`b = 1` and `b = 3`), gyro at `b+12`, accelerometer at `b+18`, status at `b+29`.

use super::state::*;

fn le16(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}

/// Linear motion calibration for one axis: `(raw - bias) * numer / denom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisCal {
    pub bias: i32,
    pub numer: i32,
    pub denom: i32,
}

impl AxisCal {
    pub fn apply(&self, raw: i16) -> f32 {
        if self.denom == 0 {
            return 0.0;
        }
        (raw as i32 - self.bias) as f32 * self.numer as f32 / self.denom as f32
    }
}

/// Gyro in degrees per second, accelerometer in g.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calib {
    pub gyro: [AxisCal; 3],
    pub accel: [AxisCal; 3],
    /// Read from the pad (and not the nominal defaults).
    pub from_pad: bool,
}

impl Default for Calib {
    fn default() -> Self {
        // Nominal: gyro full scale 2000 deg/s over 16 bits, accelerometer 8192 counts per g.
        let g = AxisCal { bias: 0, numer: 2000, denom: 32768 };
        let a = AxisCal { bias: 0, numer: 1, denom: 8192 };
        Calib { gyro: [g; 3], accel: [a; 3], from_pad: false }
    }
}

impl Calib {
    /// Parses feature report `0x05` (41 bytes, id first). `None` when it is too short or not plausible.
    pub fn from_report(d: &[u8]) -> Option<Calib> {
        if d.len() < 35 || d[0] != 0x05 {
            return None;
        }
        let v = |k: usize| le16(d, 1 + 2 * k) as i32;
        let speed_2x = v(9) + v(10);
        let mut gyro = [AxisCal { bias: 0, numer: 1, denom: 1 }; 3];
        // The report orders the pairs pitch, yaw, roll.
        for (i, g) in gyro.iter_mut().enumerate() {
            *g = AxisCal { bias: v(i), numer: speed_2x, denom: v(3 + 2 * i) - v(4 + 2 * i) };
        }
        let mut accel = [AxisCal { bias: 0, numer: 1, denom: 1 }; 3];
        for (i, a) in accel.iter_mut().enumerate() {
            let plus = v(11 + 2 * i);
            let minus = v(12 + 2 * i);
            let range = plus - minus;
            *a = AxisCal { bias: plus - range / 2, numer: 2, denom: range };
        }
        if gyro.iter().chain(accel.iter()).any(|c| c.denom == 0) || speed_2x <= 0 {
            return None;
        }
        Some(Calib { gyro, accel, from_pad: true })
    }
}

/// Which report this buffer is, for a DualSense.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Usb,
    BtFull,
    BtReduced,
}

pub fn dualsense_layout(d: &[u8]) -> Option<Layout> {
    match (d.first().copied()?, d.len()) {
        (0x01, n) if n >= 64 => Some(Layout::Usb),
        (0x31, n) if n >= 78 => Some(Layout::BtFull),
        (0x01, n) if n >= 10 => Some(Layout::BtReduced),
        _ => None,
    }
}

fn apply_hat(s: &mut PadState, hat: u8) {
    let h = hat & 0x0F;
    s.set(Btn::Up, matches!(h, 7 | 0 | 1));
    s.set(Btn::Right, matches!(h, 1..=3));
    s.set(Btn::Down, matches!(h, 3..=5));
    s.set(Btn::Left, matches!(h, 5..=7));
}

fn apply_buttons(s: &mut PadState, b0: u8, b1: u8, b2: u8) {
    apply_hat(s, b0);
    s.set(Btn::Square, b0 & 0x10 != 0);
    s.set(Btn::Cross, b0 & 0x20 != 0);
    s.set(Btn::Circle, b0 & 0x40 != 0);
    s.set(Btn::Triangle, b0 & 0x80 != 0);
    s.set(Btn::L1, b1 & 0x01 != 0);
    s.set(Btn::R1, b1 & 0x02 != 0);
    s.set(Btn::L2, b1 & 0x04 != 0);
    s.set(Btn::R2, b1 & 0x08 != 0);
    s.set(Btn::Create, b1 & 0x10 != 0);
    s.set(Btn::Options, b1 & 0x20 != 0);
    s.set(Btn::L3, b1 & 0x40 != 0);
    s.set(Btn::R3, b1 & 0x80 != 0);
    s.set(Btn::Ps, b2 & 0x01 != 0);
    s.set(Btn::Touchpad, b2 & 0x02 != 0);
}

fn finger(d: &[u8]) -> Finger {
    let active = d[0] & 0x80 == 0;
    let x = d[1] as u16 | ((d[2] as u16 & 0x0F) << 8);
    let y = (d[2] as u16 >> 4) | ((d[3] as u16) << 4);
    Finger { active, id: d[0] & 0x7F, x: x.min(1919), y: y.min(1079) }
}

/// Parses a DualSense input report. `None` for anything that is not one.
pub fn parse_dualsense(d: &[u8], cal: &Calib) -> Option<PadState> {
    let layout = dualsense_layout(d)?;
    let mut s = PadState::default();
    if layout == Layout::BtReduced {
        s.conn = Conn::Bluetooth;
        s.reduced = true;
        s.lx = d[1];
        s.ly = d[2];
        s.rx = d[3];
        s.ry = d[4];
        apply_buttons(&mut s, d[5], d[6], d[7]);
        s.l2 = d[8];
        s.r2 = d[9];
        // The click bits of the triggers are not in this report: take them from the analog value.
        s.set(Btn::L2, s.l2 > 0);
        s.set(Btn::R2, s.r2 > 0);
        return Some(s);
    }
    let b = if layout == Layout::Usb { 1 } else { 2 };
    s.conn = if layout == Layout::Usb { Conn::Usb } else { Conn::Bluetooth };
    s.lx = d[b];
    s.ly = d[b + 1];
    s.rx = d[b + 2];
    s.ry = d[b + 3];
    s.l2 = d[b + 4];
    s.r2 = d[b + 5];
    apply_buttons(&mut s, d[b + 7], d[b + 8], d[b + 9]);
    s.set(Btn::Mute, d[b + 9] & 0x04 != 0);
    for i in 0..3 {
        s.gyro[i] = cal.gyro[i].apply(le16(d, b + 15 + 2 * i));
        s.accel[i] = cal.accel[i].apply(le16(d, b + 21 + 2 * i));
    }
    s.counter = u32::from_le_bytes([d[b + 27], d[b + 28], d[b + 29], d[b + 30]]);
    s.touch = [finger(&d[b + 32..b + 36]), finger(&d[b + 36..b + 40])];
    let st = d[b + 52];
    let level = (st & 0x0F) as u32;
    let pct = (level * 10 + 5).min(100) as u8;
    match st >> 4 {
        0 => {
            s.battery = Some(pct);
            s.charge = Charge::Discharging;
        }
        1 => {
            s.battery = Some(pct);
            s.charge = Charge::Charging;
        }
        2 => {
            s.battery = Some(100);
            s.charge = Charge::Full;
        }
        _ => {
            s.battery = None;
            s.charge = Charge::Error;
        }
    }
    Some(s)
}

/// Parses a DualShock 4 input report (optional extra, untested on hardware).
pub fn parse_ds4(d: &[u8]) -> Option<PadState> {
    let (b, conn) = match (d.first().copied()?, d.len()) {
        (0x01, n) if n >= 64 => (1, Conn::Usb),
        (0x11, n) if n >= 70 => (3, Conn::Bluetooth),
        (0x01, n) if n >= 10 => (1, Conn::Bluetooth),
        _ => return None,
    };
    let mut s = PadState { conn, ..PadState::default() };
    s.lx = d[b];
    s.ly = d[b + 1];
    s.rx = d[b + 2];
    s.ry = d[b + 3];
    apply_buttons(&mut s, d[b + 4], d[b + 5], d[b + 6]);
    s.l2 = d[b + 7];
    s.r2 = d[b + 8];
    s.set(Btn::L2, s.l2 > 0);
    s.set(Btn::R2, s.r2 > 0);
    if d.len() < b + 30 {
        s.reduced = true;
        return Some(s);
    }
    for i in 0..3 {
        s.gyro[i] = le16(d, b + 12 + 2 * i) as f32 * 0.0625;
        s.accel[i] = le16(d, b + 18 + 2 * i) as f32 / 8192.0;
    }
    let st = d[b + 29];
    let level = (st & 0x0F) as u32;
    let cable = st & 0x10 != 0;
    let max = if cable { 11 } else { 8 };
    s.battery = Some(((level * 100) / max).min(100) as u8);
    s.charge = if cable {
        if level >= 11 {
            Charge::Full
        } else {
            Charge::Charging
        }
    } else {
        Charge::Discharging
    };
    Some(s)
}

pub fn parse(kind: PadKind, d: &[u8], cal: &Calib) -> Option<PadState> {
    match kind {
        PadKind::DualSense => parse_dualsense(d, cal),
        PadKind::DualShock4 => parse_ds4(d),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::demo::{bt_report_from, usb_report_from};

    #[test]
    fn usb_fixture_parses_every_field() {
        let mut r = [0u8; 64];
        r[0] = 0x01;
        r[1..7].copy_from_slice(&[0, 255, 64, 192, 10, 200]);
        r[8] = 0x02 | 0x20 | 0x80; // hat 2 (right), cross, triangle
        r[9] = 0x01 | 0x10 | 0x80; // L1, Create, R3
        r[10] = 0x01 | 0x04; // PS, Mute
        // gyro 100, -200, 300; accel 8192, 0, -8192
        for (i, v) in [100i16, -200, 300, 8192, 0, -8192].iter().enumerate() {
            r[16 + 2 * i..18 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        r[28..32].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        // finger 0 down at (1000, 500), id 5; finger 1 up
        r[33] = 5;
        r[34] = (1000 & 0xFF) as u8;
        r[35] = ((1000 >> 8) & 0x0F) as u8 | (((500 & 0x0F) as u8) << 4);
        r[36] = (500 >> 4) as u8;
        r[37] = 0x80;
        r[53] = 0x07 | (1 << 4); // 7 -> 75 %, charging
        let s = parse_dualsense(&r, &Calib::default()).unwrap();
        assert_eq!((s.lx, s.ly, s.rx, s.ry, s.l2, s.r2), (0, 255, 64, 192, 10, 200));
        assert!(s.pressed(Btn::Right) && !s.pressed(Btn::Up) && !s.pressed(Btn::Down) && !s.pressed(Btn::Left));
        assert!(s.pressed(Btn::Cross) && s.pressed(Btn::Triangle) && !s.pressed(Btn::Circle) && !s.pressed(Btn::Square));
        assert!(s.pressed(Btn::L1) && s.pressed(Btn::Create) && s.pressed(Btn::R3) && !s.pressed(Btn::R1));
        assert!(s.pressed(Btn::Ps) && s.pressed(Btn::Mute) && !s.pressed(Btn::Touchpad));
        assert_eq!(s.conn, Conn::Usb);
        assert_eq!(s.counter, 0x0102_0304);
        assert!(s.touch[0].active && s.touch[0].id == 5 && s.touch[0].x == 1000 && s.touch[0].y == 500);
        assert!(!s.touch[1].active);
        assert_eq!((s.battery, s.charge), (Some(75), Charge::Charging));
        // Default calibration: 100 counts of gyro = 100 * 2000 / 32768 deg/s, 8192 counts = 1 g.
        assert!((s.gyro[0] - 6.1035).abs() < 0.01);
        assert!((s.accel[0] - 1.0).abs() < 1e-6 && (s.accel[2] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn hat_directions_and_diagonals() {
        let hat = |h: u8| {
            let mut r = [0u8; 64];
            r[0] = 1;
            r[8] = h;
            let s = parse_dualsense(&r, &Calib::default()).unwrap();
            (s.pressed(Btn::Up), s.pressed(Btn::Right), s.pressed(Btn::Down), s.pressed(Btn::Left))
        };
        assert_eq!(hat(0), (true, false, false, false));
        assert_eq!(hat(1), (true, true, false, false));
        assert_eq!(hat(2), (false, true, false, false));
        assert_eq!(hat(3), (false, true, true, false));
        assert_eq!(hat(4), (false, false, true, false));
        assert_eq!(hat(5), (false, false, true, true));
        assert_eq!(hat(6), (false, false, false, true));
        assert_eq!(hat(7), (true, false, false, true));
        assert_eq!(hat(8), (false, false, false, false));
    }

    #[test]
    fn bluetooth_full_report_has_one_extra_byte() {
        let mut st = PadState::default();
        st.lx = 10;
        st.l2 = 77;
        st.set(Btn::Square, true);
        st.set(Btn::Touchpad, true);
        st.battery = Some(55);
        let usb = usb_report_from(&st);
        let bt = bt_report_from(&st);
        assert_eq!(bt.len(), 78);
        assert_eq!(bt[0], 0x31);
        let a = parse_dualsense(&usb, &Calib::default()).unwrap();
        let b = parse_dualsense(&bt, &Calib::default()).unwrap();
        assert_eq!((a.conn, b.conn), (Conn::Usb, Conn::Bluetooth));
        assert_eq!((a.lx, a.l2), (10, 77));
        assert_eq!((b.lx, b.l2), (10, 77));
        assert!(b.pressed(Btn::Square) && b.pressed(Btn::Touchpad));
        assert_eq!(b.battery, a.battery);
    }

    #[test]
    fn reduced_bluetooth_report() {
        let r = [0x01, 128, 130, 120, 100, 0x24, 0x02, 0x03, 40, 0];
        let s = parse_dualsense(&r, &Calib::default()).unwrap();
        assert!(s.reduced && s.conn == Conn::Bluetooth);
        assert_eq!((s.lx, s.ly, s.rx, s.ry, s.l2, s.r2), (128, 130, 120, 100, 40, 0));
        assert!(s.pressed(Btn::Cross) && s.pressed(Btn::Down) && s.pressed(Btn::R1) && s.pressed(Btn::Ps) && s.pressed(Btn::Touchpad));
        assert!(s.pressed(Btn::L2) && !s.pressed(Btn::R2));
        assert_eq!(s.battery, None);
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse_dualsense(&[], &Calib::default()).is_none());
        assert!(parse_dualsense(&[0x01, 1, 2], &Calib::default()).is_none());
        assert!(parse_dualsense(&[0x77; 64], &Calib::default()).is_none());
        assert!(parse_dualsense(&[0x31; 40], &Calib::default()).is_none());
    }

    #[test]
    fn battery_states() {
        let mut r = [0u8; 64];
        r[0] = 1;
        r[53] = 0x0A;
        assert_eq!(parse_dualsense(&r, &Calib::default()).unwrap().battery, Some(100));
        r[53] = 0x20;
        let s = parse_dualsense(&r, &Calib::default()).unwrap();
        assert_eq!((s.battery, s.charge), (Some(100), Charge::Full));
        r[53] = 0xA3;
        let s = parse_dualsense(&r, &Calib::default()).unwrap();
        assert_eq!((s.battery, s.charge), (None, Charge::Error));
        r[53] = 0x00;
        assert_eq!(parse_dualsense(&r, &Calib::default()).unwrap().battery, Some(5));
    }

    fn calib_report(vals: [i16; 17]) -> Vec<u8> {
        let mut d = vec![0u8; 41];
        d[0] = 0x05;
        for (i, v) in vals.iter().enumerate() {
            d[1 + 2 * i..3 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        d
    }

    #[test]
    fn calibration_maths() {
        // biases 10, -20, 30; each gyro pair +-8192; speed 540 + 540; accel x +8300/-8100, y +-8192, z +-8192.
        let d = calib_report([10, -20, 30, 8192, -8192, 8192, -8192, 8192, -8192, 540, 540, 8300, -8100, 8192, -8192, 8192, -8192]);
        let c = Calib::from_report(&d).unwrap();
        assert!(c.from_pad);
        // Raw equal to the bias is rest.
        assert!(c.gyro[0].apply(10).abs() < 1e-4 && c.gyro[1].apply(-20).abs() < 1e-4);
        // bias + 8192 counts is half of the 1080 span: 540 deg/s.
        assert!((c.gyro[0].apply(10 + 8192) - 540.0).abs() < 0.5);
        assert!((c.gyro[2].apply(30 - 8192) + 540.0).abs() < 0.5);
        // Accel x: bias = 8300 - 8200 = 100, range 16400; +1 g at 8300, -1 g at -8100, 0 g at 100.
        assert!((c.accel[0].apply(8300) - 1.0).abs() < 1e-4);
        assert!((c.accel[0].apply(-8100) + 1.0).abs() < 1e-4);
        assert!(c.accel[0].apply(100).abs() < 1e-4);
        assert!((c.accel[2].apply(8192) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn broken_calibration_falls_back() {
        assert!(Calib::from_report(&calib_report([0; 17])).is_none());
        assert!(Calib::from_report(&[0x05; 10]).is_none());
        assert!(Calib::from_report(&[0x06; 41]).is_none());
        let c = Calib::default();
        assert!(!c.from_pad);
        assert_eq!(c.accel[1].apply(8192), 1.0);
    }

    #[test]
    fn dualshock4_fixture() {
        let mut r = [0u8; 64];
        r[0] = 0x01;
        r[1..5].copy_from_slice(&[1, 2, 3, 4]);
        r[5] = 0x08 | 0x40; // hat none, circle
        r[6] = 0x02 | 0x20; // R1, Options
        r[7] = 0x01;
        r[8] = 9;
        r[9] = 99;
        r[30] = 0x10 | 0x05; // cable, level 5 of 11
        let s = parse_ds4(&r).unwrap();
        assert_eq!((s.lx, s.ry, s.l2, s.r2), (1, 4, 9, 99));
        assert!(s.pressed(Btn::Circle) && s.pressed(Btn::R1) && s.pressed(Btn::Options) && s.pressed(Btn::Ps));
        assert_eq!((s.battery, s.charge), (Some(45), Charge::Charging));
        assert_eq!(s.conn, Conn::Usb);
    }
}
