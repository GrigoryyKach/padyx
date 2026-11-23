# Padyx

A DualSense tester and helper for Windows and Linux. It shows every control of a PlayStation DualSense live, finds stick drift, and sets the lightbar, player LEDs, rumble and adaptive triggers.

> Status: the tester and the pad setup are done. The Xbox controller mode (ViGEmBus and HidHide on Windows, uinput on Linux), dead zones, remapping, macros and per-game profiles are added in the same 1.0.0 and are described here when they land.

![Overview](assets/screenshots/dark-en-overview.png)

## What it does

- **Overview.** A drawn DualSense lights up as you press it. Triggers fill by value, sticks show their position, the touchpad shows both fingers. Connection (USB or Bluetooth), battery and the measured polling rate are always on top.
- **Sticks and drift.** A large plot per stick with its trail and the 5 % and 10 % rings. A five second no-touch drift test gives the resting offset and jitter with a verdict. A circularity test shows the outer shape of the travel and its average error.
- **Triggers.** Live values and a test of the adaptive effects (resistance, weapon, vibration).
- **Buttons and timing.** A checklist of 17 controls that tick as you press them, and the interval between reports with its jitter.
- **Touchpad and motion.** The touchpad surface with two fingers, the gyroscope and accelerometer (with the calibration read from the pad) and a small orientation glyph.
- **Lightbar and rumble.** The NIMVORO palette or a custom RGB colour, brightness, player LEDs, the mute button light and both rumble motors. The colour is saved and applied every time the pad connects.
- **Settings.** Language (English, Russian, Ukrainian), theme, start with the computer (off by default), updates and about.

Padyx has two parts in one program: a small background part that reads the pad and keeps the tray icon (it draws nothing), and the window, which starts when you open Padyx and closes completely when you close it.

## Pads

| Pad | Connection | State |
|---|---|---|
| DualSense | USB, Bluetooth | supported, the target |
| DualShock 4 | USB, Bluetooth | parsed (sticks, buttons, triggers, motion, battery), **untested** |

Over Bluetooth a DualSense starts in a reduced mode with few fields. Padyx reads the pad's calibration report, which switches it to the full report, and writes output reports with the CRC32 the pad requires.

## Linux

- Padyx reads the pad through hidraw. The kernel's `hid-playstation` driver stays bound, and for the lightbar Padyx prefers the kernel LED class (`/sys/class/leds/*:rgb:indicator`), so the two do not fight.
- A normal user may not open hidraw devices. On the first run, if a pad is there but cannot be opened, Padyx shows one screen that explains this and offers to install a udev rule (`/etc/udev/rules.d/70-padyx.rules`, `TAG+="uaccess"` for the Sony devices) through `pkexec`. Nothing is installed without that prompt. The same step is in Settings.
- The tray icon needs GTK 3 and libayatana-appindicator3. GNOME also needs the AppIndicator extension.
- Build dependencies: `libgtk-3-dev libayatana-appindicator3-dev libxkbcommon-dev libwayland-dev libx11-dev libxcb1-dev libxdo-dev libudev-dev`.

## Build

```
cargo build --release
cargo test
```

On Windows the GNU and MSVC toolchains both work. `Padyx --demo` runs the window with a scripted fake pad and no hardware (set `PADYX_DATA_DIR` to a temporary folder first). `Padyx --list` prints the Sony pads the system sees and exits; it only enumerates and never writes to a pad.

Environment variables for tests and screenshots: `PADYX_DATA_DIR`, `PADYX_DEMO=1`, `PADYX_DEMO_T` (start of the script, seconds), `PADYX_DEMO_HOLD=1` (freeze the script), `PADYX_LANG`, `PADYX_THEME`. `tools/screenshots.ps1` drives the window through the pages and saves a PNG of each.

## Report layouts

The parsers and writers are written from the public descriptions of the reports (the Linux `hid-playstation` driver notes, SDL's PS5 and PS4 HIDAPI drivers, DS4Windows). The byte layouts are documented at the top of `src/pad/parse.rs` and `src/pad/output.rs`, and the tests use synthetic fixtures built from those layouts.

## Support

If Padyx is useful to you, you can support its development: https://donatello.to/MicsGame

![QR code](assets/support-qr.png)

## License

MIT, see [LICENSE](LICENSE). The IBM Plex fonts are under the SIL Open Font License, see `assets/fonts/OFL.txt`.
