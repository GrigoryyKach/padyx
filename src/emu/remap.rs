//! Remapping: any DualSense input to any Xbox output (or to nothing). Pure data and maths.
//!
//! The mapping layer works in three steps (see `map`):
//!
//! 1. [`inputs`] turns a [`PadState`] into one bit set of digital inputs ([`DIn`]): the buttons, the two trigger
//!    "clicks" (the analog value against a threshold), the two touchpad halves and the eight stick directions;
//! 2. the table of [`Remap`] gives each input its [`Out`];
//! 3. the sticks and triggers pass through the dead zones and are re-routed (swap, invert, stick as the D-pad).
//!
//! Only the differences from the default positional mapping are stored (`Remap::binds`), so a new default or an
//! old file never fights the user's choices.

use super::xbox::XBtn;
use crate::pad::state::TOUCH_W;
use crate::pad::{Btn, PadState};
use serde::{Deserialize, Serialize};

/// A digital input of the DualSense. The first 17 are the checklist controls in the order of [`Btn`], so their
/// bit is the same as in `PadState::buttons`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum DIn {
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
    /// The click of the touchpad (anywhere).
    Touchpad,
    Mute,
    /// The trigger past the threshold (`Remap::trigger_threshold`).
    L2,
    R2,
    /// The touchpad click with a finger in the left or the right half.
    TouchLeft,
    TouchRight,
    LsUp,
    LsDown,
    LsLeft,
    LsRight,
    RsUp,
    RsDown,
    RsLeft,
    RsRight,
}

impl DIn {
    pub const ALL: [DIn; 29] = [
        DIn::Up, DIn::Down, DIn::Left, DIn::Right, DIn::Cross, DIn::Circle, DIn::Square, DIn::Triangle, DIn::L1, DIn::R1, DIn::L3, DIn::R3,
        DIn::Create, DIn::Options, DIn::Ps, DIn::Touchpad, DIn::Mute, DIn::L2, DIn::R2, DIn::TouchLeft, DIn::TouchRight, DIn::LsUp, DIn::LsDown,
        DIn::LsLeft, DIn::LsRight, DIn::RsUp, DIn::RsDown, DIn::RsLeft, DIn::RsRight,
    ];

    pub fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// The tester's name of a physical button, when this input is one.
    pub fn as_btn(self) -> Option<Btn> {
        Some(match self {
            DIn::Up => Btn::Up,
            DIn::Down => Btn::Down,
            DIn::Left => Btn::Left,
            DIn::Right => Btn::Right,
            DIn::Cross => Btn::Cross,
            DIn::Circle => Btn::Circle,
            DIn::Square => Btn::Square,
            DIn::Triangle => Btn::Triangle,
            DIn::L1 => Btn::L1,
            DIn::R1 => Btn::R1,
            DIn::L3 => Btn::L3,
            DIn::R3 => Btn::R3,
            DIn::Create => Btn::Create,
            DIn::Options => Btn::Options,
            DIn::Ps => Btn::Ps,
            DIn::Touchpad => Btn::Touchpad,
            DIn::Mute => Btn::Mute,
            _ => return None,
        })
    }
}

/// What an input becomes on the virtual pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Out {
    /// Nothing.
    None,
    Btn(XBtn),
    /// The left trigger, fully pressed (for a trigger input: the analog value is passed on).
    Lt,
    /// The right trigger.
    Rt,
}

impl Out {
    /// `Some(None)` for nothing, `Some(Some(b))` for a button, `None` for a trigger.
    fn legacy(self) -> Option<Option<XBtn>> {
        match self {
            Out::None => Some(None),
            Out::Btn(b) => Some(Some(b)),
            Out::Lt | Out::Rt => None,
        }
    }
}

/// The default mapping by position (the table of `map`), plus the triggers passing through.
pub fn default_out(d: DIn) -> Out {
    match d {
        DIn::Up => Out::Btn(XBtn::Up),
        DIn::Down => Out::Btn(XBtn::Down),
        DIn::Left => Out::Btn(XBtn::Left),
        DIn::Right => Out::Btn(XBtn::Right),
        DIn::Cross => Out::Btn(XBtn::A),
        DIn::Circle => Out::Btn(XBtn::B),
        DIn::Square => Out::Btn(XBtn::X),
        DIn::Triangle => Out::Btn(XBtn::Y),
        DIn::L1 => Out::Btn(XBtn::Lb),
        DIn::R1 => Out::Btn(XBtn::Rb),
        DIn::L3 => Out::Btn(XBtn::L3),
        DIn::R3 => Out::Btn(XBtn::R3),
        DIn::Create => Out::Btn(XBtn::Back),
        DIn::Options => Out::Btn(XBtn::Start),
        DIn::Ps => Out::Btn(XBtn::Guide),
        DIn::L2 => Out::Lt,
        DIn::R2 => Out::Rt,
        // The click of the touchpad and Mute come from `EmuConfig::touchpad` and `EmuConfig::mute`.
        _ => Out::None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StickMode {
    /// The stick is a stick.
    #[default]
    Stick,
    /// The stick presses the D-pad (eight directions).
    Dpad,
    /// The stick does nothing.
    Off,
}

/// What one physical stick does on the virtual pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StickMap {
    pub mode: StickMode,
    pub invert_x: bool,
    pub invert_y: bool,
}

pub const TRIGGER_THRESHOLD_DEFAULT: u8 = 50;

/// The user's remapping: the differences from the positional default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remap {
    /// Inputs that do not do their default. One entry per input.
    pub binds: Vec<(DIn, Out)>,
    /// The left stick drives the right one and the other way round (after the dead zones).
    pub swap_sticks: bool,
    /// The physical left and right sticks.
    pub left: StickMap,
    pub right: StickMap,
    /// How far (percent) a trigger must be pulled to count as a button press, 5..95.
    pub trigger_threshold: u8,
}

impl Default for Remap {
    fn default() -> Self {
        Self { binds: Vec::new(), swap_sticks: false, left: StickMap::default(), right: StickMap::default(), trigger_threshold: TRIGGER_THRESHOLD_DEFAULT }
    }
}

impl Remap {
    pub fn sanitize(&mut self) {
        self.trigger_threshold = self.trigger_threshold.clamp(5, 95);
        // One entry per input, the last wins; no entry that only repeats the default.
        let mut seen: Vec<(DIn, Out)> = Vec::new();
        for (d, o) in self.binds.drain(..) {
            seen.retain(|(k, _)| *k != d);
            seen.push((d, o));
        }
        seen.retain(|(d, o)| !matches!(d, DIn::Touchpad | DIn::Mute) || matches!(o, Out::Lt | Out::Rt));
        seen.retain(|(d, o)| default_out(*d) != *o || matches!(d, DIn::Touchpad | DIn::Mute));
        self.binds = seen;
    }

    /// The trigger threshold as a raw value, 0..255.
    pub fn threshold_raw(&self) -> u8 {
        ((self.trigger_threshold as u32 * 255 + 50) / 100) as u8
    }
}

/// Every input at once, as a bit set of [`DIn`]. Pure and allocation free.
pub fn inputs(pad: &PadState, remap: &Remap) -> u32 {
    let mut ins = pad.buttons & 0x1_FFFF;
    let thr = remap.threshold_raw().max(1);
    if pad.l2 >= thr {
        ins |= DIn::L2.bit();
    }
    if pad.r2 >= thr {
        ins |= DIn::R2.bit();
    }
    if pad.pressed(Btn::Touchpad) {
        for f in pad.touch.iter().filter(|f| f.active) {
            ins |= if (f.x as f32) < TOUCH_W / 2.0 { DIn::TouchLeft.bit() } else { DIn::TouchRight.bit() };
        }
    }
    const T: f32 = 0.5;
    let (lx, ly) = pad.left();
    let (rx, ry) = pad.right();
    for (x, y, up, down, left, right) in [
        (lx, ly, DIn::LsUp, DIn::LsDown, DIn::LsLeft, DIn::LsRight),
        (rx, ry, DIn::RsUp, DIn::RsDown, DIn::RsLeft, DIn::RsRight),
    ] {
        if y >= T {
            ins |= up.bit();
        } else if y <= -T {
            ins |= down.bit();
        }
        if x >= T {
            ins |= right.bit();
        } else if x <= -T {
            ins |= left.bit();
        }
    }
    ins
}

/// The D-pad bits a stick position presses (threshold 0.5, so a diagonal is two bits).
pub fn dpad_from_stick(x: f32, y: f32) -> u16 {
    let mut b = 0;
    if y >= 0.5 {
        b |= XBtn::Up.mask();
    } else if y <= -0.5 {
        b |= XBtn::Down.mask();
    }
    if x >= 0.5 {
        b |= XBtn::Right.mask();
    } else if x <= -0.5 {
        b |= XBtn::Left.mask();
    }
    b
}

/// The names of the Xbox outputs for the pickers are in the UI; this keeps them in one order.
pub fn out_options() -> Vec<Out> {
    std::iter::once(Out::None).chain(XBtn::ALL.iter().map(|b| Out::Btn(*b))).chain([Out::Lt, Out::Rt]).collect()
}

/// Where an `Out` picked for a touchpad click or Mute is stored (the older fields are kept so existing files
/// load): `Some` when it fits the field, `None` when it needs an entry in `binds`.
pub fn fits_legacy(o: Out) -> Option<Option<XBtn>> {
    o.legacy()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::state::Finger;

    #[test]
    fn the_first_17_inputs_share_bits_with_the_pad_buttons() {
        for d in DIn::ALL.iter().take(17) {
            assert_eq!(d.as_btn().expect("a button").bit(), d.bit());
        }
        assert_eq!(DIn::ALL.len(), 29);
        let mut all = 0u32;
        for d in DIn::ALL {
            assert_eq!(all & d.bit(), 0);
            all |= d.bit();
        }
    }

    #[test]
    fn triggers_count_as_buttons_past_the_threshold_only() {
        let mut r = Remap::default();
        let mut p = PadState::default();
        p.l2 = 127;
        p.r2 = 129;
        assert_eq!(inputs(&p, &r) & (DIn::L2.bit() | DIn::R2.bit()), DIn::R2.bit());
        r.trigger_threshold = 10;
        assert_eq!(inputs(&p, &r) & (DIn::L2.bit() | DIn::R2.bit()), DIn::L2.bit() | DIn::R2.bit());
        // The digital click of the trigger is not an input of its own.
        let mut q = PadState::default();
        q.set(Btn::L2, true);
        assert_eq!(inputs(&q, &Remap::default()), 0);
    }

    #[test]
    fn touchpad_halves_need_the_click_and_a_finger() {
        let r = Remap::default();
        let mut p = PadState::default();
        p.touch[0] = Finger { active: true, id: 1, x: 300, y: 500 };
        assert_eq!(inputs(&p, &r), 0, "touching is not clicking");
        p.set(Btn::Touchpad, true);
        assert_eq!(inputs(&p, &r), DIn::Touchpad.bit() | DIn::TouchLeft.bit());
        p.touch[0].x = 1500;
        assert_eq!(inputs(&p, &r), DIn::Touchpad.bit() | DIn::TouchRight.bit());
        p.touch[1] = Finger { active: true, id: 2, x: 100, y: 100 };
        assert_eq!(inputs(&p, &r), DIn::Touchpad.bit() | DIn::TouchLeft.bit() | DIn::TouchRight.bit());
    }

    #[test]
    fn stick_directions_are_inputs_at_half_travel() {
        let r = Remap::default();
        let mut p = PadState::default();
        p.ly = 0; // up
        p.rx = 255; // right
        let ins = inputs(&p, &r);
        assert_eq!(ins, DIn::LsUp.bit() | DIn::RsRight.bit());
        p.ly = 128 - 40; // a third of the way up: not yet
        assert_eq!(inputs(&p, &r) & DIn::LsUp.bit(), 0);
    }

    #[test]
    fn dpad_from_stick_has_eight_directions() {
        assert_eq!(dpad_from_stick(0.0, 0.0), 0);
        assert_eq!(dpad_from_stick(0.0, 1.0), XBtn::Up.mask());
        assert_eq!(dpad_from_stick(-0.9, -0.9), XBtn::Down.mask() | XBtn::Left.mask());
        assert_eq!(dpad_from_stick(0.49, 0.0), 0);
        assert_eq!(dpad_from_stick(0.5, 0.0), XBtn::Right.mask());
    }

    #[test]
    fn sanitize_dedups_and_drops_default_entries() {
        let mut r = Remap::default();
        r.trigger_threshold = 0;
        r.binds = vec![(DIn::Cross, Out::Btn(XBtn::B)), (DIn::Cross, Out::Btn(XBtn::Y)), (DIn::Circle, Out::Btn(XBtn::B)), (DIn::Touchpad, Out::Btn(XBtn::A))];
        r.sanitize();
        assert_eq!(r.trigger_threshold, 5);
        assert_eq!(r.binds, vec![(DIn::Cross, Out::Btn(XBtn::Y))]);
    }
}
