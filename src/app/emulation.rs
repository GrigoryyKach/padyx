//! The Xbox emulation page: the switch, the drivers, the pads, dead zones and curves with a live before/after
//! plot, the extra buttons, the rumble strength and the "restore pad visibility" button.
//!
//! Everything the user changes here lives in `config.json` under `emu` (saved a moment later, then the
//! background part re-reads it); the on/off switch goes straight to whoever owns the pads.

use super::light::slider_row_w;
use super::settings::{combo_row, switch_row};
use super::{draw, group, page, pad_label, widgets, App};
use crate::emu::backend::BackendStatus;
use crate::emu::drivers::{self, Driver, DriverError, Phase, Shared};
use crate::emu::map::{pad_key, EmuConfig};
use crate::emu::xbox::XBtn;
use crate::emu::zones::{Curve, StickCfg};
use crate::emu::PadMode;
use crate::pad::PadSnapshot;
use crate::strings::S;
use crate::theme::{self, Btn, Pal};
use eframe::egui::{self, vec2, Align, Layout, Rect, RichText, Sense, Stroke, Ui};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct State {
    /// One download/installer state per driver: ViGEmBus, HidHide.
    pub phase: [Shared; 2],
    seen: Option<(Instant, drivers::DriversStatus)>,
    /// The switch as the user just set it, until the background part confirms (so the click feels instant).
    switched: Option<(bool, Instant)>,
    /// The answer of the last "restore" click: (text, ok).
    pub msg: Option<(String, bool)>,
}

impl State {
    fn drivers_status(&mut self) -> drivers::DriversStatus {
        if let Some((at, s)) = self.seen {
            if at.elapsed() < Duration::from_secs(2) {
                return s;
            }
        }
        let s = drivers::status();
        self.seen = Some((Instant::now(), s));
        s
    }
}

pub(super) fn xbtn_name(t: &S, b: XBtn) -> String {
    match b {
        XBtn::Up => t.xb_up,
        XBtn::Down => t.xb_down,
        XBtn::Left => t.xb_left,
        XBtn::Right => t.xb_right,
        XBtn::Start => t.xb_start,
        XBtn::Back => t.xb_back,
        XBtn::L3 => t.xb_l3,
        XBtn::R3 => t.xb_r3,
        XBtn::Guide => t.xb_guide,
        other => return other.label().to_string(),
    }
    .to_string()
}

fn xbtn_options(t: &S) -> Vec<(Option<XBtn>, String)> {
    std::iter::once((None, t.emu_unmapped.to_string())).chain(XBtn::ALL.iter().map(|b| (Some(*b), xbtn_name(t, *b)))).collect()
}

/// The response of a stick: raw magnitude on the bottom, what the game gets on the left, with a dot for now.
fn curve_plot(ui: &mut Ui, size: f32, p: &Pal, cfg: &StickCfg, live: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(rect, 10.0, p.input, Stroke::new(1.0, p.line), egui::StrokeKind::Inside);
    let inner = rect.shrink(14.0);
    let at = |x: f32, y: f32| egui::pos2(inner.left() + x * inner.width(), inner.bottom() - y * inner.height());
    painter.line_segment([at(0.0, 0.0), at(1.0, 1.0)], Stroke::new(1.0, p.line));
    let pts: Vec<egui::Pos2> = (0..=64).map(|i| {
        let x = i as f32 / 64.0;
        at(x, cfg.magnitude(x))
    }).collect();
    painter.add(egui::Shape::line(pts, Stroke::new(2.0, p.accent)));
    let x = live.clamp(0.0, 1.0);
    painter.circle_filled(at(x, cfg.magnitude(x)), 4.5, p.text);
}

fn mini_bar(ui: &mut Ui, p: &Pal, label: &str, v: u8) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (lr, _) = ui.allocate_exact_size(vec2(96.0, 14.0), Sense::hover());
        ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, label, theme::reg(12.5), p.text2);
        let (rect, _) = ui.allocate_exact_size(vec2((ui.available_width() - 56.0).max(60.0), 14.0), Sense::hover());
        ui.painter().rect(rect, 7.0, p.input, Stroke::new(1.0, p.line), egui::StrokeKind::Inside);
        let w = rect.width() * v as f32 / 255.0;
        if w > 1.0 {
            ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())).shrink(1.0), 6.0, p.accent);
        }
        ui.label(RichText::new(format!("{v:>3}")).font(theme::mono(12.5)).color(p.text));
    });
}

/// The sliders and the curve picker of one stick.
fn stick_controls(ui: &mut Ui, p: &Pal, t: &S, which: usize, sc: &mut StickCfg, lw: f32) {
    let mut v = sc.inner * 100.0;
    if slider_row_w(ui, p, t.emu_dz_inner, lw, &mut v, 0.0, 60.0, &pct) {
        sc.inner = v / 100.0;
    }
    let mut v = sc.outer * 100.0;
    if slider_row_w(ui, p, t.emu_dz_outer, lw, &mut v, 0.0, 30.0, &pct) {
        sc.outer = v / 100.0;
    }
    let mut v = sc.anti * 100.0;
    if slider_row_w(ui, p, t.emu_dz_anti, lw, &mut v, 0.0, 30.0, &pct) {
        sc.anti = v / 100.0;
    }
    ui.add_space(8.0);
    ui.label(RichText::new(t.emu_curve).font(theme::reg(13.0)).color(p.text2));
    ui.add_space(6.0);
    let names = [t.curve_linear, t.curve_smooth, t.curve_aggressive, t.curve_custom];
    let cur = Curve::ALL.iter().position(|c| *c == sc.curve).unwrap_or(0);
    ui.horizontal_wrapped(|ui| {
        if let Some(k) = widgets::segmented(ui, &format!("curve-{which}"), &names, cur, p) {
            sc.curve = Curve::ALL[k];
        }
    });
    if sc.curve == Curve::Custom {
        ui.add_space(8.0);
        for (n, pt) in [(1, &mut sc.p1), (2, &mut sc.p2)] {
            let name = t.emu_point_n.replace("{n}", &n.to_string());
            let mut x = pt.0 * 100.0;
            if slider_row_w(ui, p, &format!("{name}, {}", t.emu_point_in), lw, &mut x, 0.0, 100.0, &pct) {
                pt.0 = x / 100.0;
            }
            let mut y = pt.1 * 100.0;
            if slider_row_w(ui, p, &format!("{name}, {}", t.emu_point_out), lw, &mut y, 0.0, 100.0, &pct) {
                pt.1 = y / 100.0;
            }
        }
    }
    ui.add_space(4.0);
    theme::hint(ui, t.emu_curve_hint, p);
}

fn pct(v: f32) -> String {
    format!("{:>2.0} %", v)
}

impl App {
    pub(super) fn emulation_view(&mut self, ui: &mut Ui, p: &Pal, pad: Option<&PadSnapshot>) {
        let ctx = ui.ctx().clone();
        let t = self.s();
        let snap_emu = self.snap.emu.clone();
        let mut cfg: EmuConfig = self.edit_cfg();
        let editing = self.edit_name();
        let ds = self.emulation_view.drivers_status();
        let mock = snap_emu.simulated;
        let mut switch_to: Option<bool> = None;
        let mut install: Option<usize> = None;
        let mut restore = false;
        let mut install_rule = false;
        let pad_pos = pad.map(|p| (p.state.left(), p.state.right(), p.state.l2, p.state.r2));
        let pads: Vec<PadSnapshot> = self.snap.pads.clone();
        // The switch shows the click at once; the background part's answer replaces it a moment later.
        let mut on = snap_emu.enabled;
        if let Some((v, at)) = self.emulation_view.switched {
            if at.elapsed() < Duration::from_millis(1500) && v != snap_emu.enabled {
                on = v;
            } else {
                self.emulation_view.switched = None;
            }
        }
        let phases: [Phase; 2] = [self.emulation_view.phase[0].lock().map(|g| g.clone()).unwrap_or_default(), self.emulation_view.phase[1].lock().map(|g| g.clone()).unwrap_or_default()];
        let msg = self.emulation_view.msg.clone();
        let setup_msg = self.setup_msg.clone();

        page(ui, p, t.nav_emulation, Some(t.emu_intro), |_| {}, |ui| {
            super::remap::editing_chip(ui, p, t, &editing);
            // ── The switch
            ui.add_space(16.0);
            theme::card(p).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut v = on;
                if switch_row(ui, p, t.emu_switch, t.emu_switch_hint, &mut v) {
                    switch_to = Some(v);
                }
                let problem = match snap_emu.backend {
                    BackendStatus::Ready => None,
                    BackendStatus::DriverMissing => Some(t.emu_backend_missing),
                    BackendStatus::NoAccess => Some(t.emu_backend_noaccess),
                    BackendStatus::Unsupported => Some(t.emu_backend_unsupported),
                };
                if let (Some(msg), true) = (problem, on || snap_emu.enabled) {
                    ui.label(RichText::new(msg).font(theme::reg(13.0)).color(p.negative));
                }
                if mock {
                    ui.add_space(4.0);
                    theme::hint(ui, t.emu_simulated, p);
                }
            });

            // ── Drivers (Windows) or the uinput access (Linux)
            if cfg!(windows) || mock {
                group(ui, p, t.emu_drivers, |ui| {
                    let list: [(&'static Driver, bool, &str); 2] = [(&drivers::VIGEMBUS, ds.vigembus, t.emu_drv_vigem_hint), (&drivers::HIDHIDE, ds.hidhide, t.emu_drv_hidhide_hint)];
                    for (i, (d, installed, hint)) in list.into_iter().enumerate() {
                        if i > 0 {
                            theme::divider(ui, p);
                        }
                        let busy = matches!(phases[i], Phase::Downloading { .. } | Phase::Verifying);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            let right_w = 330.0;
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - right_w).max(200.0));
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(d.name).font(theme::medium(14.0)).color(p.text));
                                theme::hint(ui, hint, p);
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                if !installed && cfg!(windows) && !mock && widgets::btn_enabled(ui, t.emu_install, Btn::Primary, !busy, p).clicked() {
                                    install = Some(i);
                                }
                                if widgets::btn(ui, t.emu_official, Btn::Link, p).clicked() {
                                    ctx.open_url(egui::OpenUrl::new_tab(d.page));
                                }
                                let (txt, col) = if installed { (t.emu_installed, p.positive) } else { (t.emu_missing, p.warning) };
                                ui.label(RichText::new(txt).font(theme::medium(13.0)).color(col));
                            });
                        });
                        match &phases[i] {
                            Phase::Downloading { done, total } => {
                                ui.add_space(8.0);
                                let f = (*done as f32 / (*total).max(1) as f32).clamp(0.0, 1.0);
                                ui.label(RichText::new(t.emu_downloading.replace("{n}", &format!("{:.0}", f * 100.0))).font(theme::reg(13.0)).color(p.text2));
                                ui.add_space(4.0);
                                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                                ui.painter().rect_filled(rect, 3.0, p.line);
                                ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(rect.width() * f, 6.0)), 3.0, p.accent);
                                ctx.request_repaint_after(Duration::from_millis(100));
                            }
                            Phase::Verifying => {
                                ui.add_space(8.0);
                                ui.label(RichText::new(t.emu_verifying).font(theme::reg(13.0)).color(p.text2));
                                ctx.request_repaint_after(Duration::from_millis(100));
                            }
                            Phase::Launched => {
                                ui.add_space(8.0);
                                ui.label(RichText::new(t.emu_launched).font(theme::reg(13.0)).color(p.text2));
                                if d.needs_restart {
                                    theme::hint(ui, t.emu_restart_note, p);
                                }
                            }
                            Phase::Failed(e) => {
                                ui.add_space(8.0);
                                let text = match e {
                                    DriverError::HashMismatch { .. } => t.emu_hash_failed.to_string(),
                                    DriverError::Launch(m) => format!("{}: {m}", t.emu_launch_failed),
                                    other => format!("{}: {other}", t.emu_download_failed),
                                };
                                ui.add(egui::Label::new(RichText::new(text).font(theme::reg(13.0)).color(p.negative)).wrap());
                            }
                            Phase::Idle => {}
                        }
                    }
                    ui.add_space(12.0);
                    theme::hint(ui, t.emu_driver_note, p);
                });
            }
            if cfg!(unix) {
                group(ui, p, t.emu_linux_title, |ui| {
                    let (txt, col) = match snap_emu.backend {
                        BackendStatus::Ready => (t.emu_uinput_ready, p.positive),
                        BackendStatus::NoAccess => (t.emu_uinput_noaccess, p.warning),
                        _ => (t.emu_uinput_missing, p.negative),
                    };
                    ui.add(egui::Label::new(RichText::new(txt).font(theme::reg(14.0)).color(col)).wrap());
                    if snap_emu.backend == BackendStatus::NoAccess {
                        ui.add_space(12.0);
                        if widgets::btn(ui, t.setup_install, Btn::Secondary, p).clicked() {
                            install_rule = true;
                        }
                    }
                    if let Some(m) = &setup_msg {
                        ui.add_space(8.0);
                        ui.label(RichText::new(m).font(theme::reg(13.0)).color(p.text2));
                    }
                });
            }

            // ── Pads
            group(ui, p, t.emu_pads, |ui| {
                if pads.is_empty() {
                    ui.add(egui::Label::new(RichText::new(t.no_pad_body).font(theme::reg(14.0)).color(p.text2)).wrap());
                }
                for (i, ps) in pads.iter().enumerate() {
                    if i > 0 {
                        theme::divider(ui, p);
                    }
                    let key = pad_key(&ps.info.serial, &ps.info.uid);
                    let st = snap_emu.pads.iter().find(|e| e.uid == ps.info.uid);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        ui.label(RichText::new(pad_label(t, ps)).font(theme::medium(14.0)).color(p.text));
                        let (txt, col) = match (snap_emu.enabled, st.map(|s| s.mode)) {
                            (false, _) => (t.emu_pad_idle, p.muted),
                            (true, Some(PadMode::Emulated)) => (t.emu_pad_emulated, p.positive),
                            (true, Some(PadMode::Native)) => (t.emu_pad_native, p.text2),
                            (true, Some(PadMode::Failed)) => (t.emu_pad_failed, p.negative),
                            (true, _) => (t.emu_pad_starting, p.text2),
                        };
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(txt).font(theme::medium(13.0)).color(col));
                        });
                    });
                    if let (true, Some(s)) = (snap_emu.enabled, st) {
                        if let (PadMode::Failed, Some(e)) = (s.mode, &s.error) {
                            ui.add(egui::Label::new(RichText::new(e).font(theme::reg(12.5)).color(p.negative)).wrap());
                        }
                        if s.mode == PadMode::Emulated && (!ps.info.demo || mock) {
                            ui.add_space(2.0);
                            if s.hidden {
                                theme::hint(ui, t.emu_hidden, p);
                            } else {
                                let why = s.error.as_deref().map(|e| format!("{}: {e}", t.emu_hide_failed)).unwrap_or_else(|| t.emu_not_hidden.to_string());
                                ui.add(egui::Label::new(RichText::new(why).font(theme::reg(12.5)).color(p.warning)).wrap());
                            }
                        }
                    }
                    ui.add_space(10.0);
                    let mut native = cfg.is_native(&key);
                    if switch_row(ui, p, t.emu_native, t.emu_native_hint, &mut native) {
                        cfg.set_native(&key, native);
                    }
                }
            });

            // ── Sticks: dead zones, curve, live before/after
            for (which, title) in [(0usize, t.left_stick), (1, t.right_stick)] {
                group(ui, p, &format!("{} · {}", t.emu_zones, title), |ui| {
                    let sc: &mut StickCfg = if which == 0 { &mut cfg.left } else { &mut cfg.right };
                    let raw = pad_pos.map(|(l, r, _, _)| if which == 0 { l } else { r }).unwrap_or((0.0, 0.0));
                    let after = sc.apply(raw.0, raw.1);
                    // Wide windows put the sliders beside the two plots, narrow ones under them.
                    let wide = ui.available_width() > 860.0;
                    let live = (raw.0 * raw.0 + raw.1 * raw.1).sqrt();
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 20.0;
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            theme::meta(ui, t.emu_before, p);
                            draw::stick_plot(ui, 220.0, p, raw, &[], &[sc.inner], None, Some(after));
                        });
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            theme::meta(ui, t.emu_curve, p);
                            curve_plot(ui, 220.0, p, sc, live);
                        });
                        if wide {
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                stick_controls(ui, p, t, which, sc, 170.0);
                            });
                        }
                    });
                    ui.add_space(6.0);
                    theme::hint(ui, t.emu_zones_hint, p);
                    if !wide {
                        ui.add_space(12.0);
                        stick_controls(ui, p, t, which, sc, 200.0);
                    }
                });
            }

            // ── Triggers
            group(ui, p, t.emu_trig_zones, |ui| {
                for (which, title) in [(0usize, t.trig_left), (1, t.trig_right)] {
                    if which == 1 {
                        theme::divider(ui, p);
                    }
                    let tc = if which == 0 { &mut cfg.lt } else { &mut cfg.rt };
                    ui.label(RichText::new(title).font(theme::medium(14.0)).color(p.text));
                    ui.add_space(6.0);
                    let raw = pad_pos.map(|(_, _, l, r)| if which == 0 { l } else { r }).unwrap_or(0);
                    mini_bar(ui, p, t.emu_before, raw);
                    ui.add_space(4.0);
                    mini_bar(ui, p, t.emu_after, tc.apply(raw));
                    ui.add_space(10.0);
                    let mut v = tc.inner * 100.0;
                    if slider_row_w(ui, p, t.emu_dz_inner, 200.0, &mut v, 0.0, 60.0, &pct) {
                        tc.inner = v / 100.0;
                    }
                    let mut v = tc.outer * 100.0;
                    if slider_row_w(ui, p, t.emu_dz_outer, 200.0, &mut v, 0.0, 40.0, &pct) {
                        tc.outer = v / 100.0;
                    }
                }
            });

            // ── Extra buttons
            group(ui, p, t.emu_buttons, |ui| {
                let opts = xbtn_options(t);
                if let Some(v) = combo_row(ui, p, t.emu_touchpad_as, "emu-touchpad", cfg.touchpad, &opts, 220.0) {
                    cfg.touchpad = v;
                }
                if let Some(v) = combo_row(ui, p, t.emu_mute_as, "emu-mute", cfg.mute, &opts, 220.0) {
                    cfg.mute = v;
                }
                theme::hint(ui, t.emu_map_hint, p);
            });

            // ── Rumble
            group(ui, p, t.emu_rumble, |ui| {
                let mut v = cfg.rumble_pct as f32;
                if slider_row_w(ui, p, t.emu_rumble_intensity, 200.0, &mut v, 0.0, 100.0, &pct) {
                    cfg.rumble_pct = v.round() as u8;
                }
                ui.add_space(4.0);
                theme::hint(ui, t.emu_rumble_hint, p);
            });

            // ── Restore
            group(ui, p, t.emu_restore, |ui| {
                ui.add(egui::Label::new(RichText::new(t.emu_restore_hint).font(theme::reg(14.0)).color(p.text2)).wrap());
                ui.add_space(12.0);
                if widgets::btn(ui, t.emu_restore, Btn::Secondary, p).clicked() {
                    restore = true;
                }
                if let Some((m, ok)) = &msg {
                    ui.add_space(8.0);
                    ui.label(RichText::new(m).font(theme::reg(13.0)).color(if *ok { p.positive } else { p.negative }));
                }
            });
        });

        if let Some(v) = switch_to {
            self.emulation_view.switched = Some((v, Instant::now()));
            self.source.emu_enable(v);
        }
        if let Some(i) = install {
            let d: &'static Driver = if i == 0 { &drivers::VIGEMBUS } else { &drivers::HIDHIDE };
            let shared = self.emulation_view.phase[i].clone();
            drivers::set_phase(&shared, Phase::Downloading { done: 0, total: d.size });
            let c = ctx.clone();
            drivers::install(d, shared, move || c.request_repaint());
        }
        if restore {
            self.emulation_view.msg = Some(match self.source.emu_restore(&self.dir) {
                Ok(()) => (t.emu_restored.to_string(), true),
                Err(e) => (format!("{}: {e}", t.emu_restore_failed), false),
            });
            self.emulation_view.switched = Some((false, Instant::now()));
        }
        if install_rule {
            match crate::linux_setup::install(&self.dir) {
                Ok(()) => {
                    self.cfg.linux_setup_seen = true;
                    self.setup_msg = Some(t.setup_done.to_string());
                    self.touch_cfg(&ctx);
                }
                Err(e) => self.setup_msg = Some(format!("{}: {e}", t.setup_failed)),
            }
        }
        self.store_edit_cfg(cfg, &ctx);
    }
}
