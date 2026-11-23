//! Flat drawings: the DualSense, a stick plot, a touchpad. Everything is drawn from the pad state, nothing is a
//! picture. Controls that are pressed fill with the accent; the rest are raised surfaces with hairlines.

use crate::pad::{stick_norm, Btn, Finger, PadSnapshot, TOUCH_H, TOUCH_W};
use crate::theme::{self, Pal};
use eframe::egui::{self, epaint::PathStroke, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use std::sync::OnceLock;

type P = (f32, f32);

/// Appends a cubic Bezier from the last point of `v` (10 samples).
fn cubic(v: &mut Vec<P>, p1: P, p2: P, p3: P) {
    let p0 = *v.last().expect("a start point");
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        v.push((a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1));
    }
}

/// A half outline (top centre to bottom centre, left side) made whole by mirroring it around x = 380.
fn mirrored(half: &[P]) -> Vec<P> {
    let mut v = half.to_vec();
    v.extend(half[1..half.len() - 1].iter().rev().map(|&(x, y)| (760.0 - x, y)));
    v
}

/// Ear clipping for a simple polygon (computed once, the shapes never change).
fn triangulate(p: &[P]) -> Vec<[usize; 3]> {
    let n = p.len();
    let area: f32 = (0..n).map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1).sum();
    let mut idx: Vec<usize> = if area > 0.0 { (0..n).collect() } else { (0..n).rev().collect() };
    let cross = |a: P, b: P, c: P| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let mut out = Vec::new();
    while idx.len() > 3 {
        let m = idx.len();
        let mut cut = None;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (p[ia], p[ib], p[ic]);
            if cross(a, b, c) <= 1e-6 {
                continue;
            }
            let blocked = idx.iter().any(|&j| j != ia && j != ib && j != ic && cross(a, b, p[j]) >= 0.0 && cross(b, c, p[j]) >= 0.0 && cross(c, a, p[j]) >= 0.0);
            if !blocked {
                cut = Some(i);
                out.push([ia, ib, ic]);
                break;
            }
        }
        match cut {
            Some(i) => {
                idx.remove(i);
            }
            None => break,
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// The DualSense in a 768 x 576 box, as measured on a top-down product photo: a flat top with the touchpad
/// jutting up, shoulders that round off, nearly straight sides into long grips with rounded tips, and a wide
/// shallow U between them. The face is two-tone: the central plate and the two wings, with a seam between.
struct Geo {
    outline: Vec<P>,
    outline_tris: Vec<[usize; 3]>,
    plate: Vec<P>,
    plate_tris: Vec<[usize; 3]>,
    /// The left seam, from the top edge down to the inner edge of the grip.
    seam: Vec<P>,
}

fn geo() -> &'static Geo {
    static G: OnceLock<Geo> = OnceLock::new();
    G.get_or_init(|| {
        let mut top = vec![(380.0, 3.0), (300.0, 3.0)];
        cubic(&mut top, (275.0, 6.0), (255.0, 11.0), (225.0, 11.0));
        // The wing's outer edge, shoulder to grip tip and up the inner edge, then the bottom U.
        let mut outer = top.clone();
        outer.push((140.0, 11.0));
        cubic(&mut outer, (70.0, 13.0), (20.0, 70.0), (6.0, 160.0));
        cubic(&mut outer, (0.0, 215.0), (-2.0, 255.0), (0.0, 290.0));
        cubic(&mut outer, (2.0, 350.0), (4.0, 405.0), (14.0, 455.0));
        cubic(&mut outer, (22.0, 488.0), (40.0, 512.0), (66.0, 512.0));
        cubic(&mut outer, (92.0, 512.0), (112.0, 496.0), (118.0, 470.0));
        cubic(&mut outer, (124.0, 440.0), (120.0, 410.0), (132.0, 386.0));
        let k = outer.len() - 1;
        cubic(&mut outer, (148.0, 362.0), (190.0, 352.0), (250.0, 354.0));
        cubic(&mut outer, (300.0, 356.0), (340.0, 358.0), (380.0, 358.0));
        // The plate: the top, the seam, the same bottom.
        let mut plate = top.clone();
        let s0 = plate.len() - 1;
        cubic(&mut plate, (232.0, 60.0), (234.0, 100.0), (232.0, 135.0));
        cubic(&mut plate, (230.0, 165.0), (185.0, 172.0), (160.0, 215.0));
        cubic(&mut plate, (142.0, 245.0), (136.0, 300.0), (133.0, 350.0));
        plate.push((132.0, 386.0));
        let s1 = plate.len() - 1;
        plate.extend_from_slice(&outer[k + 1..]);
        let seam = plate[s0..=s1].to_vec();
        let outline = mirrored(&outer);
        let plate = mirrored(&plate);
        Geo { outline_tris: triangulate(&outline), plate_tris: triangulate(&plate), outline, plate, seam }
    })
}

fn fill_mesh(painter: &Painter, m: &Map, pts: &[P], tris: &[[usize; 3]], col: Color32) {
    let mut mesh = egui::Mesh::default();
    for q in pts {
        mesh.colored_vertex(m.pt(q.0, q.1), col);
    }
    for t in tris {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    painter.add(Shape::mesh(mesh));
}

/// Draws `text` centred in `r`, in `plain` colour where the fill (`filled`, a part of `r`) does not reach and in
/// `on_fill` colour where it does, so the label stays readable at any trigger value.
fn label(painter: &Painter, r: Rect, text: &str, size: f32, plain: Color32, on_fill: Color32, filled: Option<Rect>) {
    let font = theme::medium(size);
    let c = r.center();
    match filled {
        Some(f) if f.height() > 0.5 => {
            let above = Rect::from_min_max(r.min, pos2(r.right(), f.top()));
            painter.with_clip_rect(above).text(c, egui::Align2::CENTER_CENTER, text, font.clone(), plain);
            painter.with_clip_rect(f).text(c, egui::Align2::CENTER_CENTER, text, font, on_fill);
        }
        _ => {
            painter.text(c, egui::Align2::CENTER_CENTER, text, font, plain);
        }
    }
}

struct Map {
    o: Pos2,
    s: f32,
}

impl Map {
    /// Pad coordinates: x 0..760 (plus a margin), y from -44 (the triggers) to 512 (the grip tips).
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2(x + 4.0, y + 44.0) * self.s
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1))
    }
}

pub const PAD_W: f32 = 768.0;
pub const PAD_H: f32 = 560.0;

/// Draws the controller in `rect`, as large as fits. Every control reflects the live state.
pub fn dualsense(ui: &mut Ui, rect: Rect, pad: &PadSnapshot, p: &Pal) {
    let st = &pad.state;
    let painter = ui.painter_at(rect);
    let s = (rect.width() / PAD_W).min(rect.height() / PAD_H);
    let o = rect.center() - vec2(PAD_W, PAD_H) * s / 2.0;
    let m = Map { o, s };
    let hair = Stroke::new(1.0, p.line);
    let g = geo();

    // Body: the wings, the central plate on top, a hairline seam, then the silhouette outline.
    let outline_pts: Vec<Pos2> = g.outline.iter().map(|q| m.pt(q.0, q.1)).collect();
    fill_mesh(&painter, &m, &g.outline, &g.outline_tris, p.card);
    fill_mesh(&painter, &m, &g.plate, &g.plate_tris, p.sidebar);
    let seam_stroke = Stroke::new(1.0, p.line_hi);
    for mirror in [false, true] {
        let pts: Vec<Pos2> = g.seam.iter().map(|&(x, y)| m.pt(if mirror { 760.0 - x } else { x }, y)).collect();
        painter.add(Shape::line(pts, PathStroke::new(seam_stroke.width, seam_stroke.color)));
    }
    painter.add(Shape::closed_line(outline_pts, Stroke::new((1.5 * s).max(1.0), p.line_hi)));

    let on = |b: Btn| st.pressed(b);
    let fill_for = |pressed: bool| if pressed { p.accent } else { p.raised };
    // Controls on the dark central plate need another shade than the ones on the wings.
    let plate_for = |pressed: bool| if pressed { p.accent } else if p.dark { p.raised } else { p.card };
    let glyph_col = |pressed: bool| if pressed { p.on_accent } else { p.text2 };

    // Triggers (fill by value) and shoulder buttons.
    for (x0, x1, v, click, name) in [(98.0, 218.0, st.trigger_l(), on(Btn::L2), "L2"), (542.0, 662.0, st.trigger_r(), on(Btn::R2), "R2")] {
        let r = m.rect(x0, -44.0, x1, 0.0);
        painter.rect(r, 8.0 * s, p.raised, if click { Stroke::new(1.5, p.accent) } else { hair }, StrokeKind::Inside);
        let h = r.height() * v;
        let mut filled = None;
        if h > 0.5 {
            let f = Rect::from_min_max(pos2(r.left() + 1.0, r.bottom() - h), pos2(r.right() - 1.0, r.bottom() - 1.0));
            painter.rect_filled(f, 6.0 * s, p.accent);
            filled = Some(f);
        }
        label(&painter, r, name, 14.0 * s, p.text2, p.on_accent, filled);
    }
    for (x0, x1, btn, name) in [(96.0, 220.0, Btn::L1, "L1"), (540.0, 664.0, Btn::R1, "R1")] {
        let r = m.rect(x0, 2.0, x1, 26.0);
        painter.rect(r, 12.0 * s, fill_for(on(btn)), hair, StrokeKind::Inside);
        label(&painter, r, name, 13.0 * s, p.text2, p.on_accent, if on(btn) { Some(r) } else { None });
    }

    // D-pad: four separate arrow-shaped pads.
    let dc = (130.0, 118.0);
    for (btn, ang) in [(Btn::Up, 0.0f32), (Btn::Right, 90.0), (Btn::Down, 180.0), (Btn::Left, 270.0)] {
        let (sn, cs) = ang.to_radians().sin_cos();
        // a pad pointing up, in its own frame, then turned and moved out from the centre
        let rot = |x: f32, y: f32| (dc.0 + x * cs - y * sn, dc.1 + x * sn + y * cs);
        let pts: Vec<Pos2> = [(-17.0, -20.0), (-17.0, -38.0), (-12.0, -50.0), (0.0, -58.0), (12.0, -50.0), (17.0, -38.0), (17.0, -20.0)]
            .iter()
            .map(|&(x, y)| {
                let (a, b) = rot(x, y);
                m.pt(a, b)
            })
            .collect();
        painter.add(Shape::convex_polygon(pts, fill_for(on(btn)), hair));
        let (cx, cy) = rot(0.0, -40.0);
        let c = m.pt(cx, cy);
        let k = 5.0 * s;
        let (a, b, d) = match btn {
            Btn::Up => (c + vec2(-k, k * 0.5), c + vec2(0.0, -k * 0.6), c + vec2(k, k * 0.5)),
            Btn::Down => (c + vec2(-k, -k * 0.5), c + vec2(0.0, k * 0.6), c + vec2(k, -k * 0.5)),
            Btn::Left => (c + vec2(k * 0.5, -k), c + vec2(-k * 0.6, 0.0), c + vec2(k * 0.5, k)),
            _ => (c + vec2(-k * 0.5, -k), c + vec2(k * 0.6, 0.0), c + vec2(-k * 0.5, k)),
        };
        painter.add(Shape::line(vec![a, b, d], PathStroke::new(1.5, glyph_col(on(btn)))));
    }

    // Face buttons: a diamond of round buttons.
    let fc = (630.0, 118.0);
    for (btn, dx, dy) in [(Btn::Triangle, 0.0, -50.0), (Btn::Circle, 50.0, 0.0), (Btn::Cross, 0.0, 50.0), (Btn::Square, -50.0, 0.0)] {
        let c = m.pt(fc.0 + dx, fc.1 + dy);
        let r = 22.0 * s;
        painter.circle(c, r, fill_for(on(btn)), hair);
        let gc = glyph_col(on(btn));
        let gs = Stroke::new(1.6, gc);
        let k = 8.0 * s;
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

    // Touchpad (juts up at the top), the light strips along the seam, the fingers, the player LEDs.
    let tp = m.rect(242.0, 6.0, 518.0, 138.0);
    let cr = CornerRadius { nw: (4.0 * s) as u8, ne: (4.0 * s) as u8, sw: (28.0 * s) as u8, se: (28.0 * s) as u8 };
    painter.rect(tp, cr, plate_for(on(Btn::Touchpad)), hair, StrokeKind::Inside);
    let lc = pad.out.light_scaled();
    let lcol = Color32::from_rgb(lc[0], lc[1], lc[2]);
    for mirror in [false, true] {
        let pts: Vec<Pos2> = g.seam[..=10].iter().map(|&(x, y)| m.pt(if mirror { 760.0 - x } else { x }, y)).collect();
        painter.add(Shape::line(pts, PathStroke::new(3.5 * s.max(0.7), lcol)));
    }
    for f in &st.touch {
        if f.active {
            let c = finger_pos(tp, f);
            painter.circle_filled(c, 7.0 * s, if on(Btn::Touchpad) { p.on_accent } else { p.accent });
        }
    }
    for i in 0..5 {
        let lit = pad.out.players & (1 << i) != 0;
        painter.circle_filled(m.pt(356.0 + 12.0 * i as f32, 160.0), 2.6 * s.max(0.7), if lit { p.text } else { p.line_hi });
    }

    // Create, Options, PS, Mute.
    for (cx, btn) in [(200.0, Btn::Create), (560.0, Btn::Options)] {
        painter.rect(m.rect(cx - 6.0, 30.0, cx + 6.0, 62.0), 6.0 * s, fill_for(on(btn)), hair, StrokeKind::Inside);
    }
    painter.circle(m.pt(380.0, 222.0), 14.0 * s, plate_for(on(Btn::Ps)), hair);
    painter.circle_stroke(m.pt(380.0, 222.0), 5.0 * s, Stroke::new(1.4, glyph_col(on(Btn::Ps))));
    painter.rect(m.rect(364.0, 262.0, 396.0, 274.0), 6.0 * s, plate_for(on(Btn::Mute)), hair, StrokeKind::Inside);

    // Sticks, closer together than the face buttons: a ring, a cap that follows the stick; the click fills the cap.
    let cap_idle = if p.dark { p.card } else { p.raised };
    for (cx, cy, raw, btn) in [(244.0, 214.0, (st.lx, st.ly), Btn::L3), (516.0, 214.0, (st.rx, st.ry), Btn::R3)] {
        let c = m.pt(cx, cy);
        painter.circle(c, 54.0 * s, plate_for(false), hair);
        painter.circle_stroke(c, 54.0 * s, Stroke::new((1.5 * s).max(1.0), p.line_hi));
        let (x, y) = stick_norm(raw.0, raw.1);
        let cap = c + vec2(x, -y) * 20.0 * s;
        painter.circle(cap, 31.0 * s, if on(btn) { p.accent } else { cap_idle }, Stroke::new(1.5, if on(btn) { p.accent } else { p.line_hi }));
        painter.circle_stroke(cap, 17.0 * s, Stroke::new(1.0, if on(btn) { p.on_accent } else { p.line }));
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
    after: Option<(f32, f32)>,
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
    // What the game gets after the dead zones (the Xbox emulation): an open ring.
    if let Some(a) = after {
        painter.circle_stroke(at(a.0, a.1), 9.0, Stroke::new(2.0, p.text));
    }
}
