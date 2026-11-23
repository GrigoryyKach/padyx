//! The two Windows drivers and how Padyx helps to install them.
//!
//! - **ViGEmBus** makes the virtual Xbox 360 pad.
//! - **HidHide** hides the real DualSense from games.
//!
//! Padyx ships neither. The "Install" button downloads the official installer from a **pinned URL** and checks it
//! against a **pinned SHA-256** before anything runs; a file that does not match is deleted and never started.
//! Only the installer itself runs elevated (Windows asks for permission), never Padyx.
//!
//! Where the pinned values come from: the URLs are the assets of the official GitHub releases
//! (github.com/nefarius/ViGEmBus v1.22.0, github.com/nefarius/HidHide v1.5.230.0) and the hashes are the
//! `InstallerSha256` of the winget manifests of the same files (Microsoft's package repository, which checks each
//! hash when a manifest is accepted). ViGEmBus is archived by its author and will not change again.

use crate::selfupdate::sha256_file;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Driver {
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    /// Lower-case hex.
    pub sha256: &'static str,
    pub size: u64,
    /// The official page, for a manual install.
    pub page: &'static str,
    /// Windows asks to restart after this one.
    pub needs_restart: bool,
}

pub const VIGEMBUS: Driver = Driver {
    name: "ViGEmBus",
    file: "ViGEmBus_1.22.0_x64_x86_arm64.exe",
    url: "https://github.com/nefarius/ViGEmBus/releases/download/v1.22.0/ViGEmBus_1.22.0_x64_x86_arm64.exe",
    sha256: "89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a",
    size: 6_278_576,
    page: "https://github.com/nefarius/ViGEmBus/releases/tag/v1.22.0",
    needs_restart: false,
};

pub const HIDHIDE: Driver = Driver {
    name: "HidHide",
    file: "HidHide_1.5.230_x64.exe",
    url: "https://github.com/nefarius/HidHide/releases/download/v1.5.230.0/HidHide_1.5.230_x64.exe",
    sha256: "f4bbbcb82e6258641b887c74bc81c4c5f66e4aa811808dfc304347687b7605f6",
    size: 8_078_016,
    page: "https://github.com/nefarius/HidHide/releases/tag/v1.5.230.0",
    needs_restart: true,
};

/// `PADYX_EMU_MOCK=1` in a test instance: nothing real is touched, the drivers read as installed (screenshots).
pub fn mock_requested() -> bool {
    crate::config::is_test_instance() && std::env::var("PADYX_EMU_MOCK").is_ok_and(|v| v == "1")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriversStatus {
    pub vigembus: bool,
    pub hidhide: bool,
}

pub fn status() -> DriversStatus {
    if mock_requested() {
        return DriversStatus { vigembus: true, hidhide: true };
    }
    #[cfg(windows)]
    {
        DriversStatus { vigembus: vigem_client::Client::connect().is_ok(), hidhide: super::hide::cli::find().is_some() }
    }
    #[cfg(not(windows))]
    {
        DriversStatus { vigembus: false, hidhide: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriverError {
    /// curl is missing or the download failed.
    Download(String),
    /// The file is not the one we pinned. It has been deleted.
    HashMismatch { expected: String, got: String },
    /// The address is not https (only tests may use http).
    NotHttps,
    /// The installer could not be started (or the permission prompt was declined).
    Launch(String),
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriverError::Download(e) => write!(f, "download failed: {e}"),
            DriverError::HashMismatch { expected, got } => write!(f, "the file does not match the pinned checksum (expected {expected}, got {got}); it was deleted and not run"),
            DriverError::NotHttps => write!(f, "the address is not https"),
            DriverError::Launch(e) => write!(f, "could not start the installer: {e}"),
        }
    }
}

/// Where we are, shown by the window.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Downloading { done: u64, total: u64 },
    Verifying,
    /// The installer has been started (the permission prompt and the installer's own window are up).
    Launched,
    Failed(DriverError),
}

pub type Shared = Arc<Mutex<Phase>>;

pub fn set_phase(s: &Shared, p: Phase) {
    if let Ok(mut g) = s.lock() {
        *g = p;
    }
}

/// Downloads `url` to `dest` with curl and checks its SHA-256. On a mismatch the file is removed.
/// `allow_http` is for the tests with a local server; the real calls never set it.
pub fn download_verified(url: &str, dest: &Path, sha256: &str, max_bytes: u64, allow_http: bool, progress: &dyn Fn(u64)) -> Result<(), DriverError> {
    if !allow_http && !url.starts_with("https://") {
        return Err(DriverError::NotHttps);
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| DriverError::Download(e.to_string()))?;
    }
    let _ = std::fs::remove_file(dest);
    let mut cmd = std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args(["--fail", "--silent", "--show-error", "--location", "--max-redirs", "5", "--max-time", "600"]);
    cmd.args(["--max-filesize", &max_bytes.to_string()]);
    if allow_http {
        cmd.args(["--noproxy", "*"]);
    } else {
        cmd.args(["--proto", "=https", "--proto-redir", "=https"]);
    }
    cmd.arg("--output").arg(dest).arg(url);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).spawn().map_err(|e| DriverError::Download(format!("curl: {e}")))?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                progress(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0));
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(DriverError::Download(e.to_string())),
        }
    };
    if !status.success() {
        let mut err = String::new();
        if let Some(mut se) = child.stderr.take() {
            use std::io::Read;
            let _ = se.read_to_string(&mut err);
        }
        let _ = std::fs::remove_file(dest);
        return Err(DriverError::Download(err.trim().to_string()));
    }
    progress(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0));
    verify(dest, sha256)
}

/// Checks a file against a pinned hash, deleting it when it does not match.
pub fn verify(file: &Path, sha256: &str) -> Result<(), DriverError> {
    let got = sha256_file(file).map_err(|e| DriverError::Download(e.to_string()))?;
    if got.eq_ignore_ascii_case(sha256) {
        Ok(())
    } else {
        let _ = std::fs::remove_file(file);
        Err(DriverError::HashMismatch { expected: sha256.to_lowercase(), got })
    }
}

/// The whole flow: download, verify, and only then call `launch`. The launcher is a parameter so a test can
/// prove that nothing starts after a bad hash.
pub fn fetch_and_run(d: &Driver, dir: &Path, url: &str, allow_http: bool, shared: &Shared, launch: &dyn Fn(&Path) -> Result<(), String>) -> Result<(), DriverError> {
    let dest = dir.join(d.file);
    set_phase(shared, Phase::Downloading { done: 0, total: d.size });
    let s = shared.clone();
    let total = d.size;
    download_verified(url, &dest, d.sha256, d.size * 2, allow_http, &move |done| set_phase(&s, Phase::Downloading { done, total: total.max(done) }))?;
    set_phase(shared, Phase::Verifying);
    // Check once more right before running: the file sat on disk for a moment.
    verify(&dest, d.sha256)?;
    launch(&dest).map_err(DriverError::Launch)?;
    set_phase(shared, Phase::Launched);
    Ok(())
}

/// A fresh folder in the system temp area for the installer.
pub fn download_dir() -> PathBuf {
    std::env::temp_dir().join(format!("padyx-driver-{}", std::process::id()))
}

/// Starts the installer with the Windows permission prompt. Returns once it has been started.
#[cfg(windows)]
pub fn launch_elevated(file: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide(std::ffi::OsStr::new("runas"));
    let path = wide(file.as_os_str());
    // SAFETY: plain ShellExecuteW with NUL-terminated strings that live through the call.
    let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), path.as_ptr(), std::ptr::null(), std::ptr::null(), 1) } as isize;
    if r > 32 {
        Ok(())
    } else if r == 5 || r == 1223 {
        Err("the permission prompt was declined".into())
    } else {
        Err(format!("ShellExecute returned {r}"))
    }
}

#[cfg(not(windows))]
pub fn launch_elevated(_file: &Path) -> Result<(), String> {
    Err("not available on this system".into())
}

/// The button: downloads, checks and starts the official installer on a thread. Progress is in `shared`.
pub fn install(d: &'static Driver, shared: Shared, repaint: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let dir = download_dir();
        let r = fetch_and_run(d, &dir, d.url, false, &shared, &|p| launch_elevated(p));
        if let Err(e) = r {
            set_phase(&shared, Phase::Failed(e));
        }
        repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("padyx-drv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A local server that answers any GET with `body`, `n` times. Returns its URL.
    fn serve(body: Vec<u8>, n: usize) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for _ in 0..n {
                let Ok((mut s, _)) = l.accept() else { return };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}/installer.exe")
    }

    #[test]
    fn the_pinned_values_are_well_formed() {
        for d in [&VIGEMBUS, &HIDHIDE] {
            assert!(d.url.starts_with("https://github.com/nefarius/"), "official source only");
            assert!(d.url.ends_with(d.file));
            assert_eq!(d.sha256.len(), 64);
            assert!(d.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
            assert!(d.page.starts_with("https://github.com/nefarius/"));
            assert!(d.size > 1_000_000);
        }
        assert!(HIDHIDE.needs_restart && !VIGEMBUS.needs_restart);
    }

    #[test]
    fn a_matching_file_is_accepted_and_launched() {
        let dir = tmp("ok");
        let body = b"pretend installer bytes".to_vec();
        let want = crate::selfupdate::sha256_hex(&body);
        let url = serve(body.clone(), 1);
        let launched = AtomicBool::new(false);
        let shared: Shared = Default::default();
        // A driver description pointing at our hash.
        let d = Driver { sha256: Box::leak(want.into_boxed_str()), size: body.len() as u64, ..VIGEMBUS };
        fetch_and_run(&d, &dir, &url, true, &shared, &|p| {
            assert_eq!(std::fs::read(p).unwrap(), body);
            launched.store(true, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert!(launched.load(Ordering::SeqCst));
        assert_eq!(*shared.lock().unwrap(), Phase::Launched);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wrong_hash_is_refused_deleted_and_never_launched() {
        let dir = tmp("bad");
        let url = serve(b"not the real installer".to_vec(), 1);
        let launched = AtomicBool::new(false);
        let shared: Shared = Default::default();
        let err = fetch_and_run(&VIGEMBUS, &dir, &url, true, &shared, &|_| {
            launched.store(true, Ordering::SeqCst);
            Ok(())
        })
        .unwrap_err();
        match err {
            DriverError::HashMismatch { expected, got } => {
                assert_eq!(expected, VIGEMBUS.sha256);
                assert_ne!(got, expected);
            }
            other => panic!("{other:?}"),
        }
        assert!(!launched.load(Ordering::SeqCst), "nothing runs after a bad hash");
        assert!(!dir.join(VIGEMBUS.file).exists(), "the file is deleted");
        assert!(err_text(&DriverError::HashMismatch { expected: "a".into(), got: "b".into() }).contains("deleted and not run"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn err_text(e: &DriverError) -> String {
        e.to_string()
    }

    #[test]
    fn http_is_refused_unless_a_test_allows_it() {
        let dir = tmp("http");
        let shared: Shared = Default::default();
        let r = fetch_and_run(&VIGEMBUS, &dir, "http://127.0.0.1:1/x.exe", false, &shared, &|_| Ok(()));
        assert_eq!(r, Err(DriverError::NotHttps));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_download_reports_and_launches_nothing() {
        let dir = tmp("down");
        // Nothing listens on this port.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let shared: Shared = Default::default();
        let launched = AtomicBool::new(false);
        let r = fetch_and_run(&VIGEMBUS, &dir, &format!("http://127.0.0.1:{port}/x.exe"), true, &shared, &|_| {
            launched.store(true, Ordering::SeqCst);
            Ok(())
        });
        assert!(matches!(r, Err(DriverError::Download(_))), "{r:?}");
        assert!(!launched.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_oversized_file_is_cut_off() {
        let dir = tmp("big");
        let url = serve(vec![7u8; 5000], 1);
        let dest = dir.join("x.exe");
        let r = download_verified(&url, &dest, &"0".repeat(64), 1000, true, &|_| {});
        assert!(matches!(r, Err(DriverError::Download(_))), "{r:?}");
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verify_accepts_either_case_of_the_hash() {
        let dir = tmp("case");
        let f = dir.join("a.bin");
        std::fs::write(&f, b"abc").unwrap();
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&f, h).is_ok());
        assert!(verify(&f, &h.to_uppercase()).is_ok());
        assert!(verify(&f, &"0".repeat(64)).is_err());
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
