//! Tests of the emulator glue: the mock back end stands in for the driver, a recording hider for HidHide.
//! No driver is touched, nothing is hidden, nothing is installed.

use super::backend::{CountingBackend, MockBackend};
use super::hide::{HideStatus, HideTarget, Hider, NullHider};
use super::map::EmuConfig;
use super::xbox::XBtn;
use super::*;
use crate::pad::hub::tests::MockIo;
use crate::pad::{Btn, Conn, PadKind};
use std::sync::atomic::AtomicU64;

/// Records every call, so a test can see exactly what the emulator asked for.
#[derive(Clone, Default)]
struct RecHider {
    log: Arc<Mutex<Vec<String>>>,
    fail_hide: Arc<AtomicBool>,
    hidden: Arc<Mutex<Vec<String>>>,
}

impl Hider for RecHider {
    fn status(&mut self) -> HideStatus {
        HideStatus::Ready
    }
    fn hide(&mut self, t: &HideTarget) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("hide {} {}", t.uid, t.path));
        if self.fail_hide.load(Ordering::Relaxed) {
            return Err("HidHide needs administrator rights".into());
        }
        self.hidden.lock().unwrap().push(t.uid.clone());
        Ok(())
    }
    fn unhide(&mut self, uid: &str) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("unhide {uid}"));
        self.hidden.lock().unwrap().retain(|u| u != uid);
        Ok(())
    }
    fn restore(&mut self) -> Result<(), String> {
        self.log.lock().unwrap().push("restore".into());
        self.hidden.lock().unwrap().clear();
        Ok(())
    }
    fn recover(&mut self) -> Result<(), String> {
        self.log.lock().unwrap().push("recover".into());
        Ok(())
    }
    fn is_hidden(&self, uid: &str) -> bool {
        self.hidden.lock().unwrap().iter().any(|u| u == uid)
    }
}

fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..300 {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn info(uid: &str, demo: bool) -> PadInfo {
    PadInfo { uid: uid.into(), kind: PadKind::DualSense, conn: Conn::Usb, serial: format!("AA:BB:CC:00:00:{uid}"), demo, path: if demo { String::new() } else { format!(r"\\?\hid#vid_054c&pid_0ce6#{uid}#{{x}}") } }
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("padyx-emu-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn rig(hider: RecHider, cfg: EmuConfig) -> (Arc<Hub>, Arc<Emulator>, Arc<MockBackend>) {
    let hub = Hub::new(crate::pad::output::Output::default());
    let be = MockBackend::new();
    let emu = Emulator::attach(&hub, be.clone(), Box::new(hider), cfg, None, false);
    (hub, emu, be)
}

fn feed(emu: &Emulator, i: &PadInfo, s: &PadState) {
    emu.on_report(i, s);
}

fn active(emu: &Emulator, uid: &str) -> bool {
    emu.status().pads.iter().any(|p| p.uid == uid && p.mode == PadMode::Emulated)
}

#[test]
fn nothing_happens_while_the_emulation_is_off() {
    let (_hub, emu, be) = rig(RecHider::default(), EmuConfig::default());
    let i = info("01", false);
    for _ in 0..5 {
        feed(&emu, &i, &PadState::default());
    }
    assert!(emu.status().pads.is_empty());
    assert_eq!(be.created.load(Ordering::Relaxed), 0);
}

#[test]
fn reports_become_xbox_states_and_unchanged_ones_are_not_resent() {
    let hider = RecHider::default();
    let (_hub, emu, be) = rig(hider, EmuConfig::default());
    emu.set_enabled(true);
    let i = info("01", true);
    let mut s = PadState::default();
    // The first report starts the setup; keep feeding until the virtual pad is up.
    assert!(wait_for(|| {
        feed(&emu, &i, &s);
        active(&emu, "01")
    }));
    s.set(Btn::Cross, true);
    s.l2 = 255;
    s.ly = 0;
    feed(&emu, &i, &s);
    feed(&emu, &i, &s);
    feed(&emu, &i, &s);
    let log = be.log.lock().unwrap().clone();
    let last = log.last().unwrap();
    assert!(last.pressed(XBtn::A) && last.lt == 255 && last.ly > 30000, "{last:?}");
    // The resting state once, the pressed one once.
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(be.live(), 1);
}

#[test]
fn off_unplugs_everything_and_gives_the_pads_back() {
    let hider = RecHider::default();
    let log = hider.log.clone();
    let (_hub, emu, be) = rig(hider.clone(), EmuConfig::default());
    emu.set_enabled(true);
    let i = info("01", false);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        active(&emu, "01") && hider.is_hidden("01")
    }));
    assert!(emu.status().pads[0].hidden);
    assert!(log.lock().unwrap().iter().any(|l| l.starts_with("hide 01")));
    emu.set_enabled(false);
    assert!(wait_for(|| be.live() == 0));
    assert!(!hider.is_hidden("01"), "visible again");
    assert!(log.lock().unwrap().last().unwrap() == "restore");
    assert!(emu.status().pads.is_empty() && !emu.status().enabled);
    // Reports while off change nothing.
    feed(&emu, &i, &PadState::default());
    assert!(emu.status().pads.is_empty());
}

#[test]
fn the_demo_pad_is_never_hidden() {
    let hider = RecHider::default();
    let log = hider.log.clone();
    let (_hub, emu, _be) = rig(hider, EmuConfig::default());
    emu.set_enabled(true);
    let i = info("demo", true);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        active(&emu, "demo")
    }));
    assert!(!log.lock().unwrap().iter().any(|l| l.starts_with("hide")), "{:?}", log.lock().unwrap());
    assert!(!emu.status().pads[0].hidden);
}

#[test]
fn a_hiding_failure_does_not_stop_the_virtual_pad_and_is_reported() {
    let hider = RecHider::default();
    hider.fail_hide.store(true, Ordering::Relaxed);
    let (_hub, emu, _be) = rig(hider, EmuConfig::default());
    emu.set_enabled(true);
    let i = info("01", false);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        active(&emu, "01")
    }));
    let p = &emu.status().pads[0];
    assert!(!p.hidden);
    assert!(p.error.as_deref().is_some_and(|e| e.contains("administrator")));
}

#[test]
fn a_driver_that_cannot_make_a_pad_shows_failed_with_the_reason() {
    let hub = Hub::new(Default::default());
    let be = Arc::new(MockBackend { fail_create: true, ..Default::default() });
    let hider = RecHider::default();
    let log = hider.log.clone();
    let emu = Emulator::attach(&hub, be, Box::new(hider), EmuConfig::default(), None, false);
    emu.set_enabled(true);
    let i = info("01", false);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        emu.status().pads.iter().any(|p| p.mode == PadMode::Failed)
    }));
    assert_eq!(emu.status().pads[0].error.as_deref(), Some("create failed"));
    assert!(!log.lock().unwrap().iter().any(|l| l.starts_with("hide")), "the pad is not hidden when there is no virtual pad to replace it");
}

#[test]
fn native_mode_means_no_virtual_pad_and_no_hiding() {
    let hider = RecHider::default();
    let log = hider.log.clone();
    let i = info("01", false);
    let mut cfg = EmuConfig::default();
    cfg.set_native(&map::pad_key(&i.serial, &i.uid), true);
    let (_hub, emu, be) = rig(hider, cfg);
    emu.set_enabled(true);
    for _ in 0..5 {
        feed(&emu, &i, &PadState::default());
    }
    let st = emu.status();
    assert_eq!(st.pads.len(), 1);
    assert_eq!(st.pads[0].mode, PadMode::Native);
    assert_eq!(be.created.load(Ordering::Relaxed), 0);
    assert!(!log.lock().unwrap().iter().any(|l| l.starts_with("hide")));
}

#[test]
fn switching_a_running_pad_to_native_drops_its_virtual_pad_and_unhides_it() {
    let hider = RecHider::default();
    let (_hub, emu, be) = rig(hider.clone(), EmuConfig::default());
    emu.set_enabled(true);
    let i = info("01", false);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        active(&emu, "01") && hider.is_hidden("01")
    }));
    let mut cfg = (*emu.config()).clone();
    cfg.set_native(&map::pad_key(&i.serial, &i.uid), true);
    emu.set_config(cfg.clone());
    assert_eq!(emu.status().pads[0].mode, PadMode::Native);
    assert!(wait_for(|| be.live() == 0));
    assert!(!hider.is_hidden("01"));
    // And back: the next report builds a new virtual pad.
    cfg.set_native(&map::pad_key(&i.serial, &i.uid), false);
    emu.set_config(cfg);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        active(&emu, "01")
    }));
    assert_eq!(be.live(), 1);
}

#[test]
fn new_dead_zones_apply_to_the_next_report() {
    let (_hub, emu, be) = rig(RecHider::default(), EmuConfig::default());
    emu.set_enabled(true);
    let i = info("01", true);
    let mut s = PadState::default();
    s.lx = 128 + 20; // about 0.16 of the travel
    assert!(wait_for(|| {
        feed(&emu, &i, &s);
        active(&emu, "01")
    }));
    feed(&emu, &i, &s);
    assert!(be.log.lock().unwrap().last().unwrap().lx > 0);
    let mut cfg = (*emu.config()).clone();
    cfg.left.inner = 0.3;
    emu.set_config(cfg);
    feed(&emu, &i, &s);
    assert_eq!(be.log.lock().unwrap().last().unwrap().lx, 0);
}

#[test]
fn a_disconnecting_pad_loses_its_virtual_pad_and_is_unhidden() {
    let hider = RecHider::default();
    let (_hub, emu, be) = rig(hider.clone(), EmuConfig::default());
    emu.set_enabled(true);
    let i = info("01", false);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        active(&emu, "01") && hider.is_hidden("01")
    }));
    emu.on_disconnect(&i);
    assert!(wait_for(|| be.live() == 0));
    assert!(!hider.is_hidden("01"));
    assert!(emu.status().pads.is_empty());
}

#[test]
fn the_switch_is_remembered_between_runs() {
    let dir = tmp("flag");
    let hub = Hub::new(Default::default());
    let emu = Emulator::attach(&hub, MockBackend::new(), Box::new(NullHider::default()), EmuConfig::default(), Some(&dir), false);
    assert!(!emu.enabled(), "off on the first run");
    emu.set_enabled(true);
    assert!(read_flag(&dir));
    let hub2 = Hub::new(Default::default());
    let emu2 = Emulator::attach(&hub2, MockBackend::new(), Box::new(NullHider::default()), EmuConfig::default(), Some(&dir), false);
    assert!(emu2.enabled());
    emu2.set_enabled(false);
    assert!(!read_flag(&dir));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn start_up_recovery_runs_before_anything_else() {
    let hider = RecHider::default();
    let log = hider.log.clone();
    let hub = Hub::new(Default::default());
    let _emu = Emulator::attach(&hub, MockBackend::new(), Box::new(hider), EmuConfig::default(), None, false);
    assert_eq!(log.lock().unwrap().first().map(|s| s.as_str()), Some("recover"));
}

#[test]
fn the_restore_button_turns_it_off_and_restores() {
    let hider = RecHider::default();
    let log = hider.log.clone();
    let (_hub, emu, _be) = rig(hider.clone(), EmuConfig::default());
    emu.set_enabled(true);
    let i = info("01", false);
    assert!(wait_for(|| {
        feed(&emu, &i, &PadState::default());
        active(&emu, "01") && hider.is_hidden("01")
    }));
    emu.restore_visibility().unwrap();
    assert!(!emu.enabled() && !hider.is_hidden("01"));
    assert!(log.lock().unwrap().iter().filter(|l| *l == "restore").count() >= 1);
}

#[test]
fn status_reaches_the_snapshot() {
    let (hub, emu, _be) = rig(RecHider::default(), EmuConfig::default());
    emu.set_enabled(true);
    let snap = hub.snapshot();
    assert!(snap.emu.present && snap.emu.enabled);
    let json = serde_json::to_string(&snap).unwrap();
    let back: crate::pad::Snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back.emu, snap.emu);
    // A snapshot from a process without an emulator says so.
    assert!(!Hub::new(Default::default()).snapshot().emu.present);
}

#[test]
fn game_rumble_reaches_the_pad_scaled_and_merged_with_the_testers() {
    use crate::pad::demo::usb_report_from;
    let (tx, rx) = std::sync::mpsc::channel();
    let io = MockIo { reports: (0..3000).map(|_| usb_report_from(&PadState::default())).collect(), writes: tx, fail_after_reports: false, feature_calls: Arc::new(AtomicU64::new(0)) };
    let hub = Hub::new(crate::pad::output::Output::default());
    let be = MockBackend::new();
    let emu = Emulator::attach(&hub, be.clone(), Box::new(RecHider::default()), EmuConfig::default(), None, false);
    emu.set_enabled(true);
    hub.attach(info("01", true), Box::new(io));
    assert!(wait_for(|| active(&emu, "01")), "the hub's own reader feeds the emulator");
    let last_write = |rx: &std::sync::mpsc::Receiver<Vec<u8>>| {
        std::thread::sleep(Duration::from_millis(80));
        std::iter::from_fn(|| rx.try_recv().ok()).last()
    };
    let _ = last_write(&rx);
    // Full strength: USB report, rumble right (small) at offset 1+2, left (large) at 1+3.
    be.game_rumble(200, 100);
    let w = last_write(&rx).expect("a write after the game's rumble");
    assert_eq!((w[1 + 3], w[1 + 2]), (200, 100));
    // Half intensity.
    let mut cfg = (*emu.config()).clone();
    cfg.rumble_pct = 50;
    emu.set_config(cfg);
    be.game_rumble(200, 100);
    let w = last_write(&rx).expect("write");
    assert_eq!((w[1 + 3], w[1 + 2]), (100, 50));
    // The tester's stronger value wins, and the game's value is not lost afterwards.
    hub.set_output(None, |o| o.rumble_left = 250);
    let w = last_write(&rx).expect("write");
    assert_eq!(w[1 + 3], 250);
    hub.set_output(None, |o| o.rumble_left = 0);
    let w = last_write(&rx).expect("write");
    assert_eq!((w[1 + 3], w[1 + 2]), (100, 50));
    // Switching off silences the game's rumble.
    emu.set_enabled(false);
    let w = last_write(&rx).expect("write");
    assert_eq!((w[1 + 3], w[1 + 2]), (0, 0));
    hub.stop();
}

/// Measures what the emulation adds between "a report was parsed" and "the virtual pad has the new state":
/// the whole of `on_report` (find the pad, map, submit) with a back end that only counts. Run with
/// `cargo test --release latency -- --nocapture` to see the numbers.
#[test]
fn added_latency_is_microseconds() {
    let hub = Hub::new(Default::default());
    let counting = Arc::new(CountingBackend(MockBackend::new()));
    let emu = Emulator::attach(&hub, counting.clone(), Box::new(NullHider::default()), EmuConfig::default(), None, false);
    emu.set_enabled(true);
    let i = info("01", true);
    let mut s = PadState::default();
    assert!(wait_for(|| {
        feed(&emu, &i, &s);
        active(&emu, "01")
    }));
    // Every report changes a stick, so every call maps and submits (the worst case).
    let n = 100_000usize;
    let mut times: Vec<u32> = Vec::with_capacity(n);
    for k in 0..n {
        s.lx = (k % 200) as u8 + 20;
        s.ly = ((k * 7) % 200) as u8 + 20;
        let t0 = Instant::now();
        emu.on_report(&i, &s);
        times.push(t0.elapsed().as_nanos().min(u32::MAX as u128) as u32);
    }
    times.sort_unstable();
    let p = |q: f64| times[((n as f64 - 1.0) * q) as usize] as f64 / 1000.0;
    println!("added latency per report (map + submit, mock back end): median {:.2} us, p99 {:.2} us, p99.9 {:.2} us, max {:.2} us", p(0.5), p(0.99), p(0.999), times[n - 1] as f64 / 1000.0);
    assert!(counting.0.submits.load(Ordering::Relaxed) >= n as u64);
    // Far under a millisecond even in a debug build on a busy machine.
    assert!(p(0.5) < 200.0, "median {} us", p(0.5));
}

// ── Section C: macros, turbo and profiles through the emulator ──

fn profile_with(name: &str, exe: &str) -> profile::Profile {
    profile::Profile { name: name.into(), exes: vec![exe.into()], ..Default::default() }
}

#[test]
fn macros_and_turbo_only_ever_reach_the_virtual_pad() {
    use super::macros::{Macro, Step, Turbo, XOut};
    use super::remap::DIn;
    let mut cfg = EmuConfig::default();
    cfg.macros.push(Macro { name: "Y tap".into(), chord: vec![DIn::Create, DIn::L1], steps: vec![Step::Hold(XOut::Btn(XBtn::Y), 40)], ..Default::default() });
    cfg.turbo.push(Turbo { btn: XBtn::A, hz: 30 });
    let (_hub, emu, be) = rig(RecHider::default(), cfg);
    emu.set_enabled(true);
    let i = info("01", true);
    let mut s = PadState::default();
    assert!(wait_for(|| {
        feed(&emu, &i, &s);
        active(&emu, "01")
    }));
    // Turbo: Cross held for 150 ms pulses A.
    s.set(Btn::Cross, true);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(150) {
        feed(&emu, &i, &s);
        std::thread::sleep(Duration::from_millis(2));
    }
    // The chord: Create + L1 starts the macro, which holds Y for 40 ms and swallows Back and LB.
    s.set(Btn::Cross, false);
    s.set(Btn::Create, true);
    s.set(Btn::L1, true);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(120) {
        feed(&emu, &i, &s);
        std::thread::sleep(Duration::from_millis(2));
    }
    let log = be.log.lock().unwrap().clone();
    let a_on = log.iter().filter(|x| x.pressed(XBtn::A)).count();
    let a_off_after = log.iter().skip_while(|x| !x.pressed(XBtn::A)).any(|x| !x.pressed(XBtn::A));
    assert!(a_on >= 1 && a_off_after, "turbo pulses A: {log:?}");
    assert!(log.iter().any(|x| x.pressed(XBtn::Y)), "the macro pressed Y");
    assert!(log.iter().skip_while(|x| !x.pressed(XBtn::Y)).any(|x| !x.pressed(XBtn::Y)), "and released it again");
    assert!(!log.iter().any(|x| x.pressed(XBtn::Back) || x.pressed(XBtn::Lb)), "the chord is swallowed");
}

#[test]
fn the_foreground_program_picks_the_profile_and_auto_off_ignores_it() {
    let (hub, emu, _be) = rig(RecHider::default(), EmuConfig::default());
    let mut shooter = profile_with("Shooter", "cod.exe");
    shooter.emu.left.inner = 0.3;
    shooter.emu.rumble_pct = 5; // not per profile: ignored
    shooter.light = Some([255, 0, 0]);
    let racing = profile_with("Racing", "forza.exe");
    let mut def = EmuConfig::default();
    def.rumble_pct = 70;
    emu.set_profiles(def.clone(), Some([1, 2, 3]), vec![shooter.clone(), racing], String::new(), true);
    assert_eq!(emu.status().profile, "");
    emu.on_foreground(Some("C:\\Games\\COD.exe".into()));
    let st = emu.status();
    assert_eq!(st.profile, "Shooter");
    assert_eq!(st.fg_exe.as_deref(), Some("COD.exe"));
    assert_eq!(emu.config().left.inner, 0.3);
    assert_eq!(emu.config().rumble_pct, 70, "rumble belongs to the machine");
    emu.on_foreground(Some("C:\\Windows\\explorer.exe".into()));
    assert_eq!(emu.status().profile, "");
    assert_eq!(emu.config().left.inner, def.left.inner);
    // Auto-switch off: the foreground does not matter, the chosen one does.
    emu.set_profiles(def.clone(), Some([1, 2, 3]), vec![shooter.clone()], "Shooter".into(), false);
    emu.on_foreground(Some("C:\\Games\\other.exe".into()));
    assert_eq!(emu.status().profile, "Shooter");
    assert!(emu.status().fg_exe.is_none(), "not watching");
    assert!(!hub.snapshot().emu.profile.is_empty());
    // A chosen profile that was deleted falls back to the default.
    emu.set_profiles(def, Some([1, 2, 3]), vec![], "Shooter".into(), false);
    assert_eq!(emu.status().profile, "");
}

#[test]
fn a_profile_can_set_the_lightbar_and_leaving_it_restores_the_usual_colour() {
    use crate::pad::demo::usb_report_from;
    let (tx, _rx) = std::sync::mpsc::channel();
    let io = MockIo { reports: (0..3000).map(|_| usb_report_from(&PadState::default())).collect(), writes: tx, fail_after_reports: false, feature_calls: Arc::new(AtomicU64::new(0)) };
    let hub = Hub::new(crate::pad::output::Output::default());
    let emu = Emulator::attach(&hub, MockBackend::new(), Box::new(RecHider::default()), EmuConfig::default(), None, false);
    hub.attach(info("01", true), Box::new(io));
    assert!(wait_for(|| !hub.snapshot().pads.is_empty()));
    let light = |h: &Arc<Hub>| h.snapshot().pads[0].out.light;
    let mut p = profile_with("Shooter", "cod.exe");
    p.light = Some([200, 10, 20]);
    emu.set_profiles(EmuConfig::default(), Some([9, 9, 9]), vec![p.clone()], String::new(), true);
    assert_eq!(light(&hub), [9, 9, 9], "the usual colour while the default profile is active");
    emu.on_foreground(Some("cod.exe".into()));
    assert_eq!(light(&hub), [200, 10, 20]);
    // Saving other settings does not repaint the colour a user picked meanwhile.
    hub.set_output(None, |o| o.light = [5, 5, 5]);
    emu.set_profiles(EmuConfig::default(), Some([9, 9, 9]), vec![p], String::new(), true);
    assert_eq!(light(&hub), [5, 5, 5]);
    emu.on_foreground(Some("explorer.exe".into()));
    assert_eq!(light(&hub), [9, 9, 9]);
    hub.stop();
}

#[test]
fn the_watcher_follows_a_probe_only_while_auto_switch_is_on() {
    use super::foreground::{spawn_watcher, Probe};
    struct Script(Arc<Mutex<Option<String>>>);
    impl Probe for Script {
        fn exe(&mut self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
    }
    let (_hub, emu, _be) = rig(RecHider::default(), EmuConfig::default());
    let now = Arc::new(Mutex::new(Some("cod.exe".to_string())));
    spawn_watcher(Arc::downgrade(&emu), Box::new(Script(now.clone())), Duration::from_millis(10));
    let list = vec![profile_with("Shooter", "cod.exe")];
    // Off: the watcher does nothing.
    emu.set_profiles(EmuConfig::default(), None, list.clone(), String::new(), false);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(emu.status().profile, "");
    // On: it finds the game, and follows when the game is left.
    emu.set_profiles(EmuConfig::default(), None, list, String::new(), true);
    assert!(wait_for(|| emu.status().profile == "Shooter"));
    *now.lock().unwrap() = Some("notepad.exe".into());
    assert!(wait_for(|| emu.status().profile.is_empty()));
}

#[test]
fn settings_with_profiles_round_trip_through_the_config_file() {
    let dir = tmp("profiles-cfg");
    let mut c = crate::config::Config::default();
    let mut p = profile_with("Racing", "forza.exe");
    p.emu.left.inner = 0.2;
    c.profiles.push(p);
    c.profile_current = "Racing".into();
    c.profile_auto = true;
    assert!(c.save_to(&dir));
    let l = crate::config::Config::load_from(&dir);
    assert_eq!(l.profiles, c.profiles);
    assert_eq!((l.profile_current.as_str(), l.profile_auto), ("Racing", true));
    // A file from before profiles existed has none, and the default profile is its own `emu`.
    std::fs::write(dir.join("config.json"), "{\"emu\":{\"rumble_pct\":30}}").unwrap();
    let l = crate::config::Config::load_from(&dir);
    assert!(l.profiles.is_empty() && !l.profile_auto && l.profile_current.is_empty());
    // A current name that does not exist is dropped on load.
    std::fs::write(dir.join("config.json"), "{\"profile_current\":\"Gone\"}").unwrap();
    assert!(crate::config::Config::load_from(&dir).profile_current.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// The cost of the whole mapping (remap, macros, turbo, dead zones) per report, with a busy configuration.
/// `cargo test --release engine_cost -- --nocapture` to see the numbers.
#[test]
fn engine_cost_with_remap_macros_and_turbo_is_nanoseconds() {
    use super::macros::{Engine, Macro, MacroMode, Step, Turbo, XOut};
    use super::remap::{DIn, Out};
    let mut cfg = EmuConfig::default();
    cfg.set_target(DIn::Cross, Out::Btn(XBtn::B));
    cfg.set_target(DIn::Circle, Out::Btn(XBtn::A));
    cfg.set_target(DIn::L2, Out::Btn(XBtn::X));
    cfg.set_target(DIn::TouchLeft, Out::Btn(XBtn::Lb));
    cfg.turbo = vec![Turbo { btn: XBtn::A, hz: 20 }, Turbo { btn: XBtn::X, hz: 12 }];
    cfg.macros = vec![
        Macro { chord: vec![DIn::Create, DIn::L1], steps: vec![Step::Hold(XOut::Btn(XBtn::B), 80), Step::Wait(40)], ..Default::default() },
        Macro { chord: vec![DIn::R3], mode: MacroMode::Toggle, steps: vec![Step::Press(XOut::Btn(XBtn::X)), Step::Wait(30), Step::Release(XOut::Btn(XBtn::X)), Step::Wait(100)], ..Default::default() },
    ];
    cfg.sanitize();
    let cfg = Arc::new(cfg);
    let mut e = Engine::new();
    let mut s = PadState::default();
    s.set(Btn::Cross, true);
    s.set(Btn::R3, true);
    let n = 200_000usize;
    let mut times: Vec<u32> = Vec::with_capacity(n);
    let mut sink = 0u32;
    for k in 0..n {
        s.lx = (k % 200) as u8 + 20;
        s.l2 = (k % 250) as u8;
        let t0 = Instant::now();
        let x = e.process(k as u64 * 4000, &s, &cfg);
        times.push(t0.elapsed().as_nanos().min(u32::MAX as u128) as u32);
        sink = sink.wrapping_add(x.buttons as u32);
    }
    times.sort_unstable();
    println!("mapping per report with remap, 2 macros and 2 turbo: median {} ns, p99 {} ns, max {} ns (sink {sink})", times[n / 2], times[n * 99 / 100], times[n - 1]);
    assert!(times[n / 2] < 50_000, "median {} ns", times[n / 2]);
}
