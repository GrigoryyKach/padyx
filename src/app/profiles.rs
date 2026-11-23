//! The Profiles page: named sets of mapping, dead zones, lightbar colour, macros and turbo; the default one is
//! always there. A profile can be picked by hand or by the game in the foreground (auto-switch), and exported
//! to or imported from a JSON file.
//!
//! "Use" says which profile runs; "Edit" says which one the Emulation, Remap and Macros pages change. They are
//! separate on purpose: you can set a game's profile up while another one is running.

use super::light::SWATCHES;
use super::settings::{open_folder, switch_row};
use super::{group, page, widgets, App};
use crate::emu::foreground::{self, Availability};
use crate::emu::profile::{self, Profile, MAX_EXES, MAX_FILE_BYTES, MAX_PROFILES};
use crate::theme::{self, Btn, Pal};
use eframe::egui::{self, vec2, Align, Layout, RichText, Sense, Stroke, Ui};
use std::path::PathBuf;

#[derive(Default)]
pub struct State {
    /// The path typed for an import.
    pub import_path: String,
    /// The program name typed to add to the edited profile.
    pub new_exe: String,
    /// The rename box, for the profile named in `rename_for`.
    rename: String,
    rename_for: Option<String>,
    /// The last result (text, ok) and the folder an export went to.
    pub msg: Option<(String, bool)>,
    pub export_dir: Option<PathBuf>,
}

enum Act {
    Use(String),
    Edit(Option<String>),
    Export(Option<String>),
    Duplicate(String),
    Delete(String),
    New,
    Import(String),
    Auto(bool),
    Rename(String),
    AddExe,
    RemoveExe(usize),
    Light(Option<[u8; 3]>),
}

fn rgb_of(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

impl App {
    /// Which profile the pages edit: the index in `cfg.profiles`, or `None` for the default one.
    pub fn edit_index(&self) -> Option<usize> {
        let n = self.edit_profile.as_ref()?;
        self.cfg.profiles.iter().position(|p| &p.name == n)
    }

    /// Its name (empty for the default one).
    pub fn edit_name(&self) -> String {
        self.edit_index().map(|i| self.cfg.profiles[i].name.clone()).unwrap_or_default()
    }

    /// The settings the pages edit. The rumble strength and the native-mode pads are the same in every profile,
    /// so they always come from the default settings.
    pub fn edit_cfg(&self) -> crate::emu::map::EmuConfig {
        let mut c = match self.edit_index() {
            Some(i) => self.cfg.profiles[i].emu.clone(),
            None => self.cfg.emu.clone(),
        };
        c.rumble_pct = self.cfg.emu.rumble_pct;
        c.native = self.cfg.emu.native.clone();
        c
    }

    /// Stores what a page changed. `true` if anything differed.
    pub fn store_edit_cfg(&mut self, mut c: crate::emu::map::EmuConfig, ctx: &egui::Context) -> bool {
        c.sanitize();
        if c == self.edit_cfg() {
            return false;
        }
        self.cfg.emu.rumble_pct = c.rumble_pct;
        self.cfg.emu.native = c.native.clone();
        match self.edit_index() {
            Some(i) => {
                c.native.clear();
                self.cfg.profiles[i].emu = c;
            }
            None => self.cfg.emu = c,
        }
        self.touch_cfg(ctx);
        true
    }

    /// The settings that are in force now (the active profile's), for the live views.
    pub fn active_cfg(&self) -> crate::emu::map::EmuConfig {
        let name = &self.snap.emu.profile;
        let p = if name.is_empty() { None } else { self.cfg.profiles.iter().find(|p| &p.name == name) };
        profile::effective(&self.cfg.emu, p)
    }

    fn export_profile(&mut self, name: Option<&str>) {
        let t = self.s();
        let p = match name.and_then(|n| self.cfg.profiles.iter().find(|p| p.name == n)) {
            Some(p) => p.clone(),
            None => Profile { name: "Default".into(), emu: self.cfg.emu.clone(), ..Default::default() },
        };
        let dir = self.dir.join("profiles");
        let path = dir.join(profile::file_name(&p.name));
        let res = std::fs::create_dir_all(&dir).and_then(|_| crate::config::write_atomic(&path, profile::to_json(&p).as_bytes()));
        self.profiles_view.msg = Some(match res {
            Ok(()) => {
                self.profiles_view.export_dir = Some(dir);
                (t.prof_exported.replace("{path}", &path.display().to_string()), true)
            }
            Err(e) => (format!("{}: {e}", t.prof_export_failed), false),
        });
    }

    fn import_profile(&mut self, path: &str) {
        let t = self.s();
        let path = path.trim().trim_matches('"');
        let res = (|| -> Result<Profile, String> {
            let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
            if meta.len() as usize > MAX_FILE_BYTES {
                return Err(t.prof_too_big.to_string());
            }
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            profile::from_json(&text).map_err(|e| if e == "not a Padyx profile" { t.prof_not_profile.to_string() } else { e })
        })();
        self.profiles_view.msg = Some(match res {
            Ok(mut p) => {
                if self.cfg.profiles.len() >= MAX_PROFILES {
                    (t.prof_limit.to_string(), false)
                } else {
                    p.name = profile::unique_name(&self.cfg.profiles, &p.name);
                    let name = p.name.clone();
                    self.cfg.profiles.push(p);
                    self.edit_profile = Some(name.clone());
                    self.profiles_view.import_path.clear();
                    (t.prof_imported.replace("{name}", &name), true)
                }
            }
            Err(e) => (format!("{}: {e}", t.prof_import_failed), false),
        });
    }

    pub(super) fn profiles_view(&mut self, ui: &mut Ui, p: &Pal) {
        let ctx = ui.ctx().clone();
        let t = self.s();
        let mut acts: Vec<Act> = Vec::new();
        let profs = self.cfg.profiles.clone();
        let current = self.cfg.profile_current.clone();
        let editing = self.edit_name();
        let active = self.snap.emu.profile.clone();
        let fg = self.snap.emu.fg_exe.clone();
        let avail = foreground::availability();
        let auto = self.cfg.profile_auto;
        let msg = self.profiles_view.msg.clone();
        let export_dir = self.profiles_view.export_dir.clone();
        // The rename box follows the profile being edited.
        if self.profiles_view.rename_for.as_deref() != Some(editing.as_str()) {
            self.profiles_view.rename = editing.clone();
            self.profiles_view.rename_for = Some(editing.clone());
        }
        let mut rename = self.profiles_view.rename.clone();
        let mut new_exe = self.profiles_view.new_exe.clone();
        let mut import_path = self.profiles_view.import_path.clone();
        let default_name = t.prof_default;

        // A profile file dropped on the window is imported.
        let dropped: Vec<String> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().display().to_string()).collect());
        for d in dropped {
            acts.push(Act::Import(d));
        }

        page(ui, p, t.nav_profiles, Some(t.prof_intro), |_| {}, |ui| {
            // ── Auto-switch
            group(ui, p, t.prof_auto, |ui| {
                match avail {
                    Availability::Available => {
                        let mut v = auto;
                        if switch_row(ui, p, t.prof_auto_switch, t.prof_auto_hint, &mut v) {
                            acts.push(Act::Auto(v));
                        }
                        ui.add_space(4.0);
                    }
                    Availability::Wayland => {
                        ui.add(egui::Label::new(RichText::new(t.prof_auto_wayland).font(theme::reg(14.0)).color(p.warning)).wrap());
                    }
                    Availability::Unsupported => {
                        ui.add(egui::Label::new(RichText::new(t.prof_auto_unsupported).font(theme::reg(14.0)).color(p.warning)).wrap());
                    }
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 32.0;
                    super::chip(ui, p, t.prof_active, if active.is_empty() { default_name } else { &active });
                    if auto && avail == Availability::Available {
                        let front = fg.clone().unwrap_or_else(|| t.prof_front_unknown.to_string());
                        super::chip(ui, p, t.prof_front, &front);
                    }
                });
            });

            // ── The list
            group(ui, p, t.prof_list, |ui| {
                let rows: Vec<(Option<&Profile>, &str)> = std::iter::once((None, default_name)).chain(profs.iter().map(|q| (Some(q), q.name.as_str()))).collect();
                for (i, (q, name)) in rows.iter().enumerate() {
                    if i > 0 {
                        theme::divider(ui, p);
                    }
                    let key = if q.is_none() { String::new() } else { name.to_string() };
                    let is_current = current == key;
                    let is_active = active == key;
                    let is_edit = editing == key;
                    ui.push_id(("prof", i), |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    ui.label(RichText::new(*name).font(theme::medium(14.0)).color(p.text));
                                    if is_active {
                                        ui.label(RichText::new(t.prof_tag_active).font(theme::medium(12.5)).color(p.positive));
                                    }
                                    if is_edit {
                                        ui.label(RichText::new(t.prof_tag_editing).font(theme::medium(12.5)).color(p.accent));
                                    }
                                });
                                let sub = match q {
                                    None => t.prof_default_hint.to_string(),
                                    Some(q) if q.exes.is_empty() => t.prof_no_programs.to_string(),
                                    Some(q) => q.exes.join(", "),
                                };
                                theme::hint(ui, &sub, p);
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if q.is_some() && widgets::danger_button(ui, t.prof_delete, p).clicked() {
                                    acts.push(Act::Delete(key.clone()));
                                }
                                if widgets::btn(ui, t.prof_export, Btn::Secondary, p).clicked() {
                                    acts.push(Act::Export(q.map(|q| q.name.clone())));
                                }
                                if q.is_some() && profs.len() < MAX_PROFILES && widgets::btn(ui, t.prof_duplicate, Btn::Secondary, p).clicked() {
                                    acts.push(Act::Duplicate(key.clone()));
                                }
                                if !is_edit && widgets::btn(ui, t.prof_edit, Btn::Secondary, p).clicked() {
                                    acts.push(Act::Edit(q.map(|q| q.name.clone())));
                                }
                                if !is_current && widgets::btn(ui, t.prof_use, Btn::Secondary, p).clicked() {
                                    acts.push(Act::Use(key.clone()));
                                }
                            });
                        });
                    });
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if widgets::btn_enabled(ui, t.prof_new, Btn::Secondary, profs.len() < MAX_PROFILES, p).clicked() {
                        acts.push(Act::New);
                    }
                });
                ui.add_space(6.0);
                theme::hint(ui, t.prof_use_hint, p);
            });

            // ── The edited profile
            if let Some(q) = profs.iter().find(|q| q.name == editing) {
                group(ui, p, &format!("{} · {}", t.prof_settings, q.name), |ui| {
                    ui.label(RichText::new(t.prof_name).font(theme::reg(13.0)).color(p.text2));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let r = widgets::input(ui, &mut rename, "", 260.0);
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (enter || r.lost_focus()) && rename.trim() != q.name {
                            acts.push(Act::Rename(rename.clone()));
                        }
                    });
                    theme::divider(ui, p);
                    ui.label(RichText::new(t.prof_programs).font(theme::reg(13.0)).color(p.text2));
                    ui.add_space(6.0);
                    for (i, e) in q.exes.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.label(RichText::new(e).font(theme::mono(13.0)).color(p.text));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::icon_button(ui, widgets::Icon::Close, 28.0, p).on_hover_text(t.prof_program_remove).clicked() {
                                    acts.push(Act::RemoveExe(i));
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let r = widgets::input(ui, &mut new_exe, t.prof_program_hint, 260.0);
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let add = widgets::btn_enabled(ui, t.prof_program_add, Btn::Secondary, q.exes.len() < MAX_EXES && !new_exe.trim().is_empty(), p).clicked();
                        if (add || enter) && !new_exe.trim().is_empty() && q.exes.len() < MAX_EXES {
                            acts.push(Act::AddExe);
                        }
                    });
                    ui.add_space(6.0);
                    theme::hint(ui, t.prof_programs_hint, p);
                    theme::divider(ui, p);
                    let mut follow = q.light.is_some();
                    if switch_row(ui, p, t.prof_light, t.prof_light_hint, &mut follow) {
                        acts.push(Act::Light(if follow { Some(self.out.light) } else { None }));
                    }
                    if let Some(cur) = q.light {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                            for c in SWATCHES {
                                let (r, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                                ui.painter().rect(r.shrink(2.0), 8.0, widgets::rgb(c), Stroke::new(1.0, p.line_hi), egui::StrokeKind::Inside);
                                if cur == rgb_of(c) {
                                    ui.painter().rect_stroke(r, 10.0, Stroke::new(2.0, p.text), egui::StrokeKind::Inside);
                                }
                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("#{c:06x}")).clicked() {
                                    acts.push(Act::Light(Some(rgb_of(c))));
                                }
                            }
                        });
                    }
                });
            }

            // ── Import
            group(ui, p, t.prof_import, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let w = (ui.available_width() - 130.0).clamp(160.0, 520.0);
                    let r = widgets::input(ui, &mut import_path, t.prof_import_hint, w);
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let go = widgets::btn_enabled(ui, t.prof_import_btn, Btn::Secondary, !import_path.trim().is_empty(), p).clicked();
                    if (go || enter) && !import_path.trim().is_empty() {
                        acts.push(Act::Import(import_path.clone()));
                    }
                });
                ui.add_space(6.0);
                theme::hint(ui, t.prof_import_drop, p);
                if let Some((m, ok)) = &msg {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(m).font(theme::reg(13.0)).color(if *ok { p.positive } else { p.negative })).wrap());
                }
                if let Some(dir) = &export_dir {
                    ui.add_space(8.0);
                    if widgets::btn(ui, t.prof_open_folder, Btn::Link, p).clicked() {
                        open_folder(dir);
                    }
                }
            });
        });

        self.profiles_view.rename = rename;
        self.profiles_view.new_exe = new_exe;
        self.profiles_view.import_path = import_path;
        self.apply_profile_acts(acts, &ctx);
    }

    fn apply_profile_acts(&mut self, acts: Vec<Act>, ctx: &egui::Context) {
        let mut changed = false;
        for a in acts {
            match a {
                Act::Use(name) => {
                    self.cfg.profile_current = name;
                    changed = true;
                }
                Act::Edit(name) => {
                    self.edit_profile = name;
                }
                Act::Export(name) => self.export_profile(name.as_deref()),
                Act::Duplicate(name) => {
                    if let Some(q) = self.cfg.profiles.iter().find(|q| q.name == name).cloned() {
                        let mut n = q;
                        n.name = profile::unique_name(&self.cfg.profiles, &format!("{} copy", n.name));
                        self.edit_profile = Some(n.name.clone());
                        self.cfg.profiles.push(n);
                        changed = true;
                    }
                }
                Act::Delete(name) => {
                    self.cfg.profiles.retain(|q| q.name != name);
                    if self.cfg.profile_current == name {
                        self.cfg.profile_current.clear();
                    }
                    if self.edit_profile.as_deref() == Some(name.as_str()) {
                        self.edit_profile = None;
                    }
                    changed = true;
                }
                Act::New => {
                    // A new profile starts as a copy of the default one: change what differs.
                    let name = profile::unique_name(&self.cfg.profiles, self.s().prof_new_name);
                    let mut emu = self.cfg.emu.clone();
                    emu.native.clear();
                    self.cfg.profiles.push(Profile { name: name.clone(), exes: Vec::new(), light: None, emu });
                    self.edit_profile = Some(name);
                    changed = true;
                }
                Act::Import(path) => {
                    let before = self.cfg.profiles.len();
                    self.import_profile(&path);
                    changed |= self.cfg.profiles.len() != before;
                }
                Act::Auto(v) => {
                    self.cfg.profile_auto = v;
                    changed = true;
                }
                Act::Rename(raw) => {
                    if let Some(i) = self.edit_index() {
                        let old = self.cfg.profiles[i].name.clone();
                        let others: Vec<Profile> = self.cfg.profiles.iter().filter(|q| q.name != old).cloned().collect();
                        let new = profile::unique_name(&others, &raw);
                        if new != old {
                            if self.cfg.profile_current == old {
                                self.cfg.profile_current = new.clone();
                            }
                            self.cfg.profiles[i].name = new.clone();
                            self.edit_profile = Some(new);
                            changed = true;
                        }
                        self.profiles_view.rename_for = None;
                    }
                }
                Act::AddExe => {
                    if let Some(i) = self.edit_index() {
                        let e = self.profiles_view.new_exe.trim().to_string();
                        if !e.is_empty() && !self.cfg.profiles[i].exes.iter().any(|x| x.eq_ignore_ascii_case(&e)) {
                            self.cfg.profiles[i].exes.push(e);
                            changed = true;
                        }
                        self.profiles_view.new_exe.clear();
                    }
                }
                Act::RemoveExe(k) => {
                    if let Some(i) = self.edit_index() {
                        if k < self.cfg.profiles[i].exes.len() {
                            self.cfg.profiles[i].exes.remove(k);
                            changed = true;
                        }
                    }
                }
                Act::Light(c) => {
                    if let Some(i) = self.edit_index() {
                        self.cfg.profiles[i].light = c;
                        changed = true;
                    }
                }
            }
        }
        if changed {
            profile::sanitize(&mut self.cfg.profiles, &mut self.cfg.profile_current);
            self.touch_cfg(ctx);
        }
    }
}
