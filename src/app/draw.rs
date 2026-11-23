//! Flat drawings: the DualSense, a stick plot, a touchpad. Everything is drawn from the pad state, nothing is a
//! picture. Controls that are pressed fill with the accent; the rest are raised surfaces with hairlines.

use crate::pad::{stick_norm, Btn, Finger, PadSnapshot, TOUCH_H, TOUCH_W};
use crate::theme::{self, Pal};
use eframe::egui::{self, epaint::PathStroke, Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

/// A rounded rectangle as a convex polygon, rotated by `deg` around its centre.
fn rrect(c: (f32, f32), w: f32, h: f32, r: f32, deg: f32) -> Vec<(f32, f32)> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pts = Vec::new();
    let corners = [(w / 2.0 - r, -h / 2.0 + r, -90.0f32), (w / 2.0 - r, h / 2.0 - r, 0.0), (-w / 2.0 + r, h / 2.0 - r, 90.0), (-w / 2.0 + r, -h / 2.0 + r, 180.0)];
    for (cx, cy, a0) in corners {
        for i in 0..=8 {
            let a = (a0 + 90.0 * i as f32 / 8.0).to_radians();
            pts.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    let (s, co) = deg.to_radians().sin_cos();
    pts.into_iter().map(|(x, y)| (c.0 + x * co - y * s, c.1 + x * s + y * co)).collect()
}

struct Map {
    o: Pos2,
    s: f32,
}

impl Map {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2(x, y) * self.s
    }
    fn poly(&self, v: &[(f32, f32)]) -> Vec<Pos2> {
        v.iter().map(|(x, y)| self.pt(*x, *y)).collect()
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1))
    }
}

pub const PAD_W: f32 = 760.0;
pub const PAD_H: f32 = 470.0;

/// Draws the controller in `rect`, as large as fits. Every control reflects the live state.
pub fn dualsense(ui: &mut Ui, rect: Rect, pad: &PadSnapshot, p: &Pal) {
    let st = &pad.state;
    let painter = ui.painter_at(rect);
    let s = (rect.width() / PAD_W).min(rect.height() / PAD_H);
    let o = rect.center() - vec2(PAD_W, PAD_H) * s / 2.0;
    let m = Map { o, s };
    let line = Stroke::new((1.5 * s).max(1.0), p.line_hi);
    let hair = Stroke::new(1.0, p.line);

    // Body: outline first (every part stroked wide in the outline colour), then the fill on top, so the shape is
    // one silhouette without inner seams.
    let body = [rrect((380.0, 200.0), 520.0, 210.0, 70.0, 0.0), rrect((185.0, 322.0), 140.0, 270.0, 66.0, 14.0), rrect((575.0, 322.0), 140.0, 270.0, 66.0, -14.0)];
    for b in &body {
        painter.add(Shape::convex_polygon(m.poly(b), p.line_hi, Stroke::new(3.0 * s.max(0.6), p.line_hi)));
    }
    for b in &body {
        painter.add(Shape::convex_polygon(m.poly(b), p.card, Stroke::NONE));
    }

    let on = |b: Btn| st.pressed(b);
    let fill_for = |pressed: bool| if pressed { p.accent } else { p.raised };
    let glyph_col = |pressed: bool| if pressed { p.on_accent } else { p.text2 };

    // Triggers (fill by value) and shoulder buttons.
    for (x0, x1, v, click) in [(150.0, 250.0, st.trigger_l(), on(Btn::L2)), (510.0, 610.0, st.trigger_r(), on(Btn::R2))] {
        let r = m.rect(x0, 12.0, x1, 56.0);
        painter.rect(r, 8.0 * s, p.raised, if click { Stroke::new(1.5, p.accent) } else { hair }, StrokeKind::Inside);
        let h = r.height() * v;
        if h > 0.5 {
            let f = Rect::from_min_max(pos2(r.left() + 1.0, r.bottom() - h), pos2(r.right() - 1.0, r.bottom() - 1.0));
            painter.rect_filled(f, 6.0 * s, p.accent);
        }
    }
    for (cx, btn) in [(205.0, Btn::L1), (555.0, Btn::R1)] {
        let r = m.rect(cx - 62.0, 66.0, cx + 62.0, 88.0);
        painter.rect(r, 11.0 * s, fill_for(on(btn)), hair, StrokeKind::Inside);
    }

    // D-pad.
    let dc = (195.0, 190.0);
    for (btn, dx, dy, w, h) in [(Btn::Up, 0.0, -25.0, 28.0, 30.0), (Btn::Down, 0.0, 25.0, 28.0, 30.0), (Btn::Left, -25.0, 0.0, 30.0, 28.0), (Btn::Right, 25.0, 0.0, 30.0, 28.0)] {
        let r = Rect::from_center_size(m.pt(dc.0 + dx, dc.1 + dy), vec2(w, h) * s);
        painter.rect(r, 6.0 * s, fill_for(on(btn)), hair, StrokeKind::Inside);
        // a small arrow
        let c = r.center();
        let k = 5.0 * s;
        let (a, b, d) = match btn {
            Btn::Up => (c + vec2(-k, k * 0.5), c + vec2(0.0, -k * 0.6), c + vec2(k, k * 0.5)),
            Btn::Down => (c + vec2(-k, -k * 0.5), c + vec2(0.0, k * 0.6), c + vec2(k, -k * 0.5)),
            Btn::Left => (c + vec2(k * 0.5, -k), c + vec2(-k * 0.6, 0.0), c + vec2(k * 0.5, k)),
            _ => (c + vec2(-k * 0.5, -k), c + vec2(k * 0.6, 0.0), c + vec2(-k * 0.5, k)),
        };
        painter.add(Shape::line(vec![a, b, d], PathStroke::new(1.5, glyph_col(on(btn)))));
    }
    painter.rect_filled(Rect::from_center_size(m.pt(dc.0, dc.1), vec2(20.0, 20.0) * s), 4.0 * s, p.raised);

    // Face buttons.
    let fc = (565.0, 190.0);
    for (btn, dx, dy) in [(Btn::Triangle, 0.0, -42.0), (Btn::Circle, 42.0, 0.0), (Btn::Cross, 0.0, 42.0), (Btn::Square, -42.0, 0.0)] {
        let c = m.pt(fc.0 + dx, fc.1 + dy);
        let r = 19.0 * s;
        painter.circle(c, r, fill_for(on(btn)), hair);
        let gc = glyph_col(on(btn));
        let gs = Stroke::new(1.6, gc);
        let k = 7.0 * s;
        match btn {
            Btn::Triangle => {
                painter.add(Shape::closed_line(vec![c + vec2(0.0, -k), c + vec2(k * 0.95, k * 0.7), c + vec2(-k * 0.95, k * 0.7)], gs));
            }
            Btn::Circle => {
                painter.circle_stroke(c, k * 0.95, gs);
            }
            Btn::Cross => {
                painter.line_segment([c + vec2(-k * 0.8, -k * 0.8), c + vec2(k * 0.8, k * 0.8)], gs);
                painter.line_segment([c + vec2(-k * 0.8, k * 0.8), c + vec2(k * 0.8, -k * 0.8)], gs);
            }
            _ => {
                painter.rect_stroke(Rect::from_center_size(c, vec2(k * 1.6, k * 1.6)), 1.5, gs, StrokeKind::Middle);
            }
        }
    }

    // Touchpad with the lightbar on both sides and the fingers.
    let tp = m.rect(292.0, 108.0, 468.0, 214.0);
    painter.rect(tp, 14.0 * s, fill_for(on(Btn::Touchpad)), hair, StrokeKind::Inside);
    let lc = pad.out.light_scaled();
    let lcol = Color32::from_rgb(lc[0], lc[1], lc[2]);
    for x in [282.0, 478.0] {
        painter.line_segment([m.pt(x, 114.0), m.pt(x, 208.0)], Stroke::new(3.0 * s.max(0.7), lcol));
    }
    for f in &st.touch {
        if f.active {
            let c = finger_pos(tp, f);
            painter.circle_filled(c, 7.0 * s, if on(Btn::Touchpad) { p.on_accent } else { p.accent });
        }
    }
    for i in 0..5 {
        let lit = pad.out.players & (1 << i) != 0;
        painter.circle_filled(m.pt(340.0 + 20.0 * i as f32, 228.0), 3.0 * s.max(0.7), if lit { p.text } else { p.line_hi });
    }

    // Create, Options, PS, Mute.
    for (cx, btn) in [(262.0, Btn::Create), (498.0, Btn::Options)] {
        painter.rect(m.rect(cx - 6.0, 130.0, cx + 6.0, 168.0), 6.0 * s, fill_for(on(btn)), hair, StrokeKind::Inside);
    }
    painter.circle(m.pt(380.0, 256.0), 13.0 * s, fill_for(on(Btn::Ps)), hair);
    painter.circle_stroke(m.pt(380.0, 256.0), 5.0 * s, Stroke::new(1.4, glyph_col(on(Btn::Ps))));
    painter.rect(m.rect(366.0, 280.0, 394.0, 290.0), 5.0 * s, fill_for(on(Btn::Mute)), hair, StrokeKind::Inside);

    // Sticks: a ring, a cap that follows the stick; the click (L3, R3) fills the cap.
    for (cx, cy, raw, btn) in [(292.0, 318.0, (st.lx, st.ly), Btn::L3), (468.0, 318.0, (st.rx, st.ry), Btn::R3)] {
        let c = m.pt(cx, cy);
        painter.circle(c, 46.0 * s, p.raised, hair);
        painter.circle_stroke(c, 46.0 * s, line);
        let (x, y) = stick_norm(raw.0, raw.1);
        let cap = c + vec2(x, -y) * 20.0 * s;
        painter.circle(cap, 26.0 * s, if on(btn) { p.accent } else { p.card }, Stroke::new(1.5, if on(btn) { p.accent } else { p.line_hi }));
        painter.circle_stroke(cap, 14.0 * s, Stroke::new(1.0, if on(btn) { p.on_accent } else { p.line }));
    }
}

/// Where a finger is on a touchpad drawn in `r`.
pub fn finger_pos(r: Rect, f: &Finger) -> Pos2 {
    pos2(r.left() + r.width() * (f.x as f32 / TOUCH_W), r.top() + r.height() * (f.y as f32 / TOUCH_H))
}

/// A dashed circle, for dead-zone rings.
pub fn dashed_circle(painter: &Painter, c: Pos2, r: f32, stroke: Stroke) {
    let n = ((r * std::f32::consts::TAU) / 8.0).max(12.0) as usize;
    for i in 0..n {
        if i % 2 == 0 {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i as f32 + 1.0) / n as f32 * std::f32::consts::TAU;
            painter.line_segment([c + vec2(a0.cos(), a0.sin()) * r, c + vec2(a1.cos(), a1.sin()) * r], stroke);
        }
    }
}

/// A square stick plot: grid, outer circle, dead-zone rings, an optional reach outline, the trail and the dot.
pub fn stick_plot(
    ui: &mut Ui,
    size: f32,
    p: &Pal,
    pos: (f32, f32),
    trail: &[(f32, f32)],
    rings: &[f32],
    reach: Option<&[(f32, f32)]>,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(rect, 10.0, p.input, Stroke::new(1.0, p.line), StrokeKind::Inside);
    let c = rect.center();
    let r = size / 2.0 - 10.0;
    let at = |x: f32, y: f32| c + vec2(x, -y) * r;
    painter.line_segment([pos2(rect.left() + 6.0, c.y), pos2(rect.right() - 6.0, c.y)], Stroke::new(1.0, p.line));
    painter.line_segment([pos2(c.x, rect.top() + 6.0), pos2(c.x, rect.bottom() - 6.0)], Stroke::new(1.0, p.line));
    painter.circle_stroke(c, r, Stroke::new(1.0, p.line_hi));
    for q in rings {
        dashed_circle(&painter, c, r * q, Stroke::new(1.0, p.muted));
    }
    if let Some(reach) = reach {
        if reach.len() > 2 {
            let pts: Vec<Pos2> = reach.iter().map(|(x, y)| at(*x, *y)).collect();
            painter.add(Shape::closed_line(pts, Stroke::new(1.5, theme::alpha(p.accent, 200))));
        }
    }
    let n = trail.len();
    for (i, w) in trail.windows(2).enumerate() {
        let a = (40.0 + 150.0 * i as f32 / n.max(1) as f32) as u8;
        painter.line_segment([at(w[0].0, w[0].1), at(w[1].0, w[1].1)], Stroke::new(1.5, theme::alpha(p.accent, a)));
    }
    painter.circle_filled(at(pos.0, pos.1), 6.0, p.accent);
    painter.circle_stroke(at(pos.0, pos.1), 6.0, Stroke::new(1.0, p.bg));
}
