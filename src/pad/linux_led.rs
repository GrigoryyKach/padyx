//! Linux: the kernel's `hid-playstation` driver exposes the lightbar as an LED class device
//! (`/sys/class/leds/inputN:rgb:indicator`). When it is there, we write the colour through it, so Padyx does not
//! fight the kernel driver for the lightbar.

use std::path::{Path, PathBuf};

/// The LED that belongs to the pad with this serial (its MAC address). With one pad and one LED the match is
/// obvious even when the serial is unknown.
pub fn find(serial: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir("/sys/class/leds").ok()?;
    let leds: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(":rgb:indicator"))).collect();
    let want = serial.to_lowercase().replace(':', "");
    for l in &leds {
        let uniq = std::fs::read_to_string(l.join("device/uniq")).unwrap_or_default().trim().to_lowercase().replace(':', "");
        if !want.is_empty() && uniq == want {
            return Some(l.clone());
        }
    }
    if leds.len() == 1 {
        return leds.into_iter().next();
    }
    None
}

/// Sets the colour: the LED has a multicolour interface (`multi_intensity` red, green, blue) and a brightness.
pub fn set_rgb(led: &Path, rgb: [u8; 3]) {
    let _ = std::fs::write(led.join("multi_intensity"), format!("{} {} {}", rgb[0], rgb[1], rgb[2]));
    let _ = std::fs::write(led.join("brightness"), "255");
}
