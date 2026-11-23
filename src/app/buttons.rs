//! Buttons and timing: a checklist of the 17 controls, and the interval between reports.

use super::{group, page, widgets, App};
use crate::pad::{Btn, PadSnapshot};
use crate::strings::S;
use crate::theme::{self, Btn as UiBtn, Pal};
use eframe::egui::{self, vec2, Align, Layout, RichText, Sense, Stroke, StrokeKind, Ui};

#[derive(Default)]
pub struct State {
    /// Bit mask of the controls that were pressed at least once.
    pub seen: u32,
}

pub fn btn_name(t: &S, b: Btn) -> &'static str {
    match b {
        Btn::Up => t.b_up,
        Btn::Down => t.b_down,
        Btn::Left => t.b_left,
        Btn::Right => t.b_right,
        Btn::Cross => t.b_cross,
        Btn::Circle => t.b_circle,
        Btn::Square => t.b_square,
        Btn::Triangle => t.b_triangle,
        Btn::L1 => t.b_l1,
        Btn::R1 => t.b_r1,
        Btn::L3 => t.b_l3,
        Btn::R3 => t.b_r3,
        Btn::Create => t.b_create,
        Btn::Options => t.b_options,
        Btn::Ps => t.b_ps,
        Btn::Touchpad => t.b_touchpad,
        Btn::Mute => t.b_mute,
        Btn::L2 | Btn::R2 => "",
    }
}

/// How many of the 17 controls are ticked in `seen`.
pub fn count_seen(seen: u32) -> usize {
    Btn::CHECKLIST.iter().filter(|b| seen & b.bit() != 0).count()
}

impl App {
    pub(super) fn buttons_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let t = self.s();
        self.buttons_view.seen |= pad.state.buttons;
        let seen = self.buttons_view.seen;
        let n = count_seen(seen);
        let mut reset = false;
        page(ui, p, t.nav_buttons, None, |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            group(ui, p, t.btn_checklist, |ui| {
                ui.horizontal(|ui| {
                    let msg = if n == 17 { t.btn_all_done.to_string() } else { t.btn_progress.replace("{n}", &n.to_string()) };
                    ui.label(RichText::new(msg).font(theme::medium(15.0)).color(if n == 17 { p.positive } else { p.text }));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::btn(ui, t.btn_reset, UiBtn::Secondary, p).clicked() {
                            reset = true;
                        }
                    });
                });
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, p.line);
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, vec2(rect.width() * n as f32 / 17.0, 6.0)), 3.0, p.accent);
                ui.add_space(14.0);
                let w = ui.available_width();
                let cols = if w > 760.0 { 4 } else if w > 520.0 { 3 } else { 2 };
                let gap = 8.0;
                let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
                for row in Btn::CHECKLIST.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for b in row {
                            let (r, _) = ui.allocate_exact_size(vec2(cw, 36.0), Sense::hover());
                            let done = seen & b.bit() != 0;
                            let now = pad.state.pressed(*b);
                            let fill = if now { p.accent } else { p.raised };
                            ui.painter().rect(r, 8.0, fill, Stroke::new(1.0, if done { p.line_hi } else { p.line }), StrokeKind::Inside);
                            let ic = egui::pos2(r.left() + 20.0, r.center().y);
                            let col = if now { p.on_accent } else if done { p.positive } else { p.muted };
                            if done || now {
                                widgets::icon(ui.painter(), widgets::Icon::Check, ic, 14.0, col);
                            } else {
                                ui.painter().circle_stroke(ic, 4.0, Stroke::new(1.5, p.muted));
                            }
                            let font = theme::reg(13.5);
                            let name = widgets::fit_text(ui.painter(), btn_name(t, *b), &font, cw - 48.0);
                            ui.painter().text(egui::pos2(r.left() + 38.0, r.center().y), egui::Align2::LEFT_CENTER, name, font, if now { p.on_accent } else { p.text });
                        }
                    });
                    ui.add_space(gap);
                }
            });

            group(ui, p, t.timing_title, |ui| {
                let tm = &pad.timing;
                for (label, value) in [
                    (t.timing_rate, format!("{:.0} Hz", tm.hz)),
                    (t.timing_avg, format!("{:.2} ms", tm.avg_ms)),
                    (t.timing_max, format!("{:.2} ms", tm.max_ms)),
                    (t.timing_jitter, format!("{:.2} ms", tm.jitter_ms)),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).font(theme::reg(13.0)).color(p.text2));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(value).font(theme::mono(13.0)).color(p.text));
                        });
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
                ui.painter().rect(rect, 8.0, p.input, Stroke::new(1.0, p.line), StrokeKind::Inside);
                let v = &tm.recent_us;
                if !v.is_empty() {
                    let top = (tm.avg_ms * 2.5).max(tm.max_ms.min(tm.avg_ms * 6.0)).max(0.5);
                    let inner = rect.shrink2(vec2(8.0, 8.0));
                    let bw = inner.width() / 120.0;
                    let offset = 120usize.saturating_sub(v.len());
                    for (i, us) in v.iter().enumerate() {
                        let ms = *us as f32 / 1000.0;
                        let h = (ms / top).min(1.0) * inner.height();
                        let x = inner.left() + (offset + i) as f32 * bw;
                        let r = egui::Rect::from_min_max(egui::pos2(x + 0.5, inner.bottom() - h), egui::pos2(x + bw - 0.5, inner.bottom()));
                        let col = if ms > tm.avg_ms * 2.0 + 0.5 { p.warning } else { p.accent };
                        ui.painter().rect_filled(r, 1.0, col);
                    }
                    // The average as a hairline.
                    let y = inner.bottom() - (tm.avg_ms / top).min(1.0) * inner.height();
                    ui.painter().hline(inner.x_range(), y, Stroke::new(1.0, p.muted));
                }
                ui.add_space(8.0);
                theme::hint(ui, t.timing_hint, p);
            });
        });
        if reset {
            self.buttons_view.seen = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checklist_counts_seventeen_and_ignores_trigger_clicks() {
        assert_eq!(Btn::CHECKLIST.len(), 17);
        let all: u32 = Btn::CHECKLIST.iter().map(|b| b.bit()).fold(0, |a, b| a | b);
        assert_eq!(count_seen(all), 17);
        assert_eq!(count_seen(Btn::L2.bit() | Btn::R2.bit()), 0);
        assert_eq!(count_seen(Btn::Cross.bit() | Btn::Cross.bit() | Btn::Mute.bit()), 2);
    }

    #[test]
    fn every_checklist_button_has_a_name_in_all_languages() {
        for lang in crate::i18n::Lang::ALL {
            let t = crate::strings::TABLE.get(lang);
            for b in Btn::CHECKLIST {
                assert!(!btn_name(t, b).is_empty());
            }
        }
    }
}
