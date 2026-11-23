//! Windows notification-area icon: Open, Support the author, Exit.
//!
//! The icon lives on the background process's main thread (the one with the message loop). Other threads change
//! it through `update`, which stores the new state and posts a message; the loop then calls `apply_pending`.

use crate::strings;
use crate::support;
pub use crate::tray_common::TrayState;
use crate::tray_common::{perform, Action};
use std::cell::RefCell;
use std::sync::Mutex;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

struct Tray {
    icon: TrayIcon,
    open: MenuItem,
    support: MenuItem,
    exit: MenuItem,
    last: Option<TrayState>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

static PENDING: Mutex<Option<TrayState>> = Mutex::new(None);

/// Creates the icon. Call on the message-loop thread.
pub fn create(state: &TrayState, icon_png: &[u8]) -> bool {
    let Ok(data) = eframe::icon_data::from_png_bytes(icon_png) else { return false };
    let Ok(normal) = Icon::from_rgba(data.rgba.clone(), data.width, data.height) else { return false };
    let t = strings::TABLE.get(state.lang);
    let open = MenuItem::new(t.tray_open, true, None);
    let support_item = MenuItem::new(support::tray_label(state.lang), true, None);
    let exit = MenuItem::new(t.tray_exit, true, None);
    let menu = Menu::new();
    if menu.append_items(&[&open, &PredefinedMenuItem::separator(), &support_item, &exit]).is_err() {
        return false;
    }
    let Ok(icon) = TrayIconBuilder::new().with_icon(normal).with_tooltip(&state.tooltip).with_menu(Box::new(menu)).with_menu_on_left_click(false).build() else {
        return false;
    };

    let ids = (open.id().clone(), support_item.id().clone(), exit.id().clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let a = if e.id == ids.0 {
            Action::Open
        } else if e.id == ids.1 {
            Action::Support
        } else if e.id == ids.2 {
            Action::Exit
        } else {
            return;
        };
        perform(a);
    }));
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            perform(Action::Open);
        }
    }));
    TRAY.with(|t| {
        *t.borrow_mut() = Some(Tray { icon, open, support: support_item, exit, last: Some(state.clone()) });
    });
    true
}

/// Stores a new state and asks the loop to draw it. Safe to call from any thread.
pub fn update(state: TrayState) {
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(state);
    }
    crate::winloop::request_tray_refresh();
}

/// Draws the state stored by `update`. Call on the message-loop thread.
pub fn apply_pending() {
    let Some(state) = PENDING.lock().ok().and_then(|mut p| p.take()) else { return };
    TRAY.with(|t| {
        let mut b = t.borrow_mut();
        let Some(tr) = b.as_mut() else { return };
        if tr.last.as_ref() == Some(&state) {
            return;
        }
        let t = strings::TABLE.get(state.lang);
        if tr.last.as_ref().is_none_or(|l| l.lang != state.lang) {
            tr.open.set_text(t.tray_open);
            tr.support.set_text(support::tray_label(state.lang));
            tr.exit.set_text(t.tray_exit);
        }
        let _ = tr.icon.set_tooltip(Some(&state.tooltip));
        tr.last = Some(state);
    });
}

pub fn remove() {
    TRAY.with(|t| {
        if let Some(tr) = t.borrow().as_ref() {
            let _ = tr.icon.set_visible(false);
        }
        *t.borrow_mut() = None;
    });
}
