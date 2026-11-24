//! Overview: the rendered DualSense, live. Connection, battery and polling rate are in the strip at the top.

use super::{padview, page, App};
use crate::pad::PadSnapshot;
use crate::theme::Pal;
use eframe::egui::{self, vec2, Ui};

impl App {
    pub(super) fn overview_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let t = self.s();
        page(ui, p, t.nav_overview, Some(t.ov_hint), |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            let w = ui.available_width();
            let avail_h = (ui.ctx().content_rect().height() - 250.0).max(250.0);
            let (nw, nh) = padview::natural_size();
            // As large as fits, never past the pictures' own pixels.
            let s = (w / nw).min(avail_h / nh).min(1.0);
            let size = vec2(nw, nh) * s;
            let (row, _) = ui.allocate_exact_size(vec2(w, size.y), egui::Sense::hover());
            let r = egui::Rect::from_center_size(row.center(), size);
            padview::pad_view(ui, r, pad, p);
            ui.add_space(8.0);
        });
    }
}
