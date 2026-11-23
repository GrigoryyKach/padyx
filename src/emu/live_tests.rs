//! Live checks against a really installed ViGEmBus, with synthetic pad states (never the user's pad, never a
//! hiding tool). They are `#[ignore]`d: run them with
//! `cargo test --release live_ -- --ignored --nocapture --test-threads=1` on a machine with ViGEmBus.
//! A game that listens to XInput during the run may see a short-lived extra pad.

use super::backend::BackendStatus;
use super::hide::NullHider;
use super::map::EmuConfig;
use super::*;
use crate::pad::hub::tests::MockIo;
use crate::pad::{Btn, Conn, PadKind};
use std::sync::atomic::AtomicU64;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct XState {
    packet: u32,
    buttons: u16,
    lt: u8,
    rt: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

#[repr(C)]
struct XVibration {
    left: u16,
    right: u16,
}

type GetState = unsafe extern "system" fn(u32, *mut XState) -> u32;
type SetState = unsafe extern "system" fn(u32, *mut XVibration) -> u32;

struct XInput {
    get: GetState,
    set: SetState,
}

fn load_xinput() -> XInput {
    let name: Vec<u16> = "xinput1_4.dll\0".encode_utf16().collect();
    // SAFETY: loading a system DLL and reading two documented exports.
    unsafe {
        let h = LoadLibraryW(name.as_ptr());
        assert!(!h.is_null(), "xinput1_4.dll");
        let get = GetProcAddress(h, b"XInputGetState\0".as_ptr()).expect("XInputGetState");
        let set = GetProcAddress(h, b"XInputSetState\0".as_ptr()).expect("XInputSetState");
        XInput { get: std::mem::transmute::<_, GetState>(get), set: std::mem::transmute::<_, SetState>(set) }
    }
}

fn info() -> PadInfo {
    PadInfo { uid: "live".into(), kind: PadKind::DualSense, conn: Conn::Usb, serial: "live-test".into(), demo: true, path: String::new() }
}

/// The XInput slot (0..3) whose state satisfies `pred`.
fn find_slot(x: &XInput, pred: impl Fn(&XState) -> bool) -> Option<u32> {
    (0..4).find(|u| {
        let mut s = XState::default();
        // SAFETY: a valid out pointer.
        unsafe { (x.get)(*u, &mut s) == 0 && pred(&s) }
    })
}

fn wait_until(mut f: impl FnMut() -> bool, ms: u64) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(ms) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

#[test]
#[ignore]
fn live_a_virtual_xbox_pad_appears_and_follows_the_demo_pad() {
    let be = backend::system_backend();
    if be.status() != BackendStatus::Ready {
        println!("SKIPPED: ViGEmBus is not installed or not usable ({:?})", be.status());
        return;
    }
    let xi = load_xinput();
    let hub = Hub::new(crate::pad::output::Output::default());
    let mut cfg = EmuConfig::default();
    cfg.left.inner = 0.0;
    let emu = Emulator::attach(&hub, be, Box::new(NullHider::default()), cfg, None, false);
    emu.set_enabled(true);

    let i = info();
    let mut s = PadState::default();
    s.set(Btn::Cross, true);
    s.set(Btn::Square, true);
    s.lx = 255;
    s.r2 = 200;
    s.l2 = 90;
    // The first report starts the setup; keep feeding until the XInput side shows our distinctive state.
    let want = |x: &XState| x.buttons == 0x1000 | 0x4000 && x.rt == 200 && x.lt == 90 && x.lx == 32767;
    let mut slot = None;
    assert!(
        wait_until(
            || {
                emu.on_report(&i, &s);
                slot = find_slot(&xi, want);
                slot.is_some()
            },
            5000
        ),
        "no XInput pad showed the state we sent"
    );
    let slot = slot.unwrap();
    println!("the virtual pad is XInput user {slot}");

    // Follows changes, and measures the whole path: on_report, driver, until XInput reports it.
    let mut times = Vec::new();
    for k in 0..300u32 {
        let v = 20 + (k % 200) as u8;
        s.r2 = v;
        let t0 = Instant::now();
        emu.on_report(&i, &s);
        let mut ok = false;
        while t0.elapsed() < Duration::from_millis(100) {
            let mut st = XState::default();
            // SAFETY: a valid out pointer.
            if unsafe { (xi.get)(slot, &mut st) } == 0 && st.rt == v {
                ok = true;
                break;
            }
        }
        assert!(ok, "the change to {v} did not reach XInput");
        times.push(t0.elapsed().as_micros() as u32);
    }
    times.sort_unstable();
    println!("report to XInput visible (on_report + driver + XInput read): median {} us, p99 {} us, max {} us", times[150], times[296], times[299]);

    // Switching off removes the pad.
    emu.set_enabled(false);
    assert!(wait_until(|| find_slot(&xi, want).is_none(), 3000), "the virtual pad did not go away");
    hub.stop();
}

/// The game's rumble (XInputSetState) comes back to the pad's output report. The pad is a scripted mock fed
/// through a real hub, so the whole chain runs: reader thread, emulator, driver, rumble notification, writer.
#[test]
#[ignore]
fn live_rumble_from_a_game_reaches_the_pad() {
    let be = backend::system_backend();
    if be.status() != BackendStatus::Ready {
        println!("SKIPPED: ViGEmBus is not installed or not usable");
        return;
    }
    let xi = load_xinput();
    let mut s = PadState::default();
    s.set(Btn::Triangle, true);
    s.r2 = 177;
    let (tx, rx) = std::sync::mpsc::channel();
    let io = MockIo { reports: (0..200_000).map(|_| crate::pad::demo::usb_report_from(&s)).collect(), writes: tx, fail_after_reports: false, feature_calls: Arc::new(AtomicU64::new(0)) };
    let hub = Hub::new(crate::pad::output::Output::default());
    let emu = Emulator::attach(&hub, be, Box::new(NullHider::default()), EmuConfig::default(), None, false);
    emu.set_enabled(true);
    hub.attach(info(), Box::new(io));
    let want = |x: &XState| x.buttons == 0x8000 && x.rt == 177;
    let mut slot = None;
    assert!(wait_until(|| { slot = find_slot(&xi, want); slot.is_some() }, 5000), "the pad did not appear");
    let slot = slot.unwrap();
    std::thread::sleep(Duration::from_millis(300));
    while rx.try_recv().is_ok() {}
    let mut vib = XVibration { left: 0xC000, right: 0x4000 };
    // SAFETY: a valid pointer.
    assert_eq!(unsafe { (xi.set)(slot, &mut vib) }, 0);
    let t0 = Instant::now();
    let mut seen = None;
    wait_until(
        || {
            while let Ok(w) = rx.try_recv() {
                if w.len() > 4 && w[0] == 0x02 && (w[1 + 3] != 0 || w[1 + 2] != 0) {
                    seen = Some((w[1 + 3], w[1 + 2]));
                }
            }
            seen.is_some()
        },
        2000,
    );
    println!("XInputSetState to the pad's output report: (large, small) = {seen:?} after {} us", t0.elapsed().as_micros());
    assert_eq!(seen, Some((0xC0, 0x40)));
    let mut off = XVibration { left: 0, right: 0 };
    // SAFETY: a valid pointer.
    unsafe { (xi.set)(slot, &mut off) };
    emu.set_enabled(false);
    assert!(wait_until(|| find_slot(&xi, want).is_none(), 3000));
    hub.stop();
}

/// How long `VirtualPad::submit` itself takes on the real driver.
#[test]
#[ignore]
fn live_submit_cost() {
    let be = backend::system_backend();
    if be.status() != BackendStatus::Ready {
        println!("SKIPPED: ViGEmBus is not installed or not usable");
        return;
    }
    let mut pad = be.create(Arc::new(|_, _| {})).expect("create");
    let mut times = Vec::new();
    for k in 0..5000i32 {
        let st = super::xbox::XboxState { lx: (k % 30000) as i16, ..Default::default() };
        let t0 = Instant::now();
        pad.submit(&st).expect("submit");
        times.push(t0.elapsed().as_micros() as u32);
    }
    times.sort_unstable();
    println!("VirtualPad::submit on ViGEmBus: median {} us, p99 {} us, max {} us", times[2500], times[4950], times[4999]);
}

