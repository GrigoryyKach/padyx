//! Overview: the drawn DualSense, live, from the front and from the top edge. Connection, battery and polling
//! rate are in the strip at the top.

use super::{draw, page, App};
use crate::pad::PadSnapshot;
use crate::theme::{self, Pal};
use eframe::egui::{self, vec2, Rect, Ui};

const GAP: f32 = 28.0;
const CAPTION: f32 = 24.0;

fn caption(ui: &Ui, r: Rect, text: &str, p: &Pal) {
    ui.painter().text(r.center_bottom() - vec2(0.0, 2.0), egui::Align2::CENTER_BOTTOM, text, theme::reg(12.0), p.muted);
}

impl App {
    pub(super) fn overview_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let t = self.s();
        page(ui, p, t.nav_overview, Some(t.ov_hint), |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            let w = ui.available_width();
            let avail_h = (ui.ctx().content_rect().height() - 250.0).max(250.0);
            let (fw, fh) = draw::FRONT;
            let (tw, th) = draw::TOP;
            if w >= 720.0 {
                // Side by side: the front view larger, the top view at the same scale or smaller, same centre line.
                let sf = ((w - GAP) * 0.5 / fw).min((avail_h - CAPTION) / fh);
                let st = ((w - GAP - fw * sf) / tw).min(sf * 1.05);
                let (fsz, tsz) = (vec2(fw, fh) * sf, vec2(tw, th) * st);
                let (row, _) = ui.allocate_exact_size(vec2(w, fsz.y + CAPTION), egui::Sense::hover());
                let x0 = row.left() + (w - fsz.x - GAP - tsz.x) / 2.0;
                let fr = Rect::from_min_size(egui::pos2(x0, row.top()), fsz);
                let tr = Rect::from_min_size(egui::pos2(x0 + fsz.x + GAP, row.top() + (fsz.y - tsz.y) / 2.0), tsz);
                draw::pad_front(ui, fr, pad, p);
                draw::pad_top(ui, tr, pad, p);
                let cap = |r: Rect| Rect::from_min_size(egui::pos2(r.left(), row.bottom() - CAPTION), vec2(r.width(), CAPTION));
                caption(ui, cap(fr), t.ov_front, p);
                caption(ui, cap(tr), t.ov_top, p);
            } else {
                // Narrow: stacked.
                let sf = (w / fw).min((avail_h - CAPTION) / fh).min(1.0);
                let st = (w / tw).min(sf * 1.05);
                for (size, name, front) in [(vec2(fw, fh) * sf, t.ov_front, true), (vec2(tw, th) * st, t.ov_top, false)] {
                    let (row, _) = ui.allocate_exact_size(vec2(w, size.y + CAPTION), egui::Sense::hover());
                    let r = Rect::from_center_size(row.center() - vec2(0.0, CAPTION / 2.0), size);
                    if front {
                        draw::pad_front(ui, r, pad, p);
                    } else {
                        draw::pad_top(ui, r, pad, p);
                    }
                    caption(ui, row, name, p);
                    ui.add_space(6.0);
                }
            }
            ui.add_space(8.0);
        });
    }
}
