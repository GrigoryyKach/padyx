//! The window: state, the per-frame logic, navigation and the pad it shows. Pages live in the sibling modules.
//!
//! The window is a client of the background process (see `source`): it never opens the pad itself. What it wants
//! the pad to do (lightbar, rumble, trigger effects) is kept in `App::out` and pushed through the source.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, Frame, Id, Key, Margin, Modifiers, Order, Rect, Sense, Stroke, Ui};

use crate::bus::{self, Cmd};
use crate::config::{Config, Window};
use crate::i18n::{self, Lang};
use crate::nimvoro::Updater;
use crate::pad::hub::Hub;
use crate::pad::output::{Output, Trigger};
use crate::pad::{Conn, PadKind, PadSnapshot, Snapshot};
use crate::source::Source;
use crate::strings::{self, S};
use crate::theme::{self, Pal};

mod buttons;
mod draw;
mod emulation;
mod light;
mod motion;
mod onboarding;
mod overview;
mod settings;
mod sticks;
mod triggers;
pub mod widgets;

/// The app accent: violet, away from Smart Switcher's indigo and the other suite accents.
pub const ACCENT: theme::Accent = theme::Accent::new(0xab7cf0, 0x7440d8, 0x141415, 0xffffff);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Overview,
    Sticks,
    Triggers,
    Buttons,
    Motion,
    Light,
    Emulation,
    Settings,
}

impl Nav {
    const ALL: [Nav; 8] = [Nav::Overview, Nav::Sticks, Nav::Triggers, Nav::Buttons, Nav::Motion, Nav::Light, Nav::Emulation, Nav::Settings];

    fn from_name(s: &str) -> Option<Nav> {
        Some(match s {
            "overview" => Nav::Overview,
            "sticks" => Nav::Sticks,
            "triggers" => Nav::Triggers,
            "buttons" => Nav::Buttons,
            "motion" => Nav::Motion,
            "light" => Nav::Light,
            "emulation" => Nav::Emulation,
            "settings" => Nav::Settings,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Nav::Overview => "overview",
            Nav::Sticks => "sticks",
            Nav::Triggers => "triggers",
            Nav::Buttons => "buttons",
            Nav::Motion => "motion",
            Nav::Light => "light",
            Nav::Emulation => "emulation",
            Nav::Settings => "settings",
        }
    }
}

pub struct Toast {
    pub text: String,
    pub until: f64,
}

/// Screenshot driver (set by `PADYX_SHOT_DIR`): walks through the screens, saves a PNG of each and quits.
struct Shots {
    dir: PathBuf,
    prefix: String,
    queue: Vec<Nav>,
    wait: u32,
    wait_frames: u32,
    asked: bool,
    done: Arc<AtomicBool>,
}

pub struct App {
    pub cfg: Config,
    pub lang: Lang,
    pub dir: PathBuf,
    pub source: Source,
    pub snap: Snapshot,
    /// The pad the pages show (`None` means the first one).
    pub uid: Option<String>,
    /// What the window wants the selected pad to do. Pushed to the source on every change.
    pub out: Output,
    out_for: Option<String>,
    pub nav: Nav,
    pub updater: Updater,
    pub demo: bool,
    pub toast: Option<Toast>,
    pub sticks_view: sticks::State,
    pub buttons_view: buttons::State,
    pub light_view: light::State,
    pub triggers_view: triggers::State,
    pub emulation_view: emulation::State,
    pub settings_view: settings::State,
    pub onboarding: onboarding::State,
    pub setup_msg: Option<String>,
    cfg_dirty: Option<f64>,
    shots: Option<Shots>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config, dir: PathBuf, demo: bool, stop_background: fn()) -> Self {
        // Screenshots and tests may force the language and theme without touching the saved settings.
        if crate::config::is_test_instance() {
            if let Ok(l) = std::env::var("PADYX_LANG") {
                cfg.language = Some(l);
            }
            if let Some(m) = std::env::var("PADYX_THEME").ok().and_then(|t| match t.as_str() {
                "dark" => Some(theme::ThemeMode::Dark),
                "light" => Some(theme::ThemeMode::Light),
                _ => None,
            }) {
                cfg.theme = m;
            }
            // A demo run in a test folder skips the first-run screen.
            if demo && std::env::var_os("PADYX_SHOW_ONBOARDING").is_none() {
                cfg.onboarded = true;
            }
        }
        theme::set_accent(ACCENT);
        theme::install(&cc.egui_ctx, cfg.theme);
        cc.egui_ctx.all_styles_mut(|s| s.spacing.interact_size.y = widgets::ROW_BTN_H);
        let lang = i18n::resolve(cfg.language.as_deref());
        let mut cfg_dirty = None;
        if cfg.language.as_deref() != Some(lang.code()) {
            cfg.language = Some(lang.code().to_string());
            cfg_dirty = Some(0.0);
        }
        bus::init(cc.egui_ctx.clone());
        let source = if demo {
            let hub = Hub::new(cfg.output_template());
            // The demo window owns its pad, so it owns the emulation too (the demo pad is never hidden).
            crate::emu::attach_system(&hub, cfg.emu.clone(), &dir);
            hub.start_demo();
            Source::local(hub)
        } else {
            Source::remote(cc.egui_ctx.clone())
        };
        let updater = Updater::new("padyx", crate::REPO_URL.trim_start_matches("https://github.com/"), env!("CARGO_PKG_VERSION"))
            .relaunch_args(&["--background"])
            .before_restart(stop_background);
        if cfg.check_updates && !crate::config::is_test_instance() {
            let ctx = cc.egui_ctx.clone();
            updater.check(move || ctx.request_repaint());
        }
        let shots = std::env::var_os("PADYX_SHOT_DIR").map(|d| {
            let names = std::env::var("PADYX_SHOT_NAVS").unwrap_or_else(|_| "overview,sticks,triggers,buttons,motion,light,emulation,settings".into());
            Shots {
                dir: PathBuf::from(d),
                prefix: std::env::var("PADYX_SHOT_PREFIX").unwrap_or_else(|_| "shot".into()),
                queue: names.split(',').filter_map(Nav::from_name).collect(),
                wait: 0,
                wait_frames: std::env::var("PADYX_SHOT_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(40),
                asked: false,
                done: Arc::new(AtomicBool::new(false)),
            }
        });
        let first = shots.as_ref().and_then(|s| s.queue.first().copied()).unwrap_or(Nav::Overview);
        let out = cfg.output_template();
        let mut app = Self {
            lang,
            dir,
            source,
            snap: Snapshot::default(),
            uid: None,
            out,
            out_for: None,
            nav: first,
            updater,
            demo,
            toast: None,
            sticks_view: Default::default(),
            buttons_view: Default::default(),
            light_view: Default::default(),
            triggers_view: Default::default(),
            emulation_view: Default::default(),
            settings_view: settings::State::new(),
            onboarding: onboarding::State { autostart: false },
            setup_msg: None,
            cfg_dirty,
            shots,
            cfg,
        };
        // Screenshots of a finished test: the test starts by itself (`PADYX_AUTOTEST=drift` or `circ`).
        match std::env::var("PADYX_AUTOTEST").ok().as_deref() {
            Some("drift") if crate::config::is_test_instance() => app.sticks_view.start_drift(0.0),
            Some("circ") if crate::config::is_test_instance() => app.sticks_view.start_circ(),
            _ => {}
        }
        app
    }

    pub fn s(&self) -> &'static S {
        strings::TABLE.get(self.lang)
    }

    /// The pad the pages show.
    pub fn pad(&self) -> Option<&PadSnapshot> {
        match &self.uid {
            Some(u) => self.snap.pads.iter().find(|p| &p.info.uid == u).or_else(|| self.snap.pads.first()),
            None => self.snap.pads.first(),
        }
    }

    /// Sends `self.out` to the selected pad.
    pub fn push_out(&self) {
        if let Some(p) = self.pad() {
            self.source.send(&p.info.uid, &self.out);
        }
    }

    /// Takes the pad's own output as the starting point when the selected pad changes.
    fn sync_out(&mut self) {
        let Some(p) = self.pad() else { return };
        let uid = p.info.uid.clone();
        if self.out_for.as_deref() != Some(uid.as_str()) {
            self.out = p.out.clone();
            self.out_for = Some(uid);
        }
    }

    /// Switches every transient output off (trigger effects, rumble) when the page that made them is left.
    fn calm_outputs(&mut self) {
        let busy = self.out.is_active();
        self.out.rumble_left = 0;
        self.out.rumble_right = 0;
        self.out.trigger_left = Trigger::Off;
        self.out.trigger_right = Trigger::Off;
        self.triggers_view = Default::default();
        self.light_view.rumble_touched = false;
        if busy {
            self.push_out();
        }
    }

    // ───────────────────────── settings ─────────────────────────

    pub fn touch_cfg(&mut self, ctx: &egui::Context) {
        self.cfg_dirty = Some(ctx.input(|i| i.time));
        ctx.request_repaint_after(Duration::from_millis(450));
    }

    /// Writes the settings and tells the background part to re-read them.
    pub fn flush(&mut self) -> bool {
        if self.cfg_dirty.take().is_some() {
            let ok = self.cfg.save_to(&self.dir);
            self.source.reload();
            return ok;
        }
        true
    }

    #[allow(dead_code)]
    pub fn notify(&mut self, ctx: &egui::Context, text: String) {
        let now = ctx.input(|i| i.time);
        self.toast = Some(Toast { text, until: now + 4.0 });
    }

    pub fn go(&mut self, nav: Nav) {
        if self.nav != nav && matches!(self.nav, Nav::Triggers | Nav::Light) {
            self.calm_outputs();
        }
        self.nav = nav;
    }

    fn track_window(&mut self, ctx: &egui::Context) {
        let (inner, outer, minimized, maximized) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().outer_rect, i.viewport().minimized, i.viewport().maximized));
        if minimized == Some(true) || maximized == Some(true) {
            return;
        }
        if let (Some(inner), Some(outer)) = (inner, outer) {
            let w = Window { w: inner.width(), h: inner.height(), x: Some(outer.min.x), y: Some(outer.min.y) };
            let same = self.cfg.window.is_some_and(|c| (c.w - w.w).abs() < 1.0 && (c.h - w.h).abs() < 1.0 && c.x == w.x && c.y == w.y);
            if !same && w.w > 300.0 && w.h > 300.0 && self.shots.is_none() {
                self.cfg.window = Some(w);
                self.touch_cfg(ctx);
            }
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let ctrl = Modifiers::COMMAND;
        for (i, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8].into_iter().enumerate() {
            if ctx.input_mut(|inp| inp.consume_key(ctrl, key)) {
                self.go(Nav::ALL[i]);
            }
        }
    }

    // ───────────────────────── frame ─────────────────────────

    fn drive_screenshots(&mut self, ctx: &egui::Context) {
        let nav_name = self.nav.name();
        let Some(sh) = self.shots.as_mut() else { return };
        ctx.request_repaint();
        if sh.done.swap(false, Ordering::Relaxed) {
            sh.queue.remove(0);
            sh.asked = false;
            sh.wait = 0;
            match sh.queue.first() {
                Some(n) => self.nav = *n,
                None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
            return;
        }
        sh.wait += 1;
        if sh.wait > sh.wait_frames && !sh.asked {
            sh.asked = true;
            let path = sh.dir.join(format!("{}-{}.png", sh.prefix, nav_name));
            let _ = std::fs::create_dir_all(&sh.dir);
            let done = sh.done.clone();
            ctx.request_screenshot(move |img| {
                let mut raw = Vec::with_capacity(img.pixels.len() * 4);
                for px in &img.pixels {
                    raw.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
                }
                let _ = image::save_buffer(path, &raw, img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8);
                done.store(true, Ordering::Relaxed);
            });
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t = ctx.input(|i| i.time);
        for cmd in bus::drain() {
            match cmd {
                Cmd::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
        if self.updater.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.updater.should_exit() {
            self.flush();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.snap = self.source.snapshot();
        self.sync_out();
        if self.cfg.onboarded {
            // A live pad is drawn about 60 times a second; with nothing to show a slow tick is enough.
            let wait = if self.snap.pads.is_empty() { 500 } else { 16 };
            ctx.request_repaint_after(Duration::from_millis(wait));
        }
        if self.cfg_dirty.is_some_and(|d| t - d > 0.4) {
            self.flush();
        }
        if self.toast.as_ref().is_some_and(|x| t > x.until) {
            self.toast = None;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.calm_outputs();
            self.flush();
        }
        self.track_window(ctx);
        self.drive_screenshots(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = Pal::of(ui.visuals().dark_mode);
        if !self.cfg.onboarded {
            self.onboarding_view(ui, &p);
            return;
        }
        self.handle_keys(&ctx);

        egui::Panel::left("sidebar")
            .exact_size(232.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar).stroke(Stroke::NONE).inner_margin(Margin { left: 12, right: 12, top: 16, bottom: 12 }))
            .show(ui, |ui| self.sidebar(ui, &p));

        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0.0, p.bg);
        ui.painter().vline(area.left(), area.y_range(), Stroke::new(1.0, p.line));
        let inner = Rect::from_min_max(area.min + egui::vec2(28.0, 24.0), area.max - egui::vec2(28.0, 8.0));
        let mut cui = ui.new_child(egui::UiBuilder::new().id_salt("central").max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        cui.set_clip_rect(area);
        let pad = self.pad().cloned();
        match (self.nav, pad) {
            (Nav::Settings, _) => self.settings_view(&mut cui, &p),
            (Nav::Emulation, pad) => self.emulation_view(&mut cui, &p, pad.as_ref()),
            (_, None) => self.empty_view(&mut cui, &p),
            (Nav::Overview, Some(pad)) => self.overview_view(&mut cui, &p, &pad),
            (Nav::Sticks, Some(pad)) => self.sticks_view(&mut cui, &p, &pad),
            (Nav::Triggers, Some(pad)) => self.triggers_view(&mut cui, &p, &pad),
            (Nav::Buttons, Some(pad)) => self.buttons_view(&mut cui, &p, &pad),
            (Nav::Motion, Some(pad)) => self.motion_view(&mut cui, &p, &pad),
            (Nav::Light, Some(pad)) => self.light_view(&mut cui, &p, &pad),
        }
        self.toast_ui(&ctx, &p);
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // Leave the pad quiet: no rumble or trigger effect outlives the window.
        if self.out.is_active() {
            self.out.rumble_left = 0;
            self.out.rumble_right = 0;
            self.out.trigger_left = Trigger::Off;
            self.out.trigger_right = Trigger::Off;
            if let Some(p) = self.pad() {
                self.source.send_blocking(&p.info.uid, &self.out);
            }
        }
        self.flush();
    }
}

// ───────────────────────── shared UI pieces ─────────────────────────

/// A page: a 24 px title with an optional subtitle, a right-aligned header, then a scrolling body in a column
/// no wider than 1120 px.
pub fn page(ui: &mut Ui, p: &Pal, title: &str, subtitle: Option<&str>, right: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui)) {
    // The title and the body share one centred column, so they line up at every window width.
    let avail = ui.available_width();
    let avail_h = ui.available_height();
    let col_w = (avail - 14.0).clamp(300.0, 1120.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(((avail - 14.0 - col_w) / 2.0).max(0.0));
        ui.vertical(|ui| {
            ui.set_width(col_w);
            ui.set_height(avail_h);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(egui::RichText::new(title).font(theme::medium(24.0)).color(p.text));
                    if let Some(s) = subtitle {
                        theme::hint(ui, s, p);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
            });
            ui.add_space(16.0);
            let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if let Some(y) = test_scroll() {
                area = area.vertical_scroll_offset(y);
            }
            area.show(ui, |ui| {
                ui.set_width(col_w - 12.0);
                body(ui);
                ui.add_space(24.0);
            });
        });
    });
}

/// Screenshots of a long page: `PADYX_SCROLL=<pixels>` (test folder only) scrolls every page to that offset.
fn test_scroll() -> Option<f32> {
    static V: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *V.get_or_init(|| if crate::config::is_test_instance() { std::env::var("PADYX_SCROLL").ok().and_then(|v| v.parse().ok()) } else { None })
}

/// A titled card: a section title (16 above, 8 below) over a card with 16 px padding.
pub fn group<R>(ui: &mut Ui, p: &Pal, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(16.0);
    theme::section(ui, title, p);
    ui.add_space(8.0);
    theme::card(p).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    })
    .inner
}

impl App {
    fn sidebar(&mut self, ui: &mut Ui, p: &Pal) {
        let t = self.s();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (mark, _) = ui.allocate_exact_size(egui::vec2(28.0, 36.0), Sense::hover());
            widgets::mark_glyph(ui.painter(), mark.center(), p.accent);
            ui.label(egui::RichText::new("Padyx").font(theme::medium(18.0)).color(p.text));
        });
        ui.add_space(16.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let rows = [
            (Nav::Overview, widgets::Icon::Overview, t.nav_overview),
            (Nav::Sticks, widgets::Icon::Sticks, t.nav_sticks),
            (Nav::Triggers, widgets::Icon::Trigger, t.nav_triggers),
            (Nav::Buttons, widgets::Icon::Buttons, t.nav_buttons),
            (Nav::Motion, widgets::Icon::Motion, t.nav_motion),
            (Nav::Light, widgets::Icon::Light, t.nav_light),
            (Nav::Emulation, widgets::Icon::Emulation, t.nav_emulation),
        ];
        for (nav, ic, label) in rows {
            if nav_row(ui, p, ic, label, self.nav == nav).clicked() {
                self.go(nav);
            }
        }
        // Settings and the pad status sit at the bottom.
        let h = ui.available_height();
        ui.add_space((h - 36.0 - 40.0).max(8.0));
        self.pad_status_line(ui, p);
        if nav_row(ui, p, widgets::Icon::Gear, t.nav_settings, self.nav == Nav::Settings).clicked() {
            self.go(Nav::Settings);
        }
    }

    /// One quiet line: a coloured dot and whether a pad is there.
    fn pad_status_line(&self, ui: &mut Ui, p: &Pal) {
        let t = self.s();
        let (color, text) = match self.pad() {
            Some(pad) => (p.positive, pad.info.kind.name()),
            None if !self.source.connected() => (p.negative, t.bg_off_title),
            None => (p.warning, t.no_pad_title),
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::hover());
        ui.painter().circle_filled(egui::pos2(rect.left() + 20.0, rect.center().y), 4.0, color);
        let font = theme::reg(12.0);
        let shown = widgets::fit_text(ui.painter(), text, &font, rect.width() - 44.0);
        ui.painter().text(egui::pos2(rect.left() + 36.0, rect.center().y), egui::Align2::LEFT_CENTER, shown, font, p.text2);
    }

    /// The pages when no pad is there: why, and what to do.
    fn empty_view(&mut self, ui: &mut Ui, p: &Pal) {
        let t = self.s();
        let remote_down = !self.source.connected();
        let denied = self.snap.access_denied && cfg!(unix);
        let mut start = false;
        let mut install = false;
        let mut later = false;
        page(ui, p, t.nav_overview, None, |_| {}, |ui| {
            if denied && !self.cfg.linux_setup_seen {
                group(ui, p, t.setup_title, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(t.setup_body).font(theme::reg(14.0)).color(p.text2)).wrap());
                    ui.add_space(8.0);
                    theme::meta(ui, &format!("{}: {}", t.setup_file, crate::linux_setup::RULE_PATH), p);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if widgets::btn(ui, t.setup_install, theme::Btn::Primary, p).clicked() {
                            install = true;
                        }
                        if widgets::btn(ui, t.setup_later, theme::Btn::Secondary, p).clicked() {
                            later = true;
                        }
                    });
                    if let Some(m) = &self.setup_msg {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(m).font(theme::reg(13.0)).color(p.text2));
                    }
                });
                return;
            }
            ui.add_space(16.0);
            theme::card(p).inner_margin(Margin::same(24)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let (title, body) = if remote_down { (t.bg_off_title, t.bg_off_body) } else { (t.no_pad_title, t.no_pad_body) };
                ui.label(egui::RichText::new(title).font(theme::medium(18.0)).color(p.text));
                ui.add_space(8.0);
                ui.add(egui::Label::new(egui::RichText::new(body).font(theme::reg(14.0)).color(p.text2)).wrap());
                if remote_down {
                    ui.add_space(16.0);
                    if widgets::btn(ui, t.bg_start, theme::Btn::Primary, p).clicked() {
                        start = true;
                    }
                }
            });
        });
        if start {
            crate::platform::spawn_background();
        }
        if install {
            let ctx = ui.ctx().clone();
            match crate::linux_setup::install(&self.dir) {
                Ok(()) => {
                    self.cfg.linux_setup_seen = true;
                    self.setup_msg = Some(t.setup_done.to_string());
                    self.touch_cfg(&ctx);
                }
                Err(e) => self.setup_msg = Some(format!("{}: {e}", t.setup_failed)),
            }
        }
        if later {
            let ctx = ui.ctx().clone();
            self.cfg.linux_setup_seen = true;
            self.touch_cfg(&ctx);
        }
    }

    /// The top strip of a pad page: which pad, how it is connected, battery and polling rate.
    pub fn status_strip(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let t = self.s();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            if self.snap.pads.len() > 1 {
                let cur = pad.info.uid.clone();
                let label = pad_label(t, pad);
                widgets::combo("pad-picker", &label, 190.0).show_ui(ui, |ui| {
                    for q in &self.snap.pads {
                        if ui.selectable_label(q.info.uid == cur, egui::RichText::new(pad_label(t, q)).font(theme::reg(13.5))).clicked() {
                            self.uid = Some(q.info.uid.clone());
                        }
                    }
                });
            } else {
                chip(ui, p, t.pad_label, &pad_label(t, pad));
            }
            let conn = match pad.state.conn {
                Conn::Usb => t.conn_usb,
                Conn::Bluetooth => t.conn_bt,
                Conn::Unknown => t.conn_unknown,
            };
            chip(ui, p, "", conn);
            let bat = match (pad.state.battery, pad.state.charge) {
                (Some(b), crate::pad::Charge::Charging) => format!("{b} %, {}", t.charging),
                (Some(_), crate::pad::Charge::Full) => t.full.to_string(),
                (Some(b), _) => format!("{b} %"),
                (None, _) => t.no_battery.to_string(),
            };
            chip(ui, p, t.battery, &bat);
            chip(ui, p, t.polling, &format!("{:.0} Hz", pad.timing.hz));
        });
        if pad.state.reduced {
            ui.add_space(6.0);
            theme::hint(ui, t.reduced_note, p);
        }
        ui.add_space(12.0);
    }
}

pub fn pad_label(t: &S, pad: &PadSnapshot) -> String {
    let mut s = if pad.info.demo { t.demo_pad.to_string() } else { pad.info.kind.name().to_string() };
    if pad.info.kind == PadKind::DualShock4 {
        s.push_str(&format!(" ({})", t.untested));
    }
    s
}

/// A small label and a value in mono, side by side with a clear gap.
pub fn chip(ui: &mut Ui, p: &Pal, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if !label.is_empty() {
            ui.label(egui::RichText::new(label).font(theme::reg(13.0)).color(p.text2));
        }
        ui.label(egui::RichText::new(value).font(theme::mono(13.0)).color(p.text));
    });
}

/// One navigation row.
fn nav_row(ui: &mut Ui, p: &Pal, ic: widgets::Icon, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    if selected {
        ui.painter().rect(rect, theme::R_CONTROL as f32, widgets::selected_fill(p), Stroke::new(1.0, p.line_hi), egui::StrokeKind::Inside);
        ui.painter().hline(rect.x_range().shrink(8.0), rect.top() + 0.5, Stroke::new(1.0, p.top_light()));
    } else {
        ui.painter().rect_filled(rect, theme::R_CONTROL as f32, widgets::hover_fill(ui, resp.id, resp.hovered(), p));
    }
    let col = if selected { p.text } else { p.text2 };
    widgets::icon(ui.painter(), ic, egui::pos2(rect.left() + 20.0, rect.center().y), 16.0, if selected { p.text } else { p.muted });
    let font = if selected { theme::medium(14.0) } else { theme::reg(14.0) };
    let shown = widgets::fit_text(ui.painter(), label, &font, rect.width() - 52.0);
    ui.painter().text(egui::pos2(rect.left() + 36.0, rect.center().y), egui::Align2::LEFT_CENTER, shown, font, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl App {
    fn toast_ui(&mut self, ctx: &egui::Context, p: &Pal) {
        let Some(toast) = &self.toast else { return };
        let text = toast.text.clone();
        let dark = Pal::dark();
        egui::Area::new(Id::unique("toast")).order(Order::Tooltip).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -22.0)).show(ctx, |ui| {
            Frame::new()
                .fill(dark.raised)
                .stroke(Stroke::new(1.0, dark.line_hi))
                .corner_radius(theme::R_CARD)
                .inner_margin(Margin::symmetric(16, 8))
                .shadow(theme::shadow_for(p))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(&text).font(theme::reg(14.0)).color(dark.text));
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_is_readable_in_both_themes() {
        theme::set_accent(ACCENT);
        assert!(theme::accent_problems().is_empty(), "{:?}", theme::accent_problems());
    }

    #[test]
    fn accent_is_distinct_from_smart_switcher_indigo() {
        // Hue distance from #5b78ff (hue 229) must be large; ours is violet at about 264.
        fn hue(c: u32) -> f32 {
            let (r, g, b) = (((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0);
            let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
            let d = mx - mn;
            let h = if mx == r { ((g - b) / d).rem_euclid(6.0) } else if mx == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
            h * 60.0
        }
        assert!((hue(0xab7cf0) - hue(0x5b78ff)).abs() >= 30.0);
        assert!((hue(0x7440d8) - hue(0x3f5bf0)).abs() >= 30.0);
    }

    #[test]
    fn nav_names_round_trip() {
        for n in Nav::ALL {
            assert_eq!(Nav::from_name(n.name()), Some(n));
        }
        assert_eq!(Nav::from_name("nope"), None);
    }
}
