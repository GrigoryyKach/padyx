//! Settings (`config.json`) and the data folder. Both the background process and the window read this file.

use crate::pad::output::{MuteLed, Output};
use crate::theme::ThemeMode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `%APPDATA%\Padyx` (`~/.config/Padyx` on Linux), or the folder from `PADYX_DATA_DIR` (tests and screenshots).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PADYX_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Padyx")
}

/// Held by every test that sets, removes or depends on `PADYX_DATA_DIR` (or the autostart override), because
/// the environment is shared by all test threads.
#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Is a data-dir override active? Test instances use their own names for the single-instance locks.
pub fn is_test_instance() -> bool {
    std::env::var_os("PADYX_DATA_DIR").is_some_and(|d| !d.is_empty())
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Window {
    pub w: f32,
    pub h: f32,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Interface language code (`en`, `ru`, `uk`); `None` until first resolved.
    pub language: Option<String>,
    pub theme: ThemeMode,
    /// The first-run screen has been confirmed.
    pub onboarded: bool,
    /// Wanted state of "start with the computer"; the real state is read from the system. Off by default.
    pub autostart: bool,
    pub check_updates: bool,
    /// Lightbar colour, brightness (0..255), player LEDs (5 bits) and the mute LED: re-applied on every connect.
    pub light: [u8; 3],
    pub brightness: u8,
    pub players: u8,
    pub mute_led: bool,
    /// The one-time Linux access screen was dealt with (installed or dismissed).
    pub linux_setup_seen: bool,
    pub window: Option<Window>,
    #[serde(skip)]
    pub fresh: bool,
}

impl Default for Config {
    fn default() -> Self {
        let o = Output::default();
        Self {
            language: None,
            theme: ThemeMode::System,
            onboarded: false,
            autostart: false,
            check_updates: true,
            light: o.light,
            brightness: o.brightness,
            players: o.players,
            mute_led: false,
            linux_setup_seen: false,
            window: None,
            fresh: false,
        }
    }
}

/// Writes a file in one step: a temporary file next to it, then a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

impl Config {
    pub fn load_from(dir: &Path) -> Self {
        let text = std::fs::read_to_string(dir.join("config.json")).ok();
        let mut cfg: Config = text.as_deref().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()).unwrap_or_default();
        cfg.fresh = text.is_none();
        cfg.players &= 0x1F;
        cfg
    }

    /// Writes the settings. `false` if they could not be written.
    pub fn save_to(&self, dir: &Path) -> bool {
        match serde_json::to_vec_pretty(self) {
            Ok(json) => write_atomic(&dir.join("config.json"), &json).is_ok(),
            Err(_) => false,
        }
    }

    /// What a pad starts with when it connects.
    pub fn output_template(&self) -> Output {
        Output {
            light: self.light,
            brightness: self.brightness,
            players: self.players,
            mute_led: if self.mute_led { MuteLed::On } else { MuteLed::Off },
            ..Output::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang};

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("padyx-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn missing_config_is_fresh_with_defaults() {
        let dir = tmp("fresh");
        let c = Config::load_from(&dir);
        assert!(c.fresh && !c.onboarded);
        assert_eq!(c.theme, ThemeMode::System);
        // Autostart is off until the user asks for it.
        assert!(!c.autostart);
        assert_eq!(c.brightness, 255);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_and_load_round_trip_keeps_the_lightbar() {
        let dir = tmp("rt");
        let mut c = Config::default();
        c.language = Some("uk".into());
        c.theme = ThemeMode::Dark;
        c.light = [1, 2, 3];
        c.brightness = 90;
        c.players = 0b10101;
        c.mute_led = true;
        c.onboarded = true;
        assert!(c.save_to(&dir));
        let l = Config::load_from(&dir);
        assert!(!l.fresh);
        assert_eq!(l.language.as_deref(), Some("uk"));
        assert_eq!(l.theme, ThemeMode::Dark);
        let t = l.output_template();
        assert_eq!((t.light, t.brightness, t.players, t.mute_led), ([1, 2, 3], 90, 0b10101, MuteLed::On));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bom_unknown_fields_and_out_of_range_values_are_tolerated() {
        let dir = tmp("bom");
        std::fs::write(dir.join("config.json"), "\u{feff}{\"theme\":\"light\",\"future_key\":1,\"players\":255}").unwrap();
        let c = Config::load_from(&dir);
        assert_eq!(c.theme, ThemeMode::Light);
        assert_eq!(c.players, 0x1F);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn data_dir_override() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("PADYX_DATA_DIR", "somewhere");
        assert_eq!(data_dir(), PathBuf::from("somewhere"));
        assert!(is_test_instance());
        std::env::remove_var("PADYX_DATA_DIR");
        assert!(data_dir().ends_with("Padyx"));
    }

    #[test]
    fn language_order_config_then_marker_then_english() {
        assert_eq!(i18n::resolve(Some("ru")), Lang::Ru);
        assert_eq!(i18n::resolve(Some("uk")), Lang::Uk);
        if i18n::marker_language().is_none() {
            assert_eq!(i18n::resolve(None), Lang::En);
        }
    }
}
