//! The Remap page: every DualSense input to any Xbox output (or nothing), the sticks (swap, invert, as the
//! D-pad, off) and the trigger threshold. The inputs light up live, so pressing a button finds its row.
//!
//! The page edits the settings of the profile picked on the Profiles page (the default one if none).

use super::emulation::xbtn_name;
use super::light::slider_row_w;
use super::settings::switch_row;
use super::{buttons::btn_name, group, page, widgets, App};
use crate::emu::remap::{self, DIn, Out, StickMap, StickMode};
use crate::pad::PadSnapshot;
use crate::strings::S;
use crate::theme::{self, Btn, Pal};
use eframe::egui::{self, vec2, Align, Layout, RichText, Sense, Ui};

pub fn out_name(t: &S, o: Out) -> String {
    match o {
        Out::None => t.emu_unmapped.to_string(),
        Out::Btn(b) => xbtn_name(t, b),
        Out::Lt => t.xb_lt.to_string(),
        Out::Rt => t.xb_rt.to_string(),
    }
}

pub fn din_name(t: &S, d: DIn) -> String {
    if let Some(b) = d.as_btn() {
        return btn_name(t, b).to_string();
    }
    match d {
        DIn::L2 => t.din_l2,
        DIn::R2 => t.din_r2,
        DIn::TouchLeft => t.din_touch_left,
        DIn::TouchRight => t.din_touch_right,
        DIn::LsUp => t.din_ls_up,
        DIn::LsDown => t.din_ls_down,
        DIn::LsLeft => t.din_ls_left,
        DIn::LsRight => t.din_ls_right,
        DIn::RsUp => t.din_rs_up,
        DIn::RsDown => t.din_rs_down,
        DIn::RsLeft => t.din_rs_left,
        DIn::RsRight => t.din_rs_right,
        _ => "",
    }
    .to_string()
}

/// A dropdown with no label of its own (the label is drawn by the row). Returns the new value if one was picked.
pub fn pick<T: PartialEq + Copy>(ui: &mut Ui, id: impl std::fmt::Debug, current: T, options: &[(T, String)], width: f32) -> Option<T> {
    let mut picked = None;
    let label = options.iter().find(|o| o.0 == current).map(|o| o.1.as_str()).unwrap_or("");
    widgets::combo(id, label, width).show_ui(ui, |ui| {
        for (v, name) in options {
            if ui.selectable_label(*v == current, RichText::new(name).font(theme::reg(13.5))).clicked() {
                picked = Some(*v);
            }
        }
    });
    picked
}

fn pct(v: f32) -> String {
    format!("{:>2.0} %", v)
}

/// One row: a live dot, the input's name, and the output picker on the right.
fn bind_row(ui: &mut Ui, p: &Pal, live: bool, label: &str, id: &str, cur: Out, opts: &[(Out, String)], changed: bool) -> Option<Out> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (dot, _) = ui.allocate_exact_size(vec2(12.0, widgets::ROW_BTN_H), Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.0, if live { p.accent } else { p.line_hi });
        ui.label(RichText::new(label).font(if changed { theme::medium(14.0) } else { theme::reg(14.0) }).color(p.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            picked = pick(ui, id, cur, opts, 230.0);
        });
    });
    ui.add_space(6.0);
    picked
}

fn stick_block(ui: &mut Ui, p: &Pal, t: &S, id: &str, title: &str, m: &mut StickMap) {
    ui.label(RichText::new(title).font(theme::medium(14.0)).color(p.text));
    ui.add_space(8.0);
    let labels = [t.stick_mode_stick, t.stick_mode_dpad, t.stick_mode_off];
    let cur = match m.mode {
        StickMode::Stick => 0,
        StickMode::Dpad => 1,
        StickMode::Off => 2,
    };
    ui.horizontal_wrapped(|ui| {
        if let Some(k) = widgets::segmented(ui, id, &labels, cur, p) {
            m.mode = [StickMode::Stick, StickMode::Dpad, StickMode::Off][k];
        }
    });
    if m.mode == StickMode::Stick {
        ui.add_space(10.0);
        switch_row(ui, p, t.stick_invert_x, "", &mut m.invert_x);
        switch_row(ui, p, t.stick_invert_y, "", &mut m.invert_y);
    }
}

/// The line that says which profile a page edits.
pub fn editing_chip(ui: &mut Ui, p: &Pal, t: &S, name: &str) {
    ui.add_space(16.0);
    super::chip(ui, p, t.prof_editing, if name.is_empty() { t.prof_default } else { name });
}

impl App {
    pub(super) fn remap_view(&mut self, ui: &mut Ui, p: &Pal, pad: Option<&PadSnapshot>) {
        let ctx = ui.ctx().clone();
        let t = self.s();
        let mut cfg = self.edit_cfg();
        let editing = self.edit_name();
        let ins = pad.map(|ps| remap::inputs(&ps.state, &cfg.remap)).unwrap_or(0);
        let opts: Vec<(Out, String)> = remap::out_options().into_iter().map(|o| (o, out_name(t, o))).collect();
        let mut reset = false;
        let def = crate::emu::map::EmuConfig::default();

        page(ui, p, t.nav_remap, Some(t.remap_intro), |_| {}, |ui| {
            editing_chip(ui, p, t, &editing);

            group(ui, p, t.remap_buttons, |ui| {
                let main: Vec<DIn> = DIn::ALL.iter().copied().filter(|d| d.as_btn().is_some()).collect();
                for d in main {
                    let cur = cfg.target(d);
                    let changed = cur != def.target(d);
                    if let Some(o) = bind_row(ui, p, ins & d.bit() != 0, &din_name(t, d), &format!("bind-{d:?}"), cur, &opts, changed) {
                        cfg.set_target(d, o);
                    }
                }
                theme::hint(ui, t.remap_buttons_hint, p);
            });

            group(ui, p, t.remap_extra, |ui| {
                for d in [DIn::L2, DIn::R2, DIn::TouchLeft, DIn::TouchRight] {
                    let cur = cfg.target(d);
                    let changed = cur != def.target(d);
                    if let Some(o) = bind_row(ui, p, ins & d.bit() != 0, &din_name(t, d), &format!("bind-{d:?}"), cur, &opts, changed) {
                        cfg.set_target(d, o);
                    }
                }
                ui.add_space(4.0);
                let mut v = cfg.remap.trigger_threshold as f32;
                if slider_row_w(ui, p, t.remap_threshold, 230.0, &mut v, 5.0, 95.0, &pct) {
                    cfg.remap.trigger_threshold = v.round() as u8;
                }
                ui.add_space(4.0);
                theme::hint(ui, t.remap_extra_hint, p);
            });

            group(ui, p, t.remap_sticks, |ui| {
                switch_row(ui, p, t.stick_swap, t.stick_swap_hint, &mut cfg.remap.swap_sticks);
                theme::divider(ui, p);
                stick_block(ui, p, t, "left-mode", t.left_stick, &mut cfg.remap.left);
                theme::divider(ui, p);
                stick_block(ui, p, t, "right-mode", t.right_stick, &mut cfg.remap.right);
                ui.add_space(4.0);
                theme::hint(ui, t.stick_hint, p);
            });

            group(ui, p, t.remap_dirs, |ui| {
                for d in [DIn::LsUp, DIn::LsDown, DIn::LsLeft, DIn::LsRight, DIn::RsUp, DIn::RsDown, DIn::RsLeft, DIn::RsRight] {
                    let cur = cfg.target(d);
                    let changed = cur != def.target(d);
                    if let Some(o) = bind_row(ui, p, ins & d.bit() != 0, &din_name(t, d), &format!("bind-{d:?}"), cur, &opts, changed) {
                        cfg.set_target(d, o);
                    }
                }
                theme::hint(ui, t.remap_dirs_hint, p);
            });

            group(ui, p, t.remap_reset, |ui| {
                ui.add(egui::Label::new(RichText::new(t.remap_reset_hint).font(theme::reg(14.0)).color(p.text2)).wrap());
                ui.add_space(12.0);
                if widgets::btn(ui, t.remap_reset, Btn::Secondary, p).clicked() {
                    reset = true;
                }
            });
        });

        if reset {
            cfg.reset_remap();
        }
        self.store_edit_cfg(cfg, &ctx);
    }
}
