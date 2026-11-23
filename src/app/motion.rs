//! Touchpad and motion: the touchpad surface with the fingers, the gyro and accelerometer live, and a small
//! orientation glyph that follows the tilt.

use super::{draw, group, page, App};
use crate::pad::{Btn, PadSnapshot};
use crate::theme::{self, Pal};
use eframe::egui::{self, pos2, vec2, Align, Layout, Pos2, RichText, Sense, Stroke, StrokeKind, Ui};

/// Roll and pitch in radians from the direction of gravity (the pad lying flat reads about 1 g on Y).
pub fn tilt(a: [f32; 3]) -> (f32, f32) {
    let roll = a[0].atan2((a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-6));
    let pitch = a[2].atan2(a[1].abs().max(1e-6));
    (roll, pitch)
}

/// Eight corners of a flat box (the pad), rotated and projected with a slight perspective.
pub fn box_corners(c: Pos2, scale: f32, roll: f32, pitch: f32) -> [Pos2; 8] {
    let mut out = [Pos2::ZERO; 8];
    let mut i = 0;
    for sz in [-1.0f32, 1.0] {
        for sy in [-1.0f32, 1.0] {
            for sx in [-1.0f32, 1.0] {
                let (x, y, z) = (sx * 1.5, sy * 0.28, sz * 0.8);
                // roll around Z, pitch around X
                let (x1, y1) = (x * roll.cos() - y * roll.sin(), x * roll.sin() + y * roll.cos());
                let (y2, z2) = (y1 * pitch.cos() - z * pitch.sin(), y1 * pitch.sin() + z * pitch.cos());
                // a fixed view from above and slightly to the side
                let (vx, vy) = (x1 + 0.35 * z2, -y2 + 0.45 * z2);
                let persp = 1.0 / (1.0 - 0.08 * z2);
                out[i] = c + vec2(vx, vy) * scale * persp;
                i += 1;
            }
        }
    }
    out
}

fn axis_bar(ui: &mut Ui, p: &Pal, name: &str, v: f32, range: f32, unit_fmt: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (nr, _) = ui.allocate_exact_size(vec2(60.0, 20.0), Sense::hover());
        ui.painter().text(nr.left_center(), egui::Align2::LEFT_CENTER, name, theme::reg(13.0), p.text2);
        let w = (ui.available_width() - 90.0).max(60.0);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 10.0), Sense::hover());
        ui.painter().rect(rect, 5.0, p.input, Stroke::new(1.0, p.line), StrokeKind::Inside);
        let mid = rect.center().x;
        let f = (v / range).clamp(-1.0, 1.0);
        let x1 = mid + f * rect.width() / 2.0;
        let r = egui::Rect::from_min_max(pos2(mid.min(x1), rect.top() + 1.0), pos2(mid.max(x1), rect.bottom() - 1.0));
        ui.painter().rect_filled(r, 4.0, p.accent);
        ui.painter().vline(mid, rect.y_range(), Stroke::new(1.0, p.muted));
        ui.label(RichText::new(unit_fmt.replace("{}", &format!("{v:+.2}"))).font(theme::mono(12.0)).color(p.text));
    });
    ui.add_space(4.0);
}

impl App {
    pub(super) fn motion_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let t = self.s();
        page(ui, p, t.nav_motion, None, |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            let st = &pad.state;
            group(ui, p, t.tp_title, |ui| {
                let w = ui.available_width().min(420.0);
                let (rect, _) = ui.allocate_exact_size(vec2(w, w * 9.0 / 16.0), Sense::hover());
                let click = st.pressed(Btn::Touchpad);
                ui.painter().rect(rect, 14.0, if click { theme::alpha(p.accent, 60) } else { p.input }, Stroke::new(if click { 1.5 } else { 1.0 }, if click { p.accent } else { p.line_hi }), StrokeKind::Inside);
                for (i, f) in st.touch.iter().enumerate() {
                    if f.active {
                        let c = draw::finger_pos(rect.shrink(10.0), f);
                        ui.painter().circle_filled(c, 11.0, if i == 0 { p.accent } else { p.text });
                        ui.painter().text(c + vec2(0.0, -20.0), egui::Align2::CENTER_CENTER, format!("{}", i + 1), theme::mono(12.0), p.text);
                    }
                }
                ui.add_space(12.0);
                for (i, f) in st.touch.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t.tp_finger.replace("{n}", &(i + 1).to_string())).font(theme::reg(13.0)).color(p.text2));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let s = if f.active { format!("x {:>4}   y {:>4}", f.x, f.y) } else { t.tp_none.to_string() };
                            ui.label(RichText::new(s).font(theme::mono(13.0)).color(if f.active { p.text } else { p.muted }));
                        });
                    });
                    ui.add_space(2.0);
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t.tp_click).font(theme::reg(13.0)).color(p.text2));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(if click { "1" } else { "0" }).font(theme::mono(13.0)).color(if click { p.accent } else { p.muted }));
                    });
                });
            });

            if st.reduced {
                group(ui, p, t.orientation, |ui| theme::hint(ui, t.no_motion, p));
                return;
            }
            group(ui, p, t.gyro, |ui| {
                for (name, v) in [(t.axis_pitch, st.gyro[0]), (t.axis_yaw, st.gyro[1]), (t.axis_roll, st.gyro[2])] {
                    axis_bar(ui, p, name, v, 500.0, "{}");
                }
            });
            group(ui, p, t.accel, |ui| {
                for (name, v) in [("X", st.accel[0]), ("Y", st.accel[1]), ("Z", st.accel[2])] {
                    axis_bar(ui, p, name, v, 2.0, "{}");
                }
            });
            group(ui, p, t.orientation, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::hover());
                let (roll, pitch) = tilt(st.accel);
                let c = box_corners(rect.center(), 50.0, roll, pitch);
                // Corner order: z (outer), y, x (inner). Edges of a box.
                let edges = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
                for (a, b) in edges {
                    ui.painter().line_segment([c[a], c[b]], Stroke::new(1.5, p.text2));
                }
                // The top face (y = +) in the accent.
                let top = [c[2], c[3], c[7], c[6]];
                ui.painter().add(egui::Shape::closed_line(top.to_vec(), Stroke::new(2.0, p.accent)));
                ui.painter().text(rect.left_bottom() + vec2(0.0, -2.0), egui::Align2::LEFT_BOTTOM, format!("roll {:+.0}   pitch {:+.0}", roll.to_degrees(), pitch.to_degrees()), theme::mono(12.0), p.muted);
                ui.add_space(4.0);
                theme::hint(ui, t.orientation_hint, p);
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_pad_has_no_tilt() {
        let (r, p) = tilt([0.0, 1.0, 0.0]);
        assert!(r.abs() < 1e-6 && p.abs() < 1e-6);
    }

    #[test]
    fn tilt_follows_gravity_and_never_divides_by_zero() {
        let (r, _) = tilt([0.5, 0.866, 0.0]);
        assert!((r.to_degrees() - 30.0).abs() < 0.1);
        let (_, p) = tilt([0.0, 0.866, 0.5]);
        assert!((p.to_degrees() - 30.0).abs() < 0.1);
        let (r, p) = tilt([0.0, 0.0, 0.0]);
        assert!(r.is_finite() && p.is_finite());
    }

    #[test]
    fn the_box_has_eight_distinct_corners() {
        let c = box_corners(pos2(100.0, 100.0), 50.0, 0.3, 0.2);
        for i in 0..8 {
            for j in i + 1..8 {
                assert!((c[i] - c[j]).length() > 1.0);
            }
        }
    }
}
