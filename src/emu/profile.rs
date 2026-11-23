//! Profiles: named sets of mapping, dead zones, lightbar colour, macros and turbo.
//!
//! There is always the **default profile**: it is `config.json`'s own `emu` section and `light`, so a file from
//! before profiles existed loads unchanged. The others are in `profiles`. One profile is active at a time:
//!
//! - by hand: `profile_current` (empty means the default one);
//! - or, with auto-switch on, by the program in the foreground: the first profile whose list of programs
//!   contains it wins; with no match (or no way to know) the hand-picked one stays.
//!
//! The rumble strength and the native-mode pads are the same in every profile (they belong to the machine, not
//! to a game), so [`effective`] takes them from the default profile.

use super::map::EmuConfig;
use serde::{Deserialize, Serialize};

pub const MAX_PROFILES: usize = 32;
pub const MAX_EXES: usize = 8;
const NAME_MAX: usize = 40;
const FILE_VERSION: u32 = 1;
/// An exported profile is a few kilobytes; refuse anything absurd before parsing it.
pub const MAX_FILE_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// Programs that switch to this profile: a file name (`game.exe`) or a full path.
    pub exes: Vec<String>,
    /// The lightbar colour while this profile is active (`None`: leave the usual colour).
    pub light: Option<[u8; 3]>,
    pub emu: EmuConfig,
}

impl Default for Profile {
    fn default() -> Self {
        Self { name: String::new(), exes: Vec::new(), light: None, emu: EmuConfig::default() }
    }
}

fn norm(s: &str) -> String {
    s.trim().replace('\\', "/").to_lowercase()
}

fn base(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

fn strip_exe(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

/// Does the list entry name this program? An entry with a folder in it must be the whole path; one without is
/// compared to the file name (the ".exe" is optional).
pub fn exe_matches(entry: &str, path: &str) -> bool {
    let (e, p) = (norm(entry), norm(path));
    if e.is_empty() || p.is_empty() {
        return false;
    }
    if e.contains('/') {
        return e == p;
    }
    strip_exe(&e) == strip_exe(base(&p))
}

/// Which profile is active: the index in `list`, or `None` for the default one.
pub fn pick(list: &[Profile], current: &str, auto: bool, foreground: Option<&str>) -> Option<usize> {
    if auto {
        if let Some(fg) = foreground {
            if let Some(i) = list.iter().position(|p| p.exes.iter().any(|e| exe_matches(e, fg))) {
                return Some(i);
            }
        }
    }
    list.iter().position(|p| !current.is_empty() && p.name == current)
}

/// The settings the emulator runs with: the profile's own, except what is one for the whole machine.
pub fn effective(default: &EmuConfig, p: Option<&Profile>) -> EmuConfig {
    let mut e = p.map(|p| p.emu.clone()).unwrap_or_else(|| default.clone());
    e.rumble_pct = default.rumble_pct;
    e.native = default.native.clone();
    e
}

fn clean_name(raw: &str) -> String {
    raw.trim().chars().filter(|c| !c.is_control()).take(NAME_MAX).collect::<String>().trim().to_string()
}

/// `name`, or `name 2`, `name 3` and so on when it is taken (case does not count).
pub fn unique_name(list: &[Profile], name: &str) -> String {
    let base = match clean_name(name) {
        n if n.is_empty() => "Profile".to_string(),
        n => n,
    };
    let taken = |n: &str| list.iter().any(|p| p.name.eq_ignore_ascii_case(n));
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|i| {
            let suffix = format!(" {i}");
            let head: String = base.chars().take(NAME_MAX - suffix.chars().count()).collect();
            format!("{head}{suffix}")
        })
        .find(|n| !taken(n))
        .unwrap_or(base)
}

/// Fixes a loaded list: names are trimmed and unique, limits hold, every profile's settings are sane, and
/// `current` names a profile that exists (or is empty).
pub fn sanitize(list: &mut Vec<Profile>, current: &mut String) {
    list.truncate(MAX_PROFILES);
    let mut done: Vec<Profile> = Vec::new();
    for mut p in list.drain(..) {
        p.name = unique_name(&done, &p.name);
        p.exes.retain(|e| !e.trim().is_empty());
        p.exes = p.exes.iter().map(|e| e.trim().chars().take(260).collect::<String>()).collect();
        p.exes.truncate(MAX_EXES);
        p.emu.sanitize();
        // Settings that are not per profile are never kept here.
        p.emu.native.clear();
        done.push(p);
    }
    *list = done;
    if !current.is_empty() && !list.iter().any(|p| &p.name == current) {
        current.clear();
    }
}

#[derive(Serialize)]
struct FileOut<'a> {
    padyx_profile: u32,
    profile: &'a Profile,
}

#[derive(Deserialize)]
struct FileIn {
    #[serde(default)]
    padyx_profile: u32,
    profile: Option<Profile>,
}

/// The text of an exported profile. Pad serial numbers (native mode) are not part of it.
pub fn to_json(p: &Profile) -> String {
    let mut q = p.clone();
    q.emu.native.clear();
    serde_json::to_string_pretty(&FileOut { padyx_profile: FILE_VERSION, profile: &q }).unwrap_or_default()
}

/// Reads an exported profile. The marker is checked, the size is limited and everything is sanitised.
pub fn from_json(text: &str) -> Result<Profile, String> {
    if text.len() > MAX_FILE_BYTES {
        return Err("the file is too large".into());
    }
    let f: FileIn = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    if f.padyx_profile == 0 || f.padyx_profile > FILE_VERSION {
        return Err("not a Padyx profile".into());
    }
    let mut p = f.profile.ok_or_else(|| "not a Padyx profile".to_string())?;
    let mut list = vec![p];
    sanitize(&mut list, &mut String::new());
    p = list.pop().ok_or_else(|| "empty".to_string())?;
    Ok(p)
}

/// `Name.json` made safe as a file name.
pub fn file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect();
    let s = s.trim().to_string();
    format!("{}.padyx-profile.json", if s.is_empty() { "profile" } else { &s })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu::macros::{Macro, Step, XOut};
    use crate::emu::remap::{DIn, Out};
    use crate::emu::xbox::XBtn;

    fn prof(name: &str, exes: &[&str]) -> Profile {
        Profile { name: name.into(), exes: exes.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn exe_matching_by_name_and_by_path() {
        assert!(exe_matches("Game.exe", "C:\\Games\\Game.exe"));
        assert!(exe_matches("game", "D:\\x\\GAME.EXE"));
        assert!(!exe_matches("game.exe", "C:\\Games\\Game2.exe"));
        assert!(!exe_matches("ame.exe", "C:\\Games\\Game.exe"), "a name is the whole file name");
        assert!(exe_matches("c:\\games\\game.exe", "C:\\Games\\Game.exe"));
        assert!(!exe_matches("d:\\games\\game.exe", "C:\\Games\\Game.exe"), "a path is the whole path");
        assert!(exe_matches("wine64-preloader", "/usr/bin/wine64-preloader"));
        assert!(!exe_matches("", "C:\\a.exe") && !exe_matches("a.exe", ""));
    }

    #[test]
    fn switching_follows_the_foreground_and_falls_back_to_the_chosen_profile() {
        let list = vec![prof("Racing", &["forza.exe"]), prof("Shooter", &["cod.exe", "C:\\Games\\halo.exe"]), prof("Chill", &[])];
        // Auto on: the game decides.
        assert_eq!(pick(&list, "Chill", true, Some("C:\\Games\\Forza.exe")), Some(0));
        assert_eq!(pick(&list, "Chill", true, Some("C:\\Games\\halo.exe")), Some(1));
        // No match: the hand-picked profile, or the default.
        assert_eq!(pick(&list, "Chill", true, Some("C:\\Windows\\explorer.exe")), Some(2));
        assert_eq!(pick(&list, "", true, Some("C:\\Windows\\explorer.exe")), None);
        // Nothing known about the foreground (Wayland): the hand-picked one.
        assert_eq!(pick(&list, "Shooter", true, None), Some(1));
        // Auto off: the foreground is ignored.
        assert_eq!(pick(&list, "", false, Some("forza.exe")), None);
        assert_eq!(pick(&list, "Racing", false, Some("cod.exe")), Some(0));
        // A name that no longer exists is the default.
        assert_eq!(pick(&list, "Gone", false, None), None);
    }

    #[test]
    fn rumble_and_native_pads_are_the_same_in_every_profile() {
        let mut d = EmuConfig::default();
        d.rumble_pct = 40;
        d.set_native("aa:bb", true);
        let mut p = prof("P", &[]);
        p.emu.rumble_pct = 90;
        p.emu.left.inner = 0.2;
        let e = effective(&d, Some(&p));
        assert_eq!(e.rumble_pct, 40);
        assert!(e.is_native("aa:bb"));
        assert_eq!(e.left.inner, 0.2, "the rest is the profile's own");
        assert_eq!(effective(&d, None), d);
    }

    #[test]
    fn names_are_unique_trimmed_and_short() {
        let mut list = vec![prof(" Racing ", &[]), prof("racing", &[]), prof("", &[]), prof(&"x".repeat(80), &[])];
        let mut cur = "gone".to_string();
        sanitize(&mut list, &mut cur);
        let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names[0], "Racing");
        assert_eq!(names[1], "racing 2");
        assert_eq!(names[2], "Profile");
        assert_eq!(names[3].chars().count(), 40);
        assert!(cur.is_empty(), "the current name must exist");
        let mut cur = "Racing".to_string();
        sanitize(&mut list, &mut cur);
        assert_eq!(cur, "Racing");
    }

    #[test]
    fn the_json_round_trip_keeps_everything_but_the_pad_serials() {
        let mut p = prof("Shooter", &["cod.exe"]);
        p.light = Some([255, 0, 40]);
        p.emu.left.inner = 0.15;
        p.emu.set_target(DIn::Cross, Out::Btn(XBtn::B));
        p.emu.turbo.push(crate::emu::macros::Turbo { btn: XBtn::Rb, hz: 15 });
        p.emu.macros.push(Macro { name: "Combo".into(), chord: vec![DIn::Create, DIn::L1], steps: vec![Step::Hold(XOut::Btn(XBtn::A), 80), Step::Wait(40)], ..Default::default() });
        p.emu.set_native("aa:bb:cc", true);
        let text = to_json(&p);
        assert!(!text.contains("aa:bb:cc"), "serial numbers are not exported");
        let back = from_json(&text).unwrap();
        p.emu.native.clear();
        assert_eq!(back, p);
    }

    #[test]
    fn import_refuses_what_is_not_a_profile() {
        assert!(from_json("not json").is_err());
        assert!(from_json("{}").is_err());
        assert!(from_json("{\"profile\":{\"name\":\"x\"}}").is_err(), "no marker");
        assert!(from_json("{\"padyx_profile\":99,\"profile\":{}}").is_err(), "a newer file version");
        assert!(from_json(&" ".repeat(MAX_FILE_BYTES + 1)).is_err());
        // Out-of-range values in a hand-edited file are fixed, a BOM is fine.
        let p = from_json("\u{feff}{\"padyx_profile\":1,\"profile\":{\"name\":\"  Hand  \",\"emu\":{\"rumble_pct\":250,\"turbo\":[{\"btn\":\"A\",\"hz\":99}]}}}").unwrap();
        assert_eq!(p.name, "Hand");
        assert_eq!(p.emu.rumble_pct, 100);
        assert_eq!(p.emu.turbo[0].hz, 30);
    }

    #[test]
    fn export_file_names_are_safe() {
        assert_eq!(file_name("Racing"), "Racing.padyx-profile.json");
        assert_eq!(file_name("a/b:c*"), "a_b_c_.padyx-profile.json");
        assert_eq!(file_name("  "), "profile.padyx-profile.json");
    }
}
