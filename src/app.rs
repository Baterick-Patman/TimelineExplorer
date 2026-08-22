//! Application state, persistence lifecycle and top-level layout.

use crate::canvas;
use crate::example;
use crate::export::{self, ExportFormat, ExportJob, ExportStage};
use crate::forms::{
    BiographyForm, CategoryEditor, Dialog, EventForm, ExportForm, GroupForm, ImportForm, TimelineForm,
};
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

/// Something a search suggestion can point at and jump the canvas to.
/// Distinct from `Selection`, even though most variants line up one to one:
/// a `Selection` is "what the inspector currently shows", a `JumpTarget` is
/// "what to reveal and frame" — keeping them separate leaves room for jump
/// targets that aren't selectable in their own right: an epoch (selecting
/// its owning timeline/biography instead) or a bare typed date (selecting
/// nothing at all).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum JumpTarget {
    Group(Id),
    Timeline(Id),
    Biography(Id),
    Event(Id),
    /// One of a timeline's or biography's own colour-coded epochs/phases,
    /// addressed by its owner and index into that owner's `epochs`/
    /// `life_phases` list.
    Epoch(OwnerRef, usize),
    /// A date typed straight into the search field rather than a name match.
    Date(HDate),
}

impl JumpTarget {
    fn selection(self) -> Option<Selection> {
        match self {
            JumpTarget::Group(id) => Some(Selection::Group(id)),
            JumpTarget::Timeline(id) => Some(Selection::Timeline(id)),
            JumpTarget::Biography(id) => Some(Selection::Biography(id)),
            JumpTarget::Event(id) => Some(Selection::Event(id)),
            JumpTarget::Epoch(OwnerRef::Timeline(id), _) => Some(Selection::Timeline(id)),
            JumpTarget::Epoch(OwnerRef::Biography(id), _) => Some(Selection::Biography(id)),
            JumpTarget::Date(_) => None,
        }
    }
}

/// Un-hide and expand whatever stands between `target` and actually being
/// visible: the timeline itself, every ancestor group (expanded, not just
/// visible), and — for an event — its owner, recursing once more for an
/// event that belongs to a biography that belongs to a culture.
fn reveal_jump_target(doc: &mut Document, target: JumpTarget) {
    fn reveal_group_chain(doc: &mut Document, group: Option<Id>) {
        let mut cursor = group;
        let mut guard = 0;
        while let Some(id) = cursor {
            guard += 1;
            if guard > 64 {
                break; // A hand-edited file could contain a parent cycle.
            }
            let parent = doc.group(id).and_then(|g| g.parent);
            if let Some(g) = doc.group_mut(id) {
                g.collapsed = false;
                g.visible = true;
            }
            cursor = parent;
        }
    }
    fn reveal_timeline(doc: &mut Document, id: Id) {
        let group = doc.timeline(id).and_then(|t| t.group);
        if let Some(t) = doc.timeline_mut(id) {
            t.visible = true;
        }
        reveal_group_chain(doc, group);
    }
    fn reveal_biography(doc: &mut Document, id: Id) {
        let timeline = doc.biography(id).and_then(|b| b.timeline);
        if let Some(b) = doc.biography_mut(id) {
            if b.display == BioDisplay::Hidden {
                b.display = if b.timeline.is_some() {
                    BioDisplay::Inline
                } else {
                    BioDisplay::Lane
                };
            }
        }
        if let Some(t) = timeline {
            reveal_timeline(doc, t);
        }
    }

    match target {
        JumpTarget::Group(id) => {
            reveal_group_chain(doc, Some(id));
            for tid in doc.group_timelines(id) {
                if let Some(t) = doc.timeline_mut(tid) {
                    t.visible = true;
                }
            }
        }
        JumpTarget::Timeline(id) => reveal_timeline(doc, id),
        JumpTarget::Biography(id) => reveal_biography(doc, id),
        JumpTarget::Event(id) => {
            if let Some(owner) = doc.event(id).map(|e| e.owner) {
                match owner {
                    OwnerRef::Timeline(t) => reveal_timeline(doc, t),
                    OwnerRef::Biography(b) => reveal_biography(doc, b),
                }
            }
        }
        JumpTarget::Epoch(owner, _) => match owner {
            OwnerRef::Timeline(t) => reveal_timeline(doc, t),
            OwnerRef::Biography(b) => reveal_biography(doc, b),
        },
        // A bare date isn't attached to anything hidden — nothing to reveal.
        JumpTarget::Date(_) => {}
    }
}

/// How the sidebar clusters biographies for collapsing — by their culture
/// (single-valued: exactly one cluster each) or by category (a biography can
/// carry several, so it can appear in more than one cluster). Session-only,
/// like the search strings it sits next to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BioGroupBy {
    #[default]
    Culture,
    Category,
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

    /// Sidebar list filters — narrow a long Timelines/Biographies list down
    /// while typing. Deliberately not part of `Document`: it is a navigation
    /// aid for the current session, not something worth remembering between
    /// launches the way the canvas search (`view.filters.search`) is.
    pub timeline_search: String,
    pub bio_search: String,
    pub bio_group_by: BioGroupBy,

    /// Biographies pinned open at their enlarged size regardless of zoom —
    /// click a biography's band to pin just it, Ctrl+click to pin several at
    /// once. A view convenience, so it lives here rather than in `Document`.
    pub enlarged_biographies: std::collections::BTreeSet<Id>,

    /// An export in progress — see `export.rs`. While `Some`, the normal
    /// panel layout is replaced with just the canvas so the screenshot it
    /// eventually takes contains nothing else.
    pub export_job: Option<ExportJob>,

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
                    text: format!("{e} — deine Datei wurde nicht verändert. Über Datei > Öffnen eine andere versuchen."),
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
            timeline_search: String::new(),
            bio_search: String::new(),
            bio_group_by: BioGroupBy::default(),
            enlarged_biographies: std::collections::BTreeSet::new(),
            export_job: None,
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
            self.info("Rückgängig gemacht");
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, next));
            self.validate_selection();
            self.mark_dirty();
            self.info("Wiederholt");
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
            .add_filter("Zeitstrahl-Bibliothek", &[store::FILE_EXTENSION])
            .set_file_name(store::DEFAULT_FILE_NAME)
            .set_directory(self.path.parent().unwrap_or(std::path::Path::new(".")));
        if let Some(p) = dialog.save_file() {
            self.path = p;
            self.save();
            if !self.dirty {
                self.info(format!("Gespeichert unter {}", self.path.display()));
            }
        }
    }

    pub fn open(&mut self) {
        // Don't lose pending edits behind the file picker.
        if self.dirty {
            self.save();
        }
        let dialog = rfd::FileDialog::new()
            .add_filter("Zeitstrahl-Bibliothek", &[store::FILE_EXTENSION])
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
                    self.info("Bibliothek geöffnet");
                }
                Ok(None) => self.error("Diese Datei ist leer."),
                Err(e) => self.error(e),
            }
        }
    }

    pub fn load_example(&mut self) {
        self.mutate(|doc| *doc = example::build());
        self.selection = None;
        self.info("Beispielbibliothek geladen — bearbeite oder lösche alles nach Belieben.");
    }

    pub fn restore_backup(&mut self, path: &std::path::Path) {
        match store::load(path) {
            Ok(Some(doc)) => {
                self.mutate(|d| *d = doc);
                self.selection = None;
                self.info("Sicherung wiederhergestellt. Mit Rückgängig lässt sich das zurücknehmen.");
            }
            Ok(None) => self.error("Diese Sicherung ist leer."),
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

    // --- Jump-to-search-result --------------------------------------------

    /// Pan/zoom to `target`, revealing it first if it is currently hidden or
    /// tucked inside a collapsed group — jumping to something the user just
    /// searched for and landing on an unchanged, empty-looking view would
    /// defeat the point.
    pub fn jump_to(&mut self, target: JumpTarget, width: f32) {
        let Some((date, importance)) = self.jump_anchor(target) else {
            return;
        };

        self.mutate(|doc| reveal_jump_target(doc, target));

        // Zoomed in enough to read individual events/lifespans comfortably,
        // without being so close that "jump to a whole timeline" only shows
        // a sliver of it.
        let ppy: f64 = 8.0;
        let base = layout::importance_threshold(ppy, 0);
        let needed_bias = (base as i32 - importance as i32).max(0);
        self.doc.view.pixels_per_year = ppy;
        self.doc.view.left_year = date - (width as f64 / ppy) * 0.4;
        self.doc.view.filters.detail_bias = self.doc.view.filters.detail_bias.max(needed_bias);
        self.y_offset = 0.0;
        self.selection = target.selection();
        self.mark_dirty();
    }

    /// Where a jump target sits on the axis, and how important it is (so the
    /// zoom-dependent threshold can be pushed aside if it would otherwise
    /// hide the very thing just jumped to).
    fn jump_anchor(&self, target: JumpTarget) -> Option<(f64, u8)> {
        match target {
            JumpTarget::Event(id) => self.doc.event(id).map(|e| (e.span.t0(), e.importance)),
            JumpTarget::Biography(id) => self.doc.biography(id).map(|b| (b.birth.decimal(), b.importance)),
            JumpTarget::Timeline(id) => self
                .doc
                .timeline(id)
                .and_then(|t| layout::timeline_band_range(&self.doc, t))
                .map(|(lo, _)| (lo, IMPORTANCE_MAX)),
            JumpTarget::Group(id) => self
                .doc
                .group_timelines(id)
                .iter()
                .filter_map(|&tid| {
                    self.doc.timeline(tid).and_then(|t| layout::timeline_band_range(&self.doc, t))
                })
                .map(|(lo, _)| lo)
                .fold(None, |acc: Option<f64>, lo| Some(acc.map_or(lo, |a: f64| a.min(lo))))
                .map(|lo| (lo, IMPORTANCE_MAX)),
            JumpTarget::Epoch(OwnerRef::Timeline(id), idx) => {
                self.doc.timeline(id).and_then(|t| t.epochs.get(idx)).map(|e| (e.t0(), IMPORTANCE_MAX))
            }
            JumpTarget::Epoch(OwnerRef::Biography(id), idx) => {
                self.doc.biography(id).and_then(|b| b.life_phases.get(idx)).map(|e| (e.t0(), IMPORTANCE_MAX))
            }
            JumpTarget::Date(d) => Some((d.decimal(), IMPORTANCE_MAX)),
        }
    }

    // --- Export -------------------------------------------------------

    /// Kick off an export: swap in the already-filtered document, frame the
    /// chosen date range, and let `tick_export` drive the rest across the
    /// following frames. Deliberately bypasses `mutate`/`mark_dirty` — this
    /// is a transient render, not an edit, and must not enter the undo
    /// stack or trigger a real autosave of the filtered document.
    #[allow(clippy::too_many_arguments)]
    pub fn start_export(
        &mut self,
        ctx: &egui::Context,
        mut export_doc: Document,
        from: f64,
        to: f64,
        width_px: f32,
        format: ExportFormat,
        path: PathBuf,
    ) {
        let (axis, _) = export::export_axis(from, to, width_px);
        export_doc.view.left_year = axis.left_year;
        export_doc.view.pixels_per_year = axis.ppy;

        let restore_window_size = ctx.content_rect().size();
        self.export_job = Some(ExportJob {
            stage: ExportStage::Preparing,
            format,
            path,
            width_px,
            restore_doc: std::mem::replace(&mut self.doc, export_doc),
            restore_y_offset: self.y_offset,
            restore_window_size,
            restore_selection: self.selection.take(),
        });
        self.y_offset = 0.0;
        ctx.request_repaint();
    }

    /// Advance the export state machine by one tick. See `ExportStage` for
    /// why `Preparing` cannot read `last_lanes` on the same frame the
    /// document was swapped in.
    fn tick_export(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.export_job else { return };
        match job.stage {
            ExportStage::Preparing => {
                job.stage = ExportStage::Measuring;
                ctx.request_repaint();
            }
            ExportStage::Measuring => {
                let content_bottom = self.last_lanes.iter().map(|l| l.bottom).fold(0.0f32, f32::max);
                let height = (content_bottom + 24.0).max(160.0);
                let size = egui::Vec2::new(job.width_px, height);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                job.stage = ExportStage::Settling(4);
                ctx.request_repaint();
            }
            ExportStage::Settling(0) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                job.stage = ExportStage::Capturing;
                ctx.request_repaint();
            }
            ExportStage::Settling(n) => {
                job.stage = ExportStage::Settling(n - 1);
                ctx.request_repaint();
            }
            ExportStage::Capturing => {
                let image = ctx.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Screenshot { image, .. } => Some(image.clone()),
                        _ => None,
                    })
                });
                match image {
                    Some(image) => self.finish_export(ctx, &image),
                    None => ctx.request_repaint(),
                }
            }
        }
    }

    fn finish_export(&mut self, ctx: &egui::Context, image: &egui::ColorImage) {
        let Some(job) = self.export_job.take() else { return };
        let [w, h] = image.size;
        let rgba: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|c| [c.r(), c.g(), c.b(), c.a()])
            .collect();
        let result = match job.format {
            ExportFormat::Png => export::save_png(&job.path, &rgba, w as u32, h as u32),
            ExportFormat::Pdf => export::save_pdf(&job.path, &rgba, w as u32, h as u32),
        };

        self.doc = job.restore_doc;
        self.y_offset = job.restore_y_offset;
        self.selection = job.restore_selection;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(job.restore_window_size));

        match result {
            Ok(()) => self.info(format!("Export gespeichert unter {}", job.path.display())),
            Err(e) => self.error(e),
        }
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

    /// The event a freshly added event should default to nesting inside,
    /// based on whatever is selected in the sidebar — selecting a range
    /// event (e.g. "Peloponnesischer Krieg") and then adding a new one is a
    /// strong signal it belongs inside it, the same intent the dedicated
    /// "+ Verschachteltes Ereignis" button already captures explicitly. The
    /// "Verschachtelt in:" field in the form itself can always override this.
    pub fn default_parent_event(&self) -> Option<Id> {
        match self.selection {
            Some(Selection::Event(id)) => self.doc.event(id).filter(|e| e.span.is_range()).map(|_| id),
            _ => None,
        }
    }

    pub fn new_event_dialog(&mut self) {
        match self.default_owner() {
            Some(owner) => {
                let mut form = EventForm::new(owner);
                form.parent = self.default_parent_event();
                self.dialog = Dialog::Event(form);
            }
            None => self.error("Zuerst einen Zeitstrahl anlegen — Ereignisse brauchen einen Träger."),
        }
    }

    /// The group a freshly added group or timeline should default to nesting
    /// under, based on whatever is selected in the sidebar — so picking
    /// "Römische Antike" and then "+ Zeitstrahl" already has that group
    /// chosen instead of defaulting to "keine".
    pub fn default_group(&self) -> Option<Id> {
        match self.selection {
            Some(Selection::Group(id)) if self.doc.group(id).is_some() => Some(id),
            Some(Selection::Timeline(id)) => self.doc.timeline(id).and_then(|t| t.group),
            _ => None,
        }
    }

    /// The timeline a freshly added biography should default to, based on
    /// whatever is selected in the sidebar.
    pub fn default_timeline_for_biography(&self) -> Option<Id> {
        match self.selection {
            Some(Selection::Timeline(id)) if self.doc.timeline(id).is_some() => Some(id),
            Some(Selection::Biography(id)) => self.doc.biography(id).and_then(|b| b.timeline),
            _ => self.doc.timelines.first().map(|t| t.id),
        }
    }

    // --- Confirmation -----------------------------------------------------

    fn apply_confirm(&mut self, c: Confirm) {
        match c {
            Confirm::DeleteGroup(id) => {
                self.mutate(|d| d.delete_group(id));
                self.validate_selection();
                self.info("Gruppe entfernt; ihr Inhalt ist eine Ebene nach oben gerückt — Strg+Z macht das rückgängig.");
            }
            Confirm::DeleteTimeline(id) => {
                self.mutate(|d| d.delete_timeline(id));
                self.validate_selection();
                self.info("Zeitstrahl gelöscht — Strg+Z macht das rückgängig.");
            }
            Confirm::DeleteBiography(id) => {
                self.mutate(|d| d.delete_biography(id));
                self.validate_selection();
                self.info("Biografie gelöscht — Strg+Z macht das rückgängig.");
            }
            Confirm::DeleteEvent(id) => {
                self.mutate(|d| d.delete_event(id));
                self.validate_selection();
                self.info("Ereignis gelöscht; verschachtelte Ereignisse sind eine Ebene nach oben gerückt — Strg+Z macht das rückgängig.");
            }
            Confirm::DeleteCategory(id) => {
                self.mutate(|d| d.delete_category(id));
                self.info("Kategorie gelöscht; Unterkategorien sind eine Ebene nach oben gerückt — Strg+Z macht das rückgängig.");
            }
            Confirm::NewLibrary => {
                self.mutate(|d| *d = Document::with_starter_categories());
                self.selection = None;
                self.info("Leere Bibliothek gestartet — Strg+Z macht das rückgängig.");
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

        let exporting = self.export_job.is_some();
        if exporting {
            // Nothing but the canvas while a screenshot is pending — the
            // capture must contain exactly what is being exported, not the
            // sidebar or toolbar around it.
            egui::CentralPanel::no_frame().show(ui, |ui| canvas::draw(self, ui));
            self.tick_export(&ctx);
            return;
        }

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

        // Never autosave the export's filtered document over the real
        // library — but this branch is only reached when `exporting` is
        // already false, so there is nothing to guard here beyond that.
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
        // Last line of defence against losing the final edit. Never while an
        // export is in flight — `self.doc` is the filtered export document
        // until the capture completes and swaps the real one back in.
        if self.dirty && self.export_job.is_none() {
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
            self.info("Gespeichert");
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
            ui.menu_button("Datei", |ui| {
                if ui.button("Neue Bibliothek…").clicked() {
                    self.confirm = Some(Confirm::NewLibrary);
                    ui.close();
                }
                if ui.button("Öffnen…").clicked() {
                    self.open();
                    ui.close();
                }
                if ui.button("Speichern   (Strg+S)").clicked() {
                    self.save();
                    self.info("Gespeichert");
                    ui.close();
                }
                if ui.button("Speichern unter…").clicked() {
                    self.save_as();
                    ui.close();
                }
                ui.separator();
                if ui.button("Beispielbibliothek laden").clicked() {
                    self.load_example();
                    ui.close();
                }
                ui.menu_button("Sicherung wiederherstellen", |ui| {
                    let list = store::backups(&self.path);
                    if list.is_empty() {
                        ui.label("Noch keine Sicherungen");
                    }
                    for (p, label) in list {
                        if ui.button(label.clone()).clicked() {
                            self.confirm = Some(Confirm::Restore(p, label));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button("Ausschnitt exportieren…").clicked() {
                    self.dialog = Dialog::Export(ExportForm::new(&self.doc));
                    ui.close();
                }
                if ui.button("Daten importieren…").clicked() {
                    self.dialog = Dialog::Import(ImportForm::default());
                    ui.close();
                }
                ui.separator();
                if ui.button("Datenordner anzeigen").clicked() {
                    store::reveal_in_explorer(&self.path);
                    ui.close();
                }
            });

            ui.menu_button("Bearbeiten", |ui| {
                if ui
                    .add_enabled(self.can_undo(), egui::Button::new("Rückgängig   (Strg+Z)"))
                    .clicked()
                {
                    self.undo();
                    ui.close();
                }
                if ui
                    .add_enabled(self.can_redo(), egui::Button::new("Wiederholen   (Strg+Y)"))
                    .clicked()
                {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                if ui.button("Kategorien…").clicked() {
                    self.dialog = Dialog::Categories(CategoryEditor::default());
                    ui.close();
                }
            });

            ui.menu_button("Ansicht", |ui| {
                let mut dark = self.doc.view.dark_mode;
                if ui.checkbox(&mut dark, "Dunkles Design").changed() {
                    self.doc.view.dark_mode = dark;
                    self.mark_dirty();
                }
                let mut labels = self.doc.view.show_labels;
                if ui.checkbox(&mut labels, "Ereignis-Beschriftungen anzeigen").changed() {
                    self.doc.view.show_labels = labels;
                    self.mark_dirty();
                }
                ui.separator();
                if ui.button("Alles einpassen   (Pos1)").clicked() {
                    let w = ui.ctx().content_rect().width() - 600.0;
                    self.fit_to_content(w.max(400.0));
                    ui.close();
                }
            });

            if ui.button("Hilfe").clicked() {
                self.show_help = true;
            }

            ui.separator();

            if ui.button("+ Ereignis").on_hover_text("Strg+N").clicked() {
                self.new_event_dialog();
            }
            if ui.button("+ Gruppe").clicked() {
                let mut form = GroupForm::new(self.doc.next_palette_color());
                form.parent = self.default_group();
                self.dialog = Dialog::Group(form);
            }
            if ui.button("+ Zeitstrahl").clicked() {
                let mut form = TimelineForm::new(self.doc.next_palette_color());
                form.group = self.default_group();
                self.dialog = Dialog::Timeline(form);
            }
            if ui.button("+ Biografie").clicked() {
                self.dialog = Dialog::Biography(BiographyForm::new(self.default_timeline_for_biography()));
            }
        });

        ui.horizontal(|ui| {
            ui.label("Suche:");
            let mut search = self.doc.view.filters.search.clone();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .desired_width(180.0)
                    .hint_text("Titel oder Beschreibung"),
            );
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            if resp.changed() {
                self.doc.view.filters.search = search;
                self.mark_dirty();
            }

            // Suggestions jump straight to the match — searching for an
            // event, person, or epoch and landing on an unchanged view would
            // defeat the point of searching at all.
            let candidates = self
                .doc
                .events
                .iter()
                .map(|e| (e.title.clone(), JumpTarget::Event(e.id)))
                .chain(self.doc.biographies.iter().map(|b| (b.name.clone(), JumpTarget::Biography(b.id))))
                .chain(self.doc.timelines.iter().map(|t| (t.name.clone(), JumpTarget::Timeline(t.id))))
                .chain(self.doc.groups.iter().map(|g| (g.name.clone(), JumpTarget::Group(g.id))))
                .chain(self.doc.timelines.iter().flat_map(|t| {
                    let tid = t.id;
                    t.epochs
                        .iter()
                        .enumerate()
                        .map(move |(i, e)| (e.name.clone(), JumpTarget::Epoch(OwnerRef::Timeline(tid), i)))
                }))
                .chain(self.doc.biographies.iter().flat_map(|b| {
                    let bid = b.id;
                    b.life_phases
                        .iter()
                        .enumerate()
                        .map(move |(i, e)| (e.name.clone(), JumpTarget::Epoch(OwnerRef::Biography(bid), i)))
                }));
            let query = self.doc.view.filters.search.clone();
            let picked = panels::suggestions(&resp, "top_search_suggest", &query, candidates, 8);
            if let Some(target) = picked {
                let width = self.last_width.unwrap_or(1200.0);
                self.doc.view.filters.search.clear();
                self.jump_to(target, width);
            } else if let Some(d) = HDate::parse(query.trim()) {
                // Nothing matched by name — try reading the query as a date
                // directly, so "Anfang 1789" or "431 v. Chr." still jump
                // somewhere instead of the field silently doing nothing.
                ui.weak(format!("↵ Enter: springe zu {}", d.label()));
                if resp.has_focus() && resp.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let width = self.last_width.unwrap_or(1200.0);
                    self.doc.view.filters.search.clear();
                    self.jump_to(JumpTarget::Date(d), width);
                }
            }

            if !self.doc.view.filters.search.is_empty() && ui.button("Leeren").clicked() {
                self.doc.view.filters.search.clear();
                self.mark_dirty();
            }

            ui.separator();

            ui.label("Detailgrad:")
                .on_hover_text("Verschiebt, wie viel der aktuelle Zoom zeigt");
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

            if ui.button("-").on_hover_text("Verkleinern").clicked() {
                self.doc.view.pixels_per_year =
                    (self.doc.view.pixels_per_year * 0.7).clamp(layout::MIN_PPY, layout::MAX_PPY);
                self.mark_dirty();
            }
            if ui.button("+").on_hover_text("Vergrößern").clicked() {
                self.doc.view.pixels_per_year =
                    (self.doc.view.pixels_per_year * 1.4).clamp(layout::MIN_PPY, layout::MAX_PPY);
                self.mark_dirty();
            }
            if ui.button("Einpassen").clicked() {
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
                let saved = if self.dirty { "speichert…" } else { "gespeichert" };
                let range = match (self.last_axis, self.last_width) {
                    (Some(a), Some(w)) => {
                        let (from, to) = a.visible_range(w);
                        format!(
                            " · zeigt {} – {} ({:.2} px/Jahr)",
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
                    "{} · {} Zeitstrahlen · {} Biografien · {} Ereignisse{} · {}",
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
                ui.weak("Rad = Zoom · Ziehen = Verschieben · Doppelklick = Hinzufügen · Hilfe für mehr")
                    .on_hover_text("Rad = Zoom, Umschalt+Rad = Verschieben, Alt+Rad = Spuren scrollen, Ziehen = Verschieben, Doppelklick auf leeren Bereich = Ereignis hinzufügen");
            });
        });
    }

    fn show_confirm(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, body) = match c {
            Confirm::DeleteGroup(id) => (
                "Gruppe entfernen?",
                format!(
                    "{}{}{} wird entfernt. Die enthaltenen Zeitstrahlen und Gruppen bleiben erhalten und rücken eine Ebene nach oben.",
                    "“",
                    self.doc.group(*id).map(|g| g.name.clone()).unwrap_or_default(),
                    "”"
                ),
            ),
            Confirm::DeleteTimeline(id) => (
                "Zeitstrahl löschen?",
                format!(
                    "“{}” und die zugehörigen {} Ereignisse werden entfernt.",
                    self.doc.timeline(*id).map(|t| t.name.clone()).unwrap_or_default(),
                    self.doc.events_of(OwnerRef::Timeline(*id)).count()
                ),
            ),
            Confirm::DeleteBiography(id) => (
                "Biografie löschen?",
                format!(
                    "“{}” und die zugehörigen {} Ereignisse werden entfernt.",
                    self.doc.biography(*id).map(|b| b.name.clone()).unwrap_or_default(),
                    self.doc.events_of(OwnerRef::Biography(*id)).count()
                ),
            ),
            Confirm::DeleteEvent(id) => (
                "Ereignis löschen?",
                format!(
                    "“{}” wird entfernt.",
                    self.doc.event(*id).map(|e| e.title.clone()).unwrap_or_default()
                ),
            ),
            Confirm::DeleteCategory(id) => (
                "Kategorie löschen?",
                format!(
                    "“{}” wird von allen Einträgen entfernt, die sie verwenden. Unterkategorien bleiben erhalten und rücken eine Ebene nach oben.",
                    self.doc.category(*id).map(|c| c.name.clone()).unwrap_or_default()
                ),
            ),
            Confirm::NewLibrary => (
                "Leere Bibliothek starten?",
                "Alles aktuell Geladene wird aus diesem Fenster entfernt.".to_string(),
            ),
            Confirm::Restore(_, label) => (
                "Sicherung wiederherstellen?",
                format!("{label} ersetzt das aktuell Geladene."),
            ),
        };

        let mut decision: Option<bool> = None;
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(title);
            ui.add_space(6.0);
            ui.label(body);
            ui.add_space(4.0);
            ui.weak("Das lässt sich mit Strg+Z rückgängig machen.");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Abbrechen").clicked() {
                    decision = Some(false);
                }
                if ui.button("Ja, fortfahren").clicked() {
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
        egui::Window::new("So funktioniert Timeline Explorer")
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Navigation");
                    ui.label("• Mausrad zoomt um den Zeiger herum.");
                    ui.label("• Umschalt + Rad, oder Ziehen, verschiebt entlang der Zeitachse.");
                    ui.label("• Alt + Rad scrollt durch die Spuren, wenn es viele gibt.");
                    ui.label("• Pos1, oder der Einpassen-Knopf, zeigt den gesamten Datenbestand.");
                    ui.add_space(8.0);

                    ui.heading("Daten hinzufügen");
                    ui.label("• Doppelklick auf eine leere Stelle einer Spur fügt dort ein Ereignis hinzu.");
                    ui.label("• Strg+N fügt ein Ereignis hinzu; die +-Knöpfe fügen Zeitstrahlen und Biografien hinzu.");
                    ui.label("• Etwas auswählen und E zum Bearbeiten drücken, Entf zum Entfernen.");
                    ui.add_space(8.0);

                    ui.heading("Datumsangaben");
                    ui.label("Datum nach Belieben eingeben: 44 v. Chr., -44, um 250 v. Chr., 1789, 14.07.1789, 1789-07-14. Für Unsicherheit ±20 anhängen. Das Formular zeigt, wie die Eingabe verstanden wurde.");
                    ui.add_space(8.0);

                    ui.heading("Zoom und Bedeutung");
                    ui.label("Jeder Eintrag hat eine Bedeutung von Detail bis Epochal. Herausgezoomt sieht man nur die großen Linien; Hineinzoomen zeigt den Rest. Der Detailgrad-Regler verschiebt diese Balance in beide Richtungen.");
                    ui.add_space(8.0);

                    ui.heading("Gruppen");
                    ui.label("Mit + Gruppe eine Oberkategorie wie \"Europäische Geschichte\" oder \"Griechische Antike\" anlegen, dann Zeitstrahlen über deren Editor hineinlegen. Gruppen lassen sich verschachteln, \"Griechische Antike > Klassische Polis > Athen\" funktioniert also.");
                    ui.label("Eine Gruppe in der Seitenleiste einklappen lässt sie zu einem einzigen Band werden, das für alles darin steht — nützlich, um ganze Kulturen zu vergleichen. Wieder ausklappen, um Sparta und Athen wieder einzeln zu bearbeiten.");
                    ui.add_space(8.0);

                    ui.heading("Zusammenlaufende Zeitstrahlen");
                    ui.label("Im Editor eines Zeitstrahls “Geht auf in einem anderen Zeitstrahl” aktivieren und das Datum angeben — das Band biegt dann in dessen Spur ein und endet dort. “Spaltet sich ab von einem anderen Zeitstrahl” macht das Gegenteil für Nachfolgestaaten.");
                    ui.add_space(8.0);

                    ui.heading("Deine Daten");
                    ui.label(format!("Alles wird als lesbares JSON gespeichert unter {}", self.path.display()));
                    ui.label("Sie speichert sich automatisch etwa eine Sekunde nach jeder Änderung, hält zehn rotierende Sicherungen vor und braucht nie eine Internetverbindung.");
                });
            });
        self.show_help = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revealing_an_event_unhides_its_timeline_and_expands_every_ancestor_group() {
        let mut doc = Document::default();
        let outer = doc.new_id();
        doc.groups.push(Group {
            id: outer,
            name: "Antiquity".into(),
            color: [0, 0, 0],
            parent: None,
            order: 0,
            collapsed: true,
            visible: true,
            notes: String::new(),
        });
        let inner = doc.new_id();
        doc.groups.push(Group {
            id: inner,
            name: "Greek antiquity".into(),
            color: [0, 0, 0],
            parent: Some(outer),
            order: 0,
            collapsed: true,
            visible: true,
            notes: String::new(),
        });
        let tl = doc.new_id();
        doc.timelines.push(Timeline {
            id: tl,
            name: "Athens".into(),
            color: [0, 0, 0],
            visible: false,
            group: Some(inner),
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });
        let ev = doc.new_id();
        doc.events.push(Event {
            id: ev,
            owner: OwnerRef::Timeline(tl),
            title: "Battle of Marathon".into(),
            description: String::new(),
            span: Span::circa_point(-490),
            importance: 3,
            categories: vec![],
            parent: None,
        });

        reveal_jump_target(&mut doc, JumpTarget::Event(ev));

        assert!(doc.timeline(tl).unwrap().visible);
        assert!(!doc.group(inner).unwrap().collapsed);
        assert!(!doc.group(outer).unwrap().collapsed);
    }

    #[test]
    fn revealing_a_hidden_biography_restores_inline_when_it_has_a_culture() {
        let mut doc = Document::default();
        let tl = doc.new_id();
        doc.timelines.push(Timeline {
            id: tl,
            name: "Rome".into(),
            color: [0, 0, 0],
            visible: true,
            group: None,
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });
        let bio = doc.new_id();
        doc.biographies.push(Biography {
            id: bio,
            name: "Cicero".into(),
            timeline: Some(tl),
            birth: HDate::year(-106),
            death: Some(HDate::year(-43)),
            color: None,
            categories: vec![],
            importance: 4,
            display: BioDisplay::Hidden,
            life_phases: Vec::new(),
            notes: String::new(),
        });

        reveal_jump_target(&mut doc, JumpTarget::Biography(bio));

        assert_eq!(doc.biography(bio).unwrap().display, BioDisplay::Inline);
    }

    #[test]
    fn revealing_an_unhidden_biography_leaves_its_display_alone() {
        // Already showing as its own lane — a jump must not silently switch
        // it to Inline just because it happens to have a culture.
        let mut doc = Document::default();
        let bio = doc.new_id();
        doc.biographies.push(Biography {
            id: bio,
            name: "Anon".into(),
            timeline: None,
            birth: HDate::year(-100),
            death: None,
            color: None,
            categories: vec![],
            importance: 3,
            display: BioDisplay::Lane,
            life_phases: Vec::new(),
            notes: String::new(),
        });
        reveal_jump_target(&mut doc, JumpTarget::Biography(bio));
        assert_eq!(doc.biography(bio).unwrap().display, BioDisplay::Lane);
    }
}
