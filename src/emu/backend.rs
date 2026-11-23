//! The virtual pad, as the emulator sees it: a trait with one real implementation per system (ViGEmBus on
//! Windows, uinput on Linux) and a mock for tests and screenshots.

use super::xbox::XboxState;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Called by the back end when a game sets the rumble: (large slow motor, small fast motor), each 0..255.
/// It runs on the back end's own thread and must not block.
pub type RumbleFn = Arc<dyn Fn(u8, u8) + Send + Sync>;

/// Is the way to make a virtual pad there?
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BackendStatus {
    #[default]
    Ready,
    /// The driver (ViGEmBus) or the kernel module (uinput) is not installed.
    DriverMissing,
    /// It is there but this user may not use it (Linux: no access to `/dev/uinput`).
    NoAccess,
    /// The system cannot do it at all.
    Unsupported,
}

pub trait VirtualPad: Send {
    /// Sends a new state. Called on the reader thread for every changed report: no sleeping, no queues.
    fn submit(&mut self, state: &XboxState) -> Result<(), String>;
}

pub trait Backend: Send + Sync {
    fn status(&self) -> BackendStatus;
    /// Creates one virtual Xbox 360 pad. `rumble` is called when a game sets the motors.
    fn create(&self, rumble: RumbleFn) -> Result<Box<dyn VirtualPad>, String>;
}

/// A back end that records everything, for tests and for screenshots with the demo pad (no driver is touched).
#[derive(Default)]
pub struct MockBackend {
    pub missing: bool,
    pub fail_create: bool,
    pub created: AtomicU64,
    pub dropped: Arc<AtomicU64>,
    /// Every state submitted by every pad, in order.
    pub log: Arc<Mutex<Vec<XboxState>>>,
    /// The rumble callbacks of the live pads (a test calls them to play a game).
    pub rumbles: Arc<Mutex<Vec<RumbleFn>>>,
    pub submits: Arc<AtomicU64>,
}

impl MockBackend {
    pub fn new() -> Arc<MockBackend> {
        Arc::new(MockBackend::default())
    }
}

#[cfg(test)]
impl MockBackend {
    /// Plays a game setting the rumble on every live pad.
    pub fn game_rumble(&self, large: u8, small: u8) {
        for r in self.rumbles.lock().unwrap().iter() {
            r(large, small);
        }
    }
    pub fn live(&self) -> u64 {
        self.created.load(Ordering::Relaxed) - self.dropped.load(Ordering::Relaxed)
    }
}

struct MockPad {
    log: Arc<Mutex<Vec<XboxState>>>,
    submits: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    keep_log: bool,
}

impl VirtualPad for MockPad {
    fn submit(&mut self, s: &XboxState) -> Result<(), String> {
        self.submits.fetch_add(1, Ordering::Relaxed);
        if self.keep_log {
            self.log.lock().unwrap().push(*s);
        }
        Ok(())
    }
}

impl Drop for MockPad {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

impl Backend for MockBackend {
    fn status(&self) -> BackendStatus {
        if self.missing {
            BackendStatus::DriverMissing
        } else {
            BackendStatus::Ready
        }
    }
    fn create(&self, rumble: RumbleFn) -> Result<Box<dyn VirtualPad>, String> {
        if self.fail_create {
            return Err("create failed".into());
        }
        self.created.fetch_add(1, Ordering::Relaxed);
        self.rumbles.lock().unwrap().push(rumble);
        Ok(Box::new(MockPad { log: self.log.clone(), submits: self.submits.clone(), dropped: self.dropped.clone(), keep_log: true }))
    }
}

/// Like the mock but keeps no log, so a long run (the latency and CPU measurements) does not grow.
#[cfg(test)]
pub struct CountingBackend(pub Arc<MockBackend>);

#[cfg(test)]
impl Backend for CountingBackend {
    fn status(&self) -> BackendStatus {
        self.0.status()
    }
    fn create(&self, rumble: RumbleFn) -> Result<Box<dyn VirtualPad>, String> {
        self.0.created.fetch_add(1, Ordering::Relaxed);
        self.0.rumbles.lock().unwrap().push(rumble);
        Ok(Box::new(MockPad { log: self.0.log.clone(), submits: self.0.submits.clone(), dropped: self.0.dropped.clone(), keep_log: false }))
    }
}

#[cfg(windows)]
pub use super::backend_win::system_backend;
#[cfg(target_os = "linux")]
pub use super::backend_linux::system_backend;
/// Other systems have no way to make a virtual pad.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn system_backend() -> Arc<dyn Backend> {
    Arc::new(MockBackend { missing: true, ..Default::default() })
}
