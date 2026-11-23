//! First run: one screen that says what Padyx does, that it all stays on this computer, and offers to start with
//! the computer (off by default).

use super::{widgets, App};
use crate::i18n;
use crate::platform;
use crate::theme::{self, Pal};
use eframe::egui::{self, vec2, Align, Layout, Margin, RichText, ScrollArea, Sense, Ui};

pub struct State {
    pub autostart: bool,
}

impl App {
    pub(super) fn onboarding_view(&mut self, ui: &mut Ui, p: &Pal) {
        let ctx = ui.ctx().clone();
        let t = self.s();
        let lang = self.lang;
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, p.bg);
        let mut start = false;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let col_w = (avail - 48.0).clamp(320.0, 560.0);
            ui.horizontal_top(|ui| {
                ui.add_space(((avail - col_w) / 2.0).max(0.0));
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    ui.add_space(28.0);
                    // Language first: a Russian or Ukrainian reader can switch before reading on.
                    ui.horizontal(|ui| {
                        widgets::mark_glyph(ui.painter(), ui.cursor().min + vec2(14.0, 18.0), p.accent);
                        ui.add_space(32.0);
                        ui.label(RichText::new("Padyx").font(theme::medium(18.0)).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            widgets::combo("ob-language", lang.native_name(), 130.0).show_ui(ui, |ui| {
                                for l in i18n::Lang::ALL {
                                    if ui.selectable_label(l == lang, RichText::new(l.native_name()).font(theme::reg(13.5))).clicked() {
                                        self.lang = l;
                                        self.cfg.language = Some(l.code().to_string());
                                        self.touch_cfg(&ctx);
                                    }
                                }
                            });
                        });
                    });
                    ui.add_space(24.0);
                    ui.label(RichText::new(t.ob_title).font(theme::medium(28.0)).color(p.text));
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(t.ob_lead).font(theme::reg(15.0)).color(p.text2)).wrap());
                    ui.add_space(24.0);

                    theme::section(ui, t.ob_does, p);
                    ui.add_space(8.0);
                    theme::card(p).inner_margin(Margin::same(16)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 10.0;
                        for line in [t.ob_do_1, t.ob_do_2, t.ob_do_3] {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let (r, _) = ui.allocate_exact_size(vec2(16.0, 20.0), Sense::hover());
                                widgets::icon(ui.painter(), widgets::Icon::Check, r.center(), 14.0, p.accent);
                                ui.add(egui::Label::new(RichText::new(line).font(theme::reg(14.0)).color(p.text)).wrap());
                            });
                        }
                    });
                    ui.add_space(12.0);
                    ui.add(egui::Label::new(RichText::new(t.ob_local).font(theme::medium(14.0)).color(p.text)).wrap());
                    ui.add_space(24.0);

                    theme::card(p).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let w = ui.available_width();
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(w - 56.0);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(if cfg!(windows) { t.autostart } else { t.autostart_os }).font(theme::reg(14.0)).color(p.text));
                                theme::hint(ui, t.autostart_hint, p);
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                theme::switch(ui, &mut self.onboarding.autostart, p);
                            });
                        });
                        ui.add_space(8.0);
                        theme::hint(ui, t.ob_autostart_note, p);
                    });
                    ui.add_space(24.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                    if theme::primary_button(ui, rect, ui.scope_id().with("ob-start"), t.ob_start, None, true, p).clicked() {
                        start = true;
                    }
                    ui.add_space(28.0);
                });
            });
        });
        if start {
            self.finish_onboarding();
        }
    }

    fn finish_onboarding(&mut self) {
        self.cfg.onboarded = true;
        self.cfg.autostart = self.onboarding.autostart;
        if self.onboarding.autostart {
            let _ = platform::set_autostart(true);
        }
        self.settings_view.autostart = platform::autostart_enabled();
        self.cfg.save_to(&self.dir);
        if !self.demo && std::env::var_os("PADYX_NO_BACKGROUND").is_none() {
            platform::spawn_background();
        }
    }
}
