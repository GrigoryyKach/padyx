//! What we want the pad to do (lightbar, player LEDs, mute LED, rumble, adaptive triggers) and the output
//! reports that say so.
//!
//! USB output report `0x02` is 63 bytes: the id, then 47 bytes of "common" data, then zeros. The Bluetooth
//! report `0x31` is 78 bytes: id, a sequence byte (high nibble), the tag `0x10`, the same 47 bytes, zeros, and a
//! CRC32 of `0xA2` followed by the first 74 bytes. A pad ignores a Bluetooth report with a wrong CRC.
//!
//! Common data (offsets into the 47 bytes), from the public notes of the Linux driver and SDL:
//!
//! | offset | field |
//! |--------|-------|
//! | 0      | valid flags 0: bit 0 and 1 compatible vibration, 2 right trigger effect, 3 left trigger effect |
//! | 1      | valid flags 1: bit 0 mute LED, 2 lightbar, 4 player LEDs |
//! | 2, 3   | rumble right (small, fast) and left (large, slow) |
//! | 8      | mute LED: 0 off, 1 on, 2 pulse |
//! | 10..20 | right trigger effect (mode byte, then parameters) |
//! | 21..31 | left trigger effect |
//! | 38     | valid flags 2: bit 1 lightbar setup |
//! | 41     | lightbar setup (2 turns the default blue light off so ours shows) |
//! | 42     | LED brightness (0 full, 1 medium, 2 low) |
//! | 43     | player LEDs, 5 bits, bit 0 the leftmost |
//! | 44..46 | lightbar red, green, blue |

use super::state::Conn;
use serde::{Deserialize, Serialize};

/// A reflected CRC-32 (the one in zlib and PNG).
pub fn crc32(seed: &[u8], data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in seed.iter().chain(data.iter()) {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MuteLed {
    #[default]
    Off,
    On,
    Pulse,
}

/// An adaptive trigger effect. Positions are 0..9 along the travel, forces 0..8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Trigger {
    #[default]
    Off,
    /// Constant resistance from `start` to the end of the travel.
    Resistance { start: u8, force: u8 },
    /// A stiff section between `start` and `end` that snaps at the end, like a weapon.
    Weapon { start: u8, end: u8, force: u8 },
    /// A buzz from `start` on: `amplitude` 1..8, `frequency` in Hz.
    Vibration { start: u8, amplitude: u8, frequency: u8 },
}

impl Trigger {
    /// The 11 bytes the pad expects.
    pub fn bytes(self) -> [u8; 11] {
        let mut b = [0u8; 11];
        match self {
            Trigger::Off => b[0] = 0x05,
            Trigger::Resistance { start, force } => {
                b[0] = 0x01;
                b[1] = start.min(9);
                b[2] = (force.min(8) as u16 * 255 / 8) as u8;
            }
            Trigger::Weapon { start, end, force } => {
                let s = start.clamp(2, 7);
                let e = end.clamp(s + 1, 8);
                let zones: u16 = (1 << s) | (1 << e);
                b[0] = 0x25;
                b[1] = (zones & 0xFF) as u8;
                b[2] = (zones >> 8) as u8;
                b[3] = force.clamp(1, 8) - 1;
            }
            Trigger::Vibration { start, amplitude, frequency } => {
                let amp = amplitude.clamp(1, 8) - 1;
                let first = start.min(9) as u32;
                let mut zones = 0u16;
                let mut strengths = 0u32;
                for z in first..10 {
                    zones |= 1 << z;
                    strengths |= (amp as u32 & 7) << (3 * z);
                }
                b[0] = 0x26;
                b[1] = (zones & 0xFF) as u8;
                b[2] = (zones >> 8) as u8;
                b[3..7].copy_from_slice(&strengths.to_le_bytes());
                b[9] = frequency;
            }
        }
        b
    }
}

/// The wanted state of every output. Cloneable and serialisable: the window sends it to the background process.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub light: [u8; 3],
    /// 0..255, scales the lightbar colour.
    pub brightness: u8,
    /// Five bits, bit 0 the leftmost LED.
    pub players: u8,
    pub mute_led: MuteLed,
    pub rumble_left: u8,
    pub rumble_right: u8,
    pub trigger_left: Trigger,
    pub trigger_right: Trigger,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            light: [0xab, 0x7c, 0xf0],
            brightness: 255,
            players: 0b00100,
            mute_led: MuteLed::Off,
            rumble_left: 0,
            rumble_right: 0,
            trigger_left: Trigger::Off,
            trigger_right: Trigger::Off,
        }
    }
}

impl Output {
    /// The lightbar colour after the brightness.
    pub fn light_scaled(&self) -> [u8; 3] {
        let k = self.brightness as u32;
        [0, 1, 2].map(|i| ((self.light[i] as u32 * k + 127) / 255) as u8)
    }

    /// Something that must be refreshed now and then (the pad stops rumble and effects on its own).
    pub fn is_active(&self) -> bool {
        self.rumble_left > 0 || self.rumble_right > 0 || self.trigger_left != Trigger::Off || self.trigger_right != Trigger::Off
    }

    /// Players pattern for the usual "player N" display (1..=4), 0 for none.
    pub fn player_pattern(n: u8) -> u8 {
        match n {
            1 => 0b00100,
            2 => 0b01010,
            3 => 0b10101,
            4 => 0b11011,
            5 => 0b11111,
            _ => 0,
        }
    }

    /// The 47 bytes of common data. `lightbar` is false when another route (the kernel LED class) owns the colour.
    pub fn common(&self, lightbar: bool) -> [u8; 47] {
        let mut c = [0u8; 47];
        c[0] = 0x01 | 0x02 | 0x04 | 0x08;
        c[1] = 0x01 | 0x10 | if lightbar { 0x04 } else { 0 };
        c[2] = self.rumble_right;
        c[3] = self.rumble_left;
        c[8] = match self.mute_led {
            MuteLed::Off => 0,
            MuteLed::On => 1,
            MuteLed::Pulse => 2,
        };
        c[10..21].copy_from_slice(&self.trigger_right.bytes());
        c[21..32].copy_from_slice(&self.trigger_left.bytes());
        if lightbar {
            c[38] = 0x02;
            c[41] = 0x02;
        }
        c[42] = 0;
        c[43] = self.players & 0x1F;
        let rgb = self.light_scaled();
        c[44] = rgb[0];
        c[45] = rgb[1];
        c[46] = rgb[2];
        c
    }
}

/// The full output report for a DualSense. `seq` is the Bluetooth sequence number (0..15).
pub fn build_dualsense(conn: Conn, out: &Output, seq: u8, lightbar: bool) -> Vec<u8> {
    let common = out.common(lightbar);
    match conn {
        Conn::Bluetooth => {
            let mut r = vec![0u8; 78];
            r[0] = 0x31;
            r[1] = (seq & 0x0F) << 4;
            r[2] = 0x10;
            r[3..50].copy_from_slice(&common);
            let crc = crc32(&[0xA2], &r[..74]);
            r[74..78].copy_from_slice(&crc.to_le_bytes());
            r
        }
        _ => {
            let mut r = vec![0u8; 63];
            r[0] = 0x02;
            r[1..48].copy_from_slice(&common);
            r
        }
    }
}

/// A valid Bluetooth report has its CRC in the last four bytes.
#[cfg(test)]
pub fn bt_crc_ok(r: &[u8]) -> bool {
    r.len() == 78 && r[0] == 0x31 && crc32(&[0xA2], &r[..74]).to_le_bytes() == r[74..78]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(&[], b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(&[], b""), 0);
        assert_eq!(crc32(&[], b"a"), 0xE8B7_BE43);
        // The seed byte is just a prefix.
        assert_eq!(crc32(&[0xA2], b"xyz"), crc32(&[], &[0xA2, b'x', b'y', b'z']));
        assert_eq!(crc32(&[b'1', b'2', b'3'], b"456789"), 0xCBF4_3926);
    }

    #[test]
    fn bluetooth_report_layout_and_crc() {
        let mut o = Output::default();
        o.light = [10, 20, 30];
        o.brightness = 255;
        o.rumble_left = 200;
        o.rumble_right = 100;
        let r = build_dualsense(Conn::Bluetooth, &o, 5, true);
        assert_eq!(r.len(), 78);
        assert_eq!((r[0], r[1], r[2]), (0x31, 0x50, 0x10));
        // common data starts at byte 3
        assert_eq!(&r[3 + 44..3 + 47], &[10, 20, 30]);
        assert_eq!((r[3 + 2], r[3 + 3]), (100, 200));
        assert!(bt_crc_ok(&r));
        // Any change breaks the CRC.
        let mut bad = r.clone();
        bad[20] ^= 1;
        assert!(!bt_crc_ok(&bad));
        // The CRC is over 0xA2 and the first 74 bytes, little endian.
        let want = crc32(&[], &[&[0xA2u8][..], &r[..74]].concat());
        assert_eq!(u32::from_le_bytes(r[74..78].try_into().unwrap()), want);
        // The sequence number is the high nibble and wraps at 16.
        assert_eq!(build_dualsense(Conn::Bluetooth, &o, 17, true)[1], 0x10);
    }

    #[test]
    fn usb_report_layout() {
        let mut o = Output::default();
        o.players = Output::player_pattern(3);
        o.mute_led = MuteLed::Pulse;
        let r = build_dualsense(Conn::Usb, &o, 0, true);
        assert_eq!(r.len(), 63);
        assert_eq!(r[0], 0x02);
        assert_eq!(r[1 + 8], 2);
        assert_eq!(r[1 + 43], 0b10101);
        // flags: lightbar control and setup are on when we own the lightbar
        assert!(r[2] & 0x04 != 0 && r[1 + 38] == 0x02 && r[1 + 41] == 0x02);
        let k = build_dualsense(Conn::Usb, &o, 0, false);
        assert!(k[2] & 0x04 == 0 && k[1 + 38] == 0 && k[1 + 41] == 0);
    }

    #[test]
    fn brightness_scales_the_colour() {
        let mut o = Output::default();
        o.light = [255, 128, 0];
        o.brightness = 255;
        assert_eq!(o.light_scaled(), [255, 128, 0]);
        o.brightness = 0;
        assert_eq!(o.light_scaled(), [0, 0, 0]);
        o.brightness = 128;
        assert_eq!(o.light_scaled(), [128, 64, 0]);
    }

    #[test]
    fn trigger_effect_bytes() {
        assert_eq!(Trigger::Off.bytes()[0], 0x05);
        let r = Trigger::Resistance { start: 3, force: 8 }.bytes();
        assert_eq!((r[0], r[1], r[2]), (0x01, 3, 255));
        let w = Trigger::Weapon { start: 2, end: 5, force: 8 }.bytes();
        assert_eq!(w[0], 0x25);
        assert_eq!(u16::from_le_bytes([w[1], w[2]]), (1 << 2) | (1 << 5));
        assert_eq!(w[3], 7);
        // A weapon with an end before the start is repaired.
        let w = Trigger::Weapon { start: 6, end: 1, force: 0 }.bytes();
        assert_eq!(u16::from_le_bytes([w[1], w[2]]), (1 << 6) | (1 << 7));
        assert_eq!(w[3], 0);
        let v = Trigger::Vibration { start: 7, amplitude: 8, frequency: 40 }.bytes();
        assert_eq!(v[0], 0x26);
        assert_eq!(u16::from_le_bytes([v[1], v[2]]), (1 << 7) | (1 << 8) | (1 << 9));
        assert_eq!(v[9], 40);
        assert_eq!(u32::from_le_bytes([v[3], v[4], v[5], v[6]]) >> 21, 0b111_111_111);
    }

    #[test]
    fn activity_flag() {
        let mut o = Output::default();
        assert!(!o.is_active());
        o.rumble_left = 1;
        assert!(o.is_active());
        o.rumble_left = 0;
        o.trigger_right = Trigger::Resistance { start: 0, force: 3 };
        assert!(o.is_active());
    }

    #[test]
    fn output_round_trips_as_json() {
        let mut o = Output::default();
        o.trigger_left = Trigger::Weapon { start: 2, end: 4, force: 5 };
        let s = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<Output>(&s).unwrap(), o);
    }
}
