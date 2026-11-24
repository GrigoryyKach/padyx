# Padyx

A DualSense tester and helper for Windows and Linux. It shows every control of a PlayStation DualSense live, finds stick drift, sets the lightbar, player LEDs, rumble and adaptive triggers, and makes the pad look like an Xbox 360 controller so games outside Steam see it.

![Overview](assets/screenshots/dark-en-overview.png)

## What it does

- **Overview.** A rendered DualSense, seen from above and a little from the trigger side, lights up as you press it. Buttons sink and glow, the sticks tilt, the triggers pull in and fill by value, the lightbar shows its colour and the touchpad shows both fingers. Connection (USB or Bluetooth), battery and the measured polling rate are always on top.
- **Sticks and drift.** A large plot per stick with its trail and the 5 % and 10 % rings. A five second no-touch drift test gives the resting offset and jitter with a verdict. A circularity test shows the outer shape of the travel and its average error.
- **Triggers.** Live values and a test of the adaptive effects (resistance, weapon, vibration).
- **Buttons and timing.** A checklist of 19 controls (the two triggers tick when pulled past half) that tick as you press them, and the interval between reports with its jitter.
- **Touchpad and motion.** The touchpad surface with two fingers, the gyroscope and accelerometer (with the calibration read from the pad) and a small orientation glyph.
- **Lightbar and rumble.** The NIMVORO palette or a custom RGB colour, brightness, player LEDs, the mute button light and both rumble motors. The colour is saved and applied every time the pad connects.
- **Xbox emulation.** A virtual Xbox 360 pad for every DualSense, with the real pad hidden from games while it is on. Dead zones (inner, outer and anti), response curves, trigger dead zones, the touchpad click and Mute as extra buttons, and the strength of the rumble games send back. A per-pad native mode for games that know the DualSense. See below.
- **Remap, macros, turbo and profiles.** Any DualSense control to any Xbox button, macros on a button or a chord, turbo from 5 to 30 Hz, and named profiles that can switch by the game in front. See below.
- **Settings.** Language (English, Russian, Ukrainian), theme, start with the computer (off by default), updates and about.

Padyx has two parts in one program: a small background part that reads the pad and keeps the tray icon (it draws nothing), and the window, which starts when you open Padyx and closes completely when you close it.

## Xbox emulation

Many games outside Steam only know Xbox controllers. With the emulation on, Padyx creates a virtual Xbox 360 pad for each DualSense and feeds it from the real one, so the game just sees an Xbox pad.

![Xbox emulation](assets/screenshots/dark-en-emulation.png)

![Xbox emulation, light theme](assets/screenshots/light-en-emulation.png)

![Dead zones and curves](assets/screenshots/dark-en-emulation-zones.png)

![Dead zones and curves, light theme](assets/screenshots/light-en-emulation-zones.png)

- **Mapping.** By position: Cross is A, Circle is B, Square is X, Triangle is Y, Create is View (Back), Options is Menu (Start), PS is Guide, L1/R1 are LB/RB, L3/R3 are the stick clicks, the D-pad is the D-pad, and the triggers are the triggers. The touchpad click is View by default and can be any Xbox button; Mute is unmapped by default and can be set too.
- **Dead zones and curves**, per stick: an inner dead zone that is round (not per axis), an outer one, and an anti dead zone that jumps over a game's own inner zone. The response curve is linear, smooth, aggressive, or custom with two points. The page shows the stick before and after, live, and the same rings on the "Sticks and drift" page while the emulation is on. Each trigger has an inner and an outer dead zone.
- **Rumble.** What a game sets through XInput goes to the DualSense motors, with an intensity slider.
- **Native mode**, per pad: no virtual pad and no hiding, for games that support the DualSense themselves.
- **Hiding the real pad.** While the emulation is on, the real DualSense is hidden from games (HidHide on Windows, an exclusive grab of its input devices on Linux), so a game never sees two pads. The pad is given back when the emulation is turned off, when Padyx exits, and at the next start if a run crashed. Settings and the emulation page have a **Restore pad visibility** button that turns the emulation off and undoes everything.
- **Tray.** The tooltip shows the battery, there is a low battery notice at 15 %, the menu has an Xbox emulation switch, and the icon is dimmed while the emulation is off.
- **Light on the machine.** With the window closed, the background part reads the pad and maps it with no queue and no polling sleeps; the CPU and memory numbers are in the notes below.

## Remap, macros, turbo and profiles

These work on the virtual Xbox pad, so they apply while the Xbox emulation is on. Everything they produce goes to the virtual pad and nowhere else: Padyx sends no keyboard or mouse input.

![Remap](assets/screenshots/dark-en-remap.png)

- **Remap.** Every DualSense control can go to any Xbox button, to a trigger, or to nothing. Besides the 17 buttons of the tester, the click of the touchpad counts separately for its **left and right halves**, the triggers count as buttons past a threshold you set, and each of the eight stick directions is a button too. Sticks can be swapped, inverted, used as the D-pad (eight directions) or switched off. The dead zones stay with the physical stick, so they are applied before any of this. A control lights up on the page while you press it.
- **Macros.** A trigger (one control, or a chord such as Create + L1) starts a list of steps: press, hold for some milliseconds, release, wait. Modes: once, while the trigger is held, or a toggled loop. A step editor builds the list. The trigger controls can be held back from the game while the chord is down.
- **Turbo.** Any Xbox button, 5 to 30 presses a second while you hold it. It follows the remap: it pulses what the game would receive.
- **Profiles.** Named sets of the remap, dead zones, macros, turbo and, if you like, a lightbar colour. The default profile is always there. Pick one by hand, or turn on **switching by game**: Padyx reads the name of the program in front (twice a second, read only) and uses the first profile whose list of programs contains it, falling back to the one you picked. Profiles can be duplicated, exported to a JSON file and imported again (also by dropping the file on the window). The rumble strength and the native-mode pads are the same in every profile.

![Macros and turbo](assets/screenshots/dark-en-macros.png)

![Profiles](assets/screenshots/dark-en-profiles.png)

**Fair play.** Macros and turbo can break the rules of online games and get an account banned. Padyx says so on the page. Use them where it is allowed, such as single-player games and your own accessibility needs.

The macro clock is the pad's report stream (250 reports a second or more), so timings are accurate to a few milliseconds and a step shorter than that is rounded up to the next report.

### Drivers (Windows)

The emulation needs two free drivers by Nefarius Software Solutions. Padyx ships neither:

| Driver | What it does | Official page |
|---|---|---|
| ViGEmBus 1.22.0 | Makes the virtual Xbox 360 pad | https://github.com/nefarius/ViGEmBus/releases/tag/v1.22.0 |
| HidHide 1.5.230 | Hides the real DualSense from games | https://github.com/nefarius/HidHide/releases/tag/v1.5.230.0 |

The **Install** button on the Xbox emulation page downloads the official installer from that GitHub release, checks it against a SHA-256 that is built into Padyx, and only then starts it; Windows asks for permission, because only the installer runs with administrator rights, never Padyx. If the checksum does not match, the file is deleted and nothing runs. You can also install both yourself from the pages above. HidHide asks for a restart after it installs. Without HidHide the emulation still works, but games may see both the real and the virtual pad.

The built-in checksums are the `InstallerSha256` values of the winget manifests of these two files:

- ViGEmBus: `89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a`
- HidHide: `f4bbbcb82e6258641b887c74bc81c4c5f66e4aa811808dfc304347687b7605f6`

### Linux

- The virtual pad comes from **uinput** and looks like the kernel's own Xbox 360 pad (vendor `045e`, product `028e`, the `xpad` buttons and axes, force feedback of the rumble type), so SDL, Steam Input and Proton map it with no setup. It needs `/dev/uinput`: load the module with `sudo modprobe uinput` if it is missing. The udev rule that Padyx installs for the tester (one `pkexec` prompt) also gives your user access to `/dev/uinput` and to the pad's input devices.
- To hide the real pad, Padyx grabs its evdev nodes (`EVIOCGRAB`) while the emulation is on and lets go when it stops; the kernel also releases a grab if Padyx dies. A program that reads the DualSense through hidraw (Steam does) can still see it: use the native mode for such games.
- Wayland and X11 both work; the emulation does not use the display.
- **Switching profiles by game works on X11 only** (it reads `_NET_ACTIVE_WINDOW` and the window's process through `/proc`). A Wayland compositor does not tell other programs which window is in front, so on Wayland the page says the feature is unavailable and the profile you picked by hand stays. A Windows game under Wine shows up as the Wine process, often `wine64-preloader`; put that name in the profile's list.

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

## Numbers

Measured with the demo pad (250 reports a second, the sticks always moving, so every report changes the virtual pad), window closed, on the release build:

- Added latency of the emulation, from a parsed report to the virtual pad having it (find the pad, map, hand over): about 0.1 microseconds in the mapping and 6 to 8 microseconds in the ViGEmBus call (median), 45 microseconds at the 99th percentile; the reader thread does all of it, with no queue and no sleep in between. A change reaches XInput in about 11 microseconds (median) by a measurement from the call to XInput showing it.
- Remap, two macros and two turbo buttons all active add about 0.1 microseconds (median, 0.2 at the 99th percentile) to the mapping of one report; the whole path with the mock back end is 0.2 microseconds (median).
- CPU and memory of the background part over five minutes (12 logical cores): with the emulation on, 1.1 % of one core on average (0.09 % of the machine, 2.5 % at most) and a 14.4 MB working set (3.4 MB private); with it off, 0.4 % of one core and 14.0 MB. The virtual pad is only updated when a report changes it.

## Build

```
cargo build --release
cargo test
```

On Windows the GNU and MSVC toolchains both work. `cargo test -- --ignored --test-threads=1` also runs the live checks against an installed ViGEmBus (a synthetic pad, never your own; they need the driver and are skipped without it). `Padyx --demo` runs the window with a scripted fake pad and no hardware (set `PADYX_DATA_DIR` to a temporary folder first). `Padyx --list` prints the Sony pads the system sees and exits; it only enumerates and never writes to a pad.

Environment variables for tests and screenshots: `PADYX_DATA_DIR`, `PADYX_DEMO=1`, `PADYX_DEMO_T` (start of the script, seconds), `PADYX_DEMO_HOLD=1` (freeze the script), `PADYX_LANG`, `PADYX_THEME`, `PADYX_EMU_MOCK=1` (a test folder only: a simulated emulation, no driver is used and nothing is hidden), `PADYX_FG_MOCK=<path>` (a test folder only: pretend this program is in front, for the profile screenshots), `PADYX_EDIT_PROFILE=<name>` (a test folder only: the profile the pages edit) and `PADYX_SCROLL` (scroll the pages, for screenshots). `tools/screenshots.ps1` drives the window through the pages and saves a PNG of each.

## Report layouts

The parsers and writers are written from the public descriptions of the reports (the Linux `hid-playstation` driver notes, SDL's PS5 and PS4 HIDAPI drivers, DS4Windows). The byte layouts are documented at the top of `src/pad/parse.rs` and `src/pad/output.rs`, and the tests use synthetic fixtures built from those layouts.

## Support

If Padyx is useful to you, you can support its development: https://donatello.to/MicsGame

![QR code](assets/support-qr.png)

## Credits

- Controller render made from a royalty-free 3D model. Only the rendered pictures are part of Padyx (`assets/pad`); the model itself is not included.

## License

MIT, see [LICENSE](LICENSE). The IBM Plex fonts are under the SIL Open Font License, see `assets/fonts/OFL.txt`.
