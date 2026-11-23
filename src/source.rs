//! Where the window gets the pad state from.
//!
//! - `Local`: the window owns a pad hub itself. Only the demo does this (a scripted fake pad, no hardware).
//! - `Remote`: the normal case. The background process owns the pads; the window polls it over `ipc` on a thread
//!   and keeps the latest snapshot, so drawing never waits for the pipe. Changes of the wanted output go back
//!   through a second thread that sends only the newest one.
//!
//! Both give the same two calls, so every page works the same with a real pad and with the demo.

use crate::ipc;
use crate::pad::hub::Hub;
use crate::pad::output::Output;
use crate::pad::Snapshot;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Shared {
    snap: Mutex<Snapshot>,
    connected: AtomicBool,
    stop: AtomicBool,
}

enum Kind {
    Local(Arc<Hub>),
    Remote { shared: Arc<Shared>, tx: Sender<(String, Output)> },
}

pub struct Source {
    kind: Kind,
}

impl Source {
    pub fn local(hub: Arc<Hub>) -> Source {
        Source { kind: Kind::Local(hub) }
    }

    /// Polls the background process about 60 times a second while the window is open.
    pub fn remote(ctx: eframe::egui::Context) -> Source {
        let shared = Arc::new(Shared { snap: Mutex::new(Snapshot::default()), connected: AtomicBool::new(false), stop: AtomicBool::new(false) });
        let s = shared.clone();
        let _ = std::thread::Builder::new().name("padyx-poll".into()).spawn(move || {
            while !s.stop.load(Ordering::Relaxed) {
                match ipc::request("daemon", "snap").and_then(|r| serde_json::from_str::<Snapshot>(&r).ok()) {
                    Some(snap) => {
                        s.connected.store(true, Ordering::Relaxed);
                        if let Ok(mut g) = s.snap.lock() {
                            *g = snap;
                        }
                        ctx.request_repaint();
                        std::thread::sleep(Duration::from_millis(14));
                    }
                    None => {
                        s.connected.store(false, Ordering::Relaxed);
                        ctx.request_repaint();
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
            }
        });
        let (tx, rx) = channel::<(String, Output)>();
        let _ = std::thread::Builder::new().name("padyx-send".into()).spawn(move || {
            while let Ok(mut msg) = rx.recv() {
                // Only the newest change per pad matters.
                while let Ok(next) = rx.try_recv() {
                    if next.0 == msg.0 {
                        msg = next;
                    } else {
                        let _ = send_one(&msg);
                        msg = next;
                    }
                }
                let _ = send_one(&msg);
            }
        });
        Source { kind: Kind::Remote { shared, tx } }
    }

    pub fn snapshot(&self) -> Snapshot {
        match &self.kind {
            Kind::Local(h) => h.snapshot(),
            Kind::Remote { shared, .. } => shared.snap.lock().map(|g| g.clone()).unwrap_or_default(),
        }
    }

    /// Is the source answering? A local hub always is.
    pub fn connected(&self) -> bool {
        match &self.kind {
            Kind::Local(_) => true,
            Kind::Remote { shared, .. } => shared.connected.load(Ordering::Relaxed),
        }
    }

    /// Sets the wanted output of one pad.
    pub fn send(&self, uid: &str, out: &Output) {
        match &self.kind {
            Kind::Local(h) => h.set_output(Some(uid), |o| *o = out.clone()),
            Kind::Remote { tx, .. } => {
                let _ = tx.send((uid.to_string(), out.clone()));
            }
        }
    }

    /// Like [`send`](Self::send) but waits for the answer (used when the window closes).
    pub fn send_blocking(&self, uid: &str, out: &Output) {
        match &self.kind {
            Kind::Local(h) => h.set_output(Some(uid), |o| *o = out.clone()),
            Kind::Remote { .. } => {
                let _ = send_one(&(uid.to_string(), out.clone()));
            }
        }
    }

    /// Switches the Xbox emulation on or off (the owner of the hub keeps the switch).
    pub fn emu_enable(&self, on: bool) {
        match &self.kind {
            Kind::Local(h) => {
                if let Some(e) = h.emulator() {
                    e.set_enabled(on);
                }
            }
            Kind::Remote { .. } => {
                let msg = if on { "emu on" } else { "emu off" };
                std::thread::spawn(move || {
                    let _ = ipc::request("daemon", msg);
                });
            }
        }
    }

    /// The emergency button: emulation off and every pad visible again. Works without the background part too.
    pub fn emu_restore(&self, data_dir: &std::path::Path) -> Result<(), String> {
        match &self.kind {
            Kind::Local(h) => h.emulator().map(|e| e.restore_visibility()).unwrap_or(Ok(())),
            Kind::Remote { .. } => match ipc::request("daemon", "emu restore") {
                Some(r) if r == "ok" => Ok(()),
                Some(r) => Err(r.trim_start_matches("error ").to_string()),
                // The background part is not running: undo from the record on disk.
                None => {
                    crate::emu::set_flag(data_dir, false);
                    crate::emu::hide::restore_from_file(data_dir)
                }
            },
        }
    }

    /// Tells the background process to re-read `config.json`.
    pub fn reload(&self) {
        match &self.kind {
            Kind::Local(h) => {
                let dir = crate::config::data_dir();
                let cfg = crate::config::Config::load_from(&dir);
                h.set_template(cfg.output_template());
                if let Some(e) = h.emulator() {
                    e.apply_config(&cfg);
                }
            }
            Kind::Remote { .. } => {
                std::thread::spawn(|| {
                    let _ = ipc::request("daemon", "reload");
                });
            }
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        match &self.kind {
            Kind::Local(h) => h.stop(),
            Kind::Remote { shared, .. } => shared.stop.store(true, Ordering::Relaxed),
        }
    }
}

fn send_one(msg: &(String, Output)) -> Option<String> {
    let json = serde_json::to_string(&msg.1).ok()?;
    ipc::request("daemon", &format!("out {} {}", msg.0, json))
}
