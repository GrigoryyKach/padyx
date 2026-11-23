//! Flat drawings: the DualSense, a stick plot, a touchpad. Everything is drawn from the pad state, nothing is a
//! picture. Controls that are pressed fill with the accent; the rest are raised surfaces with hairlines.

use crate::pad::{stick_norm, Btn, Finger, PadSnapshot, TOUCH_H, TOUCH_W};
use crate::theme::{self, Pal};
use eframe::egui::{self, epaint::PathStroke, Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
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
        cubic(&mut outer, (2.0, 335.0), (4.0, 380.0), (14.0, 420.0));
        cubic(&mut outer, (22.0, 450.0), (40.0, 470.0), (66.0, 470.0));
        cubic(&mut outer, (92.0, 470.0), (112.0, 456.0), (118.0, 432.0));
        cubic(&mut outer, (124.0, 415.0), (120.0, 398.0), (132.0, 386.0));
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

fn fill_mesh(painter: &Painter, pts: &[Pos2], tris: &[[usize; 3]], col: Color32) {
    let mut mesh = egui::Mesh::default();
    for q in pts {
        mesh.colored_vertex(*q, col);
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

/// The pad is tilted about 23 degrees around the X axis (its top edge leans away) and seen through a weak
/// perspective. Pad coordinates: x and y on the face (the outline above), z the height above the face.
const TILT: f32 = 24.0;
const CAM: f32 = 1500.0;
/// How deep the body is below the face.
const THICK: f32 = 46.0;

struct Map {
    o: Pos2,
    s: f32,
    sn: f32,
    cs: f32,
}

impl Map {
    fn new(o: Pos2, s: f32) -> Self {
        let (sn, cs) = TILT.to_radians().sin_cos();
        Self { o, s, sn, cs }
    }
    /// A point of the pad on the screen.
    fn p(&self, x: f32, y: f32, z: f32) -> Pos2 {
        let (xc, yc) = (x - 380.0, y - 240.0);
        let far = -yc * self.sn - z * self.cs;
        let k = CAM / (CAM + far);
        self.o + vec2(400.0 + xc * k, 272.0 + (yc * self.cs - z * self.sn) * k) * self.s
    }
    fn pts(&self, v: &[P], z: f32) -> Vec<Pos2> {
        v.iter().map(|q| self.p(q.0, q.1, z)).collect()
    }
}

pub const PAD_W: f32 = 800.0;
pub const PAD_H: f32 = 520.0;

/// The convex hull of screen points (Andrew's monotone chain).
fn hull(pts: &[Pos2]) -> Vec<Pos2> {
    let mut p: Vec<P> = pts.iter().map(|q| (q.x, q.y)).collect();
    p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    p.dedup();
    let cross = |o: P, a: P, b: P| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut h: Vec<P> = Vec::new();
    for pass in 0..2 {
        let start = h.len();
        let it: Box<dyn Iterator<Item = &P>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in it {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    h.into_iter().map(|(x, y)| pos2(x, y)).collect()
}

fn circle_pts(c: P, r: f32) -> Vec<P> {
    (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// A rounded rectangle with a radius per corner (nw, ne, se, sw).
fn rr_pts(x0: f32, y0: f32, x1: f32, y1: f32, r: [f32; 4]) -> Vec<P> {
    let mut v = Vec::new();
    let corners = [((x0 + r[0], y0 + r[0]), 180.0f32, r[0]), ((x1 - r[1], y0 + r[1]), 270.0, r[1]), ((x1 - r[2], y1 - r[2]), 0.0, r[2]), ((x0 + r[3], y1 - r[3]), 90.0, r[3])];
    for (c, a0, rad) in corners {
        for i in 0..=6 {
            let a = (a0 + 90.0 * i as f32 / 6.0).to_radians();
            v.push((c.0 + rad * a.cos(), c.1 + rad * a.sin()));
        }
    }
    v
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// A convex solid: the walls are the hull of the base and the top, the top is drawn over them.
fn solid(painter: &Painter, base: &[Pos2], top: &[Pos2], top_fill: Color32, wall: Color32, stroke: Stroke) {
    let all: Vec<Pos2> = base.iter().chain(top.iter()).copied().collect();
    painter.add(Shape::convex_polygon(hull(&all), wall, stroke));
    painter.add(Shape::convex_polygon(top.to_vec(), top_fill, stroke));
}

/// Draws the controller in `rect`, as large as fits. Every control reflects the live state.
pub fn dualsense(ui: &mut Ui, rect: Rect, pad: &PadSnapshot, p: &Pal) {
    let st = &pad.state;
    let painter = ui.painter_at(rect);
    let s = (rect.width() / PAD_W).min(rect.height() / PAD_H);
    let o = rect.center() - vec2(PAD_W, PAD_H) * s / 2.0;
    let m = Map::new(o, s);
    let hair = Stroke::new(1.0, p.line);
    let edge = Stroke::new((1.5 * s).max(1.0), p.line_hi);
    let g = geo();
    // Flat shades: a top face, and darker walls by how much they face the viewer.
    let shade = |c: Color32, k: f32| mix(c, if p.dark { Color32::BLACK } else { Color32::from_gray(135) }, k);

    // Body: the extruded walls (by the direction they face), then the wings, the central plate, the seam, the edge.
    {
        let n = g.outline.len();
        let area: f32 = (0..n).map(|i| g.outline[i].0 * g.outline[(i + 1) % n].1 - g.outline[(i + 1) % n].0 * g.outline[i].1).sum();
        let mut mesh = egui::Mesh::default();
        let top = m.pts(&g.outline, 0.0);
        let low = m.pts(&g.outline, -THICK);
        for i in 0..n {
            let j = (i + 1) % n;
            let (dx, dy) = (g.outline[j].0 - g.outline[i].0, g.outline[j].1 - g.outline[i].1);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            let ny = if area > 0.0 { -dx / len } else { dx / len };
            let col = if ny > 0.6 {
                shade(p.card, 0.5)
            } else if ny > 0.2 {
                shade(p.card, 0.38)
            } else if ny > -0.15 {
                shade(p.card, 0.26)
            } else {
                continue;
            };
            let b = mesh.vertices.len() as u32;
            for q in [top[i], top[j], low[j], low[i]] {
                mesh.colored_vertex(q, col);
            }
            mesh.add_triangle(b, b + 1, b + 2);
            mesh.add_triangle(b, b + 2, b + 3);
        }
        painter.add(Shape::mesh(mesh));
        painter.add(Shape::closed_line(low, Stroke::new(1.0, p.line_hi)));
    }
    let top_pts = m.pts(&g.outline, 0.0);
    fill_mesh(&painter, &top_pts, &g.outline_tris, p.card);
    fill_mesh(&painter, &m.pts(&g.plate, 0.0), &g.plate_tris, p.sidebar);
    for mirror in [false, true] {
        let pts: Vec<Pos2> = g.seam.iter().map(|&(x, y)| m.p(if mirror { 760.0 - x } else { x }, y, 0.0)).collect();
        painter.add(Shape::line(pts, PathStroke::new(1.0, p.line_hi)));
    }
    painter.add(Shape::closed_line(top_pts, edge));

    let on = |b: Btn| st.pressed(b);
    let fill_for = |pressed: bool| if pressed { p.accent } else { p.raised };
    // Controls on the dark central plate need another shade than the ones on the wings.
    let plate_for = |pressed: bool| if pressed { p.accent } else if p.dark { p.raised } else { p.card };
    let glyph_col = |pressed: bool| if pressed { p.on_accent } else { p.text2 };
    let wall_for = |pressed: bool| if pressed { shade(p.accent, 0.35) } else { shade(p.raised, 0.4) };

    // Light strips along the seam, the touchpad, the fingers, the player LEDs: all on the face.
    let lc = pad.out.light_scaled();
    let lcol = Color32::from_rgb(lc[0], lc[1], lc[2]);
    for mirror in [false, true] {
        let pts: Vec<Pos2> = g.seam[..=10].iter().map(|&(x, y)| m.p(if mirror { 760.0 - x } else { x }, y, 0.0)).collect();
        painter.add(Shape::line(pts, PathStroke::new(3.5 * s.max(0.7), lcol)));
    }
    let (tx0, ty0, tx1, ty1) = (242.0, 6.0, 518.0, 138.0);
    painter.add(Shape::convex_polygon(m.pts(&rr_pts(tx0, ty0, tx1, ty1, [4.0, 4.0, 28.0, 28.0]), 0.0), plate_for(on(Btn::Touchpad)), hair));
    for f in &st.touch {
        if f.active {
            let c = m.p(tx0 + (tx1 - tx0) * f.x as f32 / TOUCH_W, ty0 + (ty1 - ty0) * f.y as f32 / TOUCH_H, 0.0);
            painter.circle_filled(c, 7.0 * s, if on(Btn::Touchpad) { p.on_accent } else { p.accent });
        }
    }
    for i in 0..5 {
        let lit = pad.out.players & (1 << i) != 0;
        painter.circle_filled(m.p(356.0 + 12.0 * i as f32, 160.0, 0.0), 2.6 * s.max(0.7), if lit { p.text } else { p.line_hi });
    }

    // Create, Options, PS, Mute.
    for (cx, btn) in [(200.0, Btn::Create), (560.0, Btn::Options)] {
        let base = rr_pts(cx - 6.0, 30.0, cx + 6.0, 62.0, [6.0; 4]);
        let z = if on(btn) { 1.5 } else { 3.0 };
        solid(&painter, &m.pts(&base, 0.0), &m.pts(&base, z), fill_for(on(btn)), wall_for(on(btn)), hair);
    }
    {
        let z = if on(Btn::Ps) { 1.5 } else { 3.0 };
        let base = circle_pts((380.0, 222.0), 14.0);
        solid(&painter, &m.pts(&base, 0.0), &m.pts(&base, z), plate_for(on(Btn::Ps)), wall_for(on(Btn::Ps)), hair);
        painter.circle_stroke(m.p(380.0, 222.0, z), 5.0 * s, Stroke::new(1.4, glyph_col(on(Btn::Ps))));
        let base = rr_pts(364.0, 262.0, 396.0, 274.0, [6.0; 4]);
        let z = if on(Btn::Mute) { 1.0 } else { 2.5 };
        solid(&painter, &m.pts(&base, 0.0), &m.pts(&base, z), plate_for(on(Btn::Mute)), wall_for(on(Btn::Mute)), hair);
    }

    // D-pad: four separate arrow-shaped pads.
    let dc = (130.0, 118.0);
    for (btn, ang) in [(Btn::Up, 0.0f32), (Btn::Right, 90.0), (Btn::Down, 180.0), (Btn::Left, 270.0)] {
        let (sn, cs) = ang.to_radians().sin_cos();
        let rot = |x: f32, y: f32| (dc.0 + x * cs - y * sn, dc.1 + x * sn + y * cs);
        let base: Vec<P> = [(-17.0, -20.0), (-17.0, -38.0), (-12.0, -50.0), (0.0, -58.0), (12.0, -50.0), (17.0, -38.0), (17.0, -20.0)].iter().map(|&(x, y)| rot(x, y)).collect();
        let z = if on(btn) { 3.0 } else { 6.0 };
        solid(&painter, &m.pts(&base, 0.0), &m.pts(&base, z), fill_for(on(btn)), wall_for(on(btn)), hair);
        let (cx, cy) = rot(0.0, -40.0);
        let c = m.p(cx, cy, z);
        let k = 5.0 * s;
        let (a, b, d) = match btn {
            Btn::Up => (c + vec2(-k, k * 0.5), c + vec2(0.0, -k * 0.6), c + vec2(k, k * 0.5)),
            Btn::Down => (c + vec2(-k, -k * 0.5), c + vec2(0.0, k * 0.6), c + vec2(k, -k * 0.5)),
            Btn::Left => (c + vec2(k * 0.5, -k), c + vec2(-k * 0.6, 0.0), c + vec2(k * 0.5, k)),
            _ => (c + vec2(-k * 0.5, -k), c + vec2(k * 0.6, 0.0), c + vec2(-k * 0.5, k)),
        };
        painter.add(Shape::line(vec![a, b, d], PathStroke::new(1.5, glyph_col(on(btn)))));
    }

    // Face buttons: a diamond of short round buttons.
    let fc = (630.0, 118.0);
    for (btn, dx, dy) in [(Btn::Triangle, 0.0, -50.0), (Btn::Circle, 50.0, 0.0), (Btn::Cross, 0.0, 50.0), (Btn::Square, -50.0, 0.0)] {
        let base = circle_pts((fc.0 + dx, fc.1 + dy), 22.0);
        let z = if on(btn) { 3.0 } else { 7.0 };
        solid(&painter, &m.pts(&base, 0.0), &m.pts(&base, z), fill_for(on(btn)), wall_for(on(btn)), hair);
        let c = m.p(fc.0 + dx, fc.1 + dy, z);
        let gs = Stroke::new(1.6, glyph_col(on(btn)));
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

    // Sticks: a well in the face and a short cylinder that leans and shifts toward the stick; the click sinks it.
    for (cx, cy, raw, btn) in [(244.0, 214.0, (st.lx, st.ly), Btn::L3), (516.0, 214.0, (st.rx, st.ry), Btn::R3)] {
        let well = m.pts(&circle_pts((cx, cy), 54.0), 0.0);
        painter.add(Shape::convex_polygon(well.clone(), plate_for(false), edge));
        let (x, y) = stick_norm(raw.0, raw.1);
        let (bx, by) = (cx + x * 14.0, cy - y * 14.0);
        let (tx, ty) = (cx + x * 22.0, cy - y * 22.0);
        let pressed = on(btn);
        let z = if pressed { 13.0 } else { 20.0 };
        let top = m.pts(&circle_pts((tx, ty), 31.0), z);
        let cap = if pressed { p.accent } else if p.dark { p.card } else { p.raised };
        solid(&painter, &m.pts(&circle_pts((bx, by), 31.0), 0.0), &top, cap, wall_for(pressed), Stroke::new(1.0, p.line_hi));
        painter.add(Shape::closed_line(m.pts(&circle_pts((tx, ty), 17.0), z), Stroke::new(1.0, if pressed { p.on_accent } else { p.line })));
    }

    // Triggers: slanted slabs on the top edge, behind the shoulders; a pull rotates them back and fills them.
    for (x0, x1, v, click, name) in [(98.0f32, 218.0f32, st.trigger_l(), on(Btn::L2), "L2"), (542.0, 662.0, st.trigger_r(), on(Btn::R2), "R2")] {
        let (len, thick, yb) = (84.0f32, 14.0f32, 16.0f32);
        let (sp, cp) = (14.0 + 26.0 * v).to_radians().sin_cos();
        let (ty, tz) = (yb - len * sp, len * cp);
        let (by, bz) = (-thick * cp, -thick * sp);
        let fbl = m.p(x0, yb, 0.0);
        let fbr = m.p(x1, yb, 0.0);
        let ftr = m.p(x1, ty, tz);
        let ftl = m.p(x0, ty, tz);
        let bback = |x: f32, y: f32, z: f32| m.p(x, y, z.max(0.0));
        let all = [fbl, fbr, ftr, ftl, bback(x0, yb + by, 0.0), bback(x1, yb + by, 0.0), bback(x1, ty + by, tz + bz), bback(x0, ty + by, tz + bz)];
        painter.add(Shape::convex_polygon(hull(&all), shade(p.raised, 0.4), hair));
        let cap = if p.dark { p.raised_hi } else { p.card };
        painter.add(Shape::convex_polygon(vec![ftl, ftr, all[6], all[7]], cap, hair));
        painter.add(Shape::convex_polygon(vec![fbl, fbr, ftr, ftl], p.raised, if click { Stroke::new(1.5, p.accent) } else { hair }));
        let mut filled = None;
        if v > 0.01 {
            let at = |x: f32| m.p(x, yb + (ty - yb) * v, tz * v);
            let q = vec![fbl, fbr, at(x1), at(x0)];
            painter.add(Shape::convex_polygon(q.clone(), p.accent, Stroke::NONE));
            filled = Some(Rect::from_points(&q));
        }
        label(&painter, Rect::from_points(&[fbl, fbr, ftr, ftl]), name, 14.0 * s, p.text2, p.on_accent, filled);
    }
    // Shoulder buttons: thin bars along the shoulder, in front of the triggers; a press sinks them.
    for (x0, x1, btn, name) in [(96.0f32, 220.0f32, Btn::L1, "L1"), (540.0, 664.0, Btn::R1, "R1")] {
        let base = rr_pts(x0, 4.0, x1, 26.0, [11.0; 4]);
        let z = if on(btn) { 11.0 } else { 16.0 };
        let top = m.pts(&base, z);
        solid(&painter, &m.pts(&base, 0.0), &top, fill_for(on(btn)), wall_for(on(btn)), hair);
        label(&painter, Rect::from_points(&top), name, 13.0 * s, p.text2, p.on_accent, if on(btn) { Some(Rect::from_points(&top)) } else { None });
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
