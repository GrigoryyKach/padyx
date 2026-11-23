//! Xbox emulation: a virtual Xbox 360 pad for every DualSense, so any game that knows XInput works.
//!
//! The flow, with no queue and no sleeping between the pad and the game:
//!
//! ```text
//! reader thread (one per pad)  ->  Emulator::on_report  ->  map::map_pad  ->  VirtualPad::submit
//! ```
//!
//! Rumble goes the other way: the virtual pad's back end calls a closure on its own thread, which hands the two
//! motor values to [`Hub::set_game_rumble`]; the hub's writer merges them with whatever the tester wants and
//! writes one report.
//!
//! Layers:
//! - [`xbox`], [`zones`], [`map`]: pure data and maths (the mapping layer the next section extends);
//! - [`backend`] (+ `backend_win`, `backend_linux`): the virtual pad itself;
//! - [`hide`]: hiding the real pad while the emulation is on, and giving it back;
//! - [`drivers`]: the status of ViGEmBus and HidHide and the checked download of their installers;
//! - [`Emulator`]: the glue. It is a [`PadListener`] of the hub.
//!
//! The on/off switch is kept by whoever owns the hub (the background part, or the window in `--demo`) in
//! `emulation.json`; the rest of the settings are in `config.json` under `emu`.

pub mod backend;
#[cfg(target_os = "linux")]
mod backend_linux;
#[cfg(windows)]
mod backend_win;
pub mod drivers;
pub mod foreground;
pub mod hide;
pub mod macros;
pub mod map;
pub mod notify;
pub mod profile;
pub mod remap;
pub mod xbox;
pub mod zones;

use crate::pad::hub::{Hub, PadListener};
use crate::pad::{PadInfo, PadState};
use backend::{Backend, BackendStatus, RumbleFn, VirtualPad};
use hide::{HideStatus, HideTarget, Hider};
use macros::Engine;
use map::{pad_key, EmuConfig};
use profile::Profile;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};
use xbox::XboxState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PadMode {
    /// The virtual pad is being set up.
    #[default]
    Starting,
    /// A virtual Xbox pad stands in for this pad.
    Emulated,
    /// Native mode: the game sees the DualSense itself.
    Native,
    /// The virtual pad could not be made (the reason is in `error`).
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PadEmu {
    pub uid: String,
    /// The key under which the native-mode switch of this pad is saved.
    pub key: String,
    pub mode: PadMode,
    /// The real pad is hidden from games.
    pub hidden: bool,
    /// Why the virtual pad failed, or why the pad could not be hidden (the emulation still runs then).
    pub error: Option<String>,
}

/// What the window shows about the emulation. Part of the snapshot the background part sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmuStatus {
    /// The emulator exists (the demo without it and a window without a background part show nothing).
    pub present: bool,
    pub enabled: bool,
    pub backend: BackendStatus,
    pub hide: HideStatus,
    pub pads: Vec<PadEmu>,
    /// A run with the mock back end: nothing real is created or hidden (screenshots, tests).
    pub simulated: bool,
    /// The active profile (empty: the default one).
    #[serde(default)]
    pub profile: String,
    /// The file name of the program in the foreground, when the emulator watches it (auto-switch on).
    #[serde(default)]
    pub fg_exe: Option<String>,
}

enum Slot {
    Starting,
    Active { pad: Box<dyn VirtualPad>, last: XboxState, sent: bool, engine: Engine },
    Native,
    Failed { error: String, retry_at: Instant },
}

struct PadSlot {
    uid: String,
    key: String,
    state: Mutex<Slot>,
    hidden: AtomicBool,
    hide_error: Mutex<Option<String>>,
}

pub struct Emulator {
    backend: Arc<dyn Backend>,
    hider: Mutex<Box<dyn Hider>>,
    cfg: RwLock<Arc<EmuConfig>>,
    enabled: AtomicBool,
    slots: Mutex<Vec<Arc<PadSlot>>>,
    hub: Mutex<Weak<Hub>>,
    flag_file: Option<PathBuf>,
    simulated: bool,
    me: Weak<Emulator>,
    cache: Mutex<(Instant, BackendStatus)>,
    hide_status: Mutex<HideStatus>,
    /// The clock macros and turbo run on.
    epoch: Instant,
    profiles: Mutex<ProfileRt>,
}

/// The profiles as the emulator knows them, and which one is active.
struct ProfileRt {
    default: EmuConfig,
    default_light: Option<[u8; 3]>,
    list: Vec<Profile>,
    current: String,
    auto: bool,
    /// The program in the foreground, as the watcher last saw it (a full path).
    fg: Option<String>,
    /// The active profile's name; empty is the default profile.
    active: String,
    /// The lightbar colour last applied for a profile (a profile with a colour sets it once, on switching).
    light_applied: Option<(String, [u8; 3])>,
}

const FAIL_RETRY: Duration = Duration::from_secs(3);
const STATUS_TTL: Duration = Duration::from_secs(2);

/// `emulation.json`: the on/off switch.
#[derive(Serialize, Deserialize, Default)]
struct Flag {
    enabled: bool,
}

pub fn flag_path(dir: &Path) -> PathBuf {
    dir.join("emulation.json")
}

pub fn read_flag(dir: &Path) -> bool {
    std::fs::read_to_string(flag_path(dir)).ok().and_then(|t| serde_json::from_str::<Flag>(t.trim_start_matches('\u{feff}')).ok()).is_some_and(|f| f.enabled)
}

/// Writes the switch without an emulator (the window, when the background part is not running).
pub fn set_flag(dir: &Path, on: bool) {
    write_flag(&flag_path(dir), on);
}

fn write_flag(path: &Path, on: bool) {
    if let Ok(json) = serde_json::to_vec(&Flag { enabled: on }) {
        let _ = crate::config::write_atomic(path, &json);
    }
}

fn scale(v: u8, pct: u8) -> u8 {
    ((v as u32 * pct.min(100) as u32 + 50) / 100) as u8
}

impl Emulator {
    /// Creates the emulator, undoes whatever a crashed run left hidden, and plugs itself into the hub.
    /// `data_dir` holds `emulation.json` (the switch); `persist` writes it when the switch changes.
    pub fn attach(hub: &Arc<Hub>, backend: Arc<dyn Backend>, mut hider: Box<dyn Hider>, cfg: EmuConfig, data_dir: Option<&Path>, simulated: bool) -> Arc<Emulator> {
        // The previous run may have died with the pad hidden: give it back before anything else.
        let _ = hider.recover();
        let mut cfg = cfg;
        cfg.sanitize();
        let enabled = data_dir.is_some_and(read_flag);
        let emu = Arc::new_cyclic(|me| Emulator {
            backend,
            hider: Mutex::new(hider),
            cfg: RwLock::new(Arc::new(cfg)),
            enabled: AtomicBool::new(enabled),
            slots: Mutex::new(Vec::new()),
            hub: Mutex::new(Arc::downgrade(hub)),
            flag_file: data_dir.map(flag_path),
            simulated,
            me: me.clone(),
            cache: Mutex::new((Instant::now() - STATUS_TTL * 2, BackendStatus::Ready)),
            hide_status: Mutex::new(HideStatus::Ready),
            epoch: Instant::now(),
            profiles: Mutex::new(ProfileRt { default: EmuConfig::default(), default_light: None, list: Vec::new(), current: String::new(), auto: false, fg: None, active: String::new(), light_applied: None }),
        });
        if let Ok(mut rt) = emu.profiles.lock() {
            rt.default = emu.config().as_ref().clone();
        }
        hub.set_emulator(emu.clone());
        emu
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn config(&self) -> Arc<EmuConfig> {
        self.cfg.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Applies one set of settings with no profiles (the default profile only).
    #[allow(dead_code)] // the tests and the live checks use it; the programs go through `apply_config`
    pub fn set_config(&self, cfg: EmuConfig) {
        self.set_profiles(cfg, None, Vec::new(), String::new(), false);
    }

    /// Applies the saved settings: the default profile (`cfg.emu`, `cfg.light`) and the others.
    pub fn apply_config(&self, cfg: &crate::config::Config) {
        self.set_profiles(cfg.emu.clone(), Some(cfg.light), cfg.profiles.clone(), cfg.profile_current.clone(), cfg.profile_auto);
    }

    /// Applies the default profile and the list, picks the active one and runs with its settings.
    pub fn set_profiles(&self, mut default: EmuConfig, default_light: Option<[u8; 3]>, mut list: Vec<Profile>, mut current: String, auto: bool) {
        default.sanitize();
        profile::sanitize(&mut list, &mut current);
        let step = {
            let Ok(mut rt) = self.profiles.lock() else { return };
            rt.default = default;
            rt.default_light = default_light;
            rt.list = list;
            rt.current = current;
            rt.auto = auto;
            Self::choose(&mut rt)
        };
        self.run_choice(step);
    }

    /// The program in the foreground changed (`None`: unknown). With auto-switch on this may change the profile.
    pub fn on_foreground(&self, exe: Option<String>) {
        let step = {
            let Ok(mut rt) = self.profiles.lock() else { return };
            if rt.fg == exe {
                return;
            }
            rt.fg = exe;
            if !rt.auto {
                return;
            }
            Self::choose(&mut rt)
        };
        self.run_choice(step);
    }

    /// Does the watcher need to look at the foreground?
    pub fn auto_switch(&self) -> bool {
        self.profiles.lock().map(|rt| rt.auto).unwrap_or(false)
    }

    /// Picks the active profile; returns the settings to run with and the lightbar colour to set, if any.
    fn choose(rt: &mut ProfileRt) -> (EmuConfig, Option<[u8; 3]>) {
        let idx = profile::pick(&rt.list, &rt.current, rt.auto, rt.fg.as_deref());
        rt.active = idx.map(|i| rt.list[i].name.clone()).unwrap_or_default();
        let eff = profile::effective(&rt.default, idx.map(|i| &rt.list[i]));
        let light = idx.and_then(|i| rt.list[i].light).or(rt.default_light);
        let apply = match light {
            Some(c) if rt.light_applied.as_ref() != Some(&(rt.active.clone(), c)) => {
                rt.light_applied = Some((rt.active.clone(), c));
                Some(c)
            }
            _ => None,
        };
        (eff, apply)
    }

    fn run_choice(&self, (cfg, light): (EmuConfig, Option<[u8; 3]>)) {
        self.apply(cfg);
        if let Some(c) = light {
            if let Some(hub) = self.hub.lock().ok().and_then(|h| h.upgrade()) {
                hub.set_output(None, |o| o.light = c);
            }
        }
    }

    /// Runs with these settings. A pad that went native loses its virtual pad at once; the others pick the new
    /// settings up on their next report.
    fn apply(&self, mut cfg: EmuConfig) {
        cfg.sanitize();
        let old = self.config();
        if *old == cfg {
            return;
        }
        if let Ok(mut g) = self.cfg.write() {
            *g = Arc::new(cfg.clone());
        }
        if !self.enabled() {
            return;
        }
        let slots: Vec<Arc<PadSlot>> = self.slots.lock().map(|g| g.clone()).unwrap_or_default();
        for s in slots {
            let native = cfg.is_native(&s.key);
            let was_native = matches!(*s.state.lock().expect("slot"), Slot::Native);
            if native && !was_native {
                self.deactivate(&s, Slot::Native);
            } else if !native && was_native {
                // Back to emulated: forget the slot, the next report creates it again.
                if let Ok(mut g) = self.slots.lock() {
                    g.retain(|x| !Arc::ptr_eq(x, &s));
                }
            }
        }
    }

    /// Turns the emulation on or off (and remembers it). Off drops every virtual pad and gives the real pads back.
    pub fn set_enabled(&self, on: bool) {
        if let Some(f) = &self.flag_file {
            write_flag(f, on);
        }
        let was = self.enabled.swap(on, Ordering::AcqRel);
        if was && !on {
            self.drop_everything();
        }
    }

    /// The emergency button: off, every pad visible again, even if the hider's own state was lost.
    pub fn restore_visibility(&self) -> Result<(), String> {
        self.set_enabled(false);
        let mut h = self.hider.lock().map_err(|_| "busy".to_string())?;
        h.restore()?;
        h.recover()
    }

    /// At exit: nothing stays hidden.
    pub fn shutdown(&self) {
        self.drop_everything();
    }

    fn drop_everything(&self) {
        let slots: Vec<Arc<PadSlot>> = self.slots.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
        for s in &slots {
            if let Ok(mut st) = s.state.lock() {
                *st = Slot::Native; // dropping an Active slot unplugs the virtual pad
            }
            self.clear_game_rumble(&s.uid);
        }
        if let Ok(mut h) = self.hider.lock() {
            let _ = h.restore();
        }
    }

    fn clear_game_rumble(&self, uid: &str) {
        if let Some(hub) = self.hub.lock().ok().and_then(|h| h.upgrade()) {
            hub.set_game_rumble(uid, 0, 0);
        }
    }

    /// Drops the virtual pad of one slot and unhides that pad.
    fn deactivate(&self, s: &Arc<PadSlot>, next: Slot) {
        if let Ok(mut st) = s.state.lock() {
            *st = next;
        }
        self.clear_game_rumble(&s.uid);
        if s.hidden.swap(false, Ordering::AcqRel) {
            if let Ok(mut h) = self.hider.lock() {
                let _ = h.unhide(&s.uid);
            }
        }
    }

    fn slot_for(&self, info: &PadInfo, cfg: &EmuConfig) -> (Arc<PadSlot>, bool) {
        let mut g = self.slots.lock().expect("slots");
        if let Some(s) = g.iter().find(|s| s.uid == info.uid) {
            return (s.clone(), false);
        }
        let key = pad_key(&info.serial, &info.uid);
        let native = cfg.is_native(&key);
        let s = Arc::new(PadSlot {
            uid: info.uid.clone(),
            key,
            state: Mutex::new(if native { Slot::Native } else { Slot::Starting }),
            hidden: AtomicBool::new(false),
            hide_error: Mutex::new(None),
        });
        g.push(s.clone());
        (s, !native)
    }

    /// Makes the virtual pad and hides the real one. Runs on its own short-lived thread: the reader never waits
    /// for a driver call or a command-line tool.
    fn start_setup(&self, slot: Arc<PadSlot>, info: PadInfo) {
        let Some(emu) = self.me.upgrade() else { return };
        let _ = std::thread::Builder::new().name("padyx-emu-setup".into()).spawn(move || emu.setup(&slot, &info));
    }

    fn rumble_fn(&self, uid: &str) -> RumbleFn {
        let emu = self.me.clone();
        let hub = self.hub.lock().map(|h| h.clone()).unwrap_or_default();
        let uid = uid.to_string();
        Arc::new(move |large, small| {
            let (Some(emu), Some(hub)) = (emu.upgrade(), hub.upgrade()) else { return };
            if !emu.enabled() {
                return;
            }
            let pct = emu.config().rumble_pct;
            hub.set_game_rumble(&uid, scale(large, pct), scale(small, pct));
        })
    }

    fn setup(&self, slot: &Arc<PadSlot>, info: &PadInfo) {
        let pad = match self.backend.create(self.rumble_fn(&info.uid)) {
            Ok(p) => p,
            Err(e) => {
                if let Ok(mut st) = slot.state.lock() {
                    *st = Slot::Failed { error: e, retry_at: Instant::now() + FAIL_RETRY };
                }
                return;
            }
        };
        // The emulation may have been switched off while the driver was busy.
        if !self.enabled() {
            drop(pad);
            return;
        }
        // Never hide the demo pad (it is not a device) or anything in a simulated run.
        if !info.demo {
            let target = HideTarget { uid: info.uid.clone(), serial: info.serial.clone(), path: info.path.clone() };
            let mut h = self.hider.lock().expect("hider");
            let status = h.status();
            if let Ok(mut g) = self.hide_status.lock() {
                *g = status;
            }
            match h.hide(&target) {
                Ok(()) => slot.hidden.store(true, Ordering::Release),
                Err(e) => {
                    if let Ok(mut g) = slot.hide_error.lock() {
                        *g = Some(e);
                    }
                }
            }
        } else if self.simulated {
            slot.hidden.store(true, Ordering::Release);
        }
        if let Ok(mut st) = slot.state.lock() {
            *st = Slot::Active { pad, last: XboxState::default(), sent: false, engine: Engine::new() };
        }
    }

    /// What the window shows.
    pub fn status(&self) -> EmuStatus {
        let backend = {
            let mut c = self.cache.lock().expect("cache");
            if c.0.elapsed() > STATUS_TTL {
                *c = (Instant::now(), self.backend.status());
            }
            c.1.clone()
        };
        let pads = self
            .slots
            .lock()
            .map(|g| {
                g.iter()
                    .map(|s| {
                        let (mode, err) = match &*s.state.lock().expect("slot") {
                            Slot::Starting => (PadMode::Starting, None),
                            Slot::Active { .. } => (PadMode::Emulated, None),
                            Slot::Native => (PadMode::Native, None),
                            Slot::Failed { error, .. } => (PadMode::Failed, Some(error.clone())),
                        };
                        let hide_err = s.hide_error.lock().ok().and_then(|g| g.clone());
                        PadEmu { uid: s.uid.clone(), key: s.key.clone(), mode, hidden: s.hidden.load(Ordering::Acquire), error: err.or(hide_err) }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (profile, fg_exe) = self
            .profiles
            .lock()
            .map(|rt| (rt.active.clone(), if rt.auto { rt.fg.as_deref().map(|p| p.rsplit(['\\', '/']).next().unwrap_or(p).to_string()) } else { None }))
            .unwrap_or_default();
        EmuStatus { present: true, enabled: self.enabled(), backend, hide: self.hide_status.lock().map(|g| *g).unwrap_or_default(), pads, simulated: self.simulated, profile, fg_exe }
    }
}

impl PadListener for Emulator {
    fn on_report(&self, info: &PadInfo, state: &PadState) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let cfg = self.config();
        let (slot, fresh) = self.slot_for(info, &cfg);
        if fresh {
            self.start_setup(slot, info.clone());
            return;
        }
        let mut st = slot.state.lock().expect("slot");
        match &mut *st {
            Slot::Active { pad, last, sent, engine } => {
                let x = engine.process(self.epoch.elapsed().as_micros() as u64, state, &cfg);
                if *sent && x == *last {
                    return;
                }
                match pad.submit(&x) {
                    Ok(()) => {
                        *last = x;
                        *sent = true;
                    }
                    Err(e) => {
                        let hub_slot = slot.clone();
                        *st = Slot::Failed { error: e, retry_at: Instant::now() + FAIL_RETRY };
                        drop(st);
                        self.clear_game_rumble(&hub_slot.uid);
                    }
                }
            }
            Slot::Failed { retry_at, .. } if Instant::now() >= *retry_at => {
                *st = Slot::Starting;
                drop(st);
                self.start_setup(slot, info.clone());
            }
            _ => {}
        }
    }

    fn on_disconnect(&self, info: &PadInfo) {
        let removed: Option<Arc<PadSlot>> = self.slots.lock().ok().and_then(|mut g| g.iter().position(|s| s.uid == info.uid).map(|i| g.remove(i)));
        if let Some(s) = removed {
            if let Ok(mut st) = s.state.lock() {
                *st = Slot::Native;
            }
            if s.hidden.swap(false, Ordering::AcqRel) {
                if let Ok(mut h) = self.hider.lock() {
                    let _ = h.unhide(&s.uid);
                }
            }
        }
    }
}

/// The emulator for this system, or the mock when `PADYX_EMU_MOCK=1` is set in a test instance (screenshots).
/// It also starts the watcher of the foreground program (idle unless auto-switch is on).
pub fn attach_system(hub: &Arc<Hub>, cfg: &crate::config::Config, data_dir: &Path) -> Arc<Emulator> {
    let emu = if drivers::mock_requested() {
        let be = backend::MockBackend::new();
        Emulator::attach(hub, be, Box::new(hide::SimHider::default()), cfg.emu.clone(), Some(data_dir), true)
    } else {
        Emulator::attach(hub, backend::system_backend(), hide::system_hider(data_dir), cfg.emu.clone(), Some(data_dir), false)
    };
    emu.apply_config(cfg);
    foreground::spawn_watcher(Arc::downgrade(&emu), foreground::system_probe(), foreground::WATCH_EVERY);
    emu
}

#[cfg(test)]
mod tests;
#[cfg(all(test, windows))]
mod live_tests;
