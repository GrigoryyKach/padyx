//! The program in the foreground, for switching profiles by game. Read only: nothing here changes a window.
//!
//! - Windows: `GetForegroundWindow`, the owning process and its image path.
//! - Linux on X11: the window manager's `_NET_ACTIVE_WINDOW`, its `_NET_WM_PID` and `/proc/<pid>/exe`.
//! - Linux on Wayland: not available. A Wayland compositor does not tell other programs which window is active,
//!   so auto-switch is shown as unavailable and the profile chosen by hand stays.
//!
//! The watcher is one thread that wakes twice a second, and only asks while auto-switch is on.

use super::Emulator;
use std::sync::Weak;
use std::time::Duration;

pub const WATCH_EVERY: Duration = Duration::from_millis(500);

#[allow(dead_code)] // which variants a build constructs depends on the platform
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// A Wayland session: the compositor does not say which window is in front.
    Wayland,
    Unsupported,
}

/// Can the foreground program be read on this system?
pub fn availability() -> Availability {
    #[cfg(windows)]
    {
        Availability::Available
    }
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"));
        if wayland {
            Availability::Wayland
        } else if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) {
            Availability::Available
        } else {
            Availability::Unsupported
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Availability::Unsupported
    }
}

/// Something that can say which program is in front (a mock in the tests).
pub trait Probe: Send {
    /// The full path of the program, or `None` when it cannot be told.
    fn exe(&mut self) -> Option<String>;
}

/// A fixed answer: `PADYX_FG_MOCK` in a test folder (screenshots), and the tests.
pub struct Fixed(pub Option<String>);

impl Probe for Fixed {
    fn exe(&mut self) -> Option<String> {
        self.0.clone()
    }
}

#[cfg(windows)]
struct Win;

#[cfg(windows)]
impl Probe for Win {
    fn exe(&mut self) -> Option<String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
        // SAFETY: plain queries with valid out pointers; the process handle is closed on every path.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            if ok == 0 || len == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::Probe;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
    use x11rb::rust_connection::RustConnection;

    /// One connection to the X server, made when first needed and made again after an error.
    #[derive(Default)]
    pub struct X11 {
        conn: Option<(RustConnection, u32, u32, u32)>,
    }

    impl X11 {
        fn connect() -> Option<(RustConnection, u32, u32, u32)> {
            let (conn, n) = RustConnection::connect(None).ok()?;
            let root = conn.setup().roots.get(n)?.root;
            let active = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok()?.reply().ok()?.atom;
            let pid = conn.intern_atom(false, b"_NET_WM_PID").ok()?.reply().ok()?.atom;
            Some((conn, root, active, pid))
        }

        fn query(&self) -> Option<Option<String>> {
            let (conn, root, active, pid) = self.conn.as_ref()?;
            let r = conn.get_property(false, *root, *active, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
            let win = r.value32().and_then(|mut v| v.next()).unwrap_or(0);
            if win == 0 {
                return Some(None);
            }
            let r = conn.get_property(false, win, *pid, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?;
            let Some(pid) = r.value32().and_then(|mut v| v.next()) else { return Some(None) };
            Some(std::fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| p.to_string_lossy().into_owned()))
        }
    }

    impl Probe for X11 {
        fn exe(&mut self) -> Option<String> {
            if self.conn.is_none() {
                self.conn = Self::connect();
            }
            match self.query() {
                Some(v) => v,
                None => {
                    // The server went away or answered badly: start again next time.
                    self.conn = None;
                    None
                }
            }
        }
    }
}

/// The probe for this system. In a test folder `PADYX_FG_MOCK=<path>` fakes the answer (screenshots).
pub fn system_probe() -> Box<dyn Probe> {
    if crate::config::is_test_instance() {
        if let Some(v) = std::env::var_os("PADYX_FG_MOCK") {
            return Box::new(Fixed(Some(v.to_string_lossy().into_owned())));
        }
    }
    if availability() == Availability::Available {
        native()
    } else {
        Box::new(Fixed(None))
    }
}

#[cfg(windows)]
fn native() -> Box<dyn Probe> {
    Box::new(Win)
}

#[cfg(target_os = "linux")]
fn native() -> Box<dyn Probe> {
    Box::new(x11::X11::default())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn native() -> Box<dyn Probe> {
    Box::new(Fixed(None))
}

/// Starts the watcher: every `every` it asks the probe and tells the emulator, while auto-switch is on. The
/// thread ends when the emulator is gone.
pub fn spawn_watcher(emu: Weak<Emulator>, mut probe: Box<dyn Probe>, every: Duration) {
    let _ = std::thread::Builder::new().name("padyx-foreground".into()).spawn(move || loop {
        std::thread::sleep(every);
        let Some(e) = emu.upgrade() else { return };
        if e.auto_switch() {
            e.on_foreground(probe.exe());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixed_probe_answers_what_it_was_given() {
        assert_eq!(Fixed(Some("a.exe".into())).exe().as_deref(), Some("a.exe"));
        assert_eq!(Fixed(None).exe(), None);
    }

    #[test]
    fn availability_is_decided_by_the_session() {
        // Whatever this machine is, the answer is one of the three and matches the platform.
        let a = availability();
        if cfg!(windows) {
            assert_eq!(a, Availability::Available);
        }
        assert!(matches!(a, Availability::Available | Availability::Wayland | Availability::Unsupported));
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_probe_runs_without_crashing() {
        // Read only. The answer depends on what is in front, so only the shape is checked.
        if let Some(p) = Win.exe() {
            assert!(!p.is_empty());
        }
    }
}
