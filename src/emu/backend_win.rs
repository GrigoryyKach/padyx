//! Windows: a virtual Xbox 360 pad through ViGEmBus (the `vigem-client` crate talks to the driver directly,
//! there is no DLL to ship). The game's rumble comes back as a driver notification on its own thread.

use super::backend::{Backend, BackendStatus, RumbleFn, VirtualPad};
use super::xbox::XboxState;
use std::sync::{Arc, Mutex};
use vigem_client::{Client, Error, TargetId, XButtons, XGamepad, Xbox360Wired};

pub struct ViGEmBackend {
    client: Mutex<Option<Arc<Client>>>,
}

pub fn system_backend() -> Arc<dyn Backend> {
    Arc::new(ViGEmBackend { client: Mutex::new(None) })
}

fn describe(e: Error) -> String {
    match e {
        Error::BusNotFound => "ViGEmBus is not installed".into(),
        Error::BusAccessFailed(c) => format!("ViGEmBus refused access (code {c})"),
        Error::BusVersionMismatch => "ViGEmBus has an incompatible version".into(),
        Error::NoFreeSlot => "ViGEmBus has no free slot for another virtual pad".into(),
        other => format!("{other:?}"),
    }
}

impl ViGEmBackend {
    fn client(&self) -> Result<Arc<Client>, Error> {
        let mut g = self.client.lock().expect("client lock");
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
        let c = Arc::new(Client::connect()?);
        *g = Some(c.clone());
        Ok(c)
    }
}

impl Backend for ViGEmBackend {
    fn status(&self) -> BackendStatus {
        match self.client() {
            Ok(_) => BackendStatus::Ready,
            Err(Error::BusNotFound) => BackendStatus::DriverMissing,
            Err(_) => BackendStatus::NoAccess,
        }
    }

    fn create(&self, rumble: RumbleFn) -> Result<Box<dyn VirtualPad>, String> {
        let client = self.client().map_err(describe)?;
        let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
        target.plugin().map_err(describe)?;
        target.wait_ready().map_err(describe)?;
        // Games set the rumble through XInput; the driver tells us on a thread that ends when the target is dropped.
        if let Ok(n) = target.request_notification() {
            n.spawn_thread(move |_, note| rumble(note.large_motor, note.small_motor));
        }
        Ok(Box::new(ViPad { target }))
    }
}

struct ViPad {
    target: Xbox360Wired<Arc<Client>>,
}

impl VirtualPad for ViPad {
    fn submit(&mut self, s: &XboxState) -> Result<(), String> {
        let g = XGamepad {
            buttons: XButtons(s.buttons),
            left_trigger: s.lt,
            right_trigger: s.rt,
            thumb_lx: s.lx,
            thumb_ly: s.ly,
            thumb_rx: s.rx,
            thumb_ry: s.ry,
        };
        // Right after the pad is plugged in, the driver may answer one of the first updates with "no more items"
        // (seen on a fast re-plug). It passes within a few milliseconds, so wait a little before giving up;
        // a pad that is really gone fails every try and is rebuilt by the emulator.
        let mut r = self.target.update(&g);
        for _ in 0..10 {
            if r.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            r = self.target.update(&g);
        }
        r.map_err(describe)
    }
}
