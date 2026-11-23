#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Padyx: a DualSense tester and helper.
//!
//! One executable, two processes:
//! - the **background part** (`--background`, started with the computer if you ask for it) reads the pad, writes
//!   the lightbar and owns the tray icon. It draws nothing and uses almost no memory;
//! - the **window** (the default) is the tester. It starts when you open Padyx and ends when you close it.
//!
//! `--demo` (or `PADYX_DEMO=1`) runs the window with a scripted fake pad and no background part, for screenshots.
//! `--list` prints the Sony pads the system sees (read-only) and exits.

mod app;
mod bus;
mod config;
mod daemon;
mod emu;
#[allow(dead_code)]
mod i18n;
mod ipc;
#[allow(dead_code)]
mod linux_setup;
#[allow(dead_code)]
mod nimvoro;
mod pad;
#[allow(dead_code)]
mod selfupdate;
mod source;
mod strings;
mod support;
#[allow(dead_code)]
mod theme;
mod tray_common;
#[allow(dead_code)]
mod update_ui;
#[cfg(windows)]
mod platform;
#[cfg(not(windows))]
#[path = "platform_linux.rs"]
mod platform;
#[cfg(windows)]
mod tray;
#[cfg(not(windows))]
#[path = "tray_linux.rs"]
mod tray;
#[cfg(windows)]
mod winloop;

use config::Config;
use eframe::egui;

/// The repository, kept in one place (About page and the update check).
pub const REPO_URL: &str = "https://github.com/GrigoryyKach/padyx";
pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn main() {
    selfupdate::startup("padyx");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = config::data_dir();
    // The release build has no console of its own: print into the one it was started from.
    #[cfg(windows)]
    if args.iter().any(|a| a == "--list" || a == "--ipc") {
        // SAFETY: plain Win32 call; failing (no parent console) is fine.
        unsafe { windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS) };
    }
    // `--list`: what is connected, read-only.
    if args.iter().any(|a| a == "--list") {
        for l in pad::hub::enumerate() {
            println!("{l}");
        }
        return;
    }
    // `--ipc <request>`: asks the running background part and prints its answer (used by tests).
    if let Some(i) = args.iter().position(|a| a == "--ipc") {
        match ipc::request("daemon", &args[i + 1..].join(" ")) {
            Some(r) => println!("{r}"),
            None => println!("not running"),
        }
        return;
    }
    let demo = args.iter().any(|a| a == "--demo") || std::env::var("PADYX_DEMO").is_ok_and(|v| v == "1");
    if args.iter().any(|a| a == "--background" || a == "--minimized") {
        let cfg = Config::load_from(&dir);
        // Nothing runs before the first-run screen has been confirmed (a demo run in a test folder is the exception).
        if !cfg.onboarded && !(demo && config::is_test_instance()) {
            return;
        }
        daemon::run(dir, cfg, demo);
    } else if let Err(e) = run_window(dir, demo) {
        eprintln!("padyx: {e}");
    }
}

/// Asks the background part to quit and waits a moment for it to leave (before an update replaces the files).
fn stop_background() {
    if ipc::request("daemon", "quit").is_some() {
        for _ in 0..30 {
            if !ipc::is_running("daemon") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

fn run_window(dir: std::path::PathBuf, demo: bool) -> eframe::Result<()> {
    // One window per data folder: a second launch brings the first one forward.
    let served = ipc::serve("ui", |m| {
        if m == "show" {
            bus::push(bus::Cmd::Show);
        }
        "ok".into()
    });
    if !served {
        platform::allow_foreground();
        ipc::request("ui", "show");
        return Ok(());
    }

    let cfg = Config::load_from(&dir);
    // The window starts the background part if it is not running (never for the demo, which has its own pad).
    if cfg.onboarded && !demo && std::env::var_os("PADYX_NO_BACKGROUND").is_none() && !ipc::is_running("daemon") {
        platform::spawn_background();
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(platform::WINDOW_TITLE)
        .with_app_id("local.padyx")
        .with_min_inner_size([960.0, 620.0])
        .with_icon(std::sync::Arc::new(app_icon()));
    let mut centered = true;
    match cfg.window {
        Some(w) if w.w >= 960.0 && w.h >= 620.0 => {
            viewport = viewport.with_inner_size([w.w, w.h]);
            if let (Some(x), Some(y)) = (w.x, w.y) {
                if x > -20000.0 && y > -20000.0 && x < 20000.0 && y < 20000.0 {
                    viewport = viewport.with_position([x, y]);
                    centered = false;
                }
            }
        }
        _ => viewport = viewport.with_inner_size([1180.0, 760.0]),
    }
    // Screenshots may ask for a size (test folder only): PADYX_WINDOW=1280x900.
    if config::is_test_instance() {
        if let Some((w, h)) = std::env::var("PADYX_WINDOW").ok().and_then(|v| v.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?)))) {
            viewport = viewport.with_inner_size([w, h]);
        }
    }
    let options = eframe::NativeOptions { viewport, centered, ..Default::default() };
    let updater_stop: fn() = stop_background;
    eframe::run_native(platform::WINDOW_TITLE, options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, cfg, dir, demo, updater_stop)))))
}

/// Window icon from `assets/icon.png` (the same artwork that is embedded in the exe).
fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(ICON_PNG).unwrap_or_default()
}
