//! Hiding the real pad from games while the emulation is on, and always giving it back.
//!
//! Windows uses HidHide (its command-line tool); Linux grabs the pad's evdev nodes. The part worth testing is
//! the **state machine**, which does not care about the system: [`HidHideHider`] works through a [`HideTool`]
//! (the CLI, or a mock) and keeps a state file next to the settings:
//!
//! - the file is written **before** the first change to HidHide, so a crash at any moment leaves a record;
//! - `restore` undoes exactly what Padyx did: it unhides only the devices Padyx hid, removes Padyx from the
//!   allow list only if Padyx added it, and puts the global cloak switch back to what it was;
//! - `recover` (called on every start of the background part) finishes an interrupted run: if the file is
//!   there, the previous run did not clean up, so it is restored now;
//! - a failing step keeps the file, so the next try (or the "Restore pad visibility" button) can repeat it.
//!
//! The HidHide part is Windows only: on Linux the same code compiles and its tests run, but only the grab
//! hider is used there.
//!
//! Checked against the HidHide sources (HidHideCLI): the flags are `--cloak-on/off/state`, `--dev-hide/unhide/list`,
//! `--app-reg/unreg/list`; state and lists are printed as the commands that would set them; the control device is
//! open to ordinary users (SDDL world-RWX) and the CLI runs unelevated; the driver matches the hidden list against the
//! instance path of the HID device being opened, which is what `instance_path` builds from the hidapi path.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What the emulator asks of a hider for one pad.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HideTarget {
    pub uid: String,
    /// The Bluetooth address / serial, as the system reports it (Linux matches the evdev nodes by it).
    pub serial: String,
    /// The HID device path (Windows): `\\?\hid#vid_054c&pid_0ce6#...`.
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HideStatus {
    #[default]
    Ready,
    /// HidHide is not installed (Windows).
    ToolMissing,
    /// The tool refused: it needs administrator rights.
    NeedsAdmin,
    /// Nothing can hide a pad here.
    Unsupported,
}

pub trait Hider: Send {
    fn status(&mut self) -> HideStatus;
    fn hide(&mut self, t: &HideTarget) -> Result<(), String>;
    /// Gives one pad back (the others stay hidden).
    fn unhide(&mut self, uid: &str) -> Result<(), String>;
    /// Gives everything back. Safe to call at any time, any number of times.
    fn restore(&mut self) -> Result<(), String>;
    /// Called once at start: undoes what a previous run left behind.
    fn recover(&mut self) -> Result<(), String>;
    #[allow(dead_code)] // the emulator keeps its own flag; tests and the Linux hider use this one
    fn is_hidden(&self, uid: &str) -> bool;
}

/// A hider that does nothing (used when there is nothing to hide with, and in tests).
#[derive(Default)]
#[allow(dead_code)]
pub struct NullHider {
    pub hidden: Vec<String>,
}

impl Hider for NullHider {
    fn status(&mut self) -> HideStatus {
        HideStatus::Unsupported
    }
    fn unhide(&mut self, uid: &str) -> Result<(), String> {
        self.hidden.retain(|h| h != uid);
        Ok(())
    }
    fn hide(&mut self, _t: &HideTarget) -> Result<(), String> {
        Ok(())
    }
    fn restore(&mut self) -> Result<(), String> {
        self.hidden.clear();
        Ok(())
    }
    fn recover(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn is_hidden(&self, uid: &str) -> bool {
        self.hidden.iter().any(|h| h == uid)
    }
}

/// A hider that pretends to work, for screenshots with the demo pad (no system is touched).
#[derive(Default)]
pub struct SimHider {
    hidden: Vec<String>,
}

impl Hider for SimHider {
    fn status(&mut self) -> HideStatus {
        HideStatus::Ready
    }
    fn unhide(&mut self, uid: &str) -> Result<(), String> {
        self.hidden.retain(|h| h != uid);
        Ok(())
    }
    fn hide(&mut self, t: &HideTarget) -> Result<(), String> {
        if !self.hidden.contains(&t.uid) {
            self.hidden.push(t.uid.clone());
        }
        Ok(())
    }
    fn restore(&mut self) -> Result<(), String> {
        self.hidden.clear();
        Ok(())
    }
    fn recover(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn is_hidden(&self, uid: &str) -> bool {
        self.hidden.iter().any(|h| h == uid)
    }
}

// ───────────────────────── HidHide state machine ─────────────────────────

/// The few HidHide operations the state machine needs. Every one is a single command of the CLI.
pub trait HideTool: Send {
    fn available(&mut self) -> HideStatus;
    fn cloak_state(&mut self) -> Result<bool, String>;
    fn set_cloak(&mut self, on: bool) -> Result<(), String>;
    /// The applications on the allow list.
    fn apps(&mut self) -> Result<Vec<String>, String>;
    fn reg_app(&mut self, path: &str) -> Result<(), String>;
    fn unreg_app(&mut self, path: &str) -> Result<(), String>;
    /// The devices that are hidden right now.
    fn devices(&mut self) -> Result<Vec<String>, String>;
    fn hide_dev(&mut self, instance: &str) -> Result<(), String>;
    fn unhide_dev(&mut self, instance: &str) -> Result<(), String>;
}

/// What Padyx changed in HidHide, kept on disk.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HideState {
    /// Device instance paths Padyx hid (not the ones the user had hidden already).
    pub devices: Vec<String>,
    /// Was the global cloak switch on before Padyx touched it?
    pub cloak_before: bool,
    /// Padyx's own exe on the allow list, and whether Padyx put it there.
    pub app: String,
    pub app_added: bool,
}

pub const STATE_FILE: &str = "hide-state.json";

pub struct HidHideHider<T: HideTool> {
    tool: T,
    file: PathBuf,
    exe: String,
    state: Option<HideState>,
    /// uid of a pad to its instance path, for `is_hidden`.
    uids: Vec<(String, String)>,
}

impl<T: HideTool> HidHideHider<T> {
    pub fn new(tool: T, data_dir: &Path, exe: &str) -> Self {
        Self { tool, file: data_dir.join(STATE_FILE), exe: exe.to_string(), state: None, uids: Vec::new() }
    }

    fn save(&self, s: &HideState) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
        crate::config::write_atomic(&self.file, &json).map_err(|e| format!("could not write {}: {e}", self.file.display()))
    }

    fn load(&self) -> Option<HideState> {
        let text = std::fs::read_to_string(&self.file).ok()?;
        serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
    }

    /// Does the state file say a run is still hidden?
    pub fn pending(&self) -> bool {
        self.file.exists()
    }

    fn same(a: &str, b: &str) -> bool {
        a.eq_ignore_ascii_case(b)
    }
}

impl<T: HideTool> Hider for HidHideHider<T> {
    fn status(&mut self) -> HideStatus {
        self.tool.available()
    }

    fn hide(&mut self, t: &HideTarget) -> Result<(), String> {
        let st = self.tool.available();
        if st != HideStatus::Ready {
            return Err(match st {
                HideStatus::ToolMissing => "HidHide is not installed".to_string(),
                HideStatus::NeedsAdmin => "HidHide needs administrator rights".to_string(),
                _ => "HidHide is not available".to_string(),
            });
        }
        let inst = instance_path(&t.path).ok_or_else(|| format!("cannot work out the device id from {}", t.path))?;
        if self.state.as_ref().is_some_and(|s| s.devices.iter().any(|d| Self::same(d, &inst))) {
            if !self.uids.iter().any(|(u, _)| *u == t.uid) {
                self.uids.push((t.uid.clone(), inst));
            }
            return Ok(());
        }
        let already_hidden = self.tool.devices()?.iter().any(|d| Self::same(d, &inst));
        let mut state = match self.state.clone() {
            Some(s) => s,
            None => {
                let cloak_before = self.tool.cloak_state()?;
                let listed = self.tool.apps()?.iter().any(|a| Self::same(a, &self.exe));
                HideState { devices: Vec::new(), cloak_before, app: self.exe.clone(), app_added: !listed }
            }
        };
        let first = self.state.is_none();
        if !already_hidden {
            state.devices.push(inst.clone());
        }
        // Write-ahead: the file exists before HidHide changes, so a crash from here on is recoverable.
        self.save(&state)?;
        self.state = Some(state.clone());
        let result = (|| {
            if first && state.app_added {
                self.tool.reg_app(&self.exe)?;
            }
            if !already_hidden {
                self.tool.hide_dev(&inst)?;
            }
            self.tool.set_cloak(true)
        })();
        match result {
            Ok(()) => {
                self.uids.push((t.uid.clone(), inst));
                Ok(())
            }
            Err(e) => {
                let _ = self.restore();
                Err(e)
            }
        }
    }

    fn unhide(&mut self, uid: &str) -> Result<(), String> {
        let Some(i) = self.uids.iter().position(|(u, _)| u == uid) else { return Ok(()) };
        let (_, inst) = self.uids.remove(i);
        let Some(mut state) = self.state.clone() else { return Ok(()) };
        if self.uids.is_empty() {
            // The last pad: undo everything, including the allow list and the cloak switch.
            return self.restore();
        }
        if let Some(j) = state.devices.iter().position(|d| Self::same(d, &inst)) {
            self.tool.unhide_dev(&inst)?;
            state.devices.remove(j);
            self.save(&state)?;
            self.state = Some(state);
        }
        Ok(())
    }

    fn restore(&mut self) -> Result<(), String> {
        let Some(state) = self.state.clone().or_else(|| self.load()) else {
            self.uids.clear();
            return Ok(());
        };
        let mut errors: Vec<String> = Vec::new();
        for d in &state.devices {
            if let Err(e) = self.tool.unhide_dev(d) {
                errors.push(e);
            }
        }
        if state.app_added {
            if let Err(e) = self.tool.unreg_app(&state.app) {
                errors.push(e);
            }
        }
        if !state.cloak_before {
            if let Err(e) = self.tool.set_cloak(false) {
                errors.push(e);
            }
        }
        if errors.is_empty() {
            let _ = std::fs::remove_file(&self.file);
            self.state = None;
            self.uids.clear();
            Ok(())
        } else {
            // Keep the record: the next try repeats every step (unhiding twice is harmless).
            self.state = Some(state);
            Err(errors.join("; "))
        }
    }

    fn recover(&mut self) -> Result<(), String> {
        if self.pending() {
            self.state = None;
            self.restore()
        } else {
            Ok(())
        }
    }

    fn is_hidden(&self, uid: &str) -> bool {
        self.uids.iter().any(|(u, _)| u == uid)
    }
}

/// `\\?\hid#vid_054c&pid_0ce6#7&2e1c9d2f&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}` to the device instance
/// path HidHide wants: `HID\VID_054C&PID_0CE6\7&2E1C9D2F&0&0000`.
pub fn instance_path(hid_path: &str) -> Option<String> {
    let p = hid_path.strip_prefix(r"\\?\").or_else(|| hid_path.strip_prefix(r"\\.\")).unwrap_or(hid_path);
    let p = match p.rfind("#{") {
        Some(i) => &p[..i],
        None => p,
    };
    let parts: Vec<&str> = p.split('#').collect();
    if parts.len() != 3 || parts.iter().any(|s| s.is_empty()) {
        return None;
    }
    Some(parts.join("\\").to_uppercase())
}

/// The output of `--dev-list` and `--app-list`. HidHideCLI prints each entry as the command that would add it
/// (read from its sources: `--dev-hide "HID\..."`, `--app-reg "C:\...\x.exe"`). A bare quoted line or a JSON
/// array of strings is accepted too.
pub fn parse_path_list(out: &str) -> Vec<String> {
    let t = out.trim().trim_start_matches('\u{feff}');
    if let Ok(v) = serde_json::from_str::<Vec<String>>(t) {
        return v;
    }
    t.lines()
        .map(|l| {
            let mut l = l.trim();
            if l.starts_with("--") {
                l = l.split_once(char::is_whitespace).map(|(_, rest)| rest).unwrap_or("").trim();
            }
            l.trim_matches(|c| c == '"' || c == ',').to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// `true`/`on`/`1` and `false`/`off`/`0` in the words the CLI uses.
pub fn parse_bool(out: &str) -> Option<bool> {
    let l = out.trim().to_lowercase();
    let w: Vec<&str> = l.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    if w.iter().any(|s| matches!(*s, "false" | "off" | "0" | "disabled" | "inactive")) {
        Some(false)
    } else if w.iter().any(|s| matches!(*s, "true" | "on" | "1" | "enabled" | "active")) {
        Some(true)
    } else {
        None
    }
}

// ───────────────────────── Windows: the HidHide command line ─────────────────────────

#[cfg(windows)]
pub mod cli {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Where the installer puts the command-line tool.
    pub fn find() -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(pf) = std::env::var_os(var) {
                dirs.push(PathBuf::from(pf).join("Nefarius Software Solutions").join("HidHide").join("x64"));
            }
        }
        dirs.into_iter().map(|d| d.join("HidHideCLI.exe")).find(|p| p.is_file())
    }

    pub struct HidHideCli {
        pub path: Option<PathBuf>,
    }

    impl HidHideCli {
        pub fn new() -> Self {
            Self { path: find() }
        }

        fn run(&mut self, args: &[&str]) -> Result<String, String> {
            let exe = self.path.clone().or_else(find).ok_or_else(|| "HidHide is not installed".to_string())?;
            let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            if out.status.success() {
                return Ok(text);
            }
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            let all = format!("{text}{err}");
            if all.to_lowercase().contains("access is denied") || out.status.code() == Some(5) {
                return Err("HidHide needs administrator rights".into());
            }
            Err(format!("HidHideCLI {} failed: {}", args.first().copied().unwrap_or(""), all.trim()))
        }
    }

    impl HideTool for HidHideCli {
        fn available(&mut self) -> HideStatus {
            if self.path.is_none() {
                self.path = find();
            }
            if self.path.is_none() {
                return HideStatus::ToolMissing;
            }
            match self.run(&["--cloak-state"]) {
                Ok(_) => HideStatus::Ready,
                Err(e) if e.contains("administrator") => HideStatus::NeedsAdmin,
                Err(_) => HideStatus::ToolMissing,
            }
        }
        fn cloak_state(&mut self) -> Result<bool, String> {
            let o = self.run(&["--cloak-state"])?;
            parse_bool(&o).ok_or_else(|| format!("unexpected answer to --cloak-state: {}", o.trim()))
        }
        fn set_cloak(&mut self, on: bool) -> Result<(), String> {
            self.run(&[if on { "--cloak-on" } else { "--cloak-off" }]).map(|_| ())
        }
        fn apps(&mut self) -> Result<Vec<String>, String> {
            self.run(&["--app-list"]).map(|o| parse_path_list(&o))
        }
        fn reg_app(&mut self, path: &str) -> Result<(), String> {
            self.run(&["--app-reg", path]).map(|_| ())
        }
        fn unreg_app(&mut self, path: &str) -> Result<(), String> {
            self.run(&["--app-unreg", path]).map(|_| ())
        }
        fn devices(&mut self) -> Result<Vec<String>, String> {
            self.run(&["--dev-list"]).map(|o| parse_path_list(&o))
        }
        fn hide_dev(&mut self, instance: &str) -> Result<(), String> {
            self.run(&["--dev-hide", instance]).map(|_| ())
        }
        fn unhide_dev(&mut self, instance: &str) -> Result<(), String> {
            self.run(&["--dev-unhide", instance]).map(|_| ())
        }
    }
}

// ───────────────────────── Linux: grab the evdev nodes ─────────────────────────

/// The evdev nodes of a Sony pad are grabbed (`EVIOCGRAB`) while the emulation is on, so no other program gets
/// its events. The kernel releases a grab when the process ends, so a crash cannot strand the pad. The hidraw
/// node cannot be hidden this way: a program that reads the DualSense through hidraw (Steam does) still sees
/// it, and for those games the per-pad "native mode" switch is the answer.
#[cfg(target_os = "linux")]
pub mod grab {
    use super::*;

    pub struct GrabHider {
        grabbed: Vec<(String, Vec<evdev::Device>)>,
    }

    impl GrabHider {
        pub fn new() -> Self {
            Self { grabbed: Vec::new() }
        }
    }

    impl Hider for GrabHider {
        fn status(&mut self) -> HideStatus {
            HideStatus::Ready
        }

        fn unhide(&mut self, uid: &str) -> Result<(), String> {
            if let Some(i) = self.grabbed.iter().position(|(u, _)| u == uid) {
                for d in self.grabbed[i].1.iter_mut() {
                    let _ = d.ungrab();
                }
                self.grabbed.remove(i);
            }
            Ok(())
        }

        fn hide(&mut self, t: &HideTarget) -> Result<(), String> {
            if self.is_hidden(&t.uid) {
                return Ok(());
            }
            if t.serial.is_empty() {
                return Err("the pad has no serial number to find its input devices by".into());
            }
            let want = t.serial.to_lowercase();
            let mut devs = Vec::new();
            for (_, mut d) in evdev::enumerate() {
                let id = d.input_id();
                let ours = id.vendor() == crate::pad::SONY_VID && d.unique_name().is_some_and(|u| u.to_lowercase() == want);
                if ours && d.grab().is_ok() {
                    devs.push(d);
                }
            }
            if devs.is_empty() {
                return Err("no input device of the pad could be grabbed (is the udev rule installed?)".into());
            }
            self.grabbed.push((t.uid.clone(), devs));
            Ok(())
        }

        fn restore(&mut self) -> Result<(), String> {
            for (_, devs) in self.grabbed.iter_mut() {
                for d in devs.iter_mut() {
                    let _ = d.ungrab();
                }
            }
            self.grabbed.clear();
            Ok(())
        }

        fn recover(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn is_hidden(&self, uid: &str) -> bool {
            self.grabbed.iter().any(|(u, _)| u == uid)
        }
    }
}

/// The hider for this system.
pub fn system_hider(data_dir: &Path) -> Box<dyn Hider> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        Box::new(HidHideHider::new(cli::HidHideCli::new(), data_dir, &exe))
    }
    #[cfg(target_os = "linux")]
    {
        let _ = data_dir;
        Box::new(grab::GrabHider::new())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = data_dir;
        Box::new(NullHider::default())
    }
}

/// Restores from the state file without a running emulator (the window, when the background part is not there).
#[cfg(windows)]
pub fn restore_from_file(data_dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let mut h = HidHideHider::new(cli::HidHideCli::new(), data_dir, &exe);
    h.recover()
}

#[cfg(not(windows))]
pub fn restore_from_file(_data_dir: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A fake HidHide: the same operations, a shared state the tests can look at, optional failures.
    #[derive(Default, Clone)]
    pub struct FakeHidHide {
        pub inner: Arc<Mutex<Fake>>,
    }

    #[derive(Default)]
    pub struct Fake {
        pub missing: bool,
        pub cloak: bool,
        pub apps: Vec<String>,
        pub hidden: Vec<String>,
        pub calls: Vec<String>,
        /// Fail the call with this name once it happens `n` more times.
        pub fail: Option<(String, u32)>,
    }

    impl Fake {
        fn step(&mut self, name: &str) -> Result<(), String> {
            self.calls.push(name.to_string());
            if let Some((n, left)) = self.fail.as_mut() {
                if n == name || name.starts_with(n.as_str()) {
                    if *left == 0 {
                        self.fail = None;
                        return Err(format!("{name} failed"));
                    }
                    *left -= 1;
                }
            }
            Ok(())
        }
    }

    impl HideTool for FakeHidHide {
        fn available(&mut self) -> HideStatus {
            if self.inner.lock().unwrap().missing {
                HideStatus::ToolMissing
            } else {
                HideStatus::Ready
            }
        }
        fn cloak_state(&mut self) -> Result<bool, String> {
            let mut g = self.inner.lock().unwrap();
            g.step("cloak_state")?;
            Ok(g.cloak)
        }
        fn set_cloak(&mut self, on: bool) -> Result<(), String> {
            let mut g = self.inner.lock().unwrap();
            g.step(if on { "cloak_on" } else { "cloak_off" })?;
            g.cloak = on;
            Ok(())
        }
        fn apps(&mut self) -> Result<Vec<String>, String> {
            let g = self.inner.lock().unwrap();
            Ok(g.apps.clone())
        }
        fn reg_app(&mut self, path: &str) -> Result<(), String> {
            let mut g = self.inner.lock().unwrap();
            g.step("reg_app")?;
            g.apps.push(path.to_string());
            Ok(())
        }
        fn unreg_app(&mut self, path: &str) -> Result<(), String> {
            let mut g = self.inner.lock().unwrap();
            g.step("unreg_app")?;
            g.apps.retain(|a| !a.eq_ignore_ascii_case(path));
            Ok(())
        }
        fn devices(&mut self) -> Result<Vec<String>, String> {
            Ok(self.inner.lock().unwrap().hidden.clone())
        }
        fn hide_dev(&mut self, i: &str) -> Result<(), String> {
            let mut g = self.inner.lock().unwrap();
            g.step("hide_dev")?;
            g.hidden.push(i.to_string());
            Ok(())
        }
        fn unhide_dev(&mut self, i: &str) -> Result<(), String> {
            let mut g = self.inner.lock().unwrap();
            g.step("unhide_dev")?;
            g.hidden.retain(|h| !h.eq_ignore_ascii_case(i));
            Ok(())
        }
    }

    const PAD1: &str = r"\\?\hid#vid_054c&pid_0ce6#7&2e1c9d2f&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
    const INST1: &str = r"HID\VID_054C&PID_0CE6\7&2E1C9D2F&0&0000";
    const EXE: &str = r"C:\Tools\Padyx.exe";

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("padyx-hide-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn target(uid: &str, path: &str) -> HideTarget {
        HideTarget { uid: uid.into(), serial: "aa:bb".into(), path: path.into() }
    }

    #[test]
    fn instance_path_from_usb_and_bluetooth_paths() {
        assert_eq!(instance_path(PAD1).as_deref(), Some(INST1));
        let bt = r"\\?\hid#{00001124-0000-1000-8000-00805f9b34fb}_vid&0002054c_pid&0ce6#8&a1b2c3d&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(instance_path(bt).as_deref(), Some(r"HID\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&0CE6\8&A1B2C3D&0&0000"));
        assert_eq!(instance_path("garbage"), None);
        assert_eq!(instance_path(r"\\?\hid#only#"), None);
        assert_eq!(instance_path(""), None);
    }

    #[test]
    fn cli_output_parsers() {
        // The real CLI answers with the command that would set that state, and lists entries the same way.
        assert_eq!(parse_bool("--cloak-on\r\n"), Some(true));
        assert_eq!(parse_bool("--cloak-off\r\n"), Some(false));
        assert_eq!(parse_path_list("--dev-hide \"HID\\A&1\"\r\n--dev-hide \"HID\\B\"\r\n"), vec!["HID\\A&1".to_string(), "HID\\B".to_string()]);
        assert_eq!(parse_path_list("--app-reg \"C:\\Tools\\Padyx.exe\"\r\n"), vec!["C:\\Tools\\Padyx.exe".to_string()]);
        assert_eq!(parse_bool("true\r\n"), Some(true));
        assert_eq!(parse_bool("Cloak: OFF"), Some(false));
        assert_eq!(parse_bool("nonsense"), None);
        assert_eq!(parse_path_list("[\"HID\\\\A\",\"HID\\\\B\"]"), vec!["HID\\A".to_string(), "HID\\B".to_string()]);
        assert_eq!(parse_path_list("\"HID\\A\",\r\n\"HID\\B\"\r\n"), vec!["HID\\A".to_string(), "HID\\B".to_string()]);
        assert!(parse_path_list("  \r\n").is_empty());
    }

    #[test]
    fn hide_then_restore_puts_everything_back() {
        let dir = tmp("basic");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        let mut h = HidHideHider::new(tool, &dir, EXE);
        h.hide(&target("u1", PAD1)).unwrap();
        {
            let f = fake.lock().unwrap();
            assert_eq!(f.hidden, vec![INST1.to_string()]);
            assert!(f.cloak, "the cloak is on while hidden");
            assert_eq!(f.apps, vec![EXE.to_string()], "Padyx is on the allow list");
        }
        assert!(h.is_hidden("u1") && h.pending());
        h.restore().unwrap();
        let f = fake.lock().unwrap();
        assert!(f.hidden.is_empty() && !f.cloak && f.apps.is_empty(), "back to how it was");
        drop(f);
        assert!(!h.pending() && !h.is_hidden("u1"));
        // Restoring again is a no-op.
        h.restore().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_the_user_had_before_is_left_alone() {
        let dir = tmp("user");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        {
            let mut f = fake.lock().unwrap();
            f.cloak = true;
            f.apps = vec![EXE.to_lowercase(), r"C:\Other\tool.exe".into()];
            f.hidden = vec![r"HID\VID_1234&PID_0001\1&1&0&0".into()];
        }
        let mut h = HidHideHider::new(tool, &dir, EXE);
        h.hide(&target("u1", PAD1)).unwrap();
        assert_eq!(fake.lock().unwrap().apps.len(), 2, "already allowed: not added twice");
        h.restore().unwrap();
        let f = fake.lock().unwrap();
        assert!(f.cloak, "the cloak was on before, so it stays on");
        assert_eq!(f.apps.len(), 2, "the allow list is untouched");
        assert_eq!(f.hidden, vec![r"HID\VID_1234&PID_0001\1&1&0&0".to_string()], "only our device was unhidden");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_device_the_user_already_hid_is_not_unhidden_by_us() {
        let dir = tmp("userdev");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        fake.lock().unwrap().hidden = vec![INST1.to_lowercase()];
        let mut h = HidHideHider::new(tool, &dir, EXE);
        h.hide(&target("u1", PAD1)).unwrap();
        h.restore().unwrap();
        assert_eq!(fake.lock().unwrap().hidden, vec![INST1.to_lowercase()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn crash_then_restart_restores_from_the_state_file() {
        let dir = tmp("crash");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        {
            let mut h = HidHideHider::new(tool, &dir, EXE);
            h.hide(&target("u1", PAD1)).unwrap();
            // The process dies here: no restore, the hider is just gone.
        }
        assert!(dir.join(STATE_FILE).exists(), "the record survived the crash");
        assert!(!fake.lock().unwrap().hidden.is_empty(), "and the pad is still hidden");
        // A new run starts with a new hider over the same HidHide and the same folder.
        let mut h2 = HidHideHider::new(FakeHidHide { inner: fake.clone() }, &dir, EXE);
        assert!(h2.pending());
        h2.recover().unwrap();
        let f = fake.lock().unwrap();
        assert!(f.hidden.is_empty() && !f.cloak && f.apps.is_empty());
        drop(f);
        assert!(!dir.join(STATE_FILE).exists());
        // With nothing pending, recover does nothing.
        let before = fake.lock().unwrap().calls.len();
        h2.recover().unwrap();
        assert_eq!(fake.lock().unwrap().calls.len(), before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_crash_in_the_middle_of_hiding_is_recoverable_too() {
        let dir = tmp("mid");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        // The record is written first. Simulate dying after the record and the allow-list entry, before the hide.
        let st = HideState { devices: vec![INST1.into()], cloak_before: false, app: EXE.into(), app_added: true };
        std::fs::write(dir.join(STATE_FILE), serde_json::to_string(&st).unwrap()).unwrap();
        fake.lock().unwrap().apps.push(EXE.into());
        let mut h = HidHideHider::new(tool, &dir, EXE);
        h.recover().unwrap();
        let f = fake.lock().unwrap();
        assert!(f.apps.is_empty() && f.hidden.is_empty() && !f.cloak);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_hide_rolls_back_and_leaves_nothing_stranded() {
        let dir = tmp("fail");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        fake.lock().unwrap().fail = Some(("cloak_on".into(), 0));
        let mut h = HidHideHider::new(tool, &dir, EXE);
        let err = h.hide(&target("u1", PAD1)).unwrap_err();
        assert!(err.contains("cloak_on"));
        let f = fake.lock().unwrap();
        assert!(f.hidden.is_empty() && !f.cloak && f.apps.is_empty(), "rolled back");
        drop(f);
        assert!(!dir.join(STATE_FILE).exists() && !h.is_hidden("u1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failing_restore_keeps_the_record_so_it_can_be_repeated() {
        let dir = tmp("keep");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        let mut h = HidHideHider::new(tool, &dir, EXE);
        h.hide(&target("u1", PAD1)).unwrap();
        fake.lock().unwrap().fail = Some(("unhide_dev".into(), 0));
        assert!(h.restore().is_err());
        assert!(dir.join(STATE_FILE).exists(), "the file stays until the pad is really visible again");
        h.restore().unwrap();
        let f = fake.lock().unwrap();
        assert!(f.hidden.is_empty() && !f.cloak && f.apps.is_empty());
        drop(f);
        assert!(!dir.join(STATE_FILE).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_pads_are_hidden_and_restored_together() {
        let dir = tmp("two");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        let mut h = HidHideHider::new(tool, &dir, EXE);
        h.hide(&target("u1", PAD1)).unwrap();
        h.hide(&target("u1", PAD1)).unwrap(); // again: nothing new
        h.hide(&target("u2", r"\\?\hid#vid_054c&pid_0ce6#7&99999999&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}")).unwrap();
        assert_eq!(fake.lock().unwrap().hidden.len(), 2);
        assert_eq!(fake.lock().unwrap().apps.len(), 1, "allowed once");
        h.restore().unwrap();
        assert!(fake.lock().unwrap().hidden.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unhiding_one_pad_keeps_the_other_hidden_and_the_last_one_restores_everything() {
        let dir = tmp("unhide");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        let mut h = HidHideHider::new(tool, &dir, EXE);
        let p2 = r"\\?\hid#vid_054c&pid_0ce6#7&99999999&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        h.hide(&target("u1", PAD1)).unwrap();
        h.hide(&target("u2", p2)).unwrap();
        h.unhide("u1").unwrap();
        {
            let f = fake.lock().unwrap();
            assert_eq!(f.hidden, vec![instance_path(p2).unwrap()]);
            assert!(f.cloak && f.apps.len() == 1, "still needed for the other pad");
        }
        assert!(!h.is_hidden("u1") && h.is_hidden("u2"));
        h.unhide("nobody").unwrap();
        h.unhide("u2").unwrap();
        let f = fake.lock().unwrap();
        assert!(f.hidden.is_empty() && !f.cloak && f.apps.is_empty());
        drop(f);
        assert!(!dir.join(STATE_FILE).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_tool_refuses_politely_and_changes_nothing() {
        let dir = tmp("missing");
        let tool = FakeHidHide::default();
        tool.inner.lock().unwrap().missing = true;
        let fake = tool.inner.clone();
        let mut h = HidHideHider::new(tool, &dir, EXE);
        assert_eq!(h.status(), HideStatus::ToolMissing);
        assert!(h.hide(&target("u1", PAD1)).unwrap_err().contains("not installed"));
        assert!(fake.lock().unwrap().calls.is_empty());
        assert!(!dir.join(STATE_FILE).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreadable_path_is_refused_before_anything_changes() {
        let dir = tmp("badpath");
        let tool = FakeHidHide::default();
        let fake = tool.inner.clone();
        let mut h = HidHideHider::new(tool, &dir, EXE);
        assert!(h.hide(&target("u1", "not-a-hid-path")).is_err());
        assert!(fake.lock().unwrap().calls.is_empty() && !dir.join(STATE_FILE).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
