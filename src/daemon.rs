//! The background process: owns the pad hub (reader threads and the writer), the tray, and the channel the
//! window talks to. It draws nothing, so it stays small; the window is a separate process that starts when you
//! open Padyx and ends when you close it.
//!
//! The messages the window sends (one line each, see `ipc`):
//! - `snap`: the whole [`Snapshot`] as JSON;
//! - `out <uid|*> <json>`: change the wanted output ([`Output`]) of one pad or all of them;
//! - `reload`: re-read `config.json` (lightbar colour for new connections, language of the tray, the emulation
//!   settings);
//! - `emu on`, `emu off`, `emu restore`: the Xbox emulation switch, and the emergency "give the pad back";
//! - `quit`.
//!
//! The Xbox emulation is a listener of the hub (see `emu`); this file owns its switch (the tray item and the
//! messages above), the low-battery notice and the shutdown that gives every pad back. Section C adds the
//! mapping layer between the listener and the virtual pad.

use crate::config::Config;
use crate::emu::notify::{self, LowBattery};
use crate::i18n::Lang;
use crate::pad::hub::Hub;
use crate::pad::output::Output;
use crate::pad::{Charge, Conn, Snapshot};
use crate::strings;
use crate::tray::{self, TrayState};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

static HUB: OnceLock<Arc<Hub>> = OnceLock::new();
static CFG: Mutex<Option<(PathBuf, Config)>> = Mutex::new(None);
static QUIT: AtomicBool = AtomicBool::new(false);
static LOW: Mutex<Option<LowBattery>> = Mutex::new(None);

pub fn hub() -> Option<&'static Arc<Hub>> {
    HUB.get()
}

pub fn request_quit() {
    QUIT.store(true, Ordering::Relaxed);
    #[cfg(windows)]
    crate::winloop::request_quit();
}

#[cfg_attr(windows, allow(dead_code))]
pub fn quit_requested() -> bool {
    QUIT.load(Ordering::Relaxed)
}

/// The tray tooltip: the name, then a line per pad with its connection and battery.
pub fn tooltip(snap: &Snapshot, lang: Lang) -> String {
    let t = strings::TABLE.get(lang);
    if snap.pads.is_empty() {
        return format!("Padyx\n{}", t.tray_no_pad);
    }
    let mut s = String::from("Padyx");
    for p in &snap.pads {
        let conn = match p.info.conn {
            Conn::Usb => "USB",
            Conn::Bluetooth => "Bluetooth",
            Conn::Unknown => "",
        };
        let bat = match (p.state.battery, p.state.charge) {
            (Some(b), Charge::Charging) => format!("{b}% +"),
            (Some(b), _) => format!("{b}%"),
            _ => String::new(),
        };
        let line: Vec<String> = [p.info.kind.name().to_string(), conn.to_string(), bat].into_iter().filter(|x| !x.is_empty()).collect();
        s.push('\n');
        s.push_str(&line.join(" "));
    }
    s
}

fn lang_now() -> Lang {
    let l = CFG.lock().ok().and_then(|g| g.as_ref().and_then(|(_, c)| c.language.clone()));
    crate::i18n::resolve(l.as_deref())
}

pub fn tray_state() -> TrayState {
    let lang = lang_now();
    let snap = hub().map(|h| h.snapshot()).unwrap_or_default();
    TrayState { lang, tooltip: tooltip(&snap, lang), emu_on: snap.emu.enabled }
}

/// The tray item: flips the emulation and shows it at once.
pub fn toggle_emulation() {
    if let Some(e) = hub().and_then(|h| h.emulator()) {
        e.set_enabled(!e.enabled());
        tray::update(tray_state());
    }
}

/// Tells about a pad that has run low, once per pad (see `emu::notify`).
fn check_batteries() {
    let snap = hub().map(|h| h.snapshot()).unwrap_or_default();
    let low = {
        let Ok(mut g) = LOW.lock() else { return };
        g.get_or_insert_with(LowBattery::default).check(&notify::pads_for_check(&snap.pads))
    };
    if !low.is_empty() {
        let t = strings::TABLE.get(lang_now());
        for uid in low {
            let pct = snap.pads.iter().find(|p| p.info.uid == uid).and_then(|p| p.state.battery).unwrap_or(0);
            notify::send(t.low_battery_title, &t.low_battery_body.replace("{n}", &pct.to_string()));
        }
    }
}

/// Called every couple of seconds on the loop thread.
pub fn on_tick() {
    tray::update(tray_state());
    check_batteries();
}

pub fn on_shutdown() {
    if let Some(h) = hub() {
        // Nothing stays hidden after Padyx is gone.
        if let Some(e) = h.emulator() {
            e.shutdown();
        }
        h.stop();
    }
}

fn reload() {
    let Some((dir, _)) = CFG.lock().ok().and_then(|g| g.clone()) else { return };
    let cfg = Config::load_from(&dir);
    if let Some(h) = hub() {
        h.set_template(cfg.output_template());
        if let Some(e) = h.emulator() {
            e.set_config(cfg.emu.clone());
        }
    }
    if let Ok(mut g) = CFG.lock() {
        *g = Some((dir, cfg));
    }
}

pub fn handle(msg: &str) -> String {
    let (cmd, rest) = msg.split_once(' ').unwrap_or((msg, ""));
    match cmd {
        "ping" => "pong".into(),
        "snap" => hub().map(|h| serde_json::to_string(&h.snapshot()).unwrap_or_default()).unwrap_or_default(),
        "out" => {
            let Some((uid, json)) = rest.split_once(' ') else { return "error".into() };
            let Ok(out) = serde_json::from_str::<Output>(json) else { return "error".into() };
            if let Some(h) = hub() {
                let uid = if uid == "*" { None } else { Some(uid) };
                h.set_output(uid, |o| *o = out.clone());
            }
            "ok".into()
        }
        "emu" => {
            let Some(e) = hub().and_then(|h| h.emulator()) else { return "error".into() };
            match rest {
                "on" => e.set_enabled(true),
                "off" => e.set_enabled(false),
                "restore" => {
                    if let Err(err) = e.restore_visibility() {
                        return format!("error {err}");
                    }
                }
                _ => return "error".into(),
            }
            tray::update(tray_state());
            "ok".into()
        }
        "reload" => {
            reload();
            "ok".into()
        }
        "quit" => {
            request_quit();
            "ok".into()
        }
        _ => "unknown".into(),
    }
}

/// Runs the background process until it is told to quit.
pub fn run(dir: PathBuf, cfg: Config, demo: bool) {
    // The endpoint is also the single-instance lock.
    if !crate::ipc::serve("daemon", handle) {
        return;
    }
    let hub = Hub::new(cfg.output_template());
    // The emulator undoes what a crashed run left hidden, then listens to every report.
    crate::emu::attach_system(&hub, cfg.emu.clone(), &dir);
    if demo {
        hub.start_demo();
    } else {
        hub.start_scanner();
    }
    let _ = HUB.set(hub);
    if let Ok(mut g) = CFG.lock() {
        *g = Some((dir, cfg));
    }
    let state = tray_state();
    #[cfg(windows)]
    {
        crate::winloop::run(|| {
            tray::create(&state, crate::ICON_PNG);
        });
        on_shutdown();
        tray::remove();
    }
    #[cfg(unix)]
    {
        tray::create(&state, crate::ICON_PNG);
        let mut n = 0u32;
        while !quit_requested() {
            std::thread::sleep(std::time::Duration::from_millis(200));
            n += 1;
            if n % 10 == 0 {
                on_tick();
            }
        }
        on_shutdown();
        tray::remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::{PadInfo, PadKind, PadSnapshot, PadState};

    fn pad(conn: Conn, battery: Option<u8>, charge: Charge) -> PadSnapshot {
        PadSnapshot {
            info: PadInfo { uid: "u".into(), kind: PadKind::DualSense, conn, serial: String::new(), demo: false, path: String::new() },
            state: PadState { battery, charge, ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn tooltip_lists_pads() {
        let none = Snapshot::default();
        assert!(tooltip(&none, Lang::En).starts_with("Padyx\n"));
        let one = Snapshot { pads: vec![pad(Conn::Usb, Some(78), Charge::Discharging)], ..Default::default() };
        assert_eq!(tooltip(&one, Lang::En), "Padyx\nDualSense USB 78%");
        let two = Snapshot { pads: vec![pad(Conn::Bluetooth, Some(40), Charge::Charging), pad(Conn::Unknown, None, Charge::Discharging)], ..Default::default() };
        assert_eq!(tooltip(&two, Lang::En), "Padyx\nDualSense Bluetooth 40% +\nDualSense");
    }

    #[test]
    fn emulation_messages_and_the_tray_switch_flip_the_emulation() {
        use crate::emu::backend::MockBackend;
        use crate::emu::hide::NullHider;
        use crate::emu::map::EmuConfig;
        let _ = HUB.set(Hub::new(Output::default()));
        let h = hub().unwrap();
        if h.emulator().is_none() {
            crate::emu::Emulator::attach(h, MockBackend::new(), Box::new(NullHider::default()), EmuConfig::default(), None, false);
        }
        assert_eq!(handle("emu off"), "ok");
        assert!(!tray_state().emu_on);
        assert_eq!(handle("emu on"), "ok");
        assert!(tray_state().emu_on, "the tray follows the switch");
        toggle_emulation();
        assert!(!tray_state().emu_on, "the tray menu item flips it");
        toggle_emulation();
        assert!(tray_state().emu_on);
        assert_eq!(handle("emu sideways"), "error");
        assert_eq!(handle("emu restore"), "ok", "the emergency button turns it off and restores");
        assert!(!tray_state().emu_on);
    }

    #[test]
    fn unknown_and_malformed_messages_are_answered() {
        assert_eq!(handle("ping"), "pong");
        assert_eq!(handle("nonsense"), "unknown");
        assert_eq!(handle("out onlyone"), "error");
        assert_eq!(handle("out * {bad json"), "error");
    }

    #[test]
    fn out_message_changes_the_wanted_output_of_a_pad() {
        use crate::pad::hub::tests::MockIo;
        use crate::pad::output::Trigger;
        use std::sync::atomic::AtomicU64;
        let (tx, _rx) = std::sync::mpsc::channel();
        let io = MockIo { reports: (0..2000).map(|_| crate::pad::demo::usb_report_from(&PadState::default())).collect(), writes: tx, fail_after_reports: false, feature_calls: Arc::new(AtomicU64::new(0)) };
        let _ = HUB.set(Hub::new(Output::default()));
        // Another test may have set the global first: the message must still be answered.
        let info = PadInfo { uid: "dtest".into(), kind: PadKind::DualSense, conn: Conn::Unknown, serial: String::new(), demo: true, path: String::new() };
        hub().unwrap().attach(info, Box::new(io));
        let mut want = Output::default();
        want.light = [9, 8, 7];
        want.trigger_left = Trigger::Weapon { start: 2, end: 4, force: 3 };
        let msg = format!("out dtest {}", serde_json::to_string(&want).unwrap());
        assert_eq!(handle(&msg), "ok");
        let snap: Snapshot = serde_json::from_str(&handle("snap")).unwrap();
        let p = snap.pads.iter().find(|p| p.info.uid == "dtest").expect("the pad is in the snapshot");
        assert_eq!(p.out, want);
        // A message for another uid leaves it alone.
        let mut other = want.clone();
        other.light = [1, 1, 1];
        assert_eq!(handle(&format!("out nobody {}", serde_json::to_string(&other).unwrap())), "ok");
        let snap: Snapshot = serde_json::from_str(&handle("snap")).unwrap();
        assert_eq!(snap.pads.iter().find(|p| p.info.uid == "dtest").unwrap().out.light, [9, 8, 7]);
        hub().unwrap().stop();
    }
}
