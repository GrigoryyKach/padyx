//! The mapping layer: a [`PadState`] in, an [`XboxState`] out. Pure, no I/O, no allocation.
//!
//! It is built as three small stages so the next section (remap, macros, turbo, profiles) can extend it
//! without touching the reader or the virtual pad:
//!
//! 1. [`buttons`]: every digital control through the table [`BASE_BUTTONS`] (positional DualSense to Xbox),
//!    plus the two configurable extras (touchpad click, Mute);
//! 2. [`sticks`]: the radial dead zones and curves of `zones`;
//! 3. [`triggers`]: the trigger dead zones.
//!
//! To add remapping, replace the table lookup in `buttons` with a per-profile table and add a stage after
//! `map_pad` that edits the returned [`XboxState`] (macros and turbo do exactly that: they only write to the
//! virtual pad). Nothing outside this module knows how the mapping is done.

use super::xbox::{XBtn, XboxState};
use super::zones::{to_axis, StickCfg, TrigCfg};
use crate::pad::{Btn, PadState};
use serde::{Deserialize, Serialize};

/// The positional mapping of the DualSense to the Xbox pad (Cross is A, Circle is B, and so on).
pub const BASE_BUTTONS: [(Btn, XBtn); 14] = [
    (Btn::Up, XBtn::Up),
    (Btn::Down, XBtn::Down),
    (Btn::Left, XBtn::Left),
    (Btn::Right, XBtn::Right),
    (Btn::Cross, XBtn::A),
    (Btn::Circle, XBtn::B),
    (Btn::Square, XBtn::X),
    (Btn::Triangle, XBtn::Y),
    (Btn::L1, XBtn::Lb),
    (Btn::R1, XBtn::Rb),
    (Btn::L3, XBtn::L3),
    (Btn::R3, XBtn::R3),
    (Btn::Create, XBtn::Back),
    (Btn::Options, XBtn::Start),
];

/// PS is the Guide button. It is in the base table's spirit but kept apart so a profile can free it.
pub const PS_BUTTON: (Btn, XBtn) = (Btn::Ps, XBtn::Guide);

/// What the user can change in the emulation (saved in `config.json` under `emu`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmuConfig {
    /// The Xbox button the touchpad click acts as (`None`: unmapped). Default View (Back).
    pub touchpad: Option<XBtn>,
    /// The Xbox button Mute acts as. Default: none.
    pub mute: Option<XBtn>,
    pub left: StickCfg,
    pub right: StickCfg,
    pub lt: TrigCfg,
    pub rt: TrigCfg,
    /// How strongly the game's rumble reaches the DualSense, 0..100 percent.
    pub rumble_pct: u8,
    /// Pads in native mode ("the game supports DualSense"), by serial number (or the id when there is none).
    pub native: Vec<String>,
}

impl Default for EmuConfig {
    fn default() -> Self {
        Self {
            touchpad: Some(XBtn::Back),
            mute: None,
            left: StickCfg::default(),
            right: StickCfg::default(),
            lt: TrigCfg::default(),
            rt: TrigCfg::default(),
            rumble_pct: 100,
            native: Vec::new(),
        }
    }
}

impl EmuConfig {
    pub fn sanitize(&mut self) {
        self.left.sanitize();
        self.right.sanitize();
        self.lt.sanitize();
        self.rt.sanitize();
        self.rumble_pct = self.rumble_pct.min(100);
        self.native.sort();
        self.native.dedup();
    }

    /// A pad is in native mode when its key (serial, or the id when the pad has no serial) is listed.
    pub fn is_native(&self, key: &str) -> bool {
        self.native.iter().any(|k| k == key)
    }

    pub fn set_native(&mut self, key: &str, on: bool) {
        self.native.retain(|k| k != key);
        if on {
            self.native.push(key.to_string());
        }
    }
}

/// The key under which a pad's native-mode switch is stored: the serial (the Bluetooth address, the same over
/// USB and Bluetooth), or the id when the pad has none.
pub fn pad_key(serial: &str, uid: &str) -> String {
    if serial.is_empty() {
        uid.to_string()
    } else {
        serial.to_lowercase()
    }
}

/// Stage 1: buttons.
pub fn buttons(pad: &PadState, cfg: &EmuConfig) -> u16 {
    let mut b = 0u16;
    for (from, to) in BASE_BUTTONS.iter().chain(std::iter::once(&PS_BUTTON)) {
        if pad.pressed(*from) {
            b |= to.mask();
        }
    }
    if pad.pressed(Btn::Touchpad) {
        if let Some(x) = cfg.touchpad {
            b |= x.mask();
        }
    }
    if pad.pressed(Btn::Mute) {
        if let Some(x) = cfg.mute {
            b |= x.mask();
        }
    }
    b
}

/// Stage 2: the sticks, after their dead zones. Each is -1..1 with Y up.
pub fn sticks(pad: &PadState, cfg: &EmuConfig) -> ((f32, f32), (f32, f32)) {
    let (lx, ly) = pad.left();
    let (rx, ry) = pad.right();
    (cfg.left.apply(lx, ly), cfg.right.apply(rx, ry))
}

/// Stage 3: the triggers.
pub fn triggers(pad: &PadState, cfg: &EmuConfig) -> (u8, u8) {
    (cfg.lt.apply(pad.l2), cfg.rt.apply(pad.r2))
}

/// The whole mapping.
pub fn map_pad(pad: &PadState, cfg: &EmuConfig) -> XboxState {
    let (l, r) = sticks(pad, cfg);
    let (lt, rt) = triggers(pad, cfg);
    XboxState { buttons: buttons(pad, cfg), lt, rt, lx: to_axis(l.0), ly: to_axis(l.1), rx: to_axis(r.0), ry: to_axis(r.1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(btns: &[Btn]) -> PadState {
        let mut s = PadState::default();
        for b in btns {
            s.set(*b, true);
        }
        s
    }

    #[test]
    fn face_buttons_map_by_position() {
        let c = EmuConfig::default();
        let m = |b: Btn| buttons(&press(&[b]), &c);
        assert_eq!(m(Btn::Cross), XBtn::A.mask());
        assert_eq!(m(Btn::Circle), XBtn::B.mask());
        assert_eq!(m(Btn::Square), XBtn::X.mask());
        assert_eq!(m(Btn::Triangle), XBtn::Y.mask());
        assert_eq!(m(Btn::Create), XBtn::Back.mask());
        assert_eq!(m(Btn::Options), XBtn::Start.mask());
        assert_eq!(m(Btn::Ps), XBtn::Guide.mask());
        assert_eq!(m(Btn::L1), XBtn::Lb.mask());
        assert_eq!(m(Btn::R1), XBtn::Rb.mask());
        assert_eq!(m(Btn::L3), XBtn::L3.mask());
        assert_eq!(m(Btn::R3), XBtn::R3.mask());
        for (d, x) in [(Btn::Up, XBtn::Up), (Btn::Down, XBtn::Down), (Btn::Left, XBtn::Left), (Btn::Right, XBtn::Right)] {
            assert_eq!(m(d), x.mask());
        }
    }

    #[test]
    fn every_dualsense_control_has_exactly_one_target_or_is_configurable() {
        // The 17 checklist controls: 15 are in the table, Touchpad and Mute are the two configurable ones.
        let in_table: Vec<Btn> = BASE_BUTTONS.iter().map(|(b, _)| *b).chain([PS_BUTTON.0]).collect();
        for b in Btn::CHECKLIST {
            let listed = in_table.contains(&b);
            assert!(listed || matches!(b, Btn::Touchpad | Btn::Mute), "{b:?} has no target");
        }
        // No two controls land on the same Xbox button in the base table.
        let mut seen = 0u16;
        for (_, x) in BASE_BUTTONS.iter().chain([&PS_BUTTON]) {
            assert_eq!(seen & x.mask(), 0);
            seen |= x.mask();
        }
    }

    #[test]
    fn touchpad_defaults_to_back_and_is_configurable_and_mute_is_unmapped() {
        let mut c = EmuConfig::default();
        assert_eq!(buttons(&press(&[Btn::Touchpad]), &c), XBtn::Back.mask());
        assert_eq!(buttons(&press(&[Btn::Mute]), &c), 0, "Mute is unmapped by default");
        c.touchpad = Some(XBtn::Guide);
        c.mute = Some(XBtn::L3);
        assert_eq!(buttons(&press(&[Btn::Touchpad, Btn::Mute]), &c), XBtn::Guide.mask() | XBtn::L3.mask());
        c.touchpad = None;
        assert_eq!(buttons(&press(&[Btn::Touchpad]), &c), 0);
    }

    #[test]
    fn trigger_clicks_do_not_press_buttons() {
        let c = EmuConfig::default();
        assert_eq!(buttons(&press(&[Btn::L2, Btn::R2]), &c), 0);
    }

    #[test]
    fn a_resting_pad_is_a_resting_xbox_pad() {
        let x = map_pad(&PadState::default(), &EmuConfig::default());
        assert_eq!(x, XboxState::default());
        // Bytes 127 and 128 are both within a few percent of the centre.
        let mut p = PadState::default();
        p.lx = 127;
        p.ly = 129;
        assert_eq!(map_pad(&p, &EmuConfig::default()).lx, 0);
    }

    #[test]
    fn stick_y_goes_up_and_triggers_pass_through() {
        let mut p = PadState::default();
        p.ly = 0; // the pad reports "up" as a small number
        p.rx = 255;
        p.l2 = 200;
        p.r2 = 255;
        let x = map_pad(&p, &EmuConfig::default());
        assert!(x.ly > 30000, "up is positive on the Xbox pad: {}", x.ly);
        assert!(x.rx > 30000);
        assert_eq!((x.lt, x.rt), (200, 255));
    }

    #[test]
    fn dead_zones_apply_per_stick_and_per_trigger() {
        let mut c = EmuConfig::default();
        c.left.inner = 0.3;
        c.rt.inner = 0.5;
        let mut p = PadState::default();
        p.lx = 128 + 30; // about 0.24
        p.rx = 128 + 30;
        p.r2 = 100;
        let x = map_pad(&p, &c);
        assert_eq!(x.lx, 0, "inside the left stick's wider zone");
        assert!(x.rx > 0, "the right stick keeps its 5 percent zone");
        assert_eq!(x.rt, 0);
    }

    #[test]
    fn native_keys_prefer_the_serial() {
        assert_eq!(pad_key("AA:BB:CC", "u1"), "aa:bb:cc");
        assert_eq!(pad_key("", "u1"), "u1");
        let mut c = EmuConfig::default();
        c.set_native("aa:bb", true);
        c.set_native("aa:bb", true);
        assert_eq!(c.native, vec!["aa:bb".to_string()]);
        assert!(c.is_native("aa:bb") && !c.is_native("cc"));
        c.set_native("aa:bb", false);
        assert!(c.native.is_empty());
    }

    #[test]
    fn config_survives_a_json_round_trip_and_old_files() {
        let mut c = EmuConfig::default();
        c.left.curve = crate::emu::zones::Curve::Custom;
        c.mute = Some(XBtn::Y);
        c.set_native("x", true);
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<EmuConfig>(&s).unwrap(), c);
        // A file with only some keys still loads, the rest default.
        let old: EmuConfig = serde_json::from_str("{\"rumble_pct\":40}").unwrap();
        assert_eq!(old.rumble_pct, 40);
        assert_eq!(old.touchpad, Some(XBtn::Back));
        let mut big = EmuConfig { rumble_pct: 250, ..Default::default() };
        big.sanitize();
        assert_eq!(big.rumble_pct, 100);
    }
}
