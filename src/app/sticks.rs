//! Sticks and drift: a big plot per stick, the drift test and the circularity test.

use super::{draw, group, page, widgets, App};
use crate::pad::analysis::{self, CircResult, Circularity, Drift, Verdict, SECTORS};
use crate::pad::PadSnapshot;
use crate::strings::S;
use crate::theme::{self, Btn, Pal};
use eframe::egui::{self, Align, Color32, Layout, RichText, Ui};
use std::collections::VecDeque;

pub const DRIFT_SECS: f64 = 5.0;
const TRAIL: usize = 90;
/// Dead-zone rings drawn on the plots, as a share of the travel.
const RINGS: [f32; 2] = [0.05, 0.10];

#[derive(Default)]
pub enum DriftTest {
    #[default]
    Idle,
    /// `start` is set on the first frame the test is seen.
    Running { start: Option<f64>, l: Vec<(f32, f32)>, r: Vec<(f32, f32)> },
    Done { l: Option<Drift>, r: Option<Drift> },
}

#[derive(Default)]
pub enum CircTest {
    #[default]
    Idle,
    Running { l: Circularity, r: Circularity },
    Done { l: Circularity, r: Circularity },
}

#[derive(Default)]
pub struct State {
    trail: [VecDeque<(f32, f32)>; 2],
    pub drift: DriftTest,
    pub circ: CircTest,
}

impl State {
    pub fn start_drift(&mut self, _now: f64) {
        self.drift = DriftTest::Running { start: None, l: Vec::new(), r: Vec::new() };
    }
    pub fn start_circ(&mut self) {
        self.circ = CircTest::Running { l: Circularity::default(), r: Circularity::default() };
    }
}

fn verdict_text(t: &S, v: Verdict) -> &'static str {
    match v {
        Verdict::Ok => t.drift_ok,
        Verdict::Light => t.drift_light,
        Verdict::Bad => t.drift_bad,
    }
}

fn verdict_color(p: &Pal, v: Verdict) -> Color32 {
    match v {
        Verdict::Ok => p.positive,
        Verdict::Light => p.warning,
        Verdict::Bad => p.negative,
    }
}

/// The outline of the reach in each sector, as plot points.
fn reach_points(c: &Circularity) -> Vec<(f32, f32)> {
    c.reach
        .iter()
        .enumerate()
        .filter(|(_, r)| **r > 0.0)
        .map(|(i, r)| {
            let a = (i as f32 + 0.5) / SECTORS as f32 * std::f32::consts::TAU;
            (a.cos() * r, a.sin() * r)
        })
        .collect()
}

fn kv(ui: &mut Ui, p: &Pal, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).font(theme::reg(13.0)).color(p.text2));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).font(theme::mono(13.0)).color(p.text));
        });
    });
}

/// A name in a fixed-width column, so the verdicts of two rows line up.
fn name_col(ui: &mut Ui, p: &Pal, name: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(120.0, 20.0), egui::Sense::hover());
    ui.painter().text(r.left_center(), egui::Align2::LEFT_CENTER, name, theme::reg(14.0), p.text);
}

fn pct(v: f32) -> String {
    format!("{:.1} %", v * 100.0)
}

impl App {
    pub(super) fn sticks_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let t = self.s();
        let (l, r) = (pad.state.left(), pad.state.right());
        // Feed the tests and the trails once per frame.
        {
            let v = &mut self.sticks_view;
            for (i, pos) in [l, r].into_iter().enumerate() {
                if v.trail[i].len() >= TRAIL {
                    v.trail[i].pop_front();
                }
                v.trail[i].push_back(pos);
            }
            let mut finished = None;
            if let DriftTest::Running { start, l: sl, r: sr } = &mut v.drift {
                let s0 = *start.get_or_insert(now);
                sl.push(l);
                sr.push(r);
                if now - s0 >= DRIFT_SECS {
                    finished = Some((analysis::drift(sl), analysis::drift(sr)));
                }
                ctx.request_repaint();
            }
            if let Some((dl, dr)) = finished {
                v.drift = DriftTest::Done { l: dl, r: dr };
            }
            if let CircTest::Running { l: cl, r: cr } = &mut v.circ {
                cl.add(l.0, l.1);
                cr.add(r.0, r.1);
            }
        }

        let mut start_drift = false;
        let mut start_circ = false;
        let mut finish_circ = false;
        let mut cancel_drift = false;
        // While the Xbox emulation is on, the plots also show what the game gets (see the Xbox emulation page).
        let emu_on = self.snap.emu.enabled;
        let emu_cfg = [self.cfg.emu.left, self.cfg.emu.right];
        page(ui, p, t.nav_sticks, Some(if emu_on { t.emu_zones_hint } else { t.dz_hint }), |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            let w = ui.available_width();
            let two = w > 720.0;
            let plot = if two { ((w - 16.0) / 2.0 - 34.0).min(300.0) } else { (w - 34.0).min(300.0) }.max(160.0);
            let circ_reach = |c: &CircTest, which: usize| -> Option<Vec<(f32, f32)>> {
                match c {
                    CircTest::Running { l, r } | CircTest::Done { l, r } => Some(reach_points(if which == 0 { l } else { r })),
                    CircTest::Idle => None,
                }
            };
            let stick_cards = |ui: &mut Ui, app: &App| {
                for (i, (title, pos, raw)) in [(t.left_stick, l, (pad.state.lx, pad.state.ly)), (t.right_stick, r, (pad.state.rx, pad.state.ry))].into_iter().enumerate() {
                    let after = emu_on.then(|| emu_cfg[i].apply(pos.0, pos.1));
                    let rings: Vec<f32> = if emu_on { vec![emu_cfg[i].inner] } else { RINGS.to_vec() };
                    let reach = circ_reach(&app.sticks_view.circ, i);
                    let trail: Vec<(f32, f32)> = app.sticks_view.trail[i].iter().copied().collect();
                    theme::card(p).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(plot);
                            ui.label(RichText::new(title).font(theme::medium(15.0)).color(p.text));
                            ui.add_space(10.0);
                            draw::stick_plot(ui, plot, p, pos, &trail, &rings, reach.as_deref(), after);
                            ui.add_space(12.0);
                            kv(ui, p, "X", &format!("{:+.3}", pos.0));
                            kv(ui, p, "Y", &format!("{:+.3}", pos.1));
                            kv(ui, p, t.stick_mag, &pct((pos.0 * pos.0 + pos.1 * pos.1).sqrt()));
                            kv(ui, p, t.stick_raw, &format!("{} / {}", raw.0, raw.1));
                        });
                    });
                    if i == 0 {
                        ui.add_space(0.0);
                    }
                }
            };
            if two {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    stick_cards(ui, self);
                });
            } else {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    stick_cards(ui, self);
                });
            }

            // Drift test.
            group(ui, p, t.drift_title, |ui| {
                match &self.sticks_view.drift {
                    DriftTest::Idle => {
                        ui.add(egui::Label::new(RichText::new(t.drift_desc).font(theme::reg(14.0)).color(p.text2)).wrap());
                        ui.add_space(12.0);
                        if widgets::btn(ui, t.drift_start, Btn::Primary, p).clicked() {
                            start_drift = true;
                        }
                    }
                    DriftTest::Running { start, .. } => {
                        let left = (DRIFT_SECS - (now - start.unwrap_or(now))).max(0.0).ceil() as u32;
                        ui.label(RichText::new(t.drift_running.replace("{n}", &left.to_string())).font(theme::medium(16.0)).color(p.text));
                        ui.add_space(8.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, p.line);
                        let f = ((now - start.unwrap_or(now)) / DRIFT_SECS).clamp(0.0, 1.0) as f32;
                        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * f, 6.0)), 3.0, p.accent);
                        ui.add_space(12.0);
                        if widgets::btn(ui, t.cancel, Btn::Secondary, p).clicked() {
                            cancel_drift = true;
                        }
                    }
                    DriftTest::Done { l: dl, r: dr } => {
                        for (name, d) in [(t.left_stick, dl), (t.right_stick, dr)] {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 16.0;
                                name_col(ui, p, name);
                                match d {
                                    Some(d) => {
                                        ui.label(RichText::new(verdict_text(t, d.verdict)).font(theme::medium(14.0)).color(verdict_color(p, d.verdict)));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            ui.label(RichText::new(format!("{} {}   {} {}", t.drift_offset, pct(d.offset), t.drift_jitter, pct(d.jitter))).font(theme::mono(13.0)).color(p.text));
                                        });
                                    }
                                    None => {
                                        ui.label(RichText::new("-").color(p.muted));
                                    }
                                }
                            });
                            ui.add_space(6.0);
                        }
                        ui.add_space(4.0);
                        theme::hint(ui, t.drift_rules, p);
                        ui.add_space(12.0);
                        if widgets::btn(ui, t.test_again, Btn::Secondary, p).clicked() {
                            start_drift = true;
                        }
                    }
                }
            });

            // Circularity test.
            group(ui, p, t.circ_title, |ui| {
                match &self.sticks_view.circ {
                    CircTest::Idle => {
                        ui.add(egui::Label::new(RichText::new(t.circ_desc).font(theme::reg(14.0)).color(p.text2)).wrap());
                        ui.add_space(12.0);
                        if widgets::btn(ui, t.circ_start, Btn::Primary, p).clicked() {
                            start_circ = true;
                        }
                    }
                    CircTest::Running { l: cl, r: cr } => {
                        ui.add(egui::Label::new(RichText::new(t.circ_desc).font(theme::reg(14.0)).color(p.text2)).wrap());
                        ui.add_space(8.0);
                        kv(ui, p, &format!("{}, {}", t.circ_coverage, t.left_stick), &format!("{:.0} %", cl.coverage() * 100.0));
                        kv(ui, p, &format!("{}, {}", t.circ_coverage, t.right_stick), &format!("{:.0} %", cr.coverage() * 100.0));
                        ui.add_space(12.0);
                        if widgets::btn(ui, t.circ_finish, Btn::Primary, p).clicked() {
                            finish_circ = true;
                        }
                    }
                    CircTest::Done { l: cl, r: cr } => {
                        for (name, c) in [(t.left_stick, cl), (t.right_stick, cr)] {
                            let res: CircResult = c.result();
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 16.0;
                                name_col(ui, p, name);
                                if res.coverage > 0.0 {
                                    let v = if res.complete { res.verdict } else { Verdict::Light };
                                    ui.label(RichText::new(if res.complete { verdict_label(t, res.verdict) } else { t.circ_incomplete }).font(theme::medium(14.0)).color(verdict_color(p, v)));
                                }
                            });
                            if res.coverage > 0.0 {
                                kv(ui, p, t.circ_error, &format!("{:.1} %", res.error_pct));
                                kv(ui, p, t.circ_reach, &pct(res.mean_reach));
                                kv(ui, p, t.circ_coverage, &format!("{:.0} %", res.coverage * 100.0));
                            }
                            ui.add_space(8.0);
                        }
                        theme::hint(ui, t.circ_rules, p);
                        ui.add_space(12.0);
                        if widgets::btn(ui, t.test_again, Btn::Secondary, p).clicked() {
                            start_circ = true;
                        }
                    }
                }
            });
        });

        let v = &mut self.sticks_view;
        if start_drift {
            v.start_drift(now);
        }
        if cancel_drift {
            v.drift = DriftTest::Idle;
        }
        if start_circ {
            v.start_circ();
        }
        if finish_circ {
            if let CircTest::Running { l, r } = std::mem::take(&mut v.circ) {
                v.circ = CircTest::Done { l, r };
            }
        }
    }
}

/// The circularity verdict in words (the same wording as the drift one: fine, worn, bad).
fn verdict_label(t: &S, v: Verdict) -> &'static str {
    match v {
        Verdict::Ok => t.circ_ok,
        Verdict::Light => t.circ_light,
        Verdict::Bad => t.circ_bad,
    }
}
