//! The low-battery notice. The decision (who is low, who was told already) is plain code with tests; showing the
//! notice is a one-line command: a Windows toast through PowerShell, `notify-send` on Linux. Both are started and
//! not waited for, and a missing tool is silently ignored.

use crate::pad::{Charge, PadSnapshot};
use std::collections::HashSet;

/// The share of charge at which the notice appears.
pub const LOW_PERCENT: u8 = 15;
/// The notice can come again after the pad has been charged past this.
pub const RECOVER_PERCENT: u8 = 20;

/// Remembers which pads were already told, and returns the ones to tell now.
#[derive(Default)]
pub struct LowBattery {
    told: HashSet<String>,
}

impl LowBattery {
    /// `pads` as `(uid, battery, charge)`. A pad is told once when it drops to the limit while not charging, and
    /// again only after it has been charged above the recovery level (or has been plugged in and charging).
    pub fn check(&mut self, pads: &[(String, Option<u8>, Charge)]) -> Vec<String> {
        let mut now_low = Vec::new();
        for (uid, battery, charge) in pads {
            let Some(b) = *battery else { continue };
            let charging = matches!(charge, Charge::Charging | Charge::Full);
            if charging || b > RECOVER_PERCENT {
                self.told.remove(uid);
            } else if b <= LOW_PERCENT && !self.told.contains(uid) {
                self.told.insert(uid.clone());
                now_low.push(uid.clone());
            }
        }
        // Forget pads that are gone.
        self.told.retain(|u| pads.iter().any(|p| &p.0 == u));
        now_low
    }
}

pub fn pads_for_check(pads: &[PadSnapshot]) -> Vec<(String, Option<u8>, Charge)> {
    pads.iter().map(|p| (p.info.uid.clone(), p.state.battery, p.state.charge)).collect()
}

/// Shows a notice. Never blocks and never fails loudly.
pub fn send(title: &str, body: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const SCRIPT: &str = "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
            $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
            $n = $t.GetElementsByTagName('text'); \
            $n.Item(0).AppendChild($t.CreateTextNode($env:PADYX_NOTE_TITLE)) > $null; \
            $n.Item(1).AppendChild($t.CreateTextNode($env:PADYX_NOTE_BODY)) > $null; \
            [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe').Show([Windows.UI.Notifications.ToastNotification]::new($t))";
        let _ = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", SCRIPT])
            .env("PADYX_NOTE_TITLE", title)
            .env("PADYX_NOTE_BODY", body)
            .creation_flags(0x0800_0000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("notify-send")
            .args(["--app-name=Padyx", "--icon=input-gaming", title, body])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(uid: &str, b: Option<u8>, c: Charge) -> (String, Option<u8>, Charge) {
        (uid.into(), b, c)
    }

    #[test]
    fn told_once_at_fifteen_percent_and_again_only_after_recovery() {
        let mut l = LowBattery::default();
        assert!(l.check(&[pad("a", Some(40), Charge::Discharging)]).is_empty());
        assert!(l.check(&[pad("a", Some(16), Charge::Discharging)]).is_empty(), "16 is not yet low");
        assert_eq!(l.check(&[pad("a", Some(15), Charge::Discharging)]), vec!["a".to_string()]);
        assert!(l.check(&[pad("a", Some(14), Charge::Discharging)]).is_empty(), "no repeat");
        assert!(l.check(&[pad("a", Some(18), Charge::Discharging)]).is_empty(), "between 15 and 20 stays quiet");
        // Charged well above the limit, then low again: the notice comes back.
        assert!(l.check(&[pad("a", Some(60), Charge::Discharging)]).is_empty());
        assert_eq!(l.check(&[pad("a", Some(10), Charge::Discharging)]), vec!["a".to_string()]);
    }

    #[test]
    fn charging_pads_and_pads_without_a_reading_are_never_told() {
        let mut l = LowBattery::default();
        assert!(l.check(&[pad("a", Some(5), Charge::Charging), pad("b", None, Charge::Discharging), pad("c", Some(3), Charge::Full)]).is_empty());
        // Plugged in while low resets it, so unplugging later at the same level tells again.
        assert_eq!(l.check(&[pad("a", Some(5), Charge::Discharging)]), vec!["a".to_string()]);
        assert!(l.check(&[pad("a", Some(6), Charge::Charging)]).is_empty());
        assert_eq!(l.check(&[pad("a", Some(7), Charge::Discharging)]), vec!["a".to_string()]);
    }

    #[test]
    fn each_pad_is_tracked_alone() {
        let mut l = LowBattery::default();
        let both = [pad("a", Some(10), Charge::Discharging), pad("b", Some(90), Charge::Discharging)];
        assert_eq!(l.check(&both), vec!["a".to_string()]);
        let later = [pad("a", Some(9), Charge::Discharging), pad("b", Some(14), Charge::Discharging)];
        assert_eq!(l.check(&later), vec!["b".to_string()]);
        // A pad that disconnects is forgotten: back at 10 later, it is told again.
        assert!(l.check(&[]).is_empty());
        assert_eq!(l.check(&[pad("a", Some(10), Charge::Discharging)]), vec!["a".to_string()]);
    }
}
