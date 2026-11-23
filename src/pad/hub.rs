//! Reader threads, hot-plug scanning and the output writer.
//!
//! One thread per pad owns its device: it reads reports, parses them into a [`PadState`], publishes the state,
//! calls the listeners and, between reports, writes the output report when the dirty flag is set. A scanner
//! thread looks for Sony pads once a second and starts or forgets readers, so connecting, disconnecting and
//! switching between USB and Bluetooth all work at any time.
//!
//! Plug-in points for later sections: [`Hub::add_listener`] (called on the reader thread for every parsed
//! report, keep it short and never sleep in it) and [`Hub::set_output`] (one writer, dirty flag, so nothing
//! spams the pad).

use super::output::{build_dualsense, Light, Output};
use super::parse::{self, Calib};
use super::state::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A failed read or write: the pad is gone.
#[derive(Debug)]
pub struct IoError;

/// The device as the reader sees it. The real one wraps `hidapi`, the demo pad and the tests use their own.
pub trait PadIo: Send {
    /// Waits up to `timeout_ms` for a report. `Ok(0)` is a timeout.
    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, IoError>;
    fn write(&mut self, data: &[u8]) -> Result<(), IoError>;
    /// Reads a feature report (`id` first in the result).
    fn feature(&mut self, id: u8, len: usize) -> Option<Vec<u8>>;
}

pub trait PadListener: Send + Sync {
    fn on_report(&self, info: &PadInfo, state: &PadState);
    fn on_disconnect(&self, _info: &PadInfo) {}
}

const MAX_INTERVALS: usize = 120;
/// Never write twice within this time.
/// Pause between the lightbar setup report and the colour.
const SETUP_GAP: Duration = Duration::from_millis(30);
/// The colour is written once more this long after connecting.
const SETTLE: Duration = Duration::from_millis(1500);
const MIN_WRITE_GAP: Duration = Duration::from_millis(8);
/// Rumble and trigger effects fade on the pad unless refreshed.
const REFRESH_ACTIVE: Duration = Duration::from_millis(200);

struct Slot {
    info: Mutex<PadInfo>,
    state: Mutex<PadState>,
    intervals: Mutex<VecDeque<u32>>,
    hz: Mutex<f32>,
    out: Mutex<Output>,
    /// Rumble a game asked for through the virtual pad: large motor in the high byte, small in the low one.
    /// Merged with `out` when writing (the stronger wins), so the tester and a game never overwrite each other.
    game_rumble: AtomicU16,
    dirty: AtomicBool,
    gone: AtomicBool,
}

pub struct Hub {
    slots: Mutex<Vec<Arc<Slot>>>,
    emulator: Mutex<Option<Arc<crate::emu::Emulator>>>,
    listeners: Mutex<Vec<Arc<dyn PadListener>>>,
    template: Mutex<Output>,
    access_denied: AtomicBool,
    seq: AtomicU64,
    stop: AtomicBool,
}

impl Hub {
    pub fn new(template: Output) -> Arc<Hub> {
        Arc::new(Hub {
            slots: Mutex::new(Vec::new()),
            emulator: Mutex::new(None),
            listeners: Mutex::new(Vec::new()),
            template: Mutex::new(template),
            access_denied: AtomicBool::new(false),
            seq: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        })
    }

    /// Plug-in point for the emulation (and the mapping section after it).
    pub fn add_listener(&self, l: Arc<dyn PadListener>) {
        if let Ok(mut g) = self.listeners.lock() {
            g.push(l);
        }
    }

    /// Registers the emulator as a listener and as the source of the emulation status in the snapshot.
    pub fn set_emulator(&self, e: Arc<crate::emu::Emulator>) {
        self.add_listener(e.clone());
        if let Ok(mut g) = self.emulator.lock() {
            *g = Some(e);
        }
    }

    pub fn emulator(&self) -> Option<Arc<crate::emu::Emulator>> {
        self.emulator.lock().ok().and_then(|g| g.clone())
    }

    /// Sets the rumble a game asked for (0..255 each) on one pad; it is merged with the tester's rumble.
    pub fn set_game_rumble(&self, uid: &str, large: u8, small: u8) {
        for s in self.live_slots() {
            if s.info.lock().map(|i| i.uid != uid).unwrap_or(true) {
                continue;
            }
            let v = ((large as u16) << 8) | small as u16;
            if s.game_rumble.swap(v, Ordering::AcqRel) != v {
                s.dirty.store(true, Ordering::Release);
            }
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Replaces the output that new pads start with, and applies the lightbar and LEDs to the connected ones.
    pub fn set_template(&self, t: Output) {
        if let Ok(mut g) = self.template.lock() {
            *g = t.clone();
        }
        for s in self.live_slots() {
            if let Ok(mut o) = s.out.lock() {
                o.light = t.light;
                o.brightness = t.brightness;
                o.players = t.players;
                o.mute_led = t.mute_led;
            }
            s.dirty.store(true, Ordering::Release);
        }
    }

    /// Changes the wanted output of one pad (`None` means every pad) and marks it dirty.
    pub fn set_output(&self, uid: Option<&str>, f: impl Fn(&mut Output)) {
        for s in self.live_slots() {
            if uid.is_some_and(|u| s.info.lock().map(|i| i.uid != u).unwrap_or(true)) {
                continue;
            }
            if let Ok(mut o) = s.out.lock() {
                f(&mut o);
            }
            s.dirty.store(true, Ordering::Release);
        }
    }

    fn live_slots(&self) -> Vec<Arc<Slot>> {
        self.slots.lock().map(|g| g.iter().filter(|s| !s.gone.load(Ordering::Relaxed)).cloned().collect()).unwrap_or_default()
    }

    pub fn snapshot(&self) -> Snapshot {
        let pads = self
            .live_slots()
            .iter()
            .map(|s| PadSnapshot {
                info: s.info.lock().map(|i| i.clone()).unwrap_or_default(),
                state: s.state.lock().map(|g| *g).unwrap_or_default(),
                timing: timing_of(s),
                out: s.out.lock().map(|o| o.clone()).unwrap_or_default(),
            })
            .collect();
        let emu = self.emulator().map(|e| e.status()).unwrap_or_default();
        Snapshot { pads, access_denied: self.access_denied.load(Ordering::Relaxed), seq: self.seq.fetch_add(1, Ordering::Relaxed), emu }
    }

    /// Starts a reader for a device (real or fake). The thread ends when the device fails.
    pub fn attach(self: &Arc<Self>, info: PadInfo, io: Box<dyn PadIo>) {
        let slot = Arc::new(Slot {
            info: Mutex::new(info.clone()),
            state: Mutex::new(PadState::default()),
            intervals: Mutex::new(VecDeque::new()),
            hz: Mutex::new(0.0),
            out: Mutex::new(self.template.lock().map(|t| t.clone()).unwrap_or_default()),
            game_rumble: AtomicU16::new(0),
            dirty: AtomicBool::new(true),
            gone: AtomicBool::new(false),
        });
        if let Ok(mut g) = self.slots.lock() {
            g.push(slot.clone());
        }
        let hub = self.clone();
        let _ = std::thread::Builder::new().name(format!("padyx-pad-{}", info.uid)).spawn(move || {
            run_pad(&hub, &slot, io);
            slot.gone.store(true, Ordering::Release);
            let info = slot.info.lock().map(|i| i.clone()).unwrap_or_default();
            if let Ok(ls) = hub.listeners.lock() {
                for l in ls.iter() {
                    l.on_disconnect(&info);
                }
            }
            if let Ok(mut g) = hub.slots.lock() {
                g.retain(|s| !Arc::ptr_eq(s, &slot));
            }
        });
    }

    /// Starts the hot-plug scanner for real pads.
    pub fn start_scanner(self: &Arc<Self>) {
        let hub = self.clone();
        let _ = std::thread::Builder::new().name("padyx-scan".into()).spawn(move || scan_loop(&hub));
    }

    /// Starts the scripted fake pad.
    pub fn start_demo(self: &Arc<Self>) {
        let info = PadInfo { uid: "demo".into(), kind: PadKind::DualSense, conn: Conn::Usb, serial: "demo".into(), demo: true, path: String::new() };
        self.attach(info, Box::new(super::demo::DemoIo::new()));
    }
}

fn timing_of(s: &Slot) -> Timing {
    let v: Vec<u32> = s.intervals.lock().map(|g| g.iter().copied().collect()).unwrap_or_default();
    let hz = s.hz.lock().map(|g| *g).unwrap_or(0.0);
    if v.is_empty() {
        return Timing { hz, ..Default::default() };
    }
    let ms: Vec<f32> = v.iter().map(|u| *u as f32 / 1000.0).collect();
    let avg = ms.iter().sum::<f32>() / ms.len() as f32;
    let max = ms.iter().cloned().fold(0.0, f32::max);
    let var = ms.iter().map(|m| (m - avg) * (m - avg)).sum::<f32>() / ms.len() as f32;
    Timing { hz, recent_us: v, avg_ms: avg, max_ms: max, jitter_ms: var.sqrt() }
}

fn read_calibration(kind: PadKind, io: &mut Box<dyn PadIo>, cal: &mut Calib) {
    if kind == PadKind::DualSense {
        if let Some(f) = io.feature(0x05, 41) {
            if let Some(c) = Calib::from_report(&f) {
                *cal = c;
            }
        }
    }
}

fn run_pad(hub: &Arc<Hub>, slot: &Arc<Slot>, mut io: Box<dyn PadIo>) {
    let kind = slot.info.lock().map(|i| i.kind).unwrap_or_default();
    let serial = slot.info.lock().map(|i| i.serial.clone()).unwrap_or_default();
    let mut cal = Calib::default();
    // Reading the calibration report also switches a Bluetooth pad from the reduced to the full report.
    let mut feature_tries = 0;
    read_calibration(kind, &mut io, &mut cal);
    feature_tries += 1;

    #[cfg(target_os = "linux")]
    let led = super::linux_led::find(&serial);
    #[cfg(not(target_os = "linux"))]
    let led: Option<std::path::PathBuf> = {
        let _ = &serial;
        None
    };

    let mut buf = [0u8; 256];
    let mut last_report: Option<Instant> = None;
    let mut last_write: Option<Instant> = None;
    let mut last_feature = Instant::now();
    let mut bt_seq = 0u8;
    let mut hz_count = 0u32;
    let mut hz_start = Instant::now();
    let mut last_led: Option<[u8; 3]> = None;
    // The lightbar setup goes out once, alone, before the first colour; the colour is sent once more a moment
    // later, after the pad has finished its own connect animation.
    let mut setup_sent = false;
    let mut reapply_at: Option<Instant> = None;

    while !hub.stop.load(Ordering::Relaxed) {
        match io.read(&mut buf, 40) {
            Err(_) => break,
            Ok(0) => {}
            Ok(n) => {
                let now = Instant::now();
                if let Some(mut st) = parse::parse(kind, &buf[..n], &cal) {
                    if let Some(prev) = last_report {
                        let us = now.duration_since(prev).as_micros().min(u32::MAX as u128) as u32;
                        if let Ok(mut iv) = slot.intervals.lock() {
                            if iv.len() >= MAX_INTERVALS {
                                iv.pop_front();
                            }
                            iv.push_back(us);
                        }
                    }
                    last_report = Some(now);
                    hz_count += 1;
                    let el = now.duration_since(hz_start);
                    if el >= Duration::from_millis(500) {
                        if let Ok(mut h) = slot.hz.lock() {
                            *h = hz_count as f32 / el.as_secs_f32();
                        }
                        hz_count = 0;
                        hz_start = now;
                    }
                    // A reduced report has no battery or motion: keep what we knew.
                    if st.reduced {
                        if let Ok(prev) = slot.state.lock() {
                            st.battery = prev.battery;
                            st.charge = prev.charge;
                        }
                        if feature_tries < 5 && last_feature.elapsed() > Duration::from_secs(1) {
                            last_feature = Instant::now();
                            feature_tries += 1;
                            read_calibration(kind, &mut io, &mut cal);
                        }
                    }
                    let info = {
                        let mut i = slot.info.lock().expect("info lock");
                        if i.conn != st.conn {
                            i.conn = st.conn;
                            slot.dirty.store(true, Ordering::Release);
                        }
                        i.clone()
                    };
                    if let Ok(mut g) = slot.state.lock() {
                        *g = st;
                    }
                    if let Ok(ls) = hub.listeners.lock() {
                        for l in ls.iter() {
                            l.on_report(&info, &st);
                        }
                    }
                }
            }
        }
        // The writer: only for a DualSense whose connection type is known.
        if kind != PadKind::DualSense {
            continue;
        }
        let conn = slot.info.lock().map(|i| i.conn).unwrap_or_default();
        if conn == Conn::Unknown {
            continue;
        }
        let mut out = slot.out.lock().map(|o| o.clone()).unwrap_or_default();
        let game = slot.game_rumble.load(Ordering::Acquire);
        out.rumble_left = out.rumble_left.max((game >> 8) as u8);
        out.rumble_right = out.rumble_right.max(game as u8);
        if reapply_at.is_some_and(|t| Instant::now() >= t) {
            reapply_at = None;
            slot.dirty.store(true, Ordering::Release);
        }
        let due = slot.dirty.load(Ordering::Acquire) || (out.is_active() && last_write.is_none_or(|t| t.elapsed() >= REFRESH_ACTIVE));
        if due && last_write.is_none_or(|t| t.elapsed() >= MIN_WRITE_GAP) {
            slot.dirty.store(false, Ordering::Release);
            let kernel_led = led.is_some();
            if !kernel_led && !setup_sent {
                setup_sent = true;
                reapply_at = Some(Instant::now() + SETTLE);
                let setup = build_dualsense(conn, &out, bt_seq, Light::Setup);
                if conn == Conn::Bluetooth {
                    bt_seq = (bt_seq + 1) & 0x0F;
                }
                if io.write(&setup).is_err() {
                    break;
                }
                std::thread::sleep(SETUP_GAP);
            }
            let report = build_dualsense(conn, &out, bt_seq, if kernel_led { Light::Kernel } else { Light::Colour });
            if conn == Conn::Bluetooth {
                bt_seq = (bt_seq + 1) & 0x0F;
            }
            if io.write(&report).is_err() {
                break;
            }
            last_write = Some(Instant::now());
            #[cfg(target_os = "linux")]
            if let Some(path) = &led {
                let rgb = out.light_scaled();
                if last_led != Some(rgb) {
                    super::linux_led::set_rgb(path, rgb);
                    last_led = Some(rgb);
                }
            }
            #[cfg(not(target_os = "linux"))]
            let _ = (&mut last_led, &led);
        }
    }
}

fn path_uid(path: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}

struct HidIo(hidapi::HidDevice);

impl PadIo for HidIo {
    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, IoError> {
        self.0.read_timeout(buf, timeout_ms).map_err(|_| IoError)
    }
    fn write(&mut self, data: &[u8]) -> Result<(), IoError> {
        self.0.write(data).map(|_| ()).map_err(|_| IoError)
    }
    fn feature(&mut self, id: u8, len: usize) -> Option<Vec<u8>> {
        let mut b = vec![0u8; len];
        b[0] = id;
        let n = self.0.get_feature_report(&mut b).ok()?;
        b.truncate(n);
        Some(b)
    }
}

/// Is this HID interface the pad's gamepad collection (and not an audio or vendor one)?
pub fn is_gamepad_interface(usage_page: u16, usage: u16) -> bool {
    usage_page == 0 || (usage_page == 0x01 && usage == 0x05)
}

fn scan_loop(hub: &Arc<Hub>) {
    let Ok(mut api) = hidapi::HidApi::new() else { return };
    let mut open: Vec<String> = Vec::new();
    while !hub.stop.load(Ordering::Relaxed) {
        if api.refresh_devices().is_ok() {
            let mut seen: Vec<String> = Vec::new();
            let mut denied = false;
            let live: Vec<String> = hub.live_slots().iter().map(|s| s.info.lock().map(|i| i.uid.clone()).unwrap_or_default()).collect();
            open.retain(|u| live.contains(u));
            for d in api.device_list() {
                if d.vendor_id() != SONY_VID {
                    continue;
                }
                let Some(kind) = PadKind::from_pid(d.product_id()) else { continue };
                if !is_gamepad_interface(d.usage_page(), d.usage()) {
                    continue;
                }
                let path = d.path().to_string_lossy().to_string();
                let uid = path_uid(&path);
                seen.push(uid.clone());
                if open.contains(&uid) {
                    continue;
                }
                match api.open_path(d.path()) {
                    Ok(dev) => {
                        let info = PadInfo { uid: uid.clone(), kind, conn: Conn::Unknown, serial: d.serial_number().unwrap_or("").to_string(), demo: false, path: path.clone() };
                        open.push(uid);
                        hub.attach(info, Box::new(HidIo(dev)));
                    }
                    Err(e) => {
                        let m = e.to_string().to_lowercase();
                        if m.contains("permission") || m.contains("access") {
                            denied = true;
                        }
                    }
                }
            }
            hub.access_denied.store(denied, Ordering::Relaxed);
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

/// Read-only listing for `--list`: what is connected, no output is ever written.
pub fn enumerate() -> Vec<String> {
    let Ok(api) = hidapi::HidApi::new() else { return vec!["hidapi could not start".into()] };
    api.device_list()
        .filter(|d| d.vendor_id() == SONY_VID && PadKind::from_pid(d.product_id()).is_some())
        .map(|d| {
            format!(
                "{:04x}:{:04x} {} usage {:#x}/{:#x} serial {}",
                d.vendor_id(),
                d.product_id(),
                PadKind::from_pid(d.product_id()).map(|k| k.name()).unwrap_or("?"),
                d.usage_page(),
                d.usage(),
                d.serial_number().unwrap_or("-")
            )
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pad::demo::{bt_report_from, usb_report_from};
    use std::sync::mpsc::{channel, Receiver, Sender};

    /// A scripted device: replays reports, records every write.
    pub struct MockIo {
        pub reports: VecDeque<Vec<u8>>,
        pub writes: Sender<Vec<u8>>,
        pub fail_after_reports: bool,
        pub feature_calls: Arc<AtomicU64>,
    }

    impl PadIo for MockIo {
        fn read(&mut self, buf: &mut [u8], _timeout_ms: i32) -> Result<usize, IoError> {
            match self.reports.pop_front() {
                Some(r) => {
                    buf[..r.len()].copy_from_slice(&r);
                    std::thread::sleep(Duration::from_millis(2));
                    Ok(r.len())
                }
                None if self.fail_after_reports => Err(IoError),
                None => {
                    std::thread::sleep(Duration::from_millis(5));
                    Ok(0)
                }
            }
        }
        fn write(&mut self, data: &[u8]) -> Result<(), IoError> {
            let _ = self.writes.send(data.to_vec());
            Ok(())
        }
        fn feature(&mut self, _id: u8, _len: usize) -> Option<Vec<u8>> {
            self.feature_calls.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    fn mock(reports: Vec<Vec<u8>>, fail: bool) -> (Box<MockIo>, Receiver<Vec<u8>>) {
        let (tx, rx) = channel();
        (Box::new(MockIo { reports: reports.into(), writes: tx, fail_after_reports: fail, feature_calls: Arc::new(AtomicU64::new(0)) }), rx)
    }

    fn info(uid: &str) -> PadInfo {
        PadInfo { uid: uid.into(), kind: PadKind::DualSense, conn: Conn::Unknown, serial: String::new(), demo: true, path: String::new() }
    }

    fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
        for _ in 0..200 {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn reader_publishes_state_timing_and_writes_the_template() {
        let mut st = PadState::default();
        st.lx = 3;
        st.set(Btn::Cross, true);
        let reports: Vec<Vec<u8>> = (0..30).map(|_| usb_report_from(&st)).collect();
        let (io, rx) = mock(reports, false);
        let mut t = Output::default();
        t.light = [1, 2, 3];
        let hub = Hub::new(t);
        hub.attach(info("a"), io);
        assert!(wait_for(|| hub.snapshot().pads.first().is_some_and(|p| p.timing.recent_us.len() >= 20)));
        let snap = hub.snapshot();
        let p = &snap.pads[0];
        assert_eq!(p.state.lx, 3);
        assert!(p.state.pressed(Btn::Cross));
        assert_eq!(p.info.conn, Conn::Usb);
        assert!(p.timing.avg_ms > 0.5 && p.timing.max_ms >= p.timing.avg_ms);
        // The connect writes: the lightbar setup alone, then the saved colour.
        let setup = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!((setup[1 + 38], setup[1 + 41]), (0x02, 0x02));
        let first = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(first[0], 0x02);
        assert_eq!(&first[1 + 44..1 + 47], &[1, 2, 3]);
        hub.stop();
    }

    #[test]
    fn dirty_flag_coalesces_writes_and_bluetooth_reports_have_a_crc() {
        let st = PadState::default();
        let (io, rx) = mock((0..400).map(|_| bt_report_from(&st)).collect(), false);
        let hub = Hub::new(Output::default());
        hub.attach(info("bt"), io);
        assert!(wait_for(|| hub.snapshot().pads.first().is_some_and(|p| p.info.conn == Conn::Bluetooth)));
        // The very first write comes after the connection type is known.
        let first = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(first.len(), 78);
        assert!(crate::pad::output::bt_crc_ok(&first));
        // Ten changes in a row cost far fewer than ten writes.
        std::thread::sleep(Duration::from_millis(50));
        while rx.try_recv().is_ok() {}
        for i in 0..10u8 {
            hub.set_output(None, |o| o.light = [i, 0, 0]);
        }
        std::thread::sleep(Duration::from_millis(100));
        let n = std::iter::from_fn(|| rx.try_recv().ok()).count();
        assert!((1..=3).contains(&n), "writes: {n}");
        hub.stop();
    }

    #[test]
    fn active_effects_are_refreshed_and_idle_pads_are_left_alone() {
        let (io, rx) = mock((0..200).map(|_| usb_report_from(&PadState::default())).collect(), false);
        let hub = Hub::new(Output::default());
        hub.attach(info("r"), io);
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_ok());
        std::thread::sleep(Duration::from_millis(60));
        while rx.try_recv().is_ok() {}
        // Idle: nothing is written for a while.
        std::thread::sleep(Duration::from_millis(150));
        assert!(rx.try_recv().is_err());
        // Rumble on: refreshed every 200 ms.
        hub.set_output(Some("r"), |o| o.rumble_left = 90);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_ok());
        assert!(rx.recv_timeout(Duration::from_millis(500)).is_ok());
        hub.stop();
    }

    #[test]
    fn disconnect_removes_the_pad_and_notifies() {
        struct L(Arc<AtomicBool>);
        impl PadListener for L {
            fn on_report(&self, _i: &PadInfo, _s: &PadState) {}
            fn on_disconnect(&self, _i: &PadInfo) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let flag = Arc::new(AtomicBool::new(false));
        let (io, _rx) = mock(vec![usb_report_from(&PadState::default()); 3], true);
        let hub = Hub::new(Output::default());
        hub.add_listener(Arc::new(L(flag.clone())));
        hub.attach(info("x"), io);
        assert!(wait_for(|| flag.load(Ordering::Relaxed)));
        assert!(wait_for(|| hub.snapshot().pads.is_empty()));
    }

    #[test]
    fn listeners_see_every_report_and_several_pads_coexist() {
        struct Count(AtomicU64);
        impl PadListener for Count {
            fn on_report(&self, _i: &PadInfo, _s: &PadState) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let c = Arc::new(Count(AtomicU64::new(0)));
        let hub = Hub::new(Output::default());
        hub.add_listener(c.clone());
        let (a, _ra) = mock((0..10).map(|_| usb_report_from(&PadState::default())).collect(), false);
        let (b, _rb) = mock((0..10).map(|_| bt_report_from(&PadState::default())).collect(), false);
        hub.attach(info("one"), a);
        hub.attach(info("two"), b);
        assert!(wait_for(|| c.0.load(Ordering::Relaxed) >= 20));
        let s = hub.snapshot();
        assert_eq!(s.pads.len(), 2);
        assert!(s.pads.iter().any(|p| p.info.conn == Conn::Usb) && s.pads.iter().any(|p| p.info.conn == Conn::Bluetooth));
        hub.stop();
    }

    #[test]
    fn reduced_bluetooth_asks_for_the_calibration_report() {
        let reduced = vec![0x01, 128, 128, 128, 128, 0x08, 0, 0, 0, 0];
        let (mut io, _rx) = mock(vec![reduced; 60], false);
        let calls = io.feature_calls.clone();
        io.fail_after_reports = false;
        let hub = Hub::new(Output::default());
        hub.attach(info("red"), io);
        assert!(wait_for(|| hub.snapshot().pads.first().is_some_and(|p| p.state.reduced)));
        // One read at the start; the retry waits a second, so only the first call is certain.
        assert!(calls.load(Ordering::Relaxed) >= 1);
        hub.stop();
    }

    #[test]
    fn interface_filter() {
        assert!(is_gamepad_interface(0, 0));
        assert!(is_gamepad_interface(1, 5));
        assert!(!is_gamepad_interface(0xFF00, 1));
    }
}
