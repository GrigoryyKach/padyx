//! Macros and turbo: the stateful part of the mapping layer.
//!
//! Both only ever write to the virtual Xbox pad (an [`XboxState`]). Nothing here sends a key or a mouse event
//! to the system, and nothing can: the engine has no handle to anything but its own return value.
//!
//! The [`Engine`] is fed every parsed report together with a clock value in microseconds, so a test can drive it
//! with a fake clock and check the timings exactly. The real clock is only read by the caller.
//!
//! - **Turbo.** A button that is held on the virtual pad (after the remap) is pulsed at 5 to 30 Hz, starting
//!   with "on" at the moment it went down.
//! - **Macros.** A trigger (one input or a chord such as Create + L1) starts a list of steps. Steps press and
//!   release Xbox outputs and wait; a "hold" step is press, wait, release. Modes: once, while the trigger is
//!   held, and toggle (loop until the trigger is pressed again). The trigger inputs can be held back from the
//!   normal mapping while the chord is down (`swallow`). Macro outputs are OR-ed on top of the mapped state.
//!
//! The pad sends 250 or more reports a second, so timings are accurate to a few milliseconds; a macro step
//! shorter than that is rounded up to the next report.

use super::map::EmuConfig;
use super::remap::{self, DIn};
use super::xbox::{XBtn, XboxState};
use crate::pad::PadState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAX_MACROS: usize = 32;
pub const MAX_STEPS: usize = 64;
pub const MAX_CHORD: usize = 4;
pub const MAX_STEP_MS: u32 = 60_000;
pub const TURBO_MIN_HZ: u8 = 5;
pub const TURBO_MAX_HZ: u8 = 30;
pub const TURBO_DEFAULT_HZ: u8 = 10;

/// What a macro step presses on the virtual pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum XOut {
    Btn(XBtn),
    /// The left trigger, fully pulled.
    Lt,
    /// The right trigger, fully pulled.
    Rt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    /// Press and keep pressed until a `Release` of it (or the end of the macro).
    Press(XOut),
    Release(XOut),
    /// Press, wait this many milliseconds, release.
    Hold(XOut, u32),
    /// Wait this many milliseconds.
    Wait(u32),
}

impl Step {
    fn ms(&self) -> u32 {
        match self {
            Step::Hold(_, ms) | Step::Wait(ms) => *ms,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MacroMode {
    /// One run of the steps per press of the trigger.
    #[default]
    Once,
    /// The steps repeat while the trigger is held.
    WhileHeld,
    /// A press starts the loop, the next press stops it.
    Toggle,
}

impl MacroMode {
    pub const ALL: [MacroMode; 3] = [MacroMode::Once, MacroMode::WhileHeld, MacroMode::Toggle];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Macro {
    pub name: String,
    pub enabled: bool,
    /// The inputs that must all be down (one input, or a chord).
    pub chord: Vec<DIn>,
    pub mode: MacroMode,
    /// While the chord is down, its inputs do not act on the virtual pad by themselves.
    pub swallow: bool,
    pub steps: Vec<Step>,
}

impl Default for Macro {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, chord: Vec::new(), mode: MacroMode::Once, swallow: true, steps: Vec::new() }
    }
}

impl Macro {
    pub fn mask(&self) -> u32 {
        self.chord.iter().fold(0, |a, d| a | d.bit())
    }

    pub fn total_ms(&self) -> u64 {
        self.steps.iter().map(|s| s.ms() as u64).sum()
    }
}

/// A button that fires repeatedly while it is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turbo {
    pub btn: XBtn,
    pub hz: u8,
}

pub fn sanitize_macros(list: &mut Vec<Macro>) {
    list.truncate(MAX_MACROS);
    for m in list.iter_mut() {
        m.name = m.name.chars().take(40).collect();
        m.steps.truncate(MAX_STEPS);
        let mut chord: Vec<DIn> = Vec::new();
        for d in m.chord.drain(..) {
            if !chord.contains(&d) && chord.len() < MAX_CHORD {
                chord.push(d);
            }
        }
        m.chord = chord;
        for s in m.steps.iter_mut() {
            let fix = |ms: &mut u32| *ms = (*ms).clamp(1, MAX_STEP_MS);
            match s {
                Step::Hold(_, ms) | Step::Wait(ms) => fix(ms),
                _ => {}
            }
        }
    }
}

pub fn sanitize_turbo(list: &mut Vec<Turbo>) {
    let mut seen: Vec<Turbo> = Vec::new();
    for mut t in list.drain(..) {
        t.hz = t.hz.clamp(TURBO_MIN_HZ, TURBO_MAX_HZ);
        seen.retain(|o| o.btn != t.btn);
        seen.push(t);
    }
    *list = seen;
}

/// What the macros hold down on the virtual pad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Held {
    buttons: u16,
    lt: bool,
    rt: bool,
}

impl Held {
    fn set(&mut self, o: XOut, on: bool) {
        match o {
            XOut::Btn(b) => {
                if on {
                    self.buttons |= b.mask();
                } else {
                    self.buttons &= !b.mask();
                }
            }
            XOut::Lt => self.lt = on,
            XOut::Rt => self.rt = on,
        }
    }

    fn merge(&mut self, o: Held) {
        self.buttons |= o.buttons;
        self.lt |= o.lt;
        self.rt |= o.rt;
    }
}

/// One macro's run state.
#[derive(Clone, Debug, Default)]
struct Run {
    active: bool,
    step: usize,
    /// When the current timed step began (microseconds on the engine clock).
    t0: u64,
    /// The current step is a started `Hold`.
    timing: bool,
    held: Held,
    prev_down: bool,
}

/// A late report (the pad stalled) must not make a macro rush through its steps to catch up.
const CATCH_UP_LIMIT_US: u64 = 100_000;

impl Run {
    fn start(&mut self, now: u64) {
        *self = Run { active: true, t0: now, prev_down: self.prev_down, ..Run::default() };
    }

    fn stop(&mut self) {
        *self = Run { prev_down: self.prev_down, ..Run::default() };
    }

    fn update(&mut self, m: &Macro, down: bool, now: u64) {
        let edge = down && !self.prev_down;
        self.prev_down = down;
        match m.mode {
            MacroMode::Once => {
                if edge && !self.active {
                    self.start(now);
                }
            }
            MacroMode::WhileHeld => {
                if edge {
                    self.start(now);
                }
                if !down && self.active {
                    self.stop();
                }
            }
            MacroMode::Toggle => {
                if edge {
                    if self.active {
                        self.stop();
                    } else {
                        self.start(now);
                    }
                }
            }
        }
        if self.active {
            self.advance(m, now);
        }
    }

    /// A timed step is done when its time is up; the next one starts where this one ended (not when the report
    /// arrived), so the timeline does not drift.
    fn finish_timer(&mut self, now: u64, ms: u32) -> bool {
        let end = self.t0 + ms as u64 * 1000;
        if now < end {
            return false;
        }
        self.t0 = if now - end > CATCH_UP_LIMIT_US { now } else { end };
        self.timing = false;
        self.step += 1;
        true
    }

    fn advance(&mut self, m: &Macro, now: u64) {
        let n = m.steps.len();
        let mut budget = n * 2 + 2;
        while budget > 0 {
            budget -= 1;
            if self.step >= n {
                if m.mode == MacroMode::Once {
                    self.stop();
                    return;
                }
                self.step = 0;
                if m.total_ms() == 0 {
                    // Nothing to wait for: one pass per report, never a spin.
                    return;
                }
                continue;
            }
            match m.steps[self.step] {
                Step::Press(o) => {
                    self.held.set(o, true);
                    self.step += 1;
                }
                Step::Release(o) => {
                    self.held.set(o, false);
                    self.step += 1;
                }
                Step::Hold(o, ms) => {
                    if !self.timing {
                        self.held.set(o, true);
                        self.timing = true;
                    }
                    if self.finish_timer(now, ms) {
                        self.held.set(o, false);
                    } else {
                        return;
                    }
                }
                Step::Wait(ms) => {
                    if !self.finish_timer(now, ms) {
                        return;
                    }
                }
            }
        }
    }
}

/// The state of one virtual pad's macros and turbo.
#[derive(Default)]
pub struct Engine {
    cfg: Option<Arc<EmuConfig>>,
    runs: Vec<Run>,
    turbo_since: [Option<u64>; 15],
}

fn btn_index(b: XBtn) -> usize {
    XBtn::ALL.iter().position(|x| *x == b).unwrap_or(0)
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// New settings: whatever was running stops (a changed macro must not keep a stale step).
    fn sync(&mut self, cfg: &Arc<EmuConfig>) {
        if self.cfg.as_ref().is_some_and(|c| Arc::ptr_eq(c, cfg)) {
            return;
        }
        self.cfg = Some(cfg.clone());
        self.runs = vec![Run::default(); cfg.macros.len()];
        self.turbo_since = [None; 15];
    }

    /// One report in, one virtual pad state out. `now_us` is a monotonic clock in microseconds.
    pub fn process(&mut self, now_us: u64, pad: &PadState, cfg: &Arc<EmuConfig>) -> XboxState {
        self.sync(cfg);
        let ins = remap::inputs(pad, &cfg.remap);
        let mut suppress = 0u32;
        let mut held = Held::default();
        for (m, run) in cfg.macros.iter().zip(self.runs.iter_mut()) {
            let mask = m.mask();
            if !m.enabled || mask == 0 || m.steps.is_empty() {
                run.stop();
                run.prev_down = false;
                continue;
            }
            let down = ins & mask == mask;
            run.update(m, down, now_us);
            if m.swallow && down {
                suppress |= mask;
            }
            held.merge(run.held);
        }
        let mut x = super::map::map_with(pad, cfg, ins, suppress);
        self.turbo(&mut x, &cfg.turbo, now_us);
        x.buttons |= held.buttons;
        if held.lt {
            x.lt = 255;
        }
        if held.rt {
            x.rt = 255;
        }
        x
    }

    fn turbo(&mut self, x: &mut XboxState, list: &[Turbo], now: u64) {
        let before = x.buttons;
        for (i, b) in XBtn::ALL.iter().enumerate() {
            if before & b.mask() == 0 {
                self.turbo_since[i] = None;
            }
        }
        for t in list {
            if before & t.btn.mask() == 0 {
                continue;
            }
            let since = *self.turbo_since[btn_index(t.btn)].get_or_insert(now);
            let period = 1_000_000 / t.hz.clamp(TURBO_MIN_HZ, TURBO_MAX_HZ) as u64;
            if (now - since) % period >= period / 2 {
                x.buttons &= !t.btn.mask();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::Btn;

    const MS: u64 = 1000;

    /// A pad with the given buttons down.
    fn pad(btns: &[Btn]) -> PadState {
        let mut s = PadState::default();
        for b in btns {
            s.set(*b, true);
        }
        s
    }

    fn cfg(m: Vec<Macro>, t: Vec<Turbo>) -> Arc<EmuConfig> {
        let mut c = EmuConfig::default();
        c.macros = m;
        c.turbo = t;
        c.sanitize();
        Arc::new(c)
    }

    /// Reports every 4 ms from `from` to `to` (exclusive) with `p` held; returns the (time, buttons) changes.
    fn run(e: &mut Engine, c: &Arc<EmuConfig>, p: &PadState, from: u64, to: u64, log: &mut Vec<(u64, u16)>) {
        let mut t = from;
        while t < to {
            let x = e.process(t * MS, p, c);
            if log.last().map(|l| l.1) != Some(x.buttons) {
                log.push((t, x.buttons));
            }
            t += 4;
        }
    }

    fn a() -> XOut {
        XOut::Btn(XBtn::A)
    }

    #[test]
    fn sanitize_limits_everything() {
        let mut m = vec![Macro { name: "x".repeat(100), chord: vec![DIn::L1, DIn::L1, DIn::R1, DIn::Cross, DIn::Circle, DIn::Square], steps: vec![Step::Wait(0), Step::Hold(a(), 999_999)], ..Default::default() }; 40];
        sanitize_macros(&mut m);
        assert_eq!(m.len(), MAX_MACROS);
        assert_eq!(m[0].name.chars().count(), 40);
        assert_eq!(m[0].chord, vec![DIn::L1, DIn::R1, DIn::Cross, DIn::Circle]);
        assert_eq!(m[0].steps, vec![Step::Wait(1), Step::Hold(a(), MAX_STEP_MS)]);
        let mut t = vec![Turbo { btn: XBtn::A, hz: 1 }, Turbo { btn: XBtn::A, hz: 200 }, Turbo { btn: XBtn::B, hz: 12 }];
        sanitize_turbo(&mut t);
        assert_eq!(t, vec![Turbo { btn: XBtn::A, hz: TURBO_MAX_HZ }, Turbo { btn: XBtn::B, hz: 12 }]);
    }

    #[test]
    fn a_once_macro_runs_its_steps_on_time_and_does_not_restart_while_running() {
        let m = Macro { chord: vec![DIn::Square], steps: vec![Step::Hold(a(), 100), Step::Wait(50), Step::Hold(XOut::Btn(XBtn::B), 100)], swallow: true, ..Default::default() };
        let c = cfg(vec![m], vec![]);
        let mut e = Engine::new();
        let mut log = Vec::new();
        run(&mut e, &c, &pad(&[]), 0, 20, &mut log);
        // Square held for 600 ms (much longer than the macro), then released.
        run(&mut e, &c, &pad(&[Btn::Square]), 20, 620, &mut log);
        run(&mut e, &c, &pad(&[]), 620, 700, &mut log);
        // Square itself is swallowed (it would be X), the macro gives A at 20, nothing at 120, B at 170, off at 270.
        let a = XBtn::A.mask();
        let b = XBtn::B.mask();
        assert_eq!(log, vec![(0, 0), (20, a), (120, 0), (172, b), (272, 0)], "{log:?}");
    }

    #[test]
    fn a_swallowed_chord_does_not_press_its_own_buttons_but_an_open_one_does() {
        let mut m = Macro { chord: vec![DIn::Create, DIn::L1], steps: vec![Step::Wait(10)], ..Default::default() };
        let c = cfg(vec![m.clone()], vec![]);
        let mut e = Engine::new();
        let x = e.process(0, &pad(&[Btn::Create, Btn::L1]), &c);
        assert_eq!(x.buttons, 0, "Back and LB are held back while the chord runs the macro");
        // One of the two is not a chord.
        let x = e.process(4 * MS, &pad(&[Btn::L1]), &c);
        assert_eq!(x.buttons, XBtn::Lb.mask());
        m.swallow = false;
        let c = cfg(vec![m], vec![]);
        let mut e = Engine::new();
        let x = e.process(0, &pad(&[Btn::Create, Btn::L1]), &c);
        assert_eq!(x.buttons, XBtn::Back.mask() | XBtn::Lb.mask());
    }

    #[test]
    fn while_held_loops_and_stops_at_once_on_release() {
        let m = Macro { chord: vec![DIn::R1], mode: MacroMode::WhileHeld, swallow: true, steps: vec![Step::Hold(a(), 40), Step::Wait(40)], ..Default::default() };
        let c = cfg(vec![m], vec![]);
        let mut e = Engine::new();
        let mut log = Vec::new();
        run(&mut e, &c, &pad(&[Btn::R1]), 0, 200, &mut log);
        let am = XBtn::A.mask();
        // 80 ms period: on 0..40, off 40..80, on 80..120, ...
        assert_eq!(log, vec![(0, am), (40, 0), (80, am), (120, 0), (160, am)], "{log:?}");
        // Released while A is down: A goes up in the same report.
        run(&mut e, &c, &pad(&[]), 200, 204, &mut log);
        assert_eq!(log.last(), Some(&(200, 0)));
        run(&mut e, &c, &pad(&[]), 204, 400, &mut log);
        assert_eq!(log.last(), Some(&(200, 0)), "nothing more after the release");
    }

    #[test]
    fn toggle_starts_on_one_press_and_stops_on_the_next() {
        let m = Macro { chord: vec![DIn::Triangle], mode: MacroMode::Toggle, swallow: true, steps: vec![Step::Press(a()), Step::Wait(20), Step::Release(a()), Step::Wait(20)], ..Default::default() };
        let c = cfg(vec![m], vec![]);
        let mut e = Engine::new();
        let mut log = Vec::new();
        run(&mut e, &c, &pad(&[Btn::Triangle]), 0, 8, &mut log);
        run(&mut e, &c, &pad(&[]), 8, 100, &mut log);
        let on_before = log.iter().filter(|l| l.1 != 0).count();
        assert!(on_before >= 2, "keeps looping with the button up: {log:?}");
        // The second press stops it, and the stop is immediate.
        let mut tail = Vec::new();
        run(&mut e, &c, &pad(&[Btn::Triangle]), 100, 108, &mut tail);
        run(&mut e, &c, &pad(&[]), 108, 400, &mut tail);
        assert_eq!(tail.last().map(|l| l.1), Some(0));
        assert!(tail.iter().all(|l| l.0 < 108), "no change after the stop: {tail:?}");
    }

    #[test]
    fn a_zero_length_loop_does_not_spin_and_a_late_report_does_not_make_it_rush() {
        let m = Macro { chord: vec![DIn::R1], mode: MacroMode::WhileHeld, steps: vec![Step::Press(a()), Step::Release(a())], ..Default::default() };
        let c = cfg(vec![m], vec![]);
        let mut e = Engine::new();
        let x = e.process(0, &pad(&[Btn::R1]), &c); // returns at all
        assert_eq!(x.buttons, 0);
        // A timed macro, and then the pad goes quiet for 5 seconds: at most one step is done per report.
        let m = Macro { chord: vec![DIn::R1], mode: MacroMode::WhileHeld, steps: vec![Step::Hold(a(), 10), Step::Wait(10)], ..Default::default() };
        let c = cfg(vec![m], vec![]);
        let mut e = Engine::new();
        e.process(0, &pad(&[Btn::R1]), &c);
        let x = e.process(5_000 * MS, &pad(&[Btn::R1]), &c);
        let y = e.process(5_004 * MS, &pad(&[Btn::R1]), &c);
        assert!(x.buttons == 0 || x.buttons == XBtn::A.mask());
        // The timeline restarted at the late report instead of replaying 500 loops.
        assert!(y.buttons == XBtn::A.mask() || y.buttons == 0);
    }

    #[test]
    fn macro_outputs_include_triggers_and_never_touch_other_buttons() {
        let m = Macro { chord: vec![DIn::Mute], steps: vec![Step::Press(XOut::Lt), Step::Press(XOut::Rt), Step::Wait(30)], ..Default::default() };
        let c = cfg(vec![m], vec![]);
        let mut e = Engine::new();
        let x = e.process(0, &pad(&[Btn::Mute, Btn::Cross]), &c);
        assert_eq!((x.lt, x.rt), (255, 255));
        assert_eq!(x.buttons, XBtn::A.mask(), "the held Cross still reaches the game");
    }

    #[test]
    fn a_disabled_or_empty_macro_does_nothing_and_a_config_change_stops_a_running_one() {
        let mut m = Macro { chord: vec![DIn::Cross], steps: vec![Step::Hold(XOut::Btn(XBtn::Y), 100)], swallow: true, ..Default::default() };
        m.enabled = false;
        let c = cfg(vec![m.clone()], vec![]);
        let mut e = Engine::new();
        assert_eq!(e.process(0, &pad(&[Btn::Cross]), &c).buttons, XBtn::A.mask());
        m.enabled = true;
        let c = cfg(vec![m.clone()], vec![]);
        assert_eq!(e.process(4 * MS, &pad(&[Btn::Cross]), &c).buttons, XBtn::Y.mask(), "A is swallowed, the macro holds Y");
        let x = e.process(8 * MS, &pad(&[Btn::Cross]), &c);
        assert_eq!(x.buttons, XBtn::Y.mask());
        // The user edits the macro: it stops (a new settings object), nothing stays pressed.
        m.name = "renamed".into();
        let c2 = cfg(vec![m], vec![]);
        let x = e.process(12 * MS, &pad(&[]), &c2);
        assert_eq!(x.buttons, 0);
    }

    #[test]
    fn turbo_pulses_at_the_rate_from_the_press_and_stops_on_release() {
        let c = cfg(vec![], vec![Turbo { btn: XBtn::A, hz: 10 }]);
        let mut e = Engine::new();
        let mut log = Vec::new();
        run(&mut e, &c, &pad(&[Btn::Cross]), 100, 400, &mut log);
        let am = XBtn::A.mask();
        // 10 Hz: 100 ms period, 50 ms on, 50 ms off, starting on at the press (t = 100).
        assert_eq!(log[0], (100, am));
        let times: Vec<u64> = log.iter().map(|l| l.0).collect();
        assert_eq!(&times[..5], &[100, 152, 200, 252, 300], "{log:?}");
        assert_eq!(log[1].1, 0);
        // Released: off, and a new press starts "on" again.
        run(&mut e, &c, &pad(&[]), 400, 420, &mut log);
        assert_eq!(log.last().map(|l| l.1), Some(0));
        let mut log2 = Vec::new();
        run(&mut e, &c, &pad(&[Btn::Cross]), 431, 440, &mut log2);
        assert_eq!(log2[0].1, am);
    }

    #[test]
    fn turbo_only_touches_its_own_button_and_follows_the_remap() {
        let mut c = EmuConfig::default();
        c.turbo = vec![Turbo { btn: XBtn::B, hz: 30 }];
        // Cross is remapped to B: turbo works on the output, so Cross pulses B.
        c.set_target(DIn::Cross, remap::Out::Btn(XBtn::B));
        let c = Arc::new(c);
        let mut e = Engine::new();
        let mut log = Vec::new();
        run(&mut e, &c, &pad(&[Btn::Cross, Btn::Square]), 0, 100, &mut log);
        assert!(log.iter().all(|l| l.1 & XBtn::X.mask() != 0), "X is not turbo and always on: {log:?}");
        assert!(log.iter().any(|l| l.1 & XBtn::B.mask() != 0) && log.iter().any(|l| l.1 & XBtn::B.mask() == 0));
    }
}
