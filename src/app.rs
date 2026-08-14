//! Application state, persistence lifecycle and top-level layout.

use crate::canvas;
use crate::example;
use crate::forms::{BiographyForm, CategoryEditor, Dialog, EventForm, GroupForm, TimelineForm};
use crate::layout::{self, Lane, TimeAxis};
use crate::model::*;
use crate::panels;
use crate::store;
use egui::{Pos2, Rect};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Quiet period after the last edit before the library is written to disk.
const AUTOSAVE_DELAY: Duration = Duration::from_millis(1200);
/// How many undo steps to retain.
const UNDO_DEPTH: usize = 60;
const TOAST_LIFETIME: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    Group(Id),
    Timeline(Id),
    Biography(Id),
    Event(Id),
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// A destructive action awaiting confirmation.
pub enum Confirm {
    DeleteGroup(Id),
    DeleteTimeline(Id),
    DeleteBiography(Id),
    DeleteEvent(Id),
    DeleteCategory(Id),
    NewLibrary,
    Restore(PathBuf, String),
}

pub struct TimelineApp {
    pub doc: Document,
    pub path: PathBuf,
    pub dirty: bool,
    last_edit: Option<Instant>,
    pub last_saved: Option<Instant>,

    // Canvas view state that is not worth persisting.
    pub y_offset: f32,
    pub max_y_offset: f32,
    pub last_lanes: Vec<Lane>,
    pub last_axis: Option<TimeAxis>,
    pub last_width: Option<f32>,

    pub selection: Option<Selection>,
    pub dialog: Dialog,
    pub confirm: Option<Confirm>,
    pub toast: Option<Toast>,
    pub show_help: bool,
    pub focus_search: bool,

    undo: Vec<Document>,
    redo: Vec<Document>,
}

impl TimelineApp {
    pub fn new() -> Self {
        let path = store::default_path();
        let (doc, toast) = match store::load(&path) {
            Ok(Some(doc)) => (doc, None),
            Ok(None) => (Document::with_starter_categories(), None),
            Err(e) => (
                // Never overwrite a file we failed to parse: start a fresh
                // in-memory document and tell the user loudly.
                Document::with_starter_categories(),
                Some(Toast {
                    text: format!("{e} — your file was left untouched. Use File > Open to try another."),
                    error: true,
                    at: Instant::now(),
                }),
            ),
        };
        Self {
            doc,
            path,
            dirty: false,
            last_edit: None,
            last_saved: None,
            y_offset: 0.0,
            max_y_offset: 0.0,
            last_lanes: Vec::new(),
            last_axis: None,
            last_width: None,
            selection: None,
            dialog: Dialog::None,
            confirm: None,
            toast,
            show_help: false,
            focus_search: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    // --- Editing ----------------------------------------------------------

    /// Apply a change to the document with undo support and autosave marking.
    pub fn mutate(&mut self, f: impl FnOnce(&mut Document)) {
        self.undo.push(self.doc.clone());
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(&mut self.doc);
        self.mark_dirty();
    }

    /// Mark the document changed without pushing an undo step. For view state
    /// such as pan and zoom, which should persist but not clutter undo.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, prev));
            self.validate_selection();
            self.mark_dirty();
            self.info("Undone");
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, next));
            self.validate_selection();
            self.mark_dirty();
            self.info("Redone");
        }
    }

    /// Drop a selection that points at something no longer in the document.
    fn validate_selection(&mut self) {
        let alive = match self.selection {
            Some(Selection::Group(id)) => self.doc.group(id).is_some(),
            Some(Selection::Timeline(id)) => self.doc.timeline(id).is_some(),
            Some(Selection::Biography(id)) => self.doc.biography(id).is_some(),
            Some(Selection::Event(id)) => self.doc.event(id).is_some(),
            None => true,
        };
        if !alive {
            self.selection = None;
        }
    }

    // --- Messages ---------------------------------------------------------

    pub fn info(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            error: false,
            at: Instant::now(),
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            error: true,
            at: Instant::now(),
        });
    }

    // --- Files ------------------------------------------------------------

    pub fn save(&mut self) {
        match store::save(&self.path, &self.doc) {
            Ok(()) => {
                self.dirty = false;
                self.last_saved = Some(Instant::now());
            }
            Err(e) => self.error(e),
        }
    }

    fn autosave_if_due(&mut self) {
        if !self.dirty {
            return;
        }
        let due = self
            .last_edit
            .map(|t| t.elapsed() >= AUTOSAVE_DELAY)
            .unwrap_or(true);
        if due {
            self.save();
        }
    }

    pub fn save_as(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Timeline library", &[store::FILE_EXTENSION])
            .set_file_name(store::DEFAULT_FILE_NAME)
            .set_directory(self.path.parent().unwrap_or(std::path::Path::new(".")));
        if let Some(p) = dialog.save_file() {
            self.path = p;
            self.save();
            if !self.dirty {
                self.info(format!("Saved to {}", self.path.display()));
            }
        }
    }

    pub fn open(&mut self) {
        // Don't lose pending edits behind the file picker.
        if self.dirty {
            self.save();
        }
        let dialog = rfd::FileDialog::new()
            .add_filter("Timeline library", &[store::FILE_EXTENSION])
            .set_directory(self.path.parent().unwrap_or(std::path::Path::new(".")));
        if let Some(p) = dialog.pick_file() {
            match store::load(&p) {
                Ok(Some(doc)) => {
                    self.doc = doc;
                    self.path = p;
                    self.undo.clear();
                    self.redo.clear();
                    self.selection = None;
                    self.dirty = false;
                    self.info("Library opened");
                }
                Ok(None) => self.error("That file is empty."),
                Err(e) => self.error(e),
            }
        }
    }

    pub fn load_example(&mut self) {
        self.mutate(|doc| *doc = example::build());
        self.selection = None;
        self.info("Example library loaded — edit or delete anything you like.");
    }

    pub fn restore_backup(&mut self, path: &std::path::Path) {
        match store::load(path) {
            Ok(Some(doc)) => {
                self.mutate(|d| *d = doc);
                self.selection = None;
                self.info("Backup restored. Undo if this was not what you wanted.");
            }
            Ok(None) => self.error("That backup is empty."),
            Err(e) => self.error(e),
        }
    }

    // --- Canvas helpers ---------------------------------------------------

    /// Frame the whole dataset in the given viewport width.
    pub fn fit_to_content(&mut self, width: f32) {
        let Some((lo, hi)) = self.doc.extent() else {
            return;
        };
        let span = (hi - lo).max(1.0);
        let pad = span * 0.06;
        let ppy = (width as f64 / (span + pad * 2.0)).clamp(layout::MIN_PPY, layout::MAX_PPY);
        self.doc.view.pixels_per_year = ppy;
        self.doc.view.left_year = lo - pad;
        self.y_offset = 0.0;
        self.mark_dirty();
    }

    pub fn open_editor_for(&mut self, sel: Selection) {
        self.selection = Some(sel);
        self.dialog = match sel {
            Selection::Group(id) => match self.doc.group(id) {
                Some(g) => Dialog::Group(GroupForm::edit(g)),
                None => Dialog::None,
            },
            Selection::Timeline(id) => match self.doc.timeline(id) {
                Some(t) => Dialog::Timeline(TimelineForm::edit(t)),
                None => Dialog::None,
            },
            Selection::Biography(id) => match self.doc.biography(id) {
                Some(b) => Dialog::Biography(BiographyForm::edit(b)),
                None => Dialog::None,
            },
            Selection::Event(id) => match self.doc.event(id) {
                Some(e) => Dialog::Event(EventForm::edit(e)),
                None => Dialog::None,
            },
        };
    }

    /// Double-click on empty canvas: start a new event on that lane at that date.
    pub fn quick_add_at(&mut self, pos: Pos2, _rect: Rect) {
        let Some(axis) = self.last_axis else { return };
        let Some(lane) = self
            .last_lanes
            .iter()
            .find(|l| pos.y >= l.top && pos.y <= l.bottom)
        else {
            return;
        };
        let year = axis.t(pos.x).floor() as i32;
        let date = HDate::year(if year >= 0 { year + 1 } else { year });
        // A collapsed group stands for several timelines; attach to the first.
        let owner = match lane.kind {
            crate::layout::LaneKind::Timeline(id) => Some(OwnerRef::Timeline(id)),
            crate::layout::LaneKind::Biography(id) => Some(OwnerRef::Biography(id)),
            crate::layout::LaneKind::Group(id) => self
                .doc
                .group_timelines(id)
                .first()
                .map(|t| OwnerRef::Timeline(*t)),
        };
        let Some(owner) = owner else { return };
        self.dialog = Dialog::Event(EventForm::new_at(owner, date));
    }

    /// The owner a freshly added event should default to.
    pub fn default_owner(&self) -> Option<OwnerRef> {
        match self.selection {
            Some(Selection::Timeline(id)) if self.doc.timeline(id).is_some() => {
                Some(OwnerRef::Timeline(id))
            }
            Some(Selection::Biography(id)) if self.doc.biography(id).is_some() => {
                Some(OwnerRef::Biography(id))
            }
            Some(Selection::Event(id)) => self.doc.event(id).map(|e| e.owner),
            _ => self
                .doc
                .timelines
                .first()
                .map(|t| OwnerRef::Timeline(t.id))
                .or_else(|| self.doc.biographies.first().map(|b| OwnerRef::Biography(b.id))),
        }
    }

    pub fn new_event_dialog(&mut self) {
        match self.default_owner() {
            Some(owner) => self.dialog = Dialog::Event(EventForm::new(owner)),
            None => self.error("Create a timeline first — events have to belong to something."),
        }
    }

    // --- Confirmation -----------------------------------------------------

    fn apply_confirm(&mut self, c: Confirm) {
        match c {
            Confirm::DeleteGroup(id) => {
                self.mutate(|d| d.delete_group(id));
                self.validate_selection();
                self.info("Group removed; its contents moved up a level - Ctrl+Z to undo.");
            }
            Confirm::DeleteTimeline(id) => {
                self.mutate(|d| d.delete_timeline(id));
                self.validate_selection();
                self.info("Timeline deleted — Ctrl+Z to undo.");
            }
            Confirm::DeleteBiography(id) => {
                self.mutate(|d| d.delete_biography(id));
                self.validate_selection();
                self.info("Biography deleted — Ctrl+Z to undo.");
            }
            Confirm::DeleteEvent(id) => {
                self.mutate(|d| d.delete_event(id));
                self.validate_selection();
                self.info("Event deleted; nested events moved up a level — Ctrl+Z to undo.");
            }
            Confirm::DeleteCategory(id) => {
                self.mutate(|d| d.delete_category(id));
                self.info("Category deleted — Ctrl+Z to undo.");
            }
            Confirm::NewLibrary => {
                self.mutate(|d| *d = Document::with_starter_categories());
                self.selection = None;
                self.info("Started an empty library — Ctrl+Z to undo.");
            }
            Confirm::Restore(path, _) => self.restore_backup(&path),
        }
    }
}

// ---------------------------------------------------------------------------
// eframe integration
// ---------------------------------------------------------------------------

impl eframe::App for TimelineApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        apply_style(&ctx, self.doc.view.dark_mode);
        self.handle_shortcuts(&ctx);

        egui::Panel::top("toolbar")
            .exact_size(64.0)
            .show(ui, |ui| self.toolbar(ui));

        egui::Panel::bottom("status")
            .exact_size(26.0)
            .show(ui, |ui| self.status_bar(ui));

        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(272.0)
            .size_range(210.0..=430.0)
            .show(ui, |ui| panels::sidebar(self, ui));

        if self.selection.is_some() {
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(300.0)
                .size_range(240.0..=460.0)
                .show(ui, |ui| panels::inspector(self, ui));
        }

        egui::CentralPanel::no_frame().show(ui, |ui| canvas::draw(self, ui));

        crate::forms::show_dialogs(self, &ctx);
        self.show_confirm(&ctx);
        self.show_help_window(&ctx);

        self.autosave_if_due();
        // Keep waking up so the autosave timer fires while the app sits idle.
        if self.dirty {
            ctx.request_repaint_after(AUTOSAVE_DELAY);
        }
        if self.toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Last line of defence against losing the final edit.
        if self.dirty {
            let _ = store::save(&self.path, &self.doc);
        }
    }
}

fn apply_style(ctx: &egui::Context, dark: bool) {
    ctx.set_visuals(crate::theme::visuals(dark));
}

impl TimelineApp {
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (ctrl, s, z, y, n, f, e, del, esc) = ctx.input(|i| {
            (
                i.modifiers.ctrl || i.modifiers.command,
                i.key_pressed(egui::Key::S),
                i.key_pressed(egui::Key::Z),
                i.key_pressed(egui::Key::Y),
                i.key_pressed(egui::Key::N),
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::E),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if ctrl && s {
            self.save();
            self.info("Saved");
        }
        if ctrl && z {
            self.undo();
        }
        if ctrl && y {
            self.redo();
        }
        if ctrl && n {
            self.new_event_dialog();
        }
        if ctrl && f {
            self.focus_search = true;
        }
        if e && !ctrl {
            if let Some(sel) = self.selection {
                self.open_editor_for(sel);
            }
        }
        if del {
            match self.selection {
                Some(Selection::Event(id)) => self.confirm = Some(Confirm::DeleteEvent(id)),
                Some(Selection::Timeline(id)) => self.confirm = Some(Confirm::DeleteTimeline(id)),
                Some(Selection::Biography(id)) => self.confirm = Some(Confirm::DeleteBiography(id)),
                Some(Selection::Group(id)) => self.confirm = Some(Confirm::DeleteGroup(id)),
                None => {}
            }
        }
        if esc {
            self.selection = None;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New library…").clicked() {
                    self.confirm = Some(Confirm::NewLibrary);
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    self.open();
                    ui.close();
                }
                if ui.button("Save   (Ctrl+S)").clicked() {
                    self.save();
                    self.info("Saved");
                    ui.close();
                }
                if ui.button("Save as…").clicked() {
                    self.save_as();
                    ui.close();
                }
                ui.separator();
                if ui.button("Load example library").clicked() {
                    self.load_example();
                    ui.close();
                }
                ui.menu_button("Restore backup", |ui| {
                    let list = store::backups(&self.path);
                    if list.is_empty() {
                        ui.label("No backups yet");
                    }
                    for (p, label) in list {
                        if ui.button(label.clone()).clicked() {
                            self.confirm = Some(Confirm::Restore(p, label));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button("Show data folder").clicked() {
                    store::reveal_in_explorer(&self.path);
                    ui.close();
                }
            });

            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(self.can_undo(), egui::Button::new("Undo   (Ctrl+Z)"))
                    .clicked()
                {
                    self.undo();
                    ui.close();
                }
                if ui
                    .add_enabled(self.can_redo(), egui::Button::new("Redo   (Ctrl+Y)"))
                    .clicked()
                {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                if ui.button("Categories…").clicked() {
                    self.dialog = Dialog::Categories(CategoryEditor::default());
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                let mut dark = self.doc.view.dark_mode;
                if ui.checkbox(&mut dark, "Dark theme").changed() {
                    self.doc.view.dark_mode = dark;
                    self.mark_dirty();
                }
                let mut labels = self.doc.view.show_labels;
                if ui.checkbox(&mut labels, "Show event labels").changed() {
                    self.doc.view.show_labels = labels;
                    self.mark_dirty();
                }
                ui.separator();
                if ui.button("Fit everything   (Home)").clicked() {
                    let w = ui.ctx().content_rect().width() - 600.0;
                    self.fit_to_content(w.max(400.0));
                    ui.close();
                }
            });

            if ui.button("Help").clicked() {
                self.show_help = true;
            }

            ui.separator();

            if ui.button("+ Event").on_hover_text("Ctrl+N").clicked() {
                self.new_event_dialog();
            }
            if ui.button("+ Group").clicked() {
                self.dialog = Dialog::Group(GroupForm::new(self.doc.next_palette_color()));
            }
            if ui.button("+ Timeline").clicked() {
                self.dialog = Dialog::Timeline(TimelineForm::new(self.doc.next_palette_color()));
            }
            if ui.button("+ Biography").clicked() {
                let default_tl = self.doc.timelines.first().map(|t| t.id);
                self.dialog = Dialog::Biography(BiographyForm::new(default_tl));
            }
        });

        ui.horizontal(|ui| {
            ui.label("Search:");
            let mut search = self.doc.view.filters.search.clone();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .desired_width(180.0)
                    .hint_text("title or description"),
            );
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            if resp.changed() {
                self.doc.view.filters.search = search;
                self.mark_dirty();
            }
            if !self.doc.view.filters.search.is_empty() && ui.button("Clear").clicked() {
                self.doc.view.filters.search.clear();
                self.mark_dirty();
            }

            ui.separator();

            ui.label("Detail:")
                .on_hover_text("Shifts how much the current zoom reveals");
            let mut bias = self.doc.view.filters.detail_bias;
            if ui
                .add(egui::Slider::new(&mut bias, -2..=3).show_value(false))
                .changed()
            {
                self.doc.view.filters.detail_bias = bias;
                self.mark_dirty();
            }
            let threshold =
                layout::importance_threshold(self.doc.view.pixels_per_year, self.doc.view.filters.detail_bias);
            ui.label(format!("≥ {}", importance_name(threshold)));

            ui.separator();

            if ui.button("-").on_hover_text("Zoom out").clicked() {
                self.doc.view.pixels_per_year =
                    (self.doc.view.pixels_per_year * 0.7).clamp(layout::MIN_PPY, layout::MAX_PPY);
                self.mark_dirty();
            }
            if ui.button("+").on_hover_text("Zoom in").clicked() {
                self.doc.view.pixels_per_year =
                    (self.doc.view.pixels_per_year * 1.4).clamp(layout::MIN_PPY, layout::MAX_PPY);
                self.mark_dirty();
            }
            if ui.button("Fit").clicked() {
                let w = ui.ctx().content_rect().width() - 600.0;
                self.fit_to_content(w.max(400.0));
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(t) = &self.toast {
                if t.at.elapsed() < TOAST_LIFETIME {
                    let color = if t.error {
                        egui::Color32::from_rgb(230, 120, 110)
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.colored_label(color, &t.text);
                } else {
                    self.toast = None;
                }
            } else {
                let saved = if self.dirty { "saving…" } else { "saved" };
                let range = match (self.last_axis, self.last_width) {
                    (Some(a), Some(w)) => {
                        let (from, to) = a.visible_range(w);
                        format!(
                            " · showing {} – {} ({:.2} px/yr)",
                            axis_year_label(from),
                            axis_year_label(to),
                            a.ppy
                        )
                    }
                    _ => String::new(),
                };
                let file = self
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| self.path.display().to_string());
                ui.weak(format!(
                    "{} · {} timelines · {} biographies · {} events{} · {}",
                    saved,
                    self.doc.timelines.len(),
                    self.doc.biographies.len(),
                    self.doc.events.len(),
                    range,
                    file
                ))
                .on_hover_text(self.path.display().to_string());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("wheel = zoom · drag = move · double-click = add · Help for more")
                    .on_hover_text("wheel = zoom, shift+wheel = pan, alt+wheel = scroll lanes, drag = move, double-click empty space = add an event");
            });
        });
    }

    fn show_confirm(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, body) = match c {
            Confirm::DeleteGroup(id) => (
                "Remove group?",
                format!(
                    "{}{}{} will be removed. The timelines and groups inside it are kept and move up a level.",
                    "“",
                    self.doc.group(*id).map(|g| g.name.clone()).unwrap_or_default(),
                    "”"
                ),
            ),
            Confirm::DeleteTimeline(id) => (
                "Delete timeline?",
                format!(
                    "“{}” and its {} event(s) will be removed.",
                    self.doc.timeline(*id).map(|t| t.name.clone()).unwrap_or_default(),
                    self.doc.events_of(OwnerRef::Timeline(*id)).count()
                ),
            ),
            Confirm::DeleteBiography(id) => (
                "Delete biography?",
                format!(
                    "“{}” and its {} event(s) will be removed.",
                    self.doc.biography(*id).map(|b| b.name.clone()).unwrap_or_default(),
                    self.doc.events_of(OwnerRef::Biography(*id)).count()
                ),
            ),
            Confirm::DeleteEvent(id) => (
                "Delete event?",
                format!(
                    "“{}” will be removed.",
                    self.doc.event(*id).map(|e| e.title.clone()).unwrap_or_default()
                ),
            ),
            Confirm::DeleteCategory(id) => (
                "Delete category?",
                format!(
                    "“{}” will be removed from every entry that uses it.",
                    self.doc.category(*id).map(|c| c.name.clone()).unwrap_or_default()
                ),
            ),
            Confirm::NewLibrary => (
                "Start an empty library?",
                "Everything currently loaded will be cleared from this window.".to_string(),
            ),
            Confirm::Restore(_, label) => (
                "Restore backup?",
                format!("{label} will replace what is currently loaded."),
            ),
        };

        let mut decision: Option<bool> = None;
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(title);
            ui.add_space(6.0);
            ui.label(body);
            ui.add_space(4.0);
            ui.weak("This can be undone with Ctrl+Z.");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
                if ui.button("Yes, continue").clicked() {
                    decision = Some(true);
                }
            });
        });

        match decision {
            Some(true) => {
                if let Some(c) = self.confirm.take() {
                    self.apply_confirm(c);
                }
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }

    fn show_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let mut open = true;
        egui::Window::new("How to use Timeline Explorer")
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Moving around");
                    ui.label("• Mouse wheel zooms in and out around the pointer.");
                    ui.label("• Shift + wheel, or dragging, pans along the time axis.");
                    ui.label("• Alt + wheel scrolls through the lanes when you have many.");
                    ui.label("• Home, or the Fit button, frames your whole dataset.");
                    ui.add_space(8.0);

                    ui.heading("Adding data");
                    ui.label("• Double-click an empty spot on a lane to add an event at that date.");
                    ui.label("• Ctrl+N adds an event; the + buttons add timelines and biographies.");
                    ui.label("• Select something and press E to edit it, Delete to remove it.");
                    ui.add_space(8.0);

                    ui.heading("Dates");
                    ui.label("Type dates however you like: 44 BC, -44, c. 250 BC, 1789, 14 Jul 1789, 1789-07-14. Add ±20 for uncertainty. The form shows how it read your entry.");
                    ui.add_space(8.0);

                    ui.heading("Zoom and importance");
                    ui.label("Each entry has an importance from Detail to Epochal. Zoomed out you only see the big things; zooming in reveals the rest. The Detail slider shifts that balance either way.");
                    ui.add_space(8.0);

                    ui.heading("Groups");
                    ui.label("Use + Group for a super-category such as \"European history\" or \"Greek antiquity\", then put timelines inside it from their editor. Groups nest, so \"Greek antiquity > Classical poleis > Athens\" works.");
                    ui.label("Collapse a group in the sidebar and it becomes a single band standing for everything inside it — useful for comparing whole civilisations. Expand it again to work with Sparta and Athens separately.");
                    ui.add_space(8.0);

                    ui.heading("Converging timelines");
                    ui.label("In a timeline's editor set “Merges into” to another timeline and give the date — its band will curve into that timeline's lane and end there. “Splits from” does the reverse for successor states.");
                    ui.add_space(8.0);

                    ui.heading("Your data");
                    ui.label(format!("Everything is stored as readable JSON at {}", self.path.display()));
                    ui.label("It saves itself about a second after each change, keeps ten rotating backups, and never needs an internet connection.");
                });
            });
        self.show_help = open;
    }
}
