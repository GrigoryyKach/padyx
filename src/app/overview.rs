//! Overview: the drawn DualSense, live. Connection, battery and polling rate are in the strip at the top.

use super::{draw, page, App};
use crate::pad::PadSnapshot;
use crate::theme::{self, Pal};
use eframe::egui::{self, Ui};

impl App {
    pub(super) fn overview_view(&mut self, ui: &mut Ui, p: &Pal, pad: &PadSnapshot) {
        let t = self.s();
        page(ui, p, t.nav_overview, Some(t.ov_hint), |_| {}, |ui| {
            self.status_strip(ui, p, pad);
            let w = ui.available_width();
            let h = (w * draw::PAD_H / draw::PAD_W).min(ui.ctx().content_rect().height() - 230.0).max(260.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            draw::dualsense(ui, rect, pad, p);
            ui.add_space(8.0);
            let _ = theme::hint;
        });
    }
}
