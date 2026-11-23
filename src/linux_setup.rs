//! Linux access to the pad: a udev rule that tags the Sony hidraw devices so the logged-in user may open them.
//!
//! Padyx never installs the rule on its own. On the first run, when a pad is there but cannot be opened, the
//! window shows one screen that explains why and offers to install the rule through `pkexec` (the system's own
//! password prompt). Nothing else is done as root. The functions here are plain and take paths, so the tests
//! use a temporary folder and never touch `/etc`.

#![allow(dead_code)] // the installer is only called on Linux

use std::path::{Path, PathBuf};

pub const RULE_PATH: &str = "/etc/udev/rules.d/70-padyx.rules";

/// The rule text. USB devices match on the vendor and product, Bluetooth ones on the HID id in the kernel name.
pub fn rule_text() -> String {
    let mut s = String::from("# Padyx: let the logged-in user open Sony pads (hidraw), for the tester and the Xbox emulation.\n");
    for pid in ["0ce6", "05c4", "09cc"] {
        s.push_str(&format!("KERNEL==\"hidraw*\", ATTRS{{idVendor}}==\"054c\", ATTRS{{idProduct}}==\"{pid}\", TAG+=\"uaccess\"\n"));
        s.push_str(&format!("KERNEL==\"hidraw*\", KERNELS==\"*054C:{}*\", TAG+=\"uaccess\"\n", pid.to_uppercase()));
    }
    s
}

/// Writes the rule into a private folder (the file `pkexec` will copy).
pub fn write_rule_file(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let p = dir.join("70-padyx.rules");
    std::fs::write(&p, rule_text())?;
    Ok(p)
}

/// The shell command `pkexec` runs: copy the file, reload the rules, re-trigger the hidraw devices.
pub fn install_script(source: &Path, target: &str) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    format!(
        "install -m 0644 {} {} && udevadm control --reload-rules && udevadm trigger --subsystem-match=hidraw",
        q(&source.display().to_string()),
        q(target)
    )
}

pub fn installed() -> bool {
    std::fs::read_to_string(RULE_PATH).is_ok_and(|t| t.contains("uaccess"))
}

/// Asks the system for permission (`pkexec`) and installs the rule.
#[cfg(unix)]
pub fn install(data_dir: &Path) -> Result<(), String> {
    let src = write_rule_file(&data_dir.join("setup")).map_err(|e| e.to_string())?;
    let script = install_script(&src, RULE_PATH);
    let status = std::process::Command::new("pkexec").args(["sh", "-c", &script]).status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("pkexec exited with {status}"))
    }
}

#[cfg(not(unix))]
pub fn install(_data_dir: &Path) -> Result<(), String> {
    Err("not available on this system".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_covers_usb_and_bluetooth_for_each_model() {
        let r = rule_text();
        assert_eq!(r.lines().filter(|l| l.contains("TAG+=\"uaccess\"")).count(), 6);
        assert!(r.contains("ATTRS{idProduct}==\"0ce6\""));
        assert!(r.contains("KERNELS==\"*054C:0CE6*\""));
        assert!(r.contains("KERNELS==\"*054C:05C4*\"") && r.contains("KERNELS==\"*054C:09CC*\""));
        assert!(!r.contains("MODE=\"0666\""), "never world-writable");
    }

    #[test]
    fn install_goes_through_a_temp_file_and_quotes_paths() {
        let dir = std::env::temp_dir().join(format!("padyx-udev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = write_rule_file(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), rule_text());
        let s = install_script(Path::new("/tmp/it's here/70.rules"), RULE_PATH);
        assert!(s.starts_with("install -m 0644 '/tmp/it'\\''s here/70.rules' '/etc/udev/rules.d/70-padyx.rules' && udevadm control --reload-rules"));
        assert!(s.ends_with("--subsystem-match=hidraw"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
