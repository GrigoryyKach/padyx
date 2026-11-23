//! Flat drawings: the DualSense from the front and from the top edge, a stick plot, a touchpad. Everything is
//! drawn from the pad state, nothing is a picture. The pad is always the white DualSense (a light body with a
//! dark centre panel), in both themes; pressed controls fill with the accent.

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

/// A half outline (top centre to bottom centre, left side) made whole by mirroring it around x = `sum / 2`.
fn mirrored(half: &[P], sum: f32) -> Vec<P> {
    let mut v = half.to_vec();
    v.extend(half[1..half.len() - 1].iter().rev().map(|&(x, y)| (sum - x, y)));
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

/// A filled shape that may be concave: its triangles are computed once with it.
struct Poly {
    pts: Vec<P>,
    tris: Vec<[usize; 3]>,
}

impl Poly {
    fn new(pts: Vec<P>) -> Self {
        Self { tris: triangulate(&pts), pts }
    }
}

/// Draws `text` centred in `r`. `filled` is the part of `r` the fill covers from the top: the label is `on_fill`
/// there and `plain` elsewhere, so it stays readable at any pull.
fn label(painter: &Painter, r: Rect, text: &str, size: f32, plain: Color32, on_fill: Color32, filled: Option<Rect>) {
    let font = theme::medium(size);
    let c = r.center();
    match filled {
        Some(f) if f.height() > 0.5 => {
            let below = Rect::from_min_max(pos2(r.left(), f.bottom()), r.max);
            painter.with_clip_rect(below).text(c, egui::Align2::CENTER_CENTER, text, font.clone(), plain);
            painter.with_clip_rect(f).text(c, egui::Align2::CENTER_CENTER, text, font, on_fill);
        }
        _ => {
            painter.text(c, egui::Align2::CENTER_CENTER, text, font, plain);
        }
    }
}

fn circle_pts(c: P, r: f32) -> Vec<P> {
    (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// A rounded rectangle with a radius per corner (nw, ne, se, sw). Seven points per corner, clockwise on screen.
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

/// A drawing space: pad coordinates (the numbers in this file) on the screen, uniformly scaled and centred.
struct View {
    o: Pos2,
    s: f32,
    org: P,
}

impl View {
    fn new(rect: Rect, size: P, org: P) -> Self {
        let s = (rect.width() / size.0).min(rect.height() / size.1);
        Self { o: rect.center() - vec2(size.0, size.1) * s / 2.0, s, org }
    }
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2(x - self.org.0, y - self.org.1) * self.s
    }
    fn pts(&self, v: &[P]) -> Vec<Pos2> {
        v.iter().map(|q| self.p(q.0, q.1)).collect()
    }
}

/// The colours of the white DualSense. They do not follow the theme: the picture is one object, it only needs
/// an edge that shows on both pages.
struct Tone {
    body: Color32,
    edge: Color32,
    /// A raised control on the body.
    ctl: Color32,
    ctl_edge: Color32,
    glyph: Color32,
    /// The dark centre panel and the triggers.
    panel: Color32,
    panel_hi: Color32,
    panel_edge: Color32,
    ink: Color32,
}

fn tone(p: &Pal) -> Tone {
    let g = Color32::from_rgb;
    Tone {
        body: if p.dark { g(208, 211, 218) } else { g(243, 244, 247) },
        edge: if p.dark { g(146, 150, 160) } else { g(168, 172, 182) },
        ctl: if p.dark { g(236, 238, 243) } else { g(255, 255, 255) },
        ctl_edge: g(158, 162, 172),
        glyph: g(98, 102, 112),
        panel: g(24, 25, 29),
        panel_hi: g(48, 49, 55),
        panel_edge: g(84, 86, 94),
        ink: g(204, 206, 212),
    }
}

// ---------------------------------------------------------------------------------------------------------
// The front view. Coordinates are those of the reference photo, the pad centred on x = 280.
// ---------------------------------------------------------------------------------------------------------

/// The front view's natural size (width, height); the drawing scales to fit a rectangle.
pub const FRONT: (f32, f32) = (452.0, 305.0);
const FRONT_ORG: P = (50.0, 114.0);
/// The pad's centre line. Everything below is placed from it, inside the outline.
const CX: f32 = 276.0;
const TOUCHPAD: [f32; 4] = [CX - 92.0, 119.0, CX + 92.0, 222.0];
const DPAD_C: P = (CX - 144.0, 202.0);
const FACE_C: P = (CX + 144.0, 202.0);
const FACE_D: f32 = 30.0;
const STICK_DX: f32 = 68.0;
const STICK_Y: f32 = 268.0;
const SIDE_BTN_DX: f32 = 110.0;

/// Every control of the front view as (name, x0, y0, x1, y1) in pad coordinates; the drawing and the tests use it.
#[cfg(test)]
fn front_controls() -> Vec<(&'static str, [f32; 4])> {
    let bb = |c: P, hx: f32, hy: f32| [c.0 - hx, c.1 - hy, c.0 + hx, c.1 + hy];
    vec![
        ("touchpad", TOUCHPAD),
        ("dpad", bb(DPAD_C, 42.0, 42.0)),
        ("face", bb(FACE_C, FACE_D + 15.0, FACE_D + 15.0)),
        ("stick-l", bb((CX - STICK_DX, STICK_Y), 36.0, 36.0)),
        ("stick-r", bb((CX + STICK_DX, STICK_Y), 36.0, 36.0)),
        ("create", bb((CX - SIDE_BTN_DX, 146.0), 3.5, 14.0)),
        ("options", bb((CX + SIDE_BTN_DX, 146.0), 3.5, 14.0)),
        ("ps", bb((CX, 268.0), 12.0, 12.0)),
        ("mute", bb((CX, 295.5), 11.0, 3.5)),
        ("grille", bb((CX, 237.0), 18.0, 6.0)),
    ]
}

struct FrontGeo {
    body: Poly,
    panel: Poly,
}

fn front_geo() -> &'static FrontGeo {
    static G: OnceLock<FrontGeo> = OnceLock::new();
    G.get_or_init(|| {
        // Wide flat top, shoulders that round off, nearly straight sides, grips with rounded tips, a shallow U.
        let mut outer = vec![(CX, 126.0), (190.0, 126.0)];
        cubic(&mut outer, (150.0, 124.0), (100.0, 132.0), (80.0, 185.0));
        cubic(&mut outer, (64.0, 235.0), (54.0, 280.0), (55.0, 320.0));
        cubic(&mut outer, (56.0, 355.0), (62.0, 380.0), (70.0, 398.0));
        let k = outer.len() - 1;
        cubic(&mut outer, (80.0, 412.0), (104.0, 416.0), (126.0, 414.0));
        cubic(&mut outer, (148.0, 412.0), (164.0, 398.0), (170.0, 378.0));
        cubic(&mut outer, (180.0, 345.0), (210.0, 312.0), (CX, 308.0));
        // The dark panel: from under the touchpad, down the inside of the grips, to the same bottom.
        let mut panel = vec![(CX, 200.0), (204.0, 200.0)];
        cubic(&mut panel, (186.0, 214.0), (172.0, 250.0), (162.0, 300.0));
        cubic(&mut panel, (150.0, 345.0), (110.0, 380.0), (70.0, 398.0));
        panel.extend_from_slice(&outer[k + 1..]);
        FrontGeo { body: Poly::new(mirrored(&outer, 2.0 * CX)), panel: Poly::new(mirrored(&panel, 2.0 * CX)) }
    })
}

/// Draws the pad from the front, as large as fits `rect`. Every control reflects the live state.
pub fn pad_front(ui: &mut Ui, rect: Rect, pad: &PadSnapshot, p: &Pal) {
    let st = &pad.state;
    let painter = ui.painter_at(rect);
    let v = View::new(rect, FRONT, FRONT_ORG);
    let s = v.s;
    let t = tone(p);
    let g = front_geo();
    let hair = Stroke::new(1.0, t.ctl_edge);
    let on = |b: Btn| st.pressed(b);
    let fill = |pressed: bool| if pressed { p.accent } else { t.ctl };
    let glyph = |pressed: bool| if pressed { p.on_accent } else { t.glyph };
    let dark_fill = |pressed: bool| if pressed { p.accent } else { t.panel_hi };
    let lc = pad.out.light_scaled();
    let lcol = Color32::from_rgb(lc[0], lc[1], lc[2]);

    fill_mesh(&painter, &v.pts(&g.body.pts), &g.body.tris, t.body);
    fill_mesh(&painter, &v.pts(&g.panel.pts), &g.panel.tris, t.panel);
    painter.add(Shape::closed_line(v.pts(&g.body.pts), Stroke::new((1.4 * s).max(1.0), t.edge)));

    // The touchpad overlaps the top edge; its light strips run along both sides and round the lower corners.
    let tp = rr_pts(TOUCHPAD[0], TOUCHPAD[1], TOUCHPAD[2], TOUCHPAD[3], [5.0, 5.0, 26.0, 26.0]);
    painter.add(Shape::convex_polygon(v.pts(&tp), fill(on(Btn::Touchpad)), Stroke::new(1.0, t.edge)));
    let strip = rr_pts(TOUCHPAD[0] - 2.0, 119.0, TOUCHPAD[2] + 2.0, 224.0, [6.0, 6.0, 28.0, 28.0]);
    let width = (3.2 * s).max(2.0);
    for part in [[&strip[21..], &strip[3..7]], [&strip[7..14], &strip[14..21]]] {
        let pts: Vec<Pos2> = part.iter().flat_map(|c| c.iter()).map(|q| v.p(q.0, q.1)).collect();
        painter.add(Shape::line(pts.clone(), PathStroke::new(width + 1.0, Color32::from_rgb(190, 194, 204))));
        painter.add(Shape::line(pts, PathStroke::new(width, lcol)));
    }
    for f in &st.touch {
        if f.active {
            let c = v.p(TOUCHPAD[0] + (TOUCHPAD[2] - TOUCHPAD[0]) * f.x as f32 / TOUCH_W, 119.0 + 103.0 * f.y as f32 / TOUCH_H);
            painter.circle_filled(c, 7.0 * s, if on(Btn::Touchpad) { p.on_accent } else { p.accent });
        }
    }
    for i in 0..5 {
        let lit = pad.out.players & (1 << i) != 0;
        painter.circle_filled(v.p(CX - 30.0 + 15.0 * i as f32, 212.0), (2.3 * s).max(1.5), if lit { Color32::from_rgb(40, 44, 52) } else { Color32::from_rgb(196, 199, 207) });
    }

    // Create and Options: slim pills at the touchpad's top corners.
    for (x, btn) in [(CX - SIDE_BTN_DX, Btn::Create), (CX + SIDE_BTN_DX, Btn::Options)] {
        painter.add(Shape::convex_polygon(v.pts(&rr_pts(x - 3.5, 132.0, x + 3.5, 160.0, [3.5; 4])), fill(on(btn)), hair));
    }

    // Speaker grille, PS button, Mute slot: on the dark panel.
    for r in 0..2 {
        for c in 0..6 {
            painter.circle_filled(v.p(CX - 17.5 + 7.0 * c as f32, 234.0 + 6.0 * r as f32), (1.1 * s).max(0.8), t.panel_edge);
        }
    }
    let psc = v.p(CX, 268.0);
    painter.circle_filled(psc, 12.0 * s, if on(Btn::Ps) { p.accent } else { t.panel });
    painter.circle_stroke(psc, 12.0 * s, Stroke::new(1.0, if on(Btn::Ps) { p.accent } else { t.panel_edge }));
    painter.text(psc, egui::Align2::CENTER_CENTER, "PS", theme::medium(9.0 * s), if on(Btn::Ps) { p.on_accent } else { t.ink });
    painter.add(Shape::convex_polygon(v.pts(&rr_pts(CX - 11.0, 292.0, CX + 11.0, 299.0, [3.5; 4])), dark_fill(on(Btn::Mute)), Stroke::new(1.0, t.panel_edge)));

    // D-pad: four separate rounded arrows.
    let dc = DPAD_C;
    for (btn, ang) in [(Btn::Up, 0.0f32), (Btn::Right, 90.0), (Btn::Down, 180.0), (Btn::Left, 270.0)] {
        let (sn, cs) = ang.to_radians().sin_cos();
        let rot = |x: f32, y: f32| (dc.0 + x * cs - y * sn, dc.1 + x * sn + y * cs);
        let arm: Vec<P> = [(-12.0, -8.0), (-12.0, -29.0), (-8.0, -38.0), (0.0, -41.0), (8.0, -38.0), (12.0, -29.0), (12.0, -8.0)].iter().map(|&(x, y)| rot(x, y)).collect();
        painter.add(Shape::convex_polygon(v.pts(&arm), fill(on(btn)), hair));
        let chev: Vec<Pos2> = [(-5.0, -25.0), (0.0, -31.0), (5.0, -25.0)].iter().map(|&(x, y)| { let q = rot(x, y); v.p(q.0, q.1) }).collect();
        painter.add(Shape::line(chev, PathStroke::new(1.4, glyph(on(btn)))));
    }

    // Face buttons: four separate circles with hairline symbols.
    let fc = FACE_C;
    for (btn, dx, dy) in [(Btn::Triangle, 0.0, -FACE_D), (Btn::Circle, FACE_D, 0.0), (Btn::Cross, 0.0, FACE_D), (Btn::Square, -FACE_D, 0.0)] {
        let c = v.p(fc.0 + dx, fc.1 + dy);
        painter.circle_filled(c, 15.0 * s, fill(on(btn)));
        painter.circle_stroke(c, 15.0 * s, hair);
        let gs = Stroke::new(1.3, glyph(on(btn)));
        let k = 6.0 * s;
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
                painter.rect_stroke(Rect::from_center_size(c, vec2(k * 1.6, k * 1.6)), 1.0, gs, StrokeKind::Middle);
            }
        }
    }

    // Sticks: a well in the panel and a dark cap that shifts toward the stick; the click makes it the accent.
    for (cx, raw, btn) in [(CX - STICK_DX, (st.lx, st.ly), Btn::L3), (CX + STICK_DX, (st.rx, st.ry), Btn::R3)] {
        let c0 = v.p(cx, STICK_Y);
        painter.circle_filled(c0, 36.0 * s, t.panel);
        painter.circle_stroke(c0, 36.0 * s, Stroke::new(1.0, t.panel_edge));
        let (x, y) = stick_norm(raw.0, raw.1);
        let c = c0 + vec2(x, -y) * 8.0 * s;
        let pressed = on(btn);
        painter.circle_filled(c, 29.0 * s, if pressed { p.accent } else { t.panel_hi });
        painter.circle_stroke(c, 29.0 * s, Stroke::new(1.0, if pressed { p.on_accent } else { t.panel_edge }));
        painter.circle_stroke(c, 18.0 * s, Stroke::new(1.0, if pressed { p.on_accent } else { t.panel_edge }));
    }
}

// ---------------------------------------------------------------------------------------------------------
// The top view: the pad seen from its top edge, as if you stood behind it. Like the photo, the pad's right side
// is on the screen's left, so R1 and R2 are left and L1 and L2 are right.
// ---------------------------------------------------------------------------------------------------------

pub const TOP: (f32, f32) = (548.0, 226.0);
const TOP_ORG: P = (14.0, 160.0);

fn top_body() -> &'static Poly {
    static G: OnceLock<Poly> = OnceLock::new();
    G.get_or_init(|| {
        let mut h = vec![(288.0, 206.0), (176.0, 206.0)];
        cubic(&mut h, (120.0, 205.0), (62.0, 205.0), (40.0, 236.0));
        cubic(&mut h, (24.0, 262.0), (18.0, 300.0), (24.0, 336.0));
        cubic(&mut h, (30.0, 364.0), (52.0, 380.0), (90.0, 380.0));
        cubic(&mut h, (130.0, 380.0), (160.0, 372.0), (190.0, 372.0));
        h.push((288.0, 372.0));
        Poly::new(mirrored(&h, 576.0))
    })
}

/// The trigger slab, narrowing toward its end.
fn slab(cx: f32, y0: f32, y1: f32, half: f32, dy: f32) -> Vec<P> {
    rr_pts(cx - half, y0, cx + half, y1, [16.0, 16.0, 40.0, 40.0])
        .into_iter()
        .map(|(x, y)| (cx + (x - cx) * (1.0 - 0.2 * (y - y0) / (y1 - y0)), y + dy))
        .collect()
}

/// Draws the pad from the top edge in `rect`. L1/R1 sink when pressed, L2/R2 fill from the top with the pull.
pub fn pad_top(ui: &mut Ui, rect: Rect, pad: &PadSnapshot, p: &Pal) {
    let st = &pad.state;
    let painter = ui.painter_at(rect);
    let v = View::new(rect, TOP, TOP_ORG);
    let s = v.s;
    let t = tone(p);
    let hair = Stroke::new(1.0, t.ctl_edge);
    let on = |b: Btn| st.pressed(b);
    let lc = pad.out.light_scaled();
    let lcol = Color32::from_rgb(lc[0], lc[1], lc[2]);

    // Stick tops peek over the body; the screen's left one is the right stick. A stick pushed away from you
    // comes toward the camera (lower), one pushed right goes left.
    for (cx, raw, btn) in [(204.0, (st.rx, st.ry), Btn::R3), (372.0, (st.lx, st.ly), Btn::L3)] {
        let (x, y) = stick_norm(raw.0, raw.1);
        let pressed = on(btn);
        let (dx, dy) = (-x * 7.0, y * 4.0 + if pressed { 3.0 } else { 0.0 });
        painter.add(Shape::convex_polygon(v.pts(&rr_pts(cx - 40.0, 196.0, cx + 40.0, 222.0, [10.0; 4])), t.panel, Stroke::new(1.0, t.panel_edge)));
        let cap: Vec<P> = circle_pts((cx + dx, 181.0 + dy), 1.0).into_iter().map(|q| (cx + dx + (q.0 - cx - dx) * 34.0, 181.0 + dy + (q.1 - 181.0 - dy) * 17.0)).collect();
        painter.add(Shape::convex_polygon(v.pts(&cap), if pressed { p.accent } else { t.panel_hi }, Stroke::new(1.0, if pressed { p.on_accent } else { t.panel_edge })));
    }

    let body = top_body();
    fill_mesh(&painter, &v.pts(&body.pts), &body.tris, t.body);
    painter.add(Shape::closed_line(v.pts(&body.pts), Stroke::new((1.4 * s).max(1.0), t.edge)));

    // The touchpad edge with its light strip, and the USB-C port below it.
    let tp = rr_pts(186.0, 211.0, 390.0, 258.0, [22.0; 4]);
    painter.add(Shape::convex_polygon(v.pts(&tp), if on(Btn::Touchpad) { p.accent } else { t.ctl }, hair));
    let strip = rr_pts(186.0, 211.0, 390.0, 259.0, [22.0; 4]);
    for part in [[&strip[21..], &strip[3..7]], [&strip[7..14], &strip[14..21]]] {
        let pts: Vec<Pos2> = part.iter().flat_map(|c| c.iter()).map(|q| v.p(q.0, q.1)).collect();
        painter.add(Shape::line(pts.clone(), PathStroke::new((3.2 * s).max(2.0) + 1.0, Color32::from_rgb(190, 194, 204))));
        painter.add(Shape::line(pts, PathStroke::new((3.2 * s).max(2.0), lcol)));
    }
    painter.add(Shape::line(vec![v.p(208.0, 259.0), v.p(368.0, 259.0)], PathStroke::new(1.5, t.panel)));
    painter.add(Shape::convex_polygon(v.pts(&rr_pts(270.0, 277.0, 306.0, 287.0, [5.0; 4])), t.panel, Stroke::NONE));
    painter.add(Shape::convex_polygon(v.pts(&rr_pts(274.0, 280.0, 302.0, 284.0, [2.0; 4])), t.panel_edge, Stroke::NONE));

    // Triggers hang below the shoulders: L2/R2 pulled fill from the top and slide a little.
    for (cx, val, click, name) in [(113.0, st.trigger_r(), on(Btn::R2), "R2"), (463.0, st.trigger_l(), on(Btn::L2), "L2")] {
        let (y0, y1) = (246.0, 376.0);
        let poly = v.pts(&slab(cx, y0, y1, 42.0, 7.0 * val));
        let bb = Rect::from_points(&poly);
        painter.add(Shape::convex_polygon(poly.clone(), t.panel, Stroke::NONE));
        let mut filled = None;
        if val > 0.01 {
            let f = Rect::from_min_max(bb.min, pos2(bb.right(), bb.top() + bb.height() * val));
            painter.with_clip_rect(f).add(Shape::convex_polygon(poly.clone(), p.accent, Stroke::NONE));
            filled = Some(f);
        }
        painter.add(Shape::closed_line(poly, if click { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, t.panel_edge) }));
        let lr = Rect::from_center_size(bb.center() + vec2(0.0, 4.0 * s), vec2(bb.width() * 0.7, 20.0 * s));
        label(&painter, lr, name, 15.0 * s, t.ink, p.on_accent, filled);
    }
    // Shoulder buttons: bars on top of the shoulders; a press sinks them.
    for (cx, btn, name) in [(113.0, Btn::R1, "R1"), (463.0, Btn::L1, "L1")] {
        let dy = if on(btn) { 3.0 } else { 0.0 };
        let r = v.pts(&rr_pts(cx - 54.0, 204.0 + dy, cx + 54.0, 228.0 + dy, [11.0; 4]));
        painter.add(Shape::convex_polygon(r.clone(), if on(btn) { p.accent } else { t.ctl }, hair));
        let bb = Rect::from_points(&r);
        label(&painter, bb, name, 13.0 * s, t.glyph, p.on_accent, if on(btn) { Some(bb) } else { None });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(v: &[P]) -> [f32; 4] {
        let f = |g: fn(f32, f32) -> f32, i: usize, init: f32| v.iter().map(|q| if i == 0 { q.0 } else { q.1 }).fold(init, g);
        [f(f32::min, 0, f32::MAX), f(f32::min, 1, f32::MAX), f(f32::max, 0, f32::MIN), f(f32::max, 1, f32::MIN)]
    }

    fn inside(poly: &[P], x: f32, y: f32) -> bool {
        let mut c = false;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
        }
        c
    }

    #[test]
    fn the_front_body_has_the_photos_proportions() {
        let b = bounds(&front_geo().body.pts);
        let ratio = (b[2] - b[0]) / (b[3] - b[1]);
        assert!((1.45..=1.65).contains(&ratio), "aspect {ratio}");
    }

    #[test]
    fn every_front_control_sits_inside_the_body_with_a_margin() {
        let g = front_geo();
        let b = bounds(&g.body.pts);
        for (name, r) in front_controls() {
            assert!(r[0] >= b[0] + 8.0 && r[2] <= b[2] - 8.0 && (name == "touchpad" || r[1] >= b[1]) && r[3] <= b[3] - 8.0, "{name} sticks out: {r:?} vs {b:?}");
            // The middle of each side of the box is on the body too, so a control cannot hang over a notch.
            let (mx, my) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
            for (x, y) in [(r[0], my), (r[2], my), (mx, r[3])] {
                assert!(inside(&g.body.pts, x, y), "{name} leaves the outline at {x},{y}");
            }
        }
    }
}
