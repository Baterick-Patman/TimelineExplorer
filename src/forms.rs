//! Modal editors for timelines, biographies, events and categories.
//!
//! Dates are entered as free text and parsed live, with the interpretation
//! echoed back under the field. That keeps entry fast for someone typing
//! hundreds of rows while making a misread date immediately obvious.

use crate::app::{Confirm, Selection, TimelineApp};
use crate::model::*;
use egui::Color32;
use std::collections::BTreeSet;

pub enum Dialog {
    None,
    Group(GroupForm),
    Timeline(TimelineForm),
    Biography(BiographyForm),
    Event(EventForm),
    Categories(CategoryEditor),
}

impl Dialog {
    pub fn is_open(&self) -> bool {
        !matches!(self, Dialog::None)
    }
}

// ---------------------------------------------------------------------------
// Shared widgets
// ---------------------------------------------------------------------------

const OK_GREEN: Color32 = Color32::from_rgb(120, 190, 130);
const BAD_RED: Color32 = Color32::from_rgb(225, 120, 110);

/// Free-text date field with a live interpretation line underneath.
///
/// Returns `Ok(None)` when the field is empty and empty is allowed.
fn date_field(
    ui: &mut egui::Ui,
    label: &str,
    buf: &mut String,
    allow_empty: bool,
) -> Result<Option<HDate>, ()> {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::TextEdit::singleline(buf)
                .desired_width(180.0)
                .hint_text("e.g. 44 BC, c. 250 BC, 1789-07-14"),
        );
    });

    let trimmed = buf.trim();
    if trimmed.is_empty() {
        if allow_empty {
            ui.indent("d", |ui| ui.weak("— none —"));
            return Ok(None);
        }
        ui.indent("d", |ui| ui.colored_label(BAD_RED, "a date is required"));
        return Err(());
    }
    match HDate::parse(trimmed) {
        Some(d) => {
            ui.indent("d", |ui| {
                ui.colored_label(OK_GREEN, format!("reads as {}", d.label()));
            });
            Ok(Some(d))
        }
        None => {
            ui.indent("d", |ui| {
                ui.colored_label(BAD_RED, "not understood — try 44 BC, -44, or 1789-07-14");
            });
            Err(())
        }
    }
}

fn importance_picker(ui: &mut egui::Ui, value: &mut u8) {
    ui.horizontal(|ui| {
        ui.label("Importance:");
        for level in (IMPORTANCE_MIN..=IMPORTANCE_MAX).rev() {
            if ui
                .selectable_label(*value == level, importance_name(level))
                .on_hover_text(format!(
                    "Level {level} — shown from {} zoom onwards",
                    if level >= 4 { "any" } else { "closer" }
                ))
                .clicked()
            {
                *value = level;
            }
        }
    });
}

fn category_picker(ui: &mut egui::Ui, doc: &Document, selected: &mut BTreeSet<Id>) {
    ui.label("Categories:");
    if doc.categories.is_empty() {
        ui.weak("No categories defined yet — add some under Edit > Categories.");
        return;
    }
    egui::ScrollArea::vertical()
        .max_height(110.0)
        .id_salt("cats")
        .show(ui, |ui| {
            for c in &doc.categories {
                let mut on = selected.contains(&c.id);
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::Vec2::new(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::same(2),
                        crate::theme::to_color(c.color),
                    );
                    if ui.checkbox(&mut on, &c.name).changed() {
                        if on {
                            selected.insert(c.id);
                        } else {
                            selected.remove(&c.id);
                        }
                    }
                });
            }
        });
}

fn owner_picker(ui: &mut egui::Ui, doc: &Document, owner: &mut OwnerRef) {
    egui::ComboBox::from_label("Belongs to")
        .selected_text(doc.owner_name(*owner))
        .width(240.0)
        .show_ui(ui, |ui| {
            for t in &doc.timelines {
                ui.selectable_value(owner, OwnerRef::Timeline(t.id), &t.name);
            }
            if !doc.biographies.is_empty() {
                ui.separator();
                for b in &doc.biographies {
                    ui.selectable_value(
                        owner,
                        OwnerRef::Biography(b.id),
                        b.name.clone(),
                    );
                }
            }
        });
}

/// Footer with Cancel / Save. Returns `Some(true)` to save, `Some(false)` to cancel.
fn dialog_buttons(ui: &mut egui::Ui, can_save: bool, save_label: &str) -> Option<bool> {
    let mut result = None;
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            result = Some(false);
        }
        if ui
            .add_enabled(can_save, egui::Button::new(save_label))
            .clicked()
        {
            result = Some(true);
        }
        if !can_save {
            ui.weak("fix the highlighted fields first");
        }
    });
    result
}

// ---------------------------------------------------------------------------
// Event form
// ---------------------------------------------------------------------------

pub struct EventForm {
    pub editing: Option<Id>,
    pub owner: OwnerRef,
    pub title: String,
    pub description: String,
    pub start_text: String,
    pub end_text: String,
    pub is_range: bool,
    pub importance: u8,
    pub categories: BTreeSet<Id>,
    /// Nests this event inside another range event on the same owner —
    /// "Peace of Nicias" inside "Peloponnesian War".
    pub parent: Option<Id>,
}

impl EventForm {
    pub fn new(owner: OwnerRef) -> Self {
        Self {
            editing: None,
            owner,
            title: String::new(),
            description: String::new(),
            start_text: String::new(),
            end_text: String::new(),
            is_range: false,
            importance: 3,
            categories: BTreeSet::new(),
            parent: None,
        }
    }

    pub fn new_at(owner: OwnerRef, date: HDate) -> Self {
        Self {
            start_text: date.label(),
            ..Self::new(owner)
        }
    }

    /// Start a new event already nested inside `parent`, e.g. from the
    /// "+ event" button shown alongside a range event.
    pub fn new_nested(owner: OwnerRef, parent: Id) -> Self {
        Self {
            parent: Some(parent),
            ..Self::new(owner)
        }
    }

    pub fn edit(ev: &Event) -> Self {
        Self {
            editing: Some(ev.id),
            owner: ev.owner,
            title: ev.title.clone(),
            description: ev.description.clone(),
            start_text: ev.span.start.label(),
            end_text: ev.span.end.map(|d| d.label()).unwrap_or_default(),
            is_range: ev.span.is_range(),
            importance: ev.importance,
            categories: ev.categories.iter().copied().collect(),
            parent: ev.parent,
        }
    }
}

/// Choose a parent event to nest under. Only range events belonging to the
/// same owner are offered, since nesting only makes visual sense within one
/// band; the event being edited and anything that would make it its own
/// ancestor are excluded.
fn event_parent_combo(
    ui: &mut egui::Ui,
    doc: &Document,
    owner: OwnerRef,
    editing: Option<Id>,
    value: &mut Option<Id>,
) {
    let text = value
        .and_then(|id| doc.event(id))
        .map(|e| e.title.clone())
        .unwrap_or_else(|| "— none (top level) —".into());
    ui.horizontal(|ui| {
        ui.label("Nested inside:");
        egui::ComboBox::from_id_salt("event_parent")
            .selected_text(text)
            .width(220.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(value, None, "— none (top level) —");
                for e in doc.events_of(owner) {
                    if !e.span.is_range() {
                        continue;
                    }
                    if let Some(editing) = editing {
                        if e.id == editing || doc.would_cycle_event(editing, Some(e.id)) {
                            continue;
                        }
                    }
                    ui.selectable_value(value, Some(e.id), &e.title);
                }
            });
    });
}

fn event_dialog(app: &mut TimelineApp, ctx: &egui::Context, form: &mut EventForm) -> bool {
    let mut keep_open = true;
    let title = if form.editing.is_some() {
        "Edit event"
    } else {
        "New event"
    };

    egui::Modal::new(egui::Id::new("event_dialog")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading(title);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Title:");
            ui.add(
                egui::TextEdit::singleline(&mut form.title)
                    .desired_width(320.0)
                    .hint_text("e.g. Battle of Pydna"),
            );
        });
        owner_picker(ui, &app.doc, &mut form.owner);
        // A parent from a different owner is meaningless once the owner
        // changes — the combo below only ever offers same-owner events.
        if let Some(pid) = form.parent {
            if app.doc.event(pid).map(|e| e.owner) != Some(form.owner) {
                form.parent = None;
            }
        }
        event_parent_combo(ui, &app.doc, form.owner, form.editing, &mut form.parent);
        ui.add_space(6.0);

        let start = date_field(ui, "Date:  ", &mut form.start_text, false);
        ui.checkbox(&mut form.is_range, "This spans a period");
        let end = if form.is_range {
            date_field(ui, "Until:", &mut form.end_text, false)
        } else {
            Ok(None)
        };

        ui.add_space(6.0);
        importance_picker(ui, &mut form.importance);
        ui.add_space(6.0);
        category_picker(ui, &app.doc, &mut form.categories);

        ui.add_space(6.0);
        ui.label("Notes:");
        ui.add(
            egui::TextEdit::multiline(&mut form.description)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );

        let start_ok = start.is_ok();
        let end_ok = end.is_ok();
        let mut ordering_ok = true;
        if let (Ok(Some(s)), Ok(Some(e))) = (&start, &end) {
            if e.decimal_end() < s.decimal() {
                ordering_ok = false;
                ui.colored_label(BAD_RED, "the end date is before the start date");
            }
        }
        let can_save = start_ok && end_ok && ordering_ok && !form.title.trim().is_empty();

        match dialog_buttons(ui, can_save, "Save") {
            Some(true) => {
                let Ok(Some(s)) = start else { return };
                let span = match (form.is_range, end) {
                    (true, Ok(Some(e))) => Span::range(s, e),
                    _ => Span::point(s),
                };
                let title = form.title.trim().to_string();
                let desc = form.description.trim().to_string();
                let cats: Vec<Id> = form.categories.iter().copied().collect();
                let importance = form.importance;
                let owner = form.owner;
                match form.editing {
                    Some(id) => {
                        // Guard again at save time: the tree may have changed
                        // while the dialog was open.
                        let parent = if app.doc.would_cycle_event(id, form.parent) {
                            None
                        } else {
                            form.parent
                        };
                        app.mutate(|doc| {
                            if let Some(ev) = doc.event_mut(id) {
                                ev.title = title;
                                ev.description = desc;
                                ev.span = span;
                                ev.importance = importance;
                                ev.categories = cats;
                                ev.owner = owner;
                                ev.parent = parent;
                            }
                        });
                        app.info("Event updated");
                    }
                    None => {
                        let parent = form.parent;
                        let mut new_id = None;
                        app.mutate(|doc| {
                            let id = doc.new_id();
                            new_id = Some(id);
                            doc.events.push(Event {
                                id,
                                owner,
                                title,
                                description: desc,
                                span,
                                importance,
                                categories: cats,
                                parent,
                            });
                        });
                        app.selection = new_id.map(Selection::Event);
                        app.info("Event added");
                    }
                }
                keep_open = false;
            }
            Some(false) => keep_open = false,
            None => {}
        }
    });

    keep_open
}

// ---------------------------------------------------------------------------
// Group form
// ---------------------------------------------------------------------------

pub struct GroupForm {
    pub editing: Option<Id>,
    pub name: String,
    pub color: Rgb,
    pub parent: Option<Id>,
    pub notes: String,
}

impl GroupForm {
    pub fn new(color: Rgb) -> Self {
        Self {
            editing: None,
            name: String::new(),
            color,
            parent: None,
            notes: String::new(),
        }
    }

    pub fn edit(g: &Group) -> Self {
        Self {
            editing: Some(g.id),
            name: g.name.clone(),
            color: g.color,
            parent: g.parent,
            notes: g.notes.clone(),
        }
    }
}

/// Choose a parent group, excluding any choice that would create a cycle.
fn group_combo(
    ui: &mut egui::Ui,
    doc: &Document,
    id_salt: &str,
    value: &mut Option<Id>,
    moving: Option<Id>,
) {
    let text = value
        .and_then(|id| doc.group(id))
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "— none (top level) —".into());
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(text)
        .width(240.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "— none (top level) —");
            for g in &doc.groups {
                // Never offer a move that would make a group its own ancestor.
                if let Some(m) = moving {
                    if g.id == m || doc.would_cycle(m, Some(g.id)) {
                        continue;
                    }
                }
                ui.selectable_value(value, Some(g.id), &g.name);
            }
        });
}

fn group_dialog(app: &mut TimelineApp, ctx: &egui::Context, form: &mut GroupForm) -> bool {
    let mut keep_open = true;
    let heading = if form.editing.is_some() {
        "Edit group"
    } else {
        "New group"
    };

    egui::Modal::new(egui::Id::new("group_dialog")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading(heading);
        ui.weak("A super-category, e.g. \"European history\" or \"Greek antiquity\". Collapse it to compare whole civilisations; expand it to see the timelines inside.");
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(
                egui::TextEdit::singleline(&mut form.name)
                    .desired_width(280.0)
                    .hint_text("e.g. Greek antiquity"),
            );
            ui.color_edit_button_srgb(&mut form.color);
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Inside:");
            group_combo(ui, &app.doc, "group_parent", &mut form.parent, form.editing);
        });

        ui.add_space(6.0);
        ui.label("Notes:");
        ui.add(
            egui::TextEdit::multiline(&mut form.notes)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );

        let can_save = !form.name.trim().is_empty();
        match dialog_buttons(ui, can_save, "Save") {
            Some(true) => {
                let name = form.name.trim().to_string();
                let color = form.color;
                let notes = form.notes.trim().to_string();
                let parent = form.parent;
                match form.editing {
                    Some(id) => {
                        // Guard again at save time: the tree may have changed
                        // while the dialog was open.
                        let safe_parent = if app.doc.would_cycle(id, parent) {
                            None
                        } else {
                            parent
                        };
                        app.mutate(|doc| {
                            if let Some(g) = doc.group_mut(id) {
                                g.name = name;
                                g.color = color;
                                g.parent = safe_parent;
                                g.notes = notes;
                            }
                        });
                        app.info("Group updated");
                    }
                    None => {
                        let mut new_id = None;
                        app.mutate(|doc| {
                            let id = doc.new_id();
                            new_id = Some(id);
                            let order = doc.groups.len() as u32;
                            doc.groups.push(Group {
                                id,
                                name,
                                color,
                                parent,
                                order,
                                collapsed: false,
                                visible: true,
                                notes,
                            });
                        });
                        app.selection = new_id.map(Selection::Group);
                        app.info("Group added — put timelines in it from their editor.");
                    }
                }
                keep_open = false;
            }
            Some(false) => keep_open = false,
            None => {}
        }
    });

    keep_open
}

// ---------------------------------------------------------------------------
// Timeline form
// ---------------------------------------------------------------------------

/// One staged row in the epoch editor, kept as text until the form is saved
/// so a date can be mid-edit without losing the rest of the row.
#[derive(Clone)]
pub struct EpochRow {
    pub name: String,
    pub color: Rgb,
    pub start_text: String,
    pub end_text: String,
}

impl EpochRow {
    fn new(color: Rgb) -> Self {
        Self {
            name: String::new(),
            color,
            start_text: String::new(),
            end_text: String::new(),
        }
    }

    fn parse(&self) -> Option<Epoch> {
        if self.name.trim().is_empty() {
            return None;
        }
        Some(Epoch {
            name: self.name.trim().to_string(),
            color: self.color,
            start: HDate::parse(&self.start_text)?,
            end: HDate::parse(&self.end_text)?,
        })
    }
}

pub struct TimelineForm {
    pub editing: Option<Id>,
    pub name: String,
    pub color: Rgb,
    pub group: Option<Id>,
    pub use_span: bool,
    pub start_text: String,
    pub end_text: String,
    pub origin_on: bool,
    pub origin_other: Option<Id>,
    pub origin_date: String,
    pub origin_label: String,
    pub merge_on: bool,
    pub merge_other: Option<Id>,
    pub merge_date: String,
    pub merge_label: String,
    pub notes: String,
    pub epochs: Vec<EpochRow>,
}

impl TimelineForm {
    pub fn new(color: Rgb) -> Self {
        Self {
            editing: None,
            name: String::new(),
            color,
            group: None,
            use_span: false,
            start_text: String::new(),
            end_text: String::new(),
            origin_on: false,
            origin_other: None,
            origin_date: String::new(),
            origin_label: String::new(),
            merge_on: false,
            merge_other: None,
            merge_date: String::new(),
            merge_label: String::new(),
            notes: String::new(),
            epochs: Vec::new(),
        }
    }

    pub fn edit(t: &Timeline) -> Self {
        Self {
            editing: Some(t.id),
            name: t.name.clone(),
            color: t.color,
            group: t.group,
            use_span: t.span.is_some(),
            start_text: t.span.map(|s| s.start.label()).unwrap_or_default(),
            end_text: t
                .span
                .and_then(|s| s.end)
                .map(|d| d.label())
                .unwrap_or_default(),
            origin_on: t.origin.is_some(),
            origin_other: t.origin.as_ref().map(|j| j.other),
            origin_date: t
                .origin
                .as_ref()
                .map(|j| j.date.label())
                .unwrap_or_default(),
            origin_label: t.origin.as_ref().map(|j| j.label.clone()).unwrap_or_default(),
            merge_on: t.merge.is_some(),
            merge_other: t.merge.as_ref().map(|j| j.other),
            merge_date: t.merge.as_ref().map(|j| j.date.label()).unwrap_or_default(),
            merge_label: t.merge.as_ref().map(|j| j.label.clone()).unwrap_or_default(),
            notes: t.notes.clone(),
            epochs: t
                .epochs
                .iter()
                .map(|e| EpochRow {
                    name: e.name.clone(),
                    color: e.color,
                    start_text: e.start.label(),
                    end_text: e.end.label(),
                })
                .collect(),
        }
    }
}

/// Combo for choosing another timeline, excluding `exclude` so a timeline
/// cannot merge into itself.
fn timeline_combo(
    ui: &mut egui::Ui,
    doc: &Document,
    id_salt: &str,
    label: &str,
    value: &mut Option<Id>,
    exclude: Option<Id>,
) {
    let text = value
        .and_then(|id| doc.timeline(id))
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "— choose —".into());
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(text)
        .width(200.0)
        .show_ui(ui, |ui| {
            for t in &doc.timelines {
                if Some(t.id) == exclude {
                    continue;
                }
                ui.selectable_value(value, Some(t.id), &t.name);
            }
        });
    ui.label(label);
}

fn timeline_dialog(app: &mut TimelineApp, ctx: &egui::Context, form: &mut TimelineForm) -> bool {
    let mut keep_open = true;
    let heading = if form.editing.is_some() {
        "Edit timeline"
    } else {
        "New timeline"
    };

    egui::Modal::new(egui::Id::new("timeline_dialog")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(heading);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(
                egui::TextEdit::singleline(&mut form.name)
                    .desired_width(280.0)
                    .hint_text("e.g. Roman Republic"),
            );
            ui.color_edit_button_srgb(&mut form.color);
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Inside group:");
            group_combo(ui, &app.doc, "tl_group", &mut form.group, None);
        });

        ui.add_space(6.0);
        ui.checkbox(
            &mut form.use_span,
            "Set an explicit lifespan (otherwise inferred from its events)",
        );
        let (start, end) = if form.use_span {
            (
                date_field(ui, "From: ", &mut form.start_text, false),
                date_field(ui, "To:   ", &mut form.end_text, true),
            )
        } else {
            (Ok(None), Ok(None))
        };

        ui.add_space(10.0);
        ui.separator();
        ui.label(
            egui::RichText::new("Relationships to other timelines")
                .strong(),
        );
        ui.weak("Bands curve into one another at these points instead of just running side by side.");
        ui.add_space(6.0);

        ui.checkbox(&mut form.origin_on, "Splits from another timeline");
        let origin_date = if form.origin_on {
            ui.horizontal(|ui| {
                timeline_combo(
                    ui,
                    &app.doc,
                    "origin_combo",
                    "is the parent",
                    &mut form.origin_other,
                    form.editing,
                );
            });
            let d = date_field(ui, "at:   ", &mut form.origin_date, false);
            ui.horizontal(|ui| {
                ui.label("Label:");
                ui.add(
                    egui::TextEdit::singleline(&mut form.origin_label)
                        .desired_width(240.0)
                        .hint_text("optional, e.g. Wars of the Diadochi"),
                );
            });
            d
        } else {
            Ok(None)
        };

        ui.add_space(6.0);
        ui.checkbox(&mut form.merge_on, "Merges into another timeline");
        let merge_date = if form.merge_on {
            ui.horizontal(|ui| {
                timeline_combo(
                    ui,
                    &app.doc,
                    "merge_combo",
                    "absorbs it",
                    &mut form.merge_other,
                    form.editing,
                );
            });
            let d = date_field(ui, "at:   ", &mut form.merge_date, false);
            ui.horizontal(|ui| {
                ui.label("Label:");
                ui.add(
                    egui::TextEdit::singleline(&mut form.merge_label)
                        .desired_width(240.0)
                        .hint_text("optional, e.g. Battle of Pydna"),
                );
            });
            d
        } else {
            Ok(None)
        };

        ui.add_space(10.0);
        ui.separator();
        ui.label(egui::RichText::new("Epochs").strong());
        ui.weak("Colour-code eras along this band — \"Archaic\", \"Classical\" — without splitting it into separate timelines.");
        ui.add_space(4.0);

        let mut remove_epoch = None;
        let mut epochs_ready = true;
        for (i, row) in form.epochs.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut row.color);
                ui.add(
                    egui::TextEdit::singleline(&mut row.name)
                        .desired_width(110.0)
                        .hint_text("e.g. Archaic"),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut row.start_text)
                        .desired_width(85.0)
                        .hint_text("start"),
                );
                ui.label("–");
                ui.add(
                    egui::TextEdit::singleline(&mut row.end_text)
                        .desired_width(85.0)
                        .hint_text("end"),
                );
                if ui.small_button("Delete").clicked() {
                    remove_epoch = Some(i);
                }
            });
            let name_ok = !row.name.trim().is_empty();
            let dates_ok = HDate::parse(&row.start_text).is_some() && HDate::parse(&row.end_text).is_some();
            if !name_ok || !dates_ok {
                epochs_ready = false;
                ui.indent("epoch_err", |ui| {
                    ui.colored_label(BAD_RED, "needs a name and two valid dates");
                });
            }
        }
        if let Some(i) = remove_epoch {
            form.epochs.remove(i);
        }
        if ui.small_button("+ Epoch").clicked() {
            let color = form
                .epochs
                .last()
                .map(|e| e.color)
                .unwrap_or(form.color);
            form.epochs.push(EpochRow::new(color));
        }

        ui.add_space(8.0);
        ui.label("Notes:");
        ui.add(
            egui::TextEdit::multiline(&mut form.notes)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );

        let origin_ready = !form.origin_on || (form.origin_other.is_some() && origin_date.is_ok());
        let merge_ready = !form.merge_on || (form.merge_other.is_some() && merge_date.is_ok());
        if form.origin_on && form.origin_other.is_none() {
            ui.colored_label(BAD_RED, "choose the timeline it splits from");
        }
        if form.merge_on && form.merge_other.is_none() {
            ui.colored_label(BAD_RED, "choose the timeline it merges into");
        }
        let can_save = !form.name.trim().is_empty()
            && start.is_ok()
            && end.is_ok()
            && origin_ready
            && merge_ready
            && epochs_ready;

        match dialog_buttons(ui, can_save, "Save") {
            Some(true) => {
                let span = match (form.use_span, &start, &end) {
                    (true, Ok(Some(s)), Ok(e)) => Some(Span { start: *s, end: *e }),
                    _ => None,
                };
                let origin = match (form.origin_on, form.origin_other, &origin_date) {
                    (true, Some(other), Ok(Some(date))) => Some(Junction {
                        other,
                        date: *date,
                        label: form.origin_label.trim().to_string(),
                    }),
                    _ => None,
                };
                let merge = match (form.merge_on, form.merge_other, &merge_date) {
                    (true, Some(other), Ok(Some(date))) => Some(Junction {
                        other,
                        date: *date,
                        label: form.merge_label.trim().to_string(),
                    }),
                    _ => None,
                };
                let name = form.name.trim().to_string();
                let color = form.color;
                let notes = form.notes.trim().to_string();
                let group = form.group;
                // Already validated by `epochs_ready` above.
                let epochs: Vec<Epoch> = form.epochs.iter().filter_map(EpochRow::parse).collect();

                match form.editing {
                    Some(id) => {
                        app.mutate(|doc| {
                            if let Some(t) = doc.timeline_mut(id) {
                                t.name = name;
                                t.color = color;
                                t.group = group;
                                t.span = span;
                                t.origin = origin;
                                t.merge = merge;
                                t.notes = notes;
                                t.epochs = epochs;
                            }
                        });
                        app.info("Timeline updated");
                    }
                    None => {
                        let mut new_id = None;
                        app.mutate(|doc| {
                            let id = doc.new_id();
                            new_id = Some(id);
                            // Order within the group it is being added to.
                            let order = doc.timelines_in(group).len() as u32;
                            doc.timelines.push(Timeline {
                                id,
                                name,
                                color,
                                visible: true,
                                group,
                                order,
                                span,
                                origin,
                                merge,
                                notes,
                                epochs,
                            });
                        });
                        app.selection = new_id.map(Selection::Timeline);
                        app.info("Timeline added");
                    }
                }
                keep_open = false;
            }
            Some(false) => keep_open = false,
            None => {}
        }
    });

    keep_open
}

// ---------------------------------------------------------------------------
// Biography form
// ---------------------------------------------------------------------------

pub struct BiographyForm {
    pub editing: Option<Id>,
    pub name: String,
    pub timeline: Option<Id>,
    pub birth_text: String,
    pub death_text: String,
    pub own_color: bool,
    pub color: Rgb,
    pub categories: BTreeSet<Id>,
    pub importance: u8,
    pub display: BioDisplay,
    pub notes: String,
}

impl BiographyForm {
    pub fn new(timeline: Option<Id>) -> Self {
        Self {
            editing: None,
            name: String::new(),
            timeline,
            birth_text: String::new(),
            death_text: String::new(),
            own_color: false,
            color: [160, 160, 180],
            categories: BTreeSet::new(),
            importance: 3,
            display: BioDisplay::Inline,
            notes: String::new(),
        }
    }

    pub fn edit(b: &Biography) -> Self {
        Self {
            editing: Some(b.id),
            name: b.name.clone(),
            timeline: b.timeline,
            birth_text: b.birth.label(),
            death_text: b.death.map(|d| d.label()).unwrap_or_default(),
            own_color: b.color.is_some(),
            color: b.color.unwrap_or([160, 160, 180]),
            categories: b.categories.iter().copied().collect(),
            importance: b.importance,
            display: b.display,
            notes: b.notes.clone(),
        }
    }
}

fn biography_dialog(app: &mut TimelineApp, ctx: &egui::Context, form: &mut BiographyForm) -> bool {
    let mut keep_open = true;
    let heading = if form.editing.is_some() {
        "Edit biography"
    } else {
        "New biography"
    };

    egui::Modal::new(egui::Id::new("bio_dialog")).show(ctx, |ui| {
        ui.set_width(450.0);
        ui.heading(heading);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(
                egui::TextEdit::singleline(&mut form.name)
                    .desired_width(300.0)
                    .hint_text("e.g. Marcus Tullius Cicero"),
            );
        });

        ui.horizontal(|ui| {
            let text = form
                .timeline
                .and_then(|id| app.doc.timeline(id))
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "— none —".into());
            egui::ComboBox::from_id_salt("bio_tl")
                .selected_text(text)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut form.timeline, None, "— none —");
                    for t in &app.doc.timelines {
                        ui.selectable_value(&mut form.timeline, Some(t.id), &t.name);
                    }
                });
            ui.label("culture / timeline");
        });

        ui.add_space(6.0);
        let birth = date_field(ui, "Born: ", &mut form.birth_text, false);
        let death = date_field(ui, "Died: ", &mut form.death_text, true);

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Show as:");
            for d in [BioDisplay::Hidden, BioDisplay::Inline, BioDisplay::Lane] {
                let enabled = d != BioDisplay::Inline || form.timeline.is_some();
                let resp = ui.add_enabled(
                    enabled,
                    egui::Button::selectable(form.display == d, d.name()),
                );
                if resp.clicked() {
                    form.display = d;
                }
                if !enabled {
                    resp.on_hover_text("Inline needs a parent culture");
                }
            }
        });
        // Inline is meaningless without a parent to nest under.
        if form.display == BioDisplay::Inline && form.timeline.is_none() {
            form.display = BioDisplay::Lane;
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut form.own_color, "Own colour");
            if form.own_color {
                ui.color_edit_button_srgb(&mut form.color);
            } else {
                ui.weak("inherits the culture's colour");
            }
        });

        ui.add_space(6.0);
        importance_picker(ui, &mut form.importance);
        ui.add_space(6.0);
        category_picker(ui, &app.doc, &mut form.categories);

        ui.add_space(6.0);
        ui.label("Notes:");
        ui.add(
            egui::TextEdit::multiline(&mut form.notes)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );

        let mut ordering_ok = true;
        if let (Ok(Some(b)), Ok(Some(d))) = (&birth, &death) {
            if d.decimal_end() < b.decimal() {
                ordering_ok = false;
                ui.colored_label(BAD_RED, "the death date is before the birth date");
            }
        }
        let can_save =
            !form.name.trim().is_empty() && birth.is_ok() && death.is_ok() && ordering_ok;

        match dialog_buttons(ui, can_save, "Save") {
            Some(true) => {
                let Ok(Some(birth)) = birth else { return };
                let death = death.unwrap_or(None);
                let name = form.name.trim().to_string();
                let timeline = form.timeline;
                let color = form.own_color.then_some(form.color);
                let cats: Vec<Id> = form.categories.iter().copied().collect();
                let importance = form.importance;
                let display = form.display;
                let notes = form.notes.trim().to_string();

                match form.editing {
                    Some(id) => {
                        app.mutate(|doc| {
                            if let Some(b) = doc.biography_mut(id) {
                                b.name = name;
                                b.timeline = timeline;
                                b.birth = birth;
                                b.death = death;
                                b.color = color;
                                b.categories = cats;
                                b.importance = importance;
                                b.display = display;
                                b.notes = notes;
                            }
                        });
                        app.info("Biography updated");
                    }
                    None => {
                        let mut new_id = None;
                        app.mutate(|doc| {
                            let id = doc.new_id();
                            new_id = Some(id);
                            doc.biographies.push(Biography {
                                id,
                                name,
                                timeline,
                                birth,
                                death,
                                color,
                                categories: cats,
                                importance,
                                display,
                                notes,
                            });
                        });
                        app.selection = new_id.map(Selection::Biography);
                        app.info("Biography added");
                    }
                }
                keep_open = false;
            }
            Some(false) => keep_open = false,
            None => {}
        }
    });

    keep_open
}

// ---------------------------------------------------------------------------
// Category editor
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CategoryEditor {
    pub new_name: String,
    pub new_color: Option<Rgb>,
}

fn category_dialog(app: &mut TimelineApp, ctx: &egui::Context, ed: &mut CategoryEditor) -> bool {
    let mut keep_open = true;
    let color = *ed.new_color.get_or_insert_with(|| {
        STARTER_CATEGORIES[app.doc.categories.len() % STARTER_CATEGORIES.len()].1
    });
    let _ = color;

    egui::Modal::new(egui::Id::new("cat_dialog")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.heading("Categories");
        ui.weak("Rename, recolour, add or remove freely — nothing depends on a fixed list.");
        ui.add_space(8.0);

        let mut rename: Option<(Id, String)> = None;
        let mut recolour: Option<(Id, Rgb)> = None;
        let mut remove: Option<Id> = None;

        egui::ScrollArea::vertical()
            .max_height(280.0)
            .show(ui, |ui| {
                for c in &app.doc.categories {
                    ui.horizontal(|ui| {
                        let mut col = c.color;
                        if ui.color_edit_button_srgb(&mut col).changed() {
                            recolour = Some((c.id, col));
                        }
                        let mut name = c.name.clone();
                        if ui
                            .add(egui::TextEdit::singleline(&mut name).desired_width(230.0))
                            .changed()
                        {
                            rename = Some((c.id, name));
                        }
                        let uses = app
                            .doc
                            .events
                            .iter()
                            .filter(|e| e.categories.contains(&c.id))
                            .count()
                            + app
                                .doc
                                .biographies
                                .iter()
                                .filter(|b| b.categories.contains(&c.id))
                                .count();
                        ui.weak(format!("{uses}"));
                        if ui.button("Delete").on_hover_text("Delete category").clicked() {
                            remove = Some(c.id);
                        }
                    });
                }
            });

        if let Some((id, name)) = rename {
            app.mutate(|doc| {
                if let Some(c) = doc.categories.iter_mut().find(|c| c.id == id) {
                    c.name = name;
                }
            });
        }
        if let Some((id, col)) = recolour {
            app.mutate(|doc| {
                if let Some(c) = doc.categories.iter_mut().find(|c| c.id == id) {
                    c.color = col;
                }
            });
        }
        if let Some(id) = remove {
            app.confirm = Some(Confirm::DeleteCategory(id));
        }

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let col = ed.new_color.get_or_insert([140, 140, 150]);
            ui.color_edit_button_srgb(col);
            ui.add(
                egui::TextEdit::singleline(&mut ed.new_name)
                    .desired_width(200.0)
                    .hint_text("new category name"),
            );
            let ok = !ed.new_name.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new("Add")).clicked() {
                let name = ed.new_name.trim().to_string();
                let color = *col;
                app.mutate(|doc| {
                    let id = doc.new_id();
                    doc.categories.push(Category { id, name, color });
                });
                ed.new_name.clear();
                ed.new_color = None;
            }
        });

        ui.add_space(10.0);
        if ui.button("Close").clicked() {
            keep_open = false;
        }
    });

    keep_open
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub fn show_dialogs(app: &mut TimelineApp, ctx: &egui::Context) {
    if !app.dialog.is_open() {
        return;
    }
    // Move the form out so the dialog code can borrow `app` mutably.
    let mut dialog = std::mem::replace(&mut app.dialog, Dialog::None);
    let keep = match &mut dialog {
        Dialog::Group(f) => group_dialog(app, ctx, f),
        Dialog::Event(f) => event_dialog(app, ctx, f),
        Dialog::Timeline(f) => timeline_dialog(app, ctx, f),
        Dialog::Biography(f) => biography_dialog(app, ctx, f),
        Dialog::Categories(f) => category_dialog(app, ctx, f),
        Dialog::None => false,
    };
    // A dialog opened *by* this dialog (e.g. a delete confirmation) wins.
    if keep && !app.dialog.is_open() {
        app.dialog = dialog;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_an_event_round_trips_through_the_form() {
        let ev = Event {
            id: Id(3),
            owner: OwnerRef::Timeline(Id(1)),
            title: "Ides of March".into(),
            description: "Assassination of Caesar".into(),
            span: Span::point(HDate {
                month: Some(3),
                day: Some(15),
                ..HDate::year(-44)
            }),
            importance: 5,
            categories: vec![Id(9)],
            parent: None,
        };
        let form = EventForm::edit(&ev);
        assert_eq!(form.editing, Some(Id(3)));
        assert!(!form.is_range);
        // The date must survive the trip through its own text form.
        let parsed = HDate::parse(&form.start_text).expect("form text must re-parse");
        assert_eq!(parsed, ev.span.start);
        assert!(form.categories.contains(&Id(9)));
    }

    #[test]
    fn editing_a_range_event_keeps_both_ends() {
        let ev = Event {
            id: Id(1),
            owner: OwnerRef::Timeline(Id(1)),
            title: "Second Punic War".into(),
            description: String::new(),
            span: Span::range(HDate::year(-218), HDate::year(-201)),
            importance: 5,
            categories: vec![],
            parent: None,
        };
        let form = EventForm::edit(&ev);
        assert!(form.is_range);
        assert_eq!(HDate::parse(&form.start_text).unwrap().year, -218);
        assert_eq!(HDate::parse(&form.end_text).unwrap().year, -201);
    }

    #[test]
    fn editing_a_timeline_preserves_its_junctions() {
        let tl = Timeline {
            id: Id(2),
            name: "Macedon".into(),
            color: [1, 2, 3],
            visible: true,
            group: None,
            order: 1,
            span: Some(Span::range(HDate::year(-306), HDate::year(-168))),
            origin: None,
            merge: Some(Junction {
                other: Id(1),
                date: HDate::year(-168),
                label: "Pydna".into(),
            }),
            notes: String::new(),
            epochs: Vec::new(),
        };
        let form = TimelineForm::edit(&tl);
        assert!(form.merge_on);
        assert_eq!(form.merge_other, Some(Id(1)));
        assert_eq!(HDate::parse(&form.merge_date).unwrap().year, -168);
        assert_eq!(form.merge_label, "Pydna");
        assert!(form.use_span);
    }

    #[test]
    fn a_timeline_without_a_span_does_not_claim_to_have_one() {
        let tl = Timeline {
            id: Id(2),
            name: "X".into(),
            color: [1, 2, 3],
            visible: true,
            group: None,
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        };
        let form = TimelineForm::edit(&tl);
        assert!(!form.use_span);
        assert!(form.start_text.is_empty());
    }

    #[test]
    fn a_biography_with_no_death_date_leaves_the_field_empty() {
        let b = Biography {
            id: Id(5),
            name: "Anon".into(),
            timeline: None,
            birth: HDate::circa(-500),
            death: None,
            color: None,
            categories: vec![],
            importance: 2,
            display: BioDisplay::Lane,
            notes: String::new(),
        };
        let form = BiographyForm::edit(&b);
        assert!(form.death_text.is_empty());
        assert!(!form.own_color);
        assert_eq!(
            HDate::parse(&form.birth_text).unwrap().qualifier,
            DateQualifier::Circa,
            "the circa qualifier must survive the round trip"
        );
    }
}
