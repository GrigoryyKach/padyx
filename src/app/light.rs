//! Lightbar and rumble: colour (suite palette or custom RGB), brightness, player LEDs, the mute light, and
//! rumble sliders. The lightbar choices are saved and re-applied on every connect.

use super::{group, page, widgets, App};
use crate::pad::output::{MuteLed, Output};
use crate::pad::PadSnapshot;
use crate::theme::{self, Btn, Pal};
use eframe::egui::{self, vec2, Align, Color32, Layout, RichText, Sense, Stroke, Ui};

/// The suite's app accents (dark values) plus white: a palette that feels like the rest of NIMVORO.
pub const SWATCHES: [u32; 12] = [
    0xab7cf0, 0x5b78ff, 0x35a7ff, 0x4fc3f7, 0x2cb5d6, 0x2ec4b6, 0x4cae85, 0xe0a82e, 0xec5b45, 0xe8669a, 0xff8a3d, 0xffffff,
];

#[derive(Default)]
pub struct State {
    pub rumble_touched: bool,
}

fn rgb_of(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// A labelled slider row: a fixed label column, the slider, and the value.
fn slider_row(ui: &mut Ui, p: &Pal, label: &str, value: &mut f32, min: f32, max: f32, shown: &dyn Fn(f32) -> String) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (lr, _) = ui.allocate_exact_size(vec2(150.0, 24.0), Sense::hover());
        ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, label, theme::reg(13.5), p.text);
        let w = (ui.available_width() - 70.0).max(80.0);
        ui.allocate_ui_with_layout(vec2(w, 24.0), Layout::left_to_right(Align::Center), |ui| {
            changed = theme::slider(ui, value, min, max, 1.0, p).changed();
        });
        ui.label(RichText::new(shown(*value)).font(theme::mono(13.0)).color(p.text));
    });
    changed
}

impl App {
    pub(super) fn light_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let ctx = ui.ctx().clone();
        let t = self.s();
        let mut light_changed = false;
        let mut rumble_changed = false;
        let dualsense = pad.info.kind == crate::pad::PadKind::DualSense;
        page(ui, p, t.nav_light, Some(t.light_saved), |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            if !dualsense {
                theme::hint(ui, t.fx_unavailable, p);
                return;
            }
            group(ui, p, t.light_title, |ui| {
                // Preview strip: the colour as the pad shows it.
                let sc = self.out.light_scaled();
                let (prev, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
                ui.painter().rect(prev, 7.0, Color32::from_rgb(sc[0], sc[1], sc[2]), Stroke::new(1.0, p.line_hi), egui::StrokeKind::Inside);
                ui.add_space(14.0);
                ui.label(RichText::new(t.light_colour).font(theme::reg(13.0)).color(p.text2));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                    for c in SWATCHES {
                        let (r, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                        let on = self.out.light == rgb_of(c);
                        ui.painter().rect(r.shrink(2.0), 8.0, widgets::rgb(c), Stroke::new(1.0, p.line_hi), egui::StrokeKind::Inside);
                        if on {
                            ui.painter().rect_stroke(r, 10.0, Stroke::new(2.0, p.text), egui::StrokeKind::Inside);
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("#{c:06x}")).clicked() {
                            self.out.light = rgb_of(c);
                            light_changed = true;
                        }
                    }
                });
                ui.add_space(16.0);
                ui.label(RichText::new(t.light_custom).font(theme::reg(13.0)).color(p.text2));
                ui.add_space(6.0);
                for (i, name) in [t.ch_red, t.ch_green, t.ch_blue].into_iter().enumerate() {
                    let mut v = self.out.light[i] as f32;
                    if slider_row(ui, p, name, &mut v, 0.0, 255.0, &|x| format!("{:>3}", x as u8)) {
                        self.out.light[i] = v as u8;
                        light_changed = true;
                    }
                }
                theme::divider(ui, p);
                let mut b = self.out.brightness as f32 / 255.0 * 100.0;
                if slider_row(ui, p, t.light_brightness, &mut b, 0.0, 100.0, &|x| format!("{:>3} %", x as u32)) {
                    self.out.brightness = (b / 100.0 * 255.0).round() as u8;
                    light_changed = true;
                }
            });

            group(ui, p, t.players_title, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for i in 0..5u8 {
                        let (r, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                        let lit = self.out.players & (1 << i) != 0;
                        ui.painter().rect(r, 8.0, if lit { p.accent } else { p.raised }, Stroke::new(1.0, if lit { p.accent } else { p.line_hi }), egui::StrokeKind::Inside);
                        ui.painter().circle_filled(r.center(), 4.0, if lit { p.on_accent } else { p.muted });
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.out.players ^= 1 << i;
                            light_changed = true;
                        }
                    }
                });
                ui.add_space(12.0);
                let labels: Vec<String> = std::iter::once(t.players_none.to_string()).chain((1..=4).map(|n| t.players_n.replace("{n}", &n.to_string()))).collect();
                let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
                let cur = (0..=4u8).position(|n| Output::player_pattern(n) == self.out.players).unwrap_or(usize::MAX);
                if let Some(k) = widgets::segmented(ui, "players", &refs, cur, p) {
                    self.out.players = Output::player_pattern(k as u8);
                    light_changed = true;
                }
                theme::divider(ui, p);
                let mut mute = self.out.mute_led != MuteLed::Off;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t.mute_led).font(theme::reg(14.0)).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::switch(ui, &mut mute, p).changed() {
                            self.out.mute_led = if mute { MuteLed::On } else { MuteLed::Off };
                            light_changed = true;
                        }
                    });
                });
            });

            group(ui, p, t.rumble_title, |ui| {
                let mut l = self.out.rumble_left as f32;
                if slider_row(ui, p, t.rumble_left, &mut l, 0.0, 255.0, &|x| format!("{:>3}", x as u8)) {
                    self.out.rumble_left = l as u8;
                    rumble_changed = true;
                }
                let mut r = self.out.rumble_right as f32;
                if slider_row(ui, p, t.rumble_right, &mut r, 0.0, 255.0, &|x| format!("{:>3}", x as u8)) {
                    self.out.rumble_right = r as u8;
                    rumble_changed = true;
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::btn(ui, t.rumble_stop, Btn::Secondary, p).clicked() {
                        self.out.rumble_left = 0;
                        self.out.rumble_right = 0;
                        rumble_changed = true;
                    }
                });
                ui.add_space(8.0);
                theme::hint(ui, t.rumble_hint, p);
            });
        });
        if light_changed {
            self.cfg.light = self.out.light;
            self.cfg.brightness = self.out.brightness;
            self.cfg.players = self.out.players & 0x1F;
            self.cfg.mute_led = self.out.mute_led != MuteLed::Off;
            self.touch_cfg(&ctx);
        }
        if light_changed || rumble_changed {
            self.light_view.rumble_touched |= rumble_changed;
            self.push_out();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swatches_are_unique_and_split_into_channels() {
        let mut v = SWATCHES.to_vec();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), SWATCHES.len());
        assert_eq!(rgb_of(0xab7cf0), [0xab, 0x7c, 0xf0]);
    }

    #[test]
    fn player_patterns_match_the_presets() {
        assert_eq!(Output::player_pattern(1), 0b00100);
        assert_eq!(Output::player_pattern(0), 0);
        assert_eq!((0..=4u8).position(|n| Output::player_pattern(n) == 0b01010), Some(2));
    }
}
