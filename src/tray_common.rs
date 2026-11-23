//! What the tray menu does, and the state the background process hands to the tray. Shared by the Windows and
//! Linux trays.

use crate::i18n::Lang;

/// Everything the tray needs to draw itself.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayState {
    pub lang: Lang,
    pub tooltip: String,
    /// The Xbox emulation is on. The icon is dimmed while it is off.
    pub emu_on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    ToggleEmulation,
    Support,
    Exit,
}

pub fn perform(a: Action) {
    match a {
        Action::Open => crate::platform::spawn_ui(),
        Action::ToggleEmulation => crate::daemon::toggle_emulation(),
        Action::Support => crate::support::open(),
        Action::Exit => crate::daemon::request_quit(),
    }
}

/// The tray icon for "emulation is off": the same artwork, grey and half transparent.
pub fn dimmed(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        let g = ((px[0] as u32 * 30 + px[1] as u32 * 59 + px[2] as u32 * 11) / 100) as u8;
        px[0] = g;
        px[1] = g;
        px[2] = g;
        px[3] = (px[3] as u32 * 55 / 100) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_off_icon_is_grey_and_fainter_and_keeps_its_shape() {
        let src = [200u8, 100, 50, 255, 10, 20, 30, 0];
        let d = dimmed(&src);
        assert_eq!(d.len(), src.len());
        assert!(d[0] == d[1] && d[1] == d[2], "grey");
        assert!(d[3] < 255 && d[3] > 100, "fainter, still visible");
        assert_eq!(d[7], 0, "transparent stays transparent");
    }
}
