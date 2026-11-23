//! The mapping layer: a [`PadState`] in, an [`XboxState`] out. Pure, no I/O, no allocation.
//!
//! Stateless part (this file), in three small stages:
//!
//! 1. buttons: every digital input ([`DIn`]) through the remap table (the positional DualSense to Xbox
//!    mapping unless the user changed it);
//! 2. [`sticks`]: the radial dead zones and curves of `zones`, then swap, invert and "stick as the D-pad";
//! 3. the triggers: the dead zones, then routed like the remap says (as an analog trigger, or as a button).
//!
//! Stateful part (`macros`): macros and turbo run after this on the result and write only to the virtual pad.
//! Nothing outside this module knows how the mapping is done.

use super::macros::{Macro, Turbo};
use super::remap::{self, dpad_from_stick, fits_legacy, DIn, Out, Remap};
use super::xbox::{XBtn, XboxState};
use super::zones::{to_axis, StickCfg, TrigCfg};
use crate::pad::PadState;
use serde::{Deserialize, Serialize};

/// What the user can change in the emulation (saved in `config.json` under `emu`, and per profile).
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
    /// How strongly the game's rumble reaches the DualSense, 0..100 percent. One value for all profiles.
    pub rumble_pct: u8,
    /// Pads in native mode ("the game supports DualSense"), by serial number (or the id when there is none).
    /// One list for all profiles.
    pub native: Vec<String>,
    /// Remapping: what differs from the positional default, stick swaps and inverts.
    pub remap: Remap,
    pub macros: Vec<Macro>,
    pub turbo: Vec<Turbo>,
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
            remap: Remap::default(),
            macros: Vec::new(),
            turbo: Vec::new(),
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
        self.remap.sanitize();
        super::macros::sanitize_macros(&mut self.macros);
        super::macros::sanitize_turbo(&mut self.turbo);
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

    /// What an input does now.
    pub fn target(&self, d: DIn) -> Out {
        if let Some((_, o)) = self.remap.binds.iter().find(|(k, _)| *k == d) {
            return *o;
        }
        match d {
            DIn::Touchpad => self.touchpad.map_or(Out::None, Out::Btn),
            DIn::Mute => self.mute.map_or(Out::None, Out::Btn),
            _ => remap::default_out(d),
        }
    }

    /// Makes an input do `o`. The touchpad click and Mute keep their older fields when the output fits them.
    pub fn set_target(&mut self, d: DIn, o: Out) {
        self.remap.binds.retain(|(k, _)| *k != d);
        match (d, fits_legacy(o)) {
            (DIn::Touchpad, Some(b)) => self.touchpad = b,
            (DIn::Mute, Some(b)) => self.mute = b,
            (DIn::Touchpad | DIn::Mute, None) => {
                // A trigger needs an entry of its own; the older field must not press anything as well.
                if d == DIn::Touchpad {
                    self.touchpad = None;
                } else {
                    self.mute = None;
                }
                self.remap.binds.push((d, o));
            }
            (_, _) => {
                if remap::default_out(d) != o {
                    self.remap.binds.push((d, o));
                }
            }
        }
    }

    /// Back to the positional default for the buttons, sticks and triggers (macros and turbo stay).
    pub fn reset_remap(&mut self) {
        self.remap = Remap::default();
        self.touchpad = Some(XBtn::Back);
        self.mute = None;
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

/// The sticks after their dead zones, before any rerouting. Each is -1..1 with Y up.
pub fn sticks(pad: &PadState, cfg: &EmuConfig) -> ((f32, f32), (f32, f32)) {
    let (lx, ly) = pad.left();
    let (rx, ry) = pad.right();
    (cfg.left.apply(lx, ly), cfg.right.apply(rx, ry))
}

/// The mapping with the digital inputs already worked out (`ins`, see `remap::inputs`) and some of them held
/// back (`suppress`: the chord of a macro that took them over).
pub fn map_with(pad: &PadState, cfg: &EmuConfig, ins: u32, suppress: u32) -> XboxState {
    let live = ins & !suppress;
    let mut buttons = 0u16;
    let (mut lt, mut rt) = (0u8, 0u8);
    if live != 0 {
        for d in DIn::ALL {
            if live & d.bit() == 0 {
                continue;
            }
            let analog_input = matches!(d, DIn::L2 | DIn::R2);
            match cfg.target(d) {
                Out::Btn(x) => buttons |= x.mask(),
                // A trigger input sent to a trigger is the analog pass-through below.
                Out::Lt if !analog_input => lt = 255,
                Out::Rt if !analog_input => rt = 255,
                _ => {}
            }
        }
    }
    // The triggers: dead zones first, then wherever the remap sends them.
    for (v, d) in [(cfg.lt.apply(pad.l2), DIn::L2), (cfg.rt.apply(pad.r2), DIn::R2)] {
        if suppress & d.bit() != 0 {
            continue;
        }
        match cfg.target(d) {
            Out::Lt => lt = lt.max(v),
            Out::Rt => rt = rt.max(v),
            _ => {}
        }
    }
    // The sticks: dead zones, then the mode (stick, D-pad, off) and the inverts, then the swap.
    let (l, r) = sticks(pad, cfg);
    let mut out = [(0.0f32, 0.0f32); 2];
    for (i, (s, m)) in [(l, cfg.remap.left), (r, cfg.remap.right)].into_iter().enumerate() {
        match m.mode {
            remap::StickMode::Stick => out[i] = (if m.invert_x { -s.0 } else { s.0 }, if m.invert_y { -s.1 } else { s.1 }),
            remap::StickMode::Dpad => buttons |= dpad_from_stick(s.0, s.1),
            remap::StickMode::Off => {}
        }
    }
    if cfg.remap.swap_sticks {
        out.swap(0, 1);
    }
    XboxState { buttons, lt, rt, lx: to_axis(out[0].0), ly: to_axis(out[0].1), rx: to_axis(out[1].0), ry: to_axis(out[1].1) }
}

/// The whole stateless mapping (no macros, no turbo): what the tests compare against.
#[cfg(test)]
pub fn map_pad(pad: &PadState, cfg: &EmuConfig) -> XboxState {
    map_with(pad, cfg, remap::inputs(pad, &cfg.remap), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::Btn;

    fn buttons(pad: &PadState, cfg: &EmuConfig) -> u16 {
        map_pad(pad, cfg).buttons
    }

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
        // The 17 checklist controls: 16 have a default button, Mute is the one that is unmapped by default.
        let c = EmuConfig::default();
        let mut seen = 0u16;
        for b in Btn::CHECKLIST {
            let d = DIn::ALL.iter().copied().find(|d| d.as_btn() == Some(b)).expect("an input for every control");
            match c.target(d) {
                Out::Btn(x) => {
                    // The touchpad click shares View with Create on purpose.
                    assert!(b == Btn::Touchpad || seen & x.mask() == 0, "{b:?} lands on a button already taken");
                    seen |= x.mask();
                }
                Out::None => assert!(matches!(b, Btn::Mute), "{b:?} has no target"),
                _ => panic!("{b:?} should press a button"),
            }
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

    // ── remap ──

    #[test]
    fn any_input_can_go_to_any_output_or_nowhere() {
        let mut c = EmuConfig::default();
        c.set_target(DIn::Cross, Out::Btn(XBtn::Y));
        c.set_target(DIn::Circle, Out::None);
        c.set_target(DIn::Triangle, Out::Lt);
        c.set_target(DIn::L1, Out::Rt);
        assert_eq!(buttons(&press(&[Btn::Cross]), &c), XBtn::Y.mask());
        assert_eq!(buttons(&press(&[Btn::Circle]), &c), 0);
        let x = map_pad(&press(&[Btn::Triangle, Btn::L1]), &c);
        assert_eq!((x.buttons, x.lt, x.rt), (0, 255, 255), "a button as a trigger presses it fully");
        // Putting an input back to its default removes the entry.
        c.set_target(DIn::Cross, Out::Btn(XBtn::A));
        assert!(c.remap.binds.iter().all(|(d, _)| *d != DIn::Cross));
        // Two inputs may share an output.
        c.set_target(DIn::Square, Out::Btn(XBtn::B));
        assert_eq!(buttons(&press(&[Btn::Square, Btn::Cross]), &c), XBtn::B.mask() | XBtn::A.mask());
    }

    #[test]
    fn touchpad_and_mute_keep_their_older_fields_unless_a_trigger_is_wanted() {
        let mut c = EmuConfig::default();
        c.set_target(DIn::Mute, Out::Btn(XBtn::L3));
        assert_eq!(c.mute, Some(XBtn::L3));
        assert!(c.remap.binds.is_empty());
        c.set_target(DIn::Touchpad, Out::Rt);
        assert_eq!(c.touchpad, None);
        assert_eq!(c.target(DIn::Touchpad), Out::Rt);
        let x = map_pad(&press(&[Btn::Touchpad]), &c);
        assert_eq!((x.buttons, x.rt), (0, 255));
        c.set_target(DIn::Touchpad, Out::Btn(XBtn::Back));
        assert_eq!(c.touchpad, Some(XBtn::Back));
        assert!(c.remap.binds.is_empty());
    }

    #[test]
    fn a_trigger_can_be_a_button_and_a_button_can_leave_the_trigger_alone() {
        let mut c = EmuConfig::default();
        c.set_target(DIn::L2, Out::Btn(XBtn::X));
        let mut p = PadState::default();
        p.l2 = 100;
        let x = map_pad(&p, &c);
        assert_eq!((x.buttons, x.lt), (0, 0), "under the threshold, and the analog value is gone");
        p.l2 = 200;
        let x = map_pad(&p, &c);
        assert_eq!((x.buttons, x.lt), (XBtn::X.mask(), 0));
        c.remap.trigger_threshold = 90;
        assert_eq!(map_pad(&p, &c).buttons, 0, "a higher threshold needs a deeper pull");
        // Triggers swapped: LT drives the right trigger and the other way round, dead zones stay with the trigger.
        let mut c = EmuConfig::default();
        c.set_target(DIn::L2, Out::Rt);
        c.set_target(DIn::R2, Out::Lt);
        c.lt.inner = 0.5;
        p.l2 = 100; // inside the left trigger's own dead zone
        p.r2 = 90;
        let x = map_pad(&p, &c);
        assert_eq!((x.lt, x.rt), (90, 0));
    }

    #[test]
    fn sticks_can_swap_invert_become_the_dpad_or_be_switched_off() {
        let mut p = PadState::default();
        p.lx = 255; // left stick fully right
        p.ry = 0; // right stick fully up
        let mut c = EmuConfig::default();
        c.remap.swap_sticks = true;
        let x = map_pad(&p, &c);
        assert!(x.rx > 30000 && x.lx.abs() < 500 && x.ly > 30000, "left is now the right stick's value: {x:?}");
        c.remap.swap_sticks = false;
        c.remap.right.invert_y = true;
        c.remap.left.invert_x = true;
        let x = map_pad(&p, &c);
        assert!(x.ry < -30000 && x.lx < -30000);
        // The left stick as the D-pad: the stick axes go quiet, the D-pad presses.
        c.remap.left.mode = remap::StickMode::Dpad;
        let x = map_pad(&p, &c);
        assert_eq!((x.lx, x.ly), (0, 0));
        assert!(x.buttons & XBtn::Right.mask() != 0 && x.buttons & XBtn::Up.mask() == 0);
        p.ly = 0;
        assert!(map_pad(&p, &c).buttons & XBtn::Up.mask() != 0, "a diagonal is two directions");
        c.remap.right.mode = remap::StickMode::Off;
        let x = map_pad(&p, &c);
        assert_eq!((x.rx, x.ry), (0, 0));
    }

    #[test]
    fn stick_directions_and_touchpad_halves_are_remappable_inputs() {
        use crate::pad::state::Finger;
        let mut c = EmuConfig::default();
        c.set_target(DIn::LsUp, Out::Btn(XBtn::A));
        c.set_target(DIn::TouchLeft, Out::Btn(XBtn::X));
        c.set_target(DIn::TouchRight, Out::Btn(XBtn::Y));
        c.set_target(DIn::Touchpad, Out::None);
        let mut p = PadState::default();
        p.ly = 0;
        assert_eq!(buttons(&p, &c), XBtn::A.mask());
        let mut q = PadState::default();
        q.set(Btn::Touchpad, true);
        q.touch[0] = Finger { active: true, id: 0, x: 200, y: 200 };
        assert_eq!(buttons(&q, &c), XBtn::X.mask());
        q.touch[0].x = 1800;
        assert_eq!(buttons(&q, &c), XBtn::Y.mask());
    }

    #[test]
    fn the_remap_survives_json_and_old_files_and_reset_restores_the_default() {
        let mut c = EmuConfig::default();
        c.set_target(DIn::Cross, Out::Btn(XBtn::Y));
        c.set_target(DIn::L2, Out::Btn(XBtn::X));
        c.remap.swap_sticks = true;
        c.remap.left.mode = remap::StickMode::Dpad;
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<EmuConfig>(&s).unwrap(), c);
        c.reset_remap();
        assert_eq!(c, EmuConfig::default());
        // Without the new keys (a file from before this section) it is the old behaviour.
        let old: EmuConfig = serde_json::from_str("{\"touchpad\":\"Guide\",\"left\":{\"inner\":0.1}}").unwrap();
        assert_eq!(old.touchpad, Some(XBtn::Guide));
        assert_eq!(old.remap, Remap::default());
        assert!(old.macros.is_empty() && old.turbo.is_empty());
    }
}
