//! The Overview pad: a rendered DualSense made of transparent layers, composited live from the pad state.
//!
//! The pictures are renders of a 3D model, made once outside the app (see `assets/pad/pad.json` for the layout);
//! the app never touches the model. Layers, bottom to top: the soft shadow and a faint rim, the body (shell with
//! the movable parts removed), the shoulder dents (L1, R1), the lightbar tint, then every movable part as a sprite
//! (idle or pressed; triggers as pull steps; sticks on a tilt grid), each with an accent mask while it is pressed,
//! the touch dots and the small L1/L2/R1/R2 badges.
//!
//! The textures are loaded once. The drawing is a few dozen textured quads a frame.

use crate::pad::{stick_norm, Btn, PadSnapshot, TOUCH_H, TOUCH_W};
use crate::theme::{self, Pal};
use eframe::egui::{self, Color32, Context, Id, Pos2, Rect, TextureFilter, TextureHandle, TextureId, TextureOptions, Ui, pos2, vec2};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

const LAYOUT_JSON: &[u8] = include_bytes!("../../assets/pad/pad.json");
const BODY_PNG: &[u8] = include_bytes!("../../assets/pad/body.png");
const SHADOW_PNG: &[u8] = include_bytes!("../../assets/pad/shadow.png");
const RIM_PNG: &[u8] = include_bytes!("../../assets/pad/rim.png");
const SPRITES_PNG: &[u8] = include_bytes!("../../assets/pad/sprites.png");
const MASKS_PNG: &[u8] = include_bytes!("../../assets/pad/masks.png");

/// Room above the pad (in picture pixels) for the L2 and R2 badges.
const TOP_MARGIN: f32 = 26.0;
/// The trigger pull steps rendered, in percent; the app blends between neighbours.
pub const TRIGGER_STEPS: [&str; 5] = ["s0", "s25", "s50", "s75", "s100"];
/// The stick tilt grid is `TILT_N` by `TILT_N`.
pub const TILT_N: usize = 5;

#[derive(Deserialize, Debug)]
pub struct Sprite {
    /// Where the sprite sits in the picture: x0, y0, x1, y1.
    pub rect: [f32; 4],
    /// Its place in the sprite atlas: x, y, w, h (`None` for mask-only entries).
    pub atlas: Option<[f32; 4]>,
    /// Its silhouette in the mask atlas: x, y, w, h.
    pub mask: Option<[f32; 4]>,
    /// Its outline (a thin ring just inside the silhouette) in the mask atlas, for the pressed highlight.
    #[serde(default)]
    pub ring: Option<[f32; 4]>,
}

#[derive(Deserialize, Debug)]
pub struct Part {
    pub rect: [f32; 4],
    pub sprites: HashMap<String, Sprite>,
}

#[derive(Deserialize, Debug)]
pub struct Layout {
    /// The picture's size in pixels (the base layers).
    pub size: [f32; 2],
    pub parts: HashMap<String, Part>,
    /// The touchpad's visible face, x0, y0, x1, y1.
    pub touchpad: [f32; 4],
}

impl Layout {
    fn sprite(&self, part: &str, name: &str) -> Option<&Sprite> {
        self.parts.get(part)?.sprites.get(name)
    }
    fn rect_of(&self, part: &str) -> Option<[f32; 4]> {
        self.parts.get(part).map(|p| p.rect)
    }
}

/// The layout parsed from the embedded JSON, once.
pub fn layout() -> &'static Layout {
    static L: OnceLock<Layout> = OnceLock::new();
    L.get_or_init(|| serde_json::from_slice(LAYOUT_JSON).expect("assets/pad/pad.json is valid"))
}

/// The picture's size with the badge margin: what a drawing area is fitted around.
pub fn natural_size() -> (f32, f32) {
    let l = layout();
    (l.size[0], l.size[1] + TOP_MARGIN)
}

/// Which two trigger steps a pull (0..1) lies between, and how far toward the upper one (0..1).
pub fn trigger_blend(v: f32) -> (usize, f32) {
    let pos = v.clamp(0.0, 1.0) * (TRIGGER_STEPS.len() - 1) as f32;
    let lo = (pos.floor() as usize).min(TRIGGER_STEPS.len() - 2);
    (lo, pos - lo as f32)
}

/// The tilt-grid cell nearest to a stick position (x right, y up, each -1..1), and how far the stick is from that
/// cell's centre in cells (-0.5..0.5 on each axis). The grid is clamped to the unit disc, like the stick's travel.
pub fn tilt_cell(x: f32, y: f32) -> (usize, usize, f32, f32) {
    let r = x.hypot(y);
    let k = if r > 1.0 { 1.0 / r } else { 1.0 };
    let half = (TILT_N - 1) as f32 / 2.0;
    let (gx, gy) = ((x * k + 1.0) * half, (y * k + 1.0) * half);
    let top = (TILT_N - 1) as f32;
    let (i, j) = (gx.round().clamp(0.0, top), gy.round().clamp(0.0, top));
    (i as usize, j as usize, gx - i, gy - j)
}

/// What the shoulders show. The bumpers (L1, R1) depend only on their own button and the triggers (L2, R2) only on
/// their own pull, so one can never move the other.
pub struct ShoulderPlan {
    /// Sprite name of the left and the right bumper.
    pub bumper: [&'static str; 2],
    /// The two trigger steps to blend and how far toward the upper one, for the left and the right trigger.
    pub trigger: [(usize, f32); 2],
}

pub fn shoulder_plan(bumper_pressed: [bool; 2], trigger_pull: [f32; 2]) -> ShoulderPlan {
    let name = |p: bool| if p { "pressed" } else { "idle" };
    ShoulderPlan { bumper: [name(bumper_pressed[0]), name(bumper_pressed[1])], trigger: [trigger_blend(trigger_pull[0]), trigger_blend(trigger_pull[1])] }
}

pub fn tilt_name(i: usize, j: usize) -> String {
    format!("t{i}{j}")
}

struct Art {
    body: TextureHandle,
    shadow: TextureHandle,
    rim: TextureHandle,
    sprites: TextureHandle,
    masks: TextureHandle,
    sprites_size: Pos2,
    masks_size: Pos2,
}

fn load(ctx: &Context, name: &str, png: &[u8]) -> Option<(TextureHandle, Pos2)> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let color = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());
    // Mipmaps keep the render clean when it is drawn at about half size.
    let opts = TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear));
    Some((ctx.load_texture(name, color, opts), pos2(w as f32, h as f32)))
}

/// The textures, loaded on first use and kept for the life of the window.
fn art(ctx: &Context) -> Option<Arc<Art>> {
    let id = Id::NULL.with("padyx-pad-art");
    if let Some(a) = ctx.data(|d| d.get_temp::<Arc<Art>>(id)) {
        return Some(a);
    }
    let (body, _) = load(ctx, "pad-body", BODY_PNG)?;
    let (shadow, _) = load(ctx, "pad-shadow", SHADOW_PNG)?;
    let (rim, _) = load(ctx, "pad-rim", RIM_PNG)?;
    let (sprites, sprites_size) = load(ctx, "pad-sprites", SPRITES_PNG)?;
    let (masks, masks_size) = load(ctx, "pad-masks", MASKS_PNG)?;
    let a = Arc::new(Art { body, shadow, rim, sprites, masks, sprites_size, masks_size });
    ctx.data_mut(|d| d.insert_temp(id, a.clone()));
    Some(a)
}

/// The picture laid over a screen rectangle: uniform scale, centred, the badge margin on top.
struct Frame {
    o: Pos2,
    s: f32,
}

impl Frame {
    fn new(rect: Rect, l: &Layout) -> Self {
        let (w, h) = (l.size[0], l.size[1] + TOP_MARGIN);
        let s = (rect.width() / w).min(rect.height() / h);
        Self { o: rect.center() - vec2(w, h) * s / 2.0 + vec2(0.0, TOP_MARGIN * s), s }
    }
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2(x, y) * self.s
    }
    fn rect(&self, r: [f32; 4]) -> Rect {
        Rect::from_min_max(self.pt(r[0], r[1]), self.pt(r[2], r[3]))
    }
}

fn uv(r: [f32; 4], size: Pos2) -> Rect {
    Rect::from_min_max(pos2(r[0] / size.x, r[1] / size.y), pos2((r[0] + r[2]) / size.x, (r[1] + r[3]) / size.y))
}

const FULL_UV: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(1.0, 1.0) };

struct Pen<'a> {
    painter: egui::Painter,
    art: &'a Art,
    l: &'a Layout,
    f: Frame,
}

impl Pen<'_> {
    fn quad(&self, tex: TextureId, r: Rect, uv: Rect, tint: Color32) {
        self.painter.image(tex, r, uv, tint);
    }
    /// A sprite from the atlas at its own place, shifted by `shift` picture pixels.
    fn sprite(&self, part: &str, name: &str, tint: Color32, shift: (f32, f32)) {
        let Some(sp) = self.l.sprite(part, name) else { return };
        let Some(a) = sp.atlas else { return };
        let r = self.f.rect([sp.rect[0] + shift.0, sp.rect[1] + shift.1, sp.rect[2] + shift.0, sp.rect[3] + shift.1]);
        self.quad(self.art.sprites.id(), r, uv(a, self.art.sprites_size), tint);
    }
    /// A thin outline along the inside of a sprite's silhouette.
    fn ring(&self, part: &str, name: &str, tint: Color32, shift: (f32, f32)) {
        let Some(sp) = self.l.sprite(part, name) else { return };
        let Some(m) = sp.ring else { return };
        let r = self.f.rect([sp.rect[0] + shift.0, sp.rect[1] + shift.1, sp.rect[2] + shift.0, sp.rect[3] + shift.1]);
        self.quad(self.art.masks.id(), r, uv(m, self.art.masks_size), tint);
    }
    /// The accent (or any colour) in the shape of a sprite's silhouette.
    fn mask(&self, part: &str, name: &str, tint: Color32, shift: (f32, f32)) {
        let Some(sp) = self.l.sprite(part, name) else { return };
        let Some(m) = sp.mask else { return };
        let r = self.f.rect([sp.rect[0] + shift.0, sp.rect[1] + shift.1, sp.rect[2] + shift.0, sp.rect[3] + shift.1]);
        self.quad(self.art.masks.id(), r, uv(m, self.art.masks_size), tint);
    }
}

fn accent(p: &Pal, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), alpha)
}

/// Draws the pad as large as fits `rect`. Every control reflects the live state.
pub fn pad_view(ui: &mut Ui, rect: Rect, pad: &PadSnapshot, p: &Pal) {
    let ctx = ui.ctx().clone();
    let Some(art) = art(&ctx) else { return };
    let l = layout();
    let pen = Pen { painter: ui.painter_at(rect), art: &art, l, f: Frame::new(rect, l) };
    let st = &pad.state;
    let on = |b: Btn| st.pressed(b);
    let whole = pen.f.rect([0.0, 0.0, l.size[0], l.size[1]]);

    // Shadow and rim: the shadow matters on the light page, the rim lifts the black parts on the dark one.
    let (shadow_a, rim_tint) = if p.dark { (110, Color32::from_rgba_unmultiplied(214, 220, 232, 70)) } else { (150, Color32::from_rgba_unmultiplied(70, 76, 92, 120)) };
    pen.quad(art.shadow.id(), whole, FULL_UV, Color32::from_white_alpha(shadow_a));
    pen.quad(art.rim.id(), whole, FULL_UV, rim_tint);
    pen.quad(art.body.id(), whole, FULL_UV, Color32::WHITE);

    // The lightbar strips in the colour the pad is told to show.
    let lc = pad.out.light_scaled();
    if lc.iter().any(|&c| c > 0) {
        pen.mask("lightbar", "glow", Color32::from_rgba_unmultiplied(lc[0], lc[1], lc[2], 120), (0.0, 0.0));
        pen.mask("lightbar", "mask", Color32::from_rgb(lc[0], lc[1], lc[2]), (0.0, 0.0));
    }

    // Buttons: the idle sprite, or the sunk one with the accent on it.
    for (b, id) in [
        (Btn::Up, "up"),
        (Btn::Down, "down"),
        (Btn::Left, "left"),
        (Btn::Right, "right"),
        (Btn::Triangle, "triangle"),
        (Btn::Circle, "circle"),
        (Btn::Cross, "cross"),
        (Btn::Square, "square"),
        (Btn::Create, "create"),
        (Btn::Options, "options"),
        (Btn::Mute, "mute"),
        (Btn::Touchpad, "touchpad"),
    ] {
        if on(b) {
            pen.sprite(id, "pressed", Color32::WHITE, (0.0, 0.0));
            pen.mask(id, "pressed", accent(p, if b == Btn::Touchpad { 45 } else { 80 }), (0.0, 0.0));
            pen.ring(id, "pressed", accent(p, 255), (0.0, 0.0));
        } else {
            pen.sprite(id, "idle", Color32::WHITE, (0.0, 0.0));
        }
    }
    // The PS button is the logo area: it only gets the accent.
    if on(Btn::Ps) {
        pen.mask("ps", "mask", accent(p, 90), (0.0, 0.0));
        pen.ring("ps", "mask", accent(p, 255), (0.0, 0.0));
    }

    // Touch dots on the visible face of the touchpad.
    let tp = l.touchpad;
    let (tw, th) = (tp[2] - tp[0], tp[3] - tp[1]);
    for f in &st.touch {
        if f.active {
            let x = tp[0] + tw * (0.06 + 0.88 * (f.x as f32 / TOUCH_W).clamp(0.0, 1.0));
            let y = tp[1] + th * (0.10 + 0.80 * (f.y as f32 / TOUCH_H).clamp(0.0, 1.0));
            let c = pen.f.pt(x, y);
            let r = (9.0 * pen.f.s).max(4.0);
            pen.painter.circle_filled(c, r + 1.5, Color32::from_black_alpha(60));
            pen.painter.circle_filled(c, r, if on(Btn::Touchpad) { p.on_accent } else { p.accent });
        }
    }

    // Triggers (the blade under the bumper) and bumpers: separate meshes. The trigger cross-fades between pull
    // steps; the bumper sinks when L1/R1 is pressed. Neither has a highlight; the badge colour is the cue.
    let plan = shoulder_plan([on(Btn::L1), on(Btn::R1)], [st.trigger_l(), st.trigger_r()]);
    for (k, (id, bumper)) in [("l2", "l1"), ("r2", "r1")].into_iter().enumerate() {
        let (lo, f) = plan.trigger[k];
        pen.sprite(id, TRIGGER_STEPS[lo], Color32::from_white_alpha(((1.0 - f) * 255.0).round() as u8), (0.0, 0.0));
        pen.sprite(id, TRIGGER_STEPS[lo + 1], Color32::from_white_alpha((f * 255.0).round() as u8), (0.0, 0.0));
        pen.sprite(bumper, plan.bumper[k], Color32::WHITE, (0.0, 0.0));
    }

    // Sticks: the nearest tilt sprite, nudged by the part of a cell the stick is off its centre.
    for (id, raw, b) in [("stick_l", (st.lx, st.ly), Btn::L3), ("stick_r", (st.rx, st.ry), Btn::R3)] {
        let (x, y) = stick_norm(raw.0, raw.1);
        let (i, j, rx, ry) = tilt_cell(x, y);
        let step = |a: &str, b: &str| -> f32 { l.sprite(id, a).zip(l.sprite(id, b)).map_or(0.0, |(p, q)| q.rect[0] - p.rect[0]) };
        let stepy = |a: &str, b: &str| -> f32 { l.sprite(id, a).zip(l.sprite(id, b)).map_or(0.0, |(p, q)| q.rect[1] - p.rect[1]) };
        let half = (TILT_N - 1) as f32;
        let dx = step(&tilt_name(0, 2), &tilt_name(TILT_N - 1, 2)) / half;
        let dy = stepy(&tilt_name(2, 0), &tilt_name(2, TILT_N - 1)) / half;
        let shift = (rx * dx, ry * dy);
        let still = x.hypot(y) < 0.12;
        let pressed = on(b);
        let (name, sh) = if pressed && still { ("pressed".to_string(), (0.0, 0.0)) } else { (tilt_name(i, j), (shift.0, shift.1 + if pressed { 3.0 } else { 0.0 })) };
        pen.sprite(id, &name, Color32::WHITE, sh);
        if pressed {
            pen.mask(id, &name, accent(p, 70), sh);
            pen.ring(id, &name, accent(p, 255), sh);
        }
    }

    // Badges: L2 and R2 above the triggers, L1 and R1 on the shoulders.
    let font = theme::medium((11.5 * pen.f.s / 0.6).clamp(10.0, 13.0));
    for (id, text, over, btn, val) in [("l2", "L2", true, Btn::L2, st.trigger_l()), ("r2", "R2", true, Btn::R2, st.trigger_r()), ("l1", "L1", false, Btn::L1, 0.0), ("r1", "R1", false, Btn::R1, 0.0)] {
        let Some(r) = l.rect_of(id) else { continue };
        let lit = on(btn) || val > 0.05;
        let (cx, cy) = ((r[0] + r[2]) / 2.0, if over { r[1] - 11.0 } else { (r[1] + r[3]) / 2.0 + 17.0 });
        let col = if lit { p.accent } else if over { p.muted } else { Color32::from_rgb(168, 174, 186) };
        pen.painter.text(pen.f.pt(cx, cy), egui::Align2::CENTER_CENTER, text, font.clone(), col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_size(png: &[u8]) -> (f32, f32) {
        let im = image::load_from_memory_with_format(png, image::ImageFormat::Png).expect("a PNG");
        (im.width() as f32, im.height() as f32)
    }

    #[test]
    fn the_layout_json_parses() {
        let l = layout();
        assert!(l.size[0] > 800.0 && l.size[1] > 500.0, "{:?}", l.size);
        assert!(l.touchpad[2] > l.touchpad[0] && l.touchpad[3] > l.touchpad[1]);
        let (base_w, base_h) = png_size(BODY_PNG);
        assert_eq!((base_w, base_h), (l.size[0], l.size[1]), "the body picture and the layout must agree");
        assert_eq!(png_size(SHADOW_PNG), (base_w, base_h));
        assert_eq!(png_size(RIM_PNG), (base_w, base_h));
    }

    #[test]
    fn every_part_has_its_sprites() {
        let l = layout();
        let mut need: Vec<(String, Vec<String>)> = Vec::new();
        for id in ["up", "down", "left", "right", "triangle", "circle", "cross", "square", "create", "options", "mute", "touchpad"] {
            need.push((id.into(), vec!["idle".into(), "pressed".into()]));
        }
        for id in ["l1", "r1"] {
            need.push((id.into(), vec!["idle".into(), "pressed".into()]));
        }
        for id in ["l2", "r2"] {
            need.push((id.into(), TRIGGER_STEPS.iter().map(|s| s.to_string()).collect()));
        }
        for id in ["stick_l", "stick_r"] {
            let mut v: Vec<String> = vec!["pressed".into()];
            for j in 0..TILT_N {
                for i in 0..TILT_N {
                    v.push(tilt_name(i, j));
                }
            }
            need.push((id.into(), v));
        }
        need.push(("ps".into(), vec!["mask".into()]));
        need.push(("lightbar".into(), vec!["mask".into(), "glow".into()]));
        let (sw, sh) = png_size(SPRITES_PNG);
        let (mw, mh) = png_size(MASKS_PNG);
        for (part, names) in need {
            for n in names {
                let sp = l.sprite(&part, &n).unwrap_or_else(|| panic!("{part}/{n} is missing"));
                if !matches!(part.as_str(), "ps" | "lightbar") {
                    let a = sp.atlas.unwrap_or_else(|| panic!("{part}/{n} has no atlas rect"));
                    assert!(a[0] >= 0.0 && a[1] >= 0.0 && a[0] + a[2] <= sw && a[1] + a[3] <= sh, "{part}/{n} leaves the sprite atlas");
                    // The sprite is drawn at its own size.
                    assert!((sp.rect[2] - sp.rect[0] - a[2]).abs() < 0.5 && (sp.rect[3] - sp.rect[1] - a[3]).abs() < 0.5, "{part}/{n} size mismatch");
                }
                let shoulder = matches!(part.as_str(), "l1" | "r1" | "l2" | "r2");
                if n != "idle" && n != "s0" && !shoulder {
                    if !matches!(part.as_str(), "lightbar") && !n.starts_with('s') && n != "idle" {
                        assert!(sp.ring.is_some(), "{part}/{n} has no outline");
                    }
                    let m = sp.mask.unwrap_or_else(|| panic!("{part}/{n} has no mask"));
                    assert!(m[0] >= 0.0 && m[1] >= 0.0 && m[0] + m[2] <= mw && m[1] + m[3] <= mh, "{part}/{n} leaves the mask atlas");
                }
            }
        }
        for (id, part) in &l.parts {
            for (n, sp) in &part.sprites {
                assert!(sp.rect[0] >= 0.0 && sp.rect[1] >= 0.0 && sp.rect[2] <= l.size[0] && sp.rect[3] <= l.size[1], "{id}/{n} sticks out of the picture: {:?}", sp.rect);
            }
        }
    }

    #[test]
    fn bumpers_ignore_the_trigger_and_triggers_ignore_the_bumper() {
        let base = shoulder_plan([false, false], [0.0, 0.0]);
        for pull in [0.0f32, 0.1, 0.33, 0.5, 0.8, 1.0] {
            let p = shoulder_plan([false, false], [pull, pull]);
            assert_eq!(p.bumper, base.bumper, "the bumper sprites must not follow the trigger ({pull})");
            let q = shoulder_plan([true, false], [pull, 1.0 - pull]);
            assert_eq!(q.bumper, ["pressed", "idle"]);
        }
        for pressed in [[false, false], [true, false], [false, true], [true, true]] {
            let p = shoulder_plan(pressed, [0.6, 0.2]);
            assert_eq!(p.trigger, [trigger_blend(0.6), trigger_blend(0.2)], "the trigger must not follow the bumper");
        }
        let l = layout();
        for id in ["l1", "r1"] {
            let idle = l.sprite(id, "idle").unwrap();
            let pressed = l.sprite(id, "pressed").unwrap();
            assert!(idle.atlas.is_some() && pressed.atlas.is_some() && idle.atlas != pressed.atlas, "{id} has two different sprites");
            assert!(idle.mask.is_none() && pressed.mask.is_none() && idle.ring.is_none(), "{id} has no highlight layers");
        }
        for id in ["l2", "r2"] {
            for n in TRIGGER_STEPS {
                let sp = l.sprite(id, n).unwrap();
                assert!(sp.mask.is_none() && sp.ring.is_none(), "{id}/{n} has no highlight layers");
            }
        }
    }

    #[test]
    fn the_tilt_grid_picks_the_nearest_cell() {
        assert_eq!(tilt_cell(0.0, 0.0).0, 2);
        assert_eq!((tilt_cell(0.0, 0.0).0, tilt_cell(0.0, 0.0).1), (2, 2));
        assert_eq!((tilt_cell(1.0, 0.0).0, tilt_cell(1.0, 0.0).1), (4, 2));
        assert_eq!((tilt_cell(-1.0, 0.0).0, tilt_cell(-1.0, 0.0).1), (0, 2));
        assert_eq!((tilt_cell(0.0, 1.0).0, tilt_cell(0.0, 1.0).1), (2, 4), "y is up");
        assert_eq!((tilt_cell(0.0, -1.0).0, tilt_cell(0.0, -1.0).1), (2, 0));
        // A corner of the square is outside the stick's disc: it is pulled onto the rim.
        let (i, j, _, _) = tilt_cell(1.0, 1.0);
        assert!(i == 3 || i == 4);
        assert!(j == 3 || j == 4);
        // Just off a centre: nearest cell and a small residual.
        let (i, j, rx, ry) = tilt_cell(0.2, -0.05);
        assert_eq!((i, j), (2, 2));
        assert!((rx - 0.4).abs() < 1e-4 && (ry + 0.1).abs() < 1e-4, "{rx} {ry}");
        assert!(rx.abs() <= 0.5 && ry.abs() <= 0.5);
        assert_eq!(tilt_name(3, 1), "t31");
    }

    #[test]
    fn the_trigger_steps_interpolate() {
        assert_eq!(trigger_blend(0.0), (0, 0.0));
        let (lo, f) = trigger_blend(0.125);
        assert_eq!(lo, 0);
        assert!((f - 0.5).abs() < 1e-5);
        let (lo, f) = trigger_blend(0.5);
        assert_eq!(lo, 2);
        assert!(f.abs() < 1e-5);
        let (lo, f) = trigger_blend(1.0);
        assert_eq!(lo, 3, "the last step is the upper end of the last pair");
        assert!((f - 1.0).abs() < 1e-5);
        assert_eq!(trigger_blend(7.0), trigger_blend(1.0));
        assert_eq!(trigger_blend(-3.0), trigger_blend(0.0));
    }

    #[test]
    fn no_model_files_are_embedded() {
        // Only rendered pictures and the layout are part of the app.
        assert!(LAYOUT_JSON.starts_with(b"{"));
        for png in [BODY_PNG, SHADOW_PNG, RIM_PNG, SPRITES_PNG, MASKS_PNG] {
            assert!(png.starts_with(b"\x89PNG"));
        }
    }
}
