//! The Macros and turbo page: a trigger (one input or a chord), a list of steps (press, hold, release, wait)
//! and a mode, plus turbo per button. Everything acts on the virtual Xbox pad only.
//!
//! The page edits the settings of the profile picked on the Profiles page (the default one if none).

use super::remap::{din_name, editing_chip, pick};
use super::settings::switch_row;
use super::{group, page, widgets, App};
use crate::emu::macros::{Macro, MacroMode, Step, Turbo, XOut, MAX_CHORD, MAX_MACROS, MAX_STEPS, TURBO_DEFAULT_HZ, TURBO_MAX_HZ, TURBO_MIN_HZ};
use crate::emu::remap::{self, DIn};
use crate::emu::xbox::XBtn;
use crate::pad::PadSnapshot;
use crate::strings::S;
use crate::theme::{self, Btn, Pal};
use eframe::egui::{self, vec2, Align, Layout, RichText, Sense, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Press,
    Hold,
    Release,
    Wait,
}

fn kind_of(s: &Step) -> Kind {
    match s {
        Step::Press(_) => Kind::Press,
        Step::Hold(..) => Kind::Hold,
        Step::Release(_) => Kind::Release,
        Step::Wait(_) => Kind::Wait,
    }
}

fn out_of(s: &Step) -> XOut {
    match s {
        Step::Press(o) | Step::Release(o) | Step::Hold(o, _) => *o,
        Step::Wait(_) => XOut::Btn(XBtn::A),
    }
}

fn ms_of(s: &Step) -> u32 {
    match s {
        Step::Hold(_, ms) | Step::Wait(ms) => *ms,
        _ => 100,
    }
}

fn make(kind: Kind, o: XOut, ms: u32) -> Step {
    match kind {
        Kind::Press => Step::Press(o),
        Kind::Hold => Step::Hold(o, ms),
        Kind::Release => Step::Release(o),
        Kind::Wait => Step::Wait(ms),
    }
}

fn xout_name(t: &S, o: XOut) -> String {
    match o {
        XOut::Btn(b) => super::emulation::xbtn_name(t, b),
        XOut::Lt => t.xb_lt.to_string(),
        XOut::Rt => t.xb_rt.to_string(),
    }
}

fn xout_options(t: &S) -> Vec<(XOut, String)> {
    XBtn::ALL.iter().map(|b| XOut::Btn(*b)).chain([XOut::Lt, XOut::Rt]).map(|o| (o, xout_name(t, o))).collect()
}

/// A small square icon button with a tooltip (move a step, remove something).
fn tool_btn(ui: &mut Ui, p: &Pal, ic: widgets::Icon, tip: &str, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let t = theme::fade(ui, resp.id.with("hover"), resp.hovered() && enabled);
    ui.painter().rect_filled(rect, theme::R_CONTROL, theme::alpha(p.raised, (t * 255.0) as u8));
    let col = if enabled { theme::mix(p.muted, p.text, t) } else { theme::alpha(p.muted, 90) };
    widgets::icon(ui.painter(), ic, rect.center(), 16.0, col);
    enabled && resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked()
}

enum Edit {
    Remove(usize),
    Up(usize),
    Down(usize),
}

fn apply_edit<T>(list: &mut Vec<T>, e: Option<Edit>) {
    match e {
        Some(Edit::Remove(i)) if i < list.len() => {
            list.remove(i);
        }
        Some(Edit::Up(i)) if i > 0 && i < list.len() => list.swap(i, i - 1),
        Some(Edit::Down(i)) if i + 1 < list.len() => list.swap(i, i + 1),
        _ => {}
    }
}

fn mode_name(t: &S, m: MacroMode) -> &'static str {
    match m {
        MacroMode::Once => t.macro_once,
        MacroMode::WhileHeld => t.macro_while,
        MacroMode::Toggle => t.macro_toggle,
    }
}

fn macro_card(ui: &mut Ui, p: &Pal, t: &S, idx: usize, m: &mut Macro, live: bool) -> bool {
    let mut remove = false;
    let din_opts: Vec<(DIn, String)> = DIn::ALL.iter().map(|d| (*d, din_name(t, *d))).collect();
    let xopts = xout_options(t);
    let kinds = [(Kind::Press, t.step_press.to_string()), (Kind::Hold, t.step_hold.to_string()), (Kind::Release, t.step_release.to_string()), (Kind::Wait, t.step_wait.to_string())];
    ui.push_id(("macro", idx), |ui| {
        theme::card(p).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Name, switch, remove.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                widgets::input(ui, &mut m.name, t.macro_name_hint, 240.0);
                if live {
                    ui.label(RichText::new(t.macro_trigger_down).font(theme::medium(13.0)).color(p.accent));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if widgets::danger_button(ui, t.macro_remove, p).clicked() {
                        remove = true;
                    }
                    theme::switch(ui, &mut m.enabled, p);
                });
            });
            ui.add_space(14.0);

            // Trigger: the chord.
            ui.label(RichText::new(t.macro_trigger).font(theme::reg(13.0)).color(p.text2));
            ui.add_space(6.0);
            let mut drop_at: Option<usize> = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for i in 0..m.chord.len() {
                    if let Some(d) = pick(ui, ("chord", idx, i), m.chord[i], &din_opts, 190.0) {
                        m.chord[i] = d;
                    }
                    if m.chord.len() > 1 && tool_btn(ui, p, widgets::Icon::Close, t.macro_chord_remove, true) {
                        drop_at = Some(i);
                    }
                    if i + 1 < m.chord.len() {
                        ui.label(RichText::new("+").font(theme::medium(14.0)).color(p.muted));
                    }
                }
                if m.chord.len() < MAX_CHORD {
                    let label = if m.chord.is_empty() { t.macro_pick_input } else { t.macro_add_input };
                    let mut added = None;
                    widgets::combo(("chord-add", idx), label, 190.0).show_ui(ui, |ui| {
                        for (d, name) in &din_opts {
                            if !m.chord.contains(d) && ui.selectable_label(false, RichText::new(name).font(theme::reg(13.5))).clicked() {
                                added = Some(*d);
                            }
                        }
                    });
                    if let Some(d) = added {
                        m.chord.push(d);
                    }
                }
            });
            if let Some(i) = drop_at {
                m.chord.remove(i);
            }
            ui.add_space(14.0);

            // Mode and swallow.
            ui.label(RichText::new(t.macro_mode).font(theme::reg(13.0)).color(p.text2));
            ui.add_space(6.0);
            let names: Vec<&str> = MacroMode::ALL.iter().map(|m| mode_name(t, *m)).collect();
            let cur = MacroMode::ALL.iter().position(|x| *x == m.mode).unwrap_or(0);
            ui.horizontal_wrapped(|ui| {
                if let Some(k) = widgets::segmented(ui, &format!("mode-{idx}"), &names, cur, p) {
                    m.mode = MacroMode::ALL[k];
                }
            });
            ui.add_space(10.0);
            switch_row(ui, p, t.macro_swallow, t.macro_swallow_hint, &mut m.swallow);

            // Steps.
            theme::divider(ui, p);
            ui.label(RichText::new(t.macro_steps).font(theme::reg(13.0)).color(p.text2));
            ui.add_space(8.0);
            let mut edit = None;
            let n = m.steps.len();
            for i in 0..n {
                ui.push_id(("step", i), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let (r, _) = ui.allocate_exact_size(vec2(22.0, 34.0), Sense::hover());
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, format!("{}", i + 1), theme::mono(13.0), p.muted);
                        let k = kind_of(&m.steps[i]);
                        let (o, ms) = (out_of(&m.steps[i]), ms_of(&m.steps[i]));
                        let mut nk = k;
                        let mut no = o;
                        let mut nms = ms;
                        if let Some(v) = pick(ui, ("kind", idx, i), k, &kinds, 130.0) {
                            nk = v;
                        }
                        if nk != Kind::Wait {
                            if let Some(v) = pick(ui, ("out", idx, i), o, &xopts, 190.0) {
                                no = v;
                            }
                        }
                        if matches!(nk, Kind::Hold | Kind::Wait) {
                            let mut v = nms;
                            ui.add(egui::DragValue::new(&mut v).range(1..=crate::emu::macros::MAX_STEP_MS).suffix(format!(" {}", t.unit_ms)).speed(2.0));
                            nms = v;
                        }
                        if (nk, no, nms) != (k, o, ms) {
                            m.steps[i] = make(nk, no, nms);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if tool_btn(ui, p, widgets::Icon::Close, t.step_remove, true) {
                                edit = Some(Edit::Remove(i));
                            }
                            if tool_btn(ui, p, widgets::Icon::Down, t.step_down, i + 1 < n) {
                                edit = Some(Edit::Down(i));
                            }
                            if tool_btn(ui, p, widgets::Icon::Up, t.step_up, i > 0) {
                                edit = Some(Edit::Up(i));
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
            apply_edit(&mut m.steps, edit);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if widgets::btn_enabled(ui, t.step_add, Btn::Secondary, m.steps.len() < MAX_STEPS, p).clicked() {
                    m.steps.push(Step::Hold(XOut::Btn(XBtn::A), 100));
                }
                theme::meta(ui, &t.macro_total.replace("{ms}", &m.total_ms().to_string()), p);
            });
            if m.chord.is_empty() || m.steps.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(t.macro_incomplete).font(theme::reg(13.0)).color(p.warning));
            }
        });
    });
    remove
}

impl App {
    pub(super) fn macros_view(&mut self, ui: &mut Ui, p: &Pal, pad: Option<&PadSnapshot>) {
        let ctx = ui.ctx().clone();
        let t = self.s();
        let mut cfg = self.edit_cfg();
        let editing = self.edit_name();
        let ins = pad.map(|ps| remap::inputs(&ps.state, &cfg.remap)).unwrap_or(0);
        let mut remove_macro: Option<usize> = None;
        let mut remove_turbo: Option<usize> = None;
        let mut add_macro = false;
        let mut add_turbo = false;
        let btn_opts: Vec<(XBtn, String)> = XBtn::ALL.iter().map(|b| (*b, super::emulation::xbtn_name(t, *b))).collect();

        page(ui, p, t.nav_macros, Some(t.macros_intro), |_| {}, |ui| {
            ui.add_space(16.0);
            // The fair-play note: one line, always visible.
            ui.add(egui::Label::new(RichText::new(t.macros_warning).font(theme::medium(13.5)).color(p.warning)).wrap());
            editing_chip(ui, p, t, &editing);

            group(ui, p, t.turbo_title, |ui| {
                ui.add(egui::Label::new(RichText::new(t.turbo_hint).font(theme::reg(14.0)).color(p.text2)).wrap());
                ui.add_space(12.0);
                for i in 0..cfg.turbo.len() {
                    ui.push_id(("turbo", i), |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            let cur = cfg.turbo[i].btn;
                            let taken: Vec<XBtn> = cfg.turbo.iter().map(|x| x.btn).filter(|b| *b != cur).collect();
                            let avail: Vec<(XBtn, String)> = btn_opts.iter().filter(|(b, _)| !taken.contains(b)).cloned().collect();
                            if let Some(b) = pick(ui, ("turbo-btn", i), cur, &avail, 210.0) {
                                cfg.turbo[i].btn = b;
                            }
                            let mut hz = cfg.turbo[i].hz as f32;
                            let w = (ui.available_width() - 150.0).max(100.0);
                            ui.allocate_ui_with_layout(vec2(w, 24.0), Layout::left_to_right(Align::Center), |ui| {
                                if theme::slider(ui, &mut hz, TURBO_MIN_HZ as f32, TURBO_MAX_HZ as f32, 1.0, p).changed() {
                                    cfg.turbo[i].hz = hz.round() as u8;
                                }
                            });
                            ui.label(RichText::new(format!("{:>2} {}", cfg.turbo[i].hz, t.unit_hz)).font(theme::mono(13.0)).color(p.text));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if tool_btn(ui, p, widgets::Icon::Close, t.turbo_remove, true) {
                                    remove_turbo = Some(i);
                                }
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
                if cfg.turbo.is_empty() {
                    theme::hint(ui, t.turbo_none, p);
                    ui.add_space(8.0);
                }
                if widgets::btn_enabled(ui, t.turbo_add, Btn::Secondary, cfg.turbo.len() < XBtn::ALL.len(), p).clicked() {
                    add_turbo = true;
                }
            });

            ui.add_space(16.0);
            theme::section(ui, t.macros_title, p);
            ui.add_space(8.0);
            for i in 0..cfg.macros.len() {
                let mask = cfg.macros[i].mask();
                let live = mask != 0 && ins & mask == mask;
                if macro_card(ui, p, t, i, &mut cfg.macros[i], live) {
                    remove_macro = Some(i);
                }
                ui.add_space(12.0);
            }
            if cfg.macros.is_empty() {
                theme::hint(ui, t.macros_none, p);
                ui.add_space(8.0);
            }
            if widgets::btn_enabled(ui, t.macro_add, Btn::Primary, cfg.macros.len() < MAX_MACROS, p).clicked() {
                add_macro = true;
            }
        });

        if let Some(i) = remove_macro {
            cfg.macros.remove(i);
        }
        if let Some(i) = remove_turbo {
            cfg.turbo.remove(i);
        }
        if add_turbo {
            if let Some(b) = XBtn::ALL.iter().copied().find(|b| !cfg.turbo.iter().any(|x| x.btn == *b)) {
                cfg.turbo.push(Turbo { btn: if cfg.turbo.is_empty() { XBtn::A } else { b }, hz: TURBO_DEFAULT_HZ });
            }
        }
        if add_macro {
            cfg.macros.push(Macro { name: t.macro_new_name.replace("{n}", &(cfg.macros.len() + 1).to_string()), steps: vec![Step::Hold(XOut::Btn(XBtn::A), 100)], ..Default::default() });
        }
        self.store_edit_cfg(cfg, &ctx);
    }
}
