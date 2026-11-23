//! The Xbox 360 side of the mapping: button bits (the XInput ones), the report shape and the names the UI shows.
//!
//! Both back ends take an [`XboxState`]: the Windows one hands it to ViGEmBus as an XInput gamepad, the Linux
//! one turns it into the evdev events an `xpad` device sends.

use serde::{Deserialize, Serialize};

/// An Xbox 360 button. The bit is the XInput one (`XINPUT_GAMEPAD_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum XBtn {
    Up,
    Down,
    Left,
    Right,
    Start,
    Back,
    L3,
    R3,
    Lb,
    Rb,
    Guide,
    A,
    B,
    X,
    Y,
}

impl XBtn {
    pub const ALL: [XBtn; 15] = [
        XBtn::Up, XBtn::Down, XBtn::Left, XBtn::Right, XBtn::Start, XBtn::Back, XBtn::L3, XBtn::R3, XBtn::Lb, XBtn::Rb, XBtn::Guide, XBtn::A,
        XBtn::B, XBtn::X, XBtn::Y,
    ];

    pub fn mask(self) -> u16 {
        match self {
            XBtn::Up => 0x0001,
            XBtn::Down => 0x0002,
            XBtn::Left => 0x0004,
            XBtn::Right => 0x0008,
            XBtn::Start => 0x0010,
            XBtn::Back => 0x0020,
            XBtn::L3 => 0x0040,
            XBtn::R3 => 0x0080,
            XBtn::Lb => 0x0100,
            XBtn::Rb => 0x0200,
            XBtn::Guide => 0x0400,
            XBtn::A => 0x1000,
            XBtn::B => 0x2000,
            XBtn::X => 0x4000,
            XBtn::Y => 0x8000,
        }
    }

    /// The label on a real Xbox pad (also what the UI shows in the pickers).
    pub fn label(self) -> &'static str {
        match self {
            XBtn::Up => "D-pad up",
            XBtn::Down => "D-pad down",
            XBtn::Left => "D-pad left",
            XBtn::Right => "D-pad right",
            XBtn::Start => "Menu (Start)",
            XBtn::Back => "View (Back)",
            XBtn::L3 => "Left stick click",
            XBtn::R3 => "Right stick click",
            XBtn::Lb => "LB",
            XBtn::Rb => "RB",
            XBtn::Guide => "Guide",
            XBtn::A => "A",
            XBtn::B => "B",
            XBtn::X => "X",
            XBtn::Y => "Y",
        }
    }
}

/// What the virtual pad reports. Sticks are -32767..32767 with Y up (as XInput has it), triggers 0..255.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XboxState {
    pub buttons: u16,
    pub lt: u8,
    pub rt: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

#[allow(dead_code)] // the Linux back end and the tests read the bits back
impl XboxState {
    pub fn pressed(&self, b: XBtn) -> bool {
        self.buttons & b.mask() != 0
    }
    pub fn set(&mut self, b: XBtn, on: bool) {
        if on {
            self.buttons |= b.mask();
        } else {
            self.buttons &= !b.mask();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_the_xinput_ones_and_distinct() {
        let mut all = 0u16;
        for b in XBtn::ALL {
            assert_eq!(all & b.mask(), 0, "{b:?} overlaps");
            all |= b.mask();
        }
        assert_eq!(all, 0xF7FF);
        assert_eq!(XBtn::A.mask(), 0x1000);
        assert_eq!(XBtn::Guide.mask(), 0x0400);
    }

    #[test]
    fn set_and_pressed() {
        let mut s = XboxState::default();
        s.set(XBtn::X, true);
        s.set(XBtn::Lb, true);
        assert!(s.pressed(XBtn::X) && s.pressed(XBtn::Lb) && !s.pressed(XBtn::A));
        s.set(XBtn::X, false);
        assert!(!s.pressed(XBtn::X) && s.pressed(XBtn::Lb));
    }
}
