//! What the tray menu does, and the state the background process hands to the tray. Shared by the Windows and
//! Linux trays.

use crate::i18n::Lang;

/// Everything the tray needs to draw itself.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayState {
    pub lang: Lang,
    pub tooltip: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Support,
    Exit,
}

pub fn perform(a: Action) {
    match a {
        Action::Open => crate::platform::spawn_ui(),
        Action::Support => crate::support::open(),
        Action::Exit => crate::daemon::request_quit(),
    }
}
