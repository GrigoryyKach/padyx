//! The parsed pad state. Plain data, serialisable (the background process sends it to the window).

use serde::{Deserialize, Serialize};

pub const SONY_VID: u16 = 0x054C;
pub const DUALSENSE_PID: u16 = 0x0CE6;
pub const DS4_PID_V1: u16 = 0x05C4;
pub const DS4_PID_V2: u16 = 0x09CC;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PadKind {
    #[default]
    DualSense,
    /// Optional extra, parsed but never tested on hardware.
    DualShock4,
}

impl PadKind {
    pub fn from_pid(pid: u16) -> Option<PadKind> {
        match pid {
            DUALSENSE_PID => Some(PadKind::DualSense),
            DS4_PID_V1 | DS4_PID_V2 => Some(PadKind::DualShock4),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            PadKind::DualSense => "DualSense",
            PadKind::DualShock4 => "DualShock 4",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Conn {
    #[default]
    Unknown,
    Usb,
    Bluetooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Charge {
    #[default]
    Discharging,
    Charging,
    Full,
    Error,
}

/// Every digital control the tester shows. The bit of a button in [`PadState::buttons`] is `1 << (b as u32)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Btn {
    Up,
    Down,
    Left,
    Right,
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L3,
    R3,
    Create,
    Options,
    Ps,
    Touchpad,
    Mute,
    /// Digital click of the triggers (the analog value is separate). Not part of the "press all 17" checklist.
    L2,
    R2,
}

impl Btn {
    #[allow(dead_code)]
    pub const ALL: [Btn; 19] = [
        Btn::Up, Btn::Down, Btn::Left, Btn::Right, Btn::Cross, Btn::Circle, Btn::Square, Btn::Triangle, Btn::L1, Btn::R1, Btn::L3, Btn::R3,
        Btn::Create, Btn::Options, Btn::Ps, Btn::Touchpad, Btn::Mute, Btn::L2, Btn::R2,
    ];
    /// The checklist: 17 controls, without the trigger clicks.
    pub const CHECKLIST: [Btn; 17] = [
        Btn::Up, Btn::Down, Btn::Left, Btn::Right, Btn::Cross, Btn::Circle, Btn::Square, Btn::Triangle, Btn::L1, Btn::R1, Btn::L3, Btn::R3,
        Btn::Create, Btn::Options, Btn::Ps, Btn::Touchpad, Btn::Mute,
    ];
    /// The tester's "press every control" list: the 17 plus the two triggers, which tick when pulled past half.
    pub const TESTER: [Btn; 19] = [
        Btn::Up, Btn::Down, Btn::Left, Btn::Right, Btn::Cross, Btn::Circle, Btn::Square, Btn::Triangle, Btn::L1, Btn::R1, Btn::L2, Btn::R2,
        Btn::L3, Btn::R3, Btn::Create, Btn::Options, Btn::Ps, Btn::Touchpad, Btn::Mute,
    ];
    pub fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Finger {
    pub active: bool,
    pub id: u8,
    /// 0..1919
    pub x: u16,
    /// 0..1079
    pub y: u16,
}

pub const TOUCH_W: f32 = 1920.0;
pub const TOUCH_H: f32 = 1080.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadState {
    /// Raw stick bytes, 0..255, about 128 at rest. Y grows downwards, as the pad reports it.
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
    pub l2: u8,
    pub r2: u8,
    pub buttons: u32,
    pub touch: [Finger; 2],
    /// Degrees per second after calibration (pitch, yaw, roll).
    pub gyro: [f32; 3],
    /// g after calibration (x, y, z).
    pub accel: [f32; 3],
    /// 0..100, `None` when the report has no battery field (the reduced Bluetooth report).
    pub battery: Option<u8>,
    pub charge: Charge,
    pub conn: Conn,
    /// The reduced Bluetooth report: no motion, touch or battery data yet.
    pub reduced: bool,
    /// The pad's own report counter / sensor timestamp, for diagnostics.
    pub counter: u32,
}

impl Default for PadState {
    fn default() -> Self {
        Self {
            lx: 128,
            ly: 128,
            rx: 128,
            ry: 128,
            l2: 0,
            r2: 0,
            buttons: 0,
            touch: [Finger::default(); 2],
            gyro: [0.0; 3],
            accel: [0.0, 1.0, 0.0],
            battery: None,
            charge: Charge::Discharging,
            conn: Conn::Unknown,
            reduced: false,
            counter: 0,
        }
    }
}

/// A stick position, -1..1, Y positive up (what a user expects on a plot).
pub fn stick_norm(x: u8, y: u8) -> (f32, f32) {
    ((x as f32 - 127.5) / 127.5, -(y as f32 - 127.5) / 127.5)
}

impl PadState {
    pub fn pressed(&self, b: Btn) -> bool {
        self.buttons & b.bit() != 0
    }
    pub fn set(&mut self, b: Btn, on: bool) {
        if on {
            self.buttons |= b.bit();
        } else {
            self.buttons &= !b.bit();
        }
    }
    pub fn left(&self) -> (f32, f32) {
        stick_norm(self.lx, self.ly)
    }
    pub fn right(&self) -> (f32, f32) {
        stick_norm(self.rx, self.ry)
    }
    pub fn trigger_l(&self) -> f32 {
        self.l2 as f32 / 255.0
    }
    pub fn trigger_r(&self) -> f32 {
        self.r2 as f32 / 255.0
    }
}

/// Who the pad is, as the operating system reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PadInfo {
    /// Short stable id of the device path, used in messages.
    pub uid: String,
    pub kind: PadKind,
    pub conn: Conn,
    pub serial: String,
    pub demo: bool,
    /// The system path of the HID device (Windows: needed to hide it; empty for the demo pad).
    #[serde(default)]
    pub path: String,
}

/// Report-timing statistics, measured on the reader thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Timing {
    /// Reports per second over the last second.
    pub hz: f32,
    /// Interval between the last reports, microseconds, newest last (at most 120).
    pub recent_us: Vec<u32>,
    pub avg_ms: f32,
    pub max_ms: f32,
    pub jitter_ms: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct PadSnapshot {
    pub info: PadInfo,
    pub state: PadState,
    pub timing: Timing,
    pub out: crate::pad::output::Output,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub pads: Vec<PadSnapshot>,
    /// A pad is there but the system refused to open it (Linux without the udev rule).
    pub access_denied: bool,
    pub seq: u64,
    /// The Xbox emulation (`present: false` when this process has no emulator).
    #[serde(default)]
    pub emu: crate::emu::EmuStatus,
}
