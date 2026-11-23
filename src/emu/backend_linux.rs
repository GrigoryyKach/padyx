//! Linux: a virtual Xbox 360 pad through uinput.
//!
//! The device looks like the kernel's `xpad` driver's pad (vendor `045e`, product `028e`): the same buttons
//! (`BTN_SOUTH`, `BTN_EAST`, `BTN_NORTH`, `BTN_WEST`, `BTN_TL`, `BTN_TR`, `BTN_SELECT`, `BTN_START`, `BTN_MODE`,
//! `BTN_THUMBL`, `BTN_THUMBR`), the same axes (`ABS_X/Y` and `ABS_RX/RY` of 16 bits with the Y axis pointing down,
//! `ABS_Z`/`ABS_RZ` for the triggers 0..255, `ABS_HAT0X/Y` for the D-pad) and force feedback of the rumble type,
//! so SDL, Steam Input and Proton recognise and map it without any database entry.
//!
//! Games set the rumble by uploading an effect and playing it. A thread reads those requests from the uinput
//! file (non-blocking, with a poll so it can stop) and passes the two motor strengths on.

use super::backend::{Backend, BackendStatus, RumbleFn, VirtualPad};
use super::xbox::{XBtn, XboxState};
use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventSummary, FFEffectCode, FFEffectKind, InputEvent, InputId, KeyCode, UInputCode, UinputAbsSetup,
};
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct UinputBackend;

pub fn system_backend() -> Arc<dyn Backend> {
    Arc::new(UinputBackend)
}

const BUTTONS: [(XBtn, KeyCode); 11] = [
    (XBtn::A, KeyCode::BTN_SOUTH),
    (XBtn::B, KeyCode::BTN_EAST),
    (XBtn::X, KeyCode::BTN_NORTH),
    (XBtn::Y, KeyCode::BTN_WEST),
    (XBtn::Lb, KeyCode::BTN_TL),
    (XBtn::Rb, KeyCode::BTN_TR),
    (XBtn::Back, KeyCode::BTN_SELECT),
    (XBtn::Start, KeyCode::BTN_START),
    (XBtn::Guide, KeyCode::BTN_MODE),
    (XBtn::L3, KeyCode::BTN_THUMBL),
    (XBtn::R3, KeyCode::BTN_THUMBR),
];

impl Backend for UinputBackend {
    fn status(&self) -> BackendStatus {
        match std::fs::OpenOptions::new().read(true).write(true).open("/dev/uinput") {
            Ok(_) => BackendStatus::Ready,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BackendStatus::DriverMissing,
            Err(_) => BackendStatus::NoAccess,
        }
    }

    fn create(&self, rumble: RumbleFn) -> Result<Box<dyn VirtualPad>, String> {
        let err = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::PermissionDenied => "no access to /dev/uinput (install the udev rule in Settings)".to_string(),
            std::io::ErrorKind::NotFound => "/dev/uinput is missing (load the uinput kernel module: sudo modprobe uinput)".to_string(),
            _ => format!("uinput: {e}"),
        };
        let mut keys = AttributeSet::<KeyCode>::new();
        for (_, k) in BUTTONS {
            keys.insert(k);
        }
        let stick = AbsInfo::new(0, -32768, 32767, 16, 128, 0);
        let trigger = AbsInfo::new(0, 0, 255, 0, 0, 0);
        let hat = AbsInfo::new(0, -1, 1, 0, 0, 0);
        let mut ff = AttributeSet::<FFEffectCode>::new();
        ff.insert(FFEffectCode::FF_RUMBLE);
        let mut b = VirtualDevice::builder().map_err(err)?.name("Microsoft X-Box 360 pad").input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0110));
        b = b.with_keys(&keys).map_err(err)?;
        for (axis, info) in [
            (AbsoluteAxisCode::ABS_X, stick),
            (AbsoluteAxisCode::ABS_Y, stick),
            (AbsoluteAxisCode::ABS_RX, stick),
            (AbsoluteAxisCode::ABS_RY, stick),
            (AbsoluteAxisCode::ABS_Z, trigger),
            (AbsoluteAxisCode::ABS_RZ, trigger),
            (AbsoluteAxisCode::ABS_HAT0X, hat),
            (AbsoluteAxisCode::ABS_HAT0Y, hat),
        ] {
            b = b.with_absolute_axis(&UinputAbsSetup::new(axis, info)).map_err(err)?;
        }
        let dev = b.with_ff(&ff).map_err(err)?.with_ff_effects_max(16).build().map_err(err)?;
        let fd = dev.as_raw_fd();
        // SAFETY: fcntl on a descriptor we own, to make reads non-blocking so the rumble thread never holds the lock while waiting.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        let dev = Arc::new(Mutex::new(dev));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (dev, stop) = (dev.clone(), stop.clone());
            let _ = std::thread::Builder::new().name("padyx-uinput-ff".into()).spawn(move || ff_loop(dev, fd, rumble, stop));
        }
        Ok(Box::new(UPad { dev, stop, last: XboxState::default(), sent: false }))
    }
}

struct UPad {
    dev: Arc<Mutex<VirtualDevice>>,
    stop: Arc<AtomicBool>,
    last: XboxState,
    sent: bool,
}

impl Drop for UPad {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn abs(code: AbsoluteAxisCode, v: i32) -> InputEvent {
    InputEvent::new(evdev::EventType::ABSOLUTE.0, code.0, v)
}

fn key(code: KeyCode, down: bool) -> InputEvent {
    InputEvent::new(evdev::EventType::KEY.0, code.0, down as i32)
}

/// The events that bring a pad that last reported `from` to `to` (everything on the first report).
fn events_between(from: Option<&XboxState>, to: &XboxState) -> Vec<InputEvent> {
    let mut ev = Vec::new();
    let was = from.copied().unwrap_or_default();
    let first = from.is_none();
    for (x, k) in BUTTONS {
        if first || was.pressed(x) != to.pressed(x) {
            ev.push(key(k, to.pressed(x)));
        }
    }
    let hat_x = |s: &XboxState| s.pressed(XBtn::Right) as i32 - s.pressed(XBtn::Left) as i32;
    let hat_y = |s: &XboxState| s.pressed(XBtn::Down) as i32 - s.pressed(XBtn::Up) as i32;
    if first || hat_x(&was) != hat_x(to) {
        ev.push(abs(AbsoluteAxisCode::ABS_HAT0X, hat_x(to)));
    }
    if first || hat_y(&was) != hat_y(to) {
        ev.push(abs(AbsoluteAxisCode::ABS_HAT0Y, hat_y(to)));
    }
    // xpad reports the Y axes pointing down.
    let axes = [
        (AbsoluteAxisCode::ABS_X, was.lx as i32, to.lx as i32),
        (AbsoluteAxisCode::ABS_Y, -(was.ly as i32), -(to.ly as i32)),
        (AbsoluteAxisCode::ABS_RX, was.rx as i32, to.rx as i32),
        (AbsoluteAxisCode::ABS_RY, -(was.ry as i32), -(to.ry as i32)),
        (AbsoluteAxisCode::ABS_Z, was.lt as i32, to.lt as i32),
        (AbsoluteAxisCode::ABS_RZ, was.rt as i32, to.rt as i32),
    ];
    for (code, a, b) in axes {
        if first || a != b {
            ev.push(abs(code, b));
        }
    }
    ev
}

impl VirtualPad for UPad {
    fn submit(&mut self, s: &XboxState) -> Result<(), String> {
        let ev = events_between(if self.sent { Some(&self.last) } else { None }, s);
        if ev.is_empty() {
            return Ok(());
        }
        // `emit` adds the SYN_REPORT that ends the batch.
        self.dev.lock().map_err(|_| "uinput lock".to_string())?.emit(&ev).map_err(|e| e.to_string())?;
        self.last = *s;
        self.sent = true;
        Ok(())
    }
}

/// An uploaded rumble effect: strengths and how long it lasts (0 means until stopped).
#[derive(Clone, Copy)]
struct Effect {
    strong: u16,
    weak: u16,
    length_ms: u16,
}

/// Reads the force-feedback requests of the games and reports the two motor strengths.
fn ff_loop(dev: Arc<Mutex<VirtualDevice>>, fd: i32, rumble: RumbleFn, stop: Arc<AtomicBool>) {
    let mut effects: HashMap<i16, Effect> = HashMap::new();
    // Effect id to the moment it ends (`None`: until stopped).
    let mut playing: HashMap<i16, Option<Instant>> = HashMap::new();
    let mut shown = (0u8, 0u8);
    let mut next_id: i16 = 0;
    while !stop.load(Ordering::Acquire) {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd; the timeout bounds the wait so the stop flag is seen.
        let n = unsafe { libc::poll(&mut pfd, 1, 50) };
        if n > 0 && pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            break;
        }
        if n > 0 && pfd.revents & libc::POLLIN != 0 {
            let Ok(mut d) = dev.lock() else { break };
            let events: Vec<InputEvent> = match d.fetch_events() {
                Ok(it) => it.collect(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Vec::new(),
                Err(_) => break,
            };
            for ev in events {
                match ev.destructure() {
                    EventSummary::UInput(e, UInputCode::UI_FF_UPLOAD, ..) => {
                        if let Ok(mut up) = d.process_ff_upload(e) {
                            let id = if up.effect_id() >= 0 { up.effect_id() } else { next_id };
                            if up.effect_id() < 0 {
                                next_id = (next_id + 1) % 16;
                            }
                            up.set_effect_id(id);
                            match up.effect().kind {
                                FFEffectKind::Rumble { strong_magnitude, weak_magnitude } => {
                                    effects.insert(id, Effect { strong: strong_magnitude, weak: weak_magnitude, length_ms: up.effect().replay.length });
                                    up.set_retval(0);
                                }
                                _ => up.set_retval(-1),
                            }
                        }
                    }
                    EventSummary::UInput(e, UInputCode::UI_FF_ERASE, ..) => {
                        if let Ok(er) = d.process_ff_erase(e) {
                            let id = er.effect_id() as i16;
                            effects.remove(&id);
                            playing.remove(&id);
                        }
                    }
                    EventSummary::ForceFeedback(_, code, value) => {
                        let id = code.0 as i16;
                        if value > 0 {
                            if let Some(fx) = effects.get(&id) {
                                let end = (fx.length_ms > 0).then(|| Instant::now() + Duration::from_millis(fx.length_ms as u64));
                                playing.insert(id, end);
                            }
                        } else {
                            playing.remove(&id);
                        }
                    }
                    _ => {}
                }
            }
        }
        let now = Instant::now();
        playing.retain(|_, end| end.is_none_or(|t| now < t));
        let (mut large, mut small) = (0u16, 0u16);
        for id in playing.keys() {
            if let Some(fx) = effects.get(id) {
                large = large.max(fx.strong);
                small = small.max(fx.weak);
            }
        }
        let now_shown = ((large >> 8) as u8, (small >> 8) as u8);
        if now_shown != shown {
            shown = now_shown;
            rumble(shown.0, shown.1);
        }
    }
    if shown != (0, 0) {
        rumble(0, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(ev: &[InputEvent]) -> Vec<(u16, i32)> {
        ev.iter().map(|e| (e.code(), e.value())).collect()
    }

    #[test]
    fn the_first_report_sends_everything_and_later_ones_only_changes() {
        let rest = XboxState::default();
        let first = events_between(None, &rest);
        // 11 buttons, 2 hats, 6 axes.
        assert_eq!(first.len(), 19);
        let mut a = rest;
        a.set(XBtn::A, true);
        a.lx = 1000;
        let ev = events_between(Some(&rest), &a);
        assert_eq!(codes(&ev), vec![(KeyCode::BTN_SOUTH.0, 1), (AbsoluteAxisCode::ABS_X.0, 1000)]);
        assert!(events_between(Some(&a), &a).is_empty());
    }

    #[test]
    fn the_y_axes_point_down_like_xpad_and_the_dpad_is_a_hat() {
        let mut s = XboxState::default();
        s.ly = 20000; // stick up
        s.set(XBtn::Up, true);
        s.set(XBtn::Right, true);
        let ev = codes(&events_between(Some(&XboxState::default()), &s));
        assert!(ev.contains(&(AbsoluteAxisCode::ABS_Y.0, -20000)));
        assert!(ev.contains(&(AbsoluteAxisCode::ABS_HAT0Y.0, -1)));
        assert!(ev.contains(&(AbsoluteAxisCode::ABS_HAT0X.0, 1)));
    }
}
