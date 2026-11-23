//! Triggers: live value bars and the adaptive trigger effects.

use super::{group, page, widgets, App};
use crate::pad::output::Trigger;
use crate::pad::{Btn, PadKind, PadSnapshot};
use crate::theme::{self, Pal};
use eframe::egui::{self, Align, Layout, RichText, Ui};

#[derive(Default)]
pub struct State {
    /// Selected preset per trigger: left, right (index into [`PRESETS`]).
    pub fx: [usize; 2],
}

/// Presets in the order of the segmented control.
pub const PRESETS: [Trigger; 5] = [
    Trigger::Off,
    Trigger::Resistance { start: 2, force: 3 },
    Trigger::Resistance { start: 1, force: 8 },
    Trigger::Weapon { start: 2, end: 5, force: 8 },
    Trigger::Vibration { start: 3, amplitude: 6, frequency: 30 },
];

fn bar(ui: &mut Ui, p: &Pal, v: u8, click: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
    ui.painter().rect(rect, 7.0, p.input, egui::Stroke::new(1.0, p.line), egui::StrokeKind::Inside);
    let w = rect.width() * v as f32 / 255.0;
    if w > 1.0 {
        let r = egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height()));
        ui.painter().rect_filled(r.shrink(1.0), 6.0, p.accent);
    }
    if click {
        ui.painter().rect_stroke(rect, 7.0, egui::Stroke::new(1.5, p.accent), egui::StrokeKind::Outside);
    }
}

impl App {
    pub(super) fn triggers_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let t = self.s();
        let mut changed: Option<(usize, usize)> = None;
        let names = [t.fx_off, t.fx_light, t.fx_heavy, t.fx_weapon, t.fx_vibration];
        let dualsense = pad.info.kind == PadKind::DualSense;
        page(ui, p, t.nav_triggers, None, |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            for (i, (title, v, click)) in [(t.trig_left, pad.state.l2, pad.state.pressed(Btn::L2)), (t.trig_right, pad.state.r2, pad.state.pressed(Btn::R2))].into_iter().enumerate() {
                group(ui, p, title, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t.trig_value).font(theme::reg(13.0)).color(p.text2));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("{:>3}  {:>3.0} %", v, v as f32 / 2.55)).font(theme::mono(13.0)).color(p.text));
                        });
                    });
                    ui.add_space(6.0);
                    bar(ui, p, v, click);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t.trig_click).font(theme::reg(13.0)).color(p.text2));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let (txt, col) = if click { ("1", p.accent) } else { ("0", p.muted) };
                            ui.label(RichText::new(txt).font(theme::mono(13.0)).color(col));
                        });
                    });
                    ui.add_space(12.0);
                    ui.label(RichText::new(t.trig_effects).font(theme::reg(13.0)).color(p.text2));
                    ui.add_space(6.0);
                    if dualsense {
                        ui.horizontal_wrapped(|ui| {
                            if let Some(k) = widgets::segmented(ui, &format!("fx-{i}"), &names, self.triggers_view.fx[i], p) {
                                changed = Some((i, k));
                            }
                        });
                    } else {
                        theme::hint(ui, t.fx_unavailable, p);
                    }
                });
            }
            ui.add_space(12.0);
            theme::hint(ui, t.trig_effects_hint, p);
        });
        if let Some((i, k)) = changed {
            self.triggers_view.fx[i] = k;
            if i == 0 {
                self.out.trigger_left = PRESETS[k];
            } else {
                self.out.trigger_right = PRESETS[k];
            }
            self.push_out();
        }
    }
}
