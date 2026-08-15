//! Left sidebar (timelines, biographies, filters) and right inspector.

use crate::app::{Confirm, Selection, TimelineApp};
use crate::forms::{CategoryEditor, Dialog, EventForm, GroupForm};
use crate::model::*;
use crate::theme::to_color;
use std::collections::BTreeSet;

/// Deferred edits, collected while iterating the document immutably.
enum Action {
    Select(Selection),
    Edit(Selection),
    Delete(Selection),
    ToggleVisible(Id),
    ToggleGroupVisible(Id),
    ToggleCollapsed(Id),
    Move(Id, i32),
    SetDisplay(Id, BioDisplay),
    AddEventTo(OwnerRef),
    AddNestedEventTo(OwnerRef, Id),
    NewGroupUnder(Option<Id>),
}

pub fn sidebar(app: &mut TimelineApp, ui: &mut egui::Ui) {
    let mut actions: Vec<Action> = Vec::new();

    egui::ScrollArea::vertical().show(ui, |ui| {
        timelines_section(app, ui, &mut actions);
        ui.add_space(10.0);
        biographies_section(app, ui, &mut actions);
        ui.add_space(10.0);
        filters_section(app, ui);
    });

    for a in actions {
        apply(app, a);
    }
}

fn apply(app: &mut TimelineApp, a: Action) {
    match a {
        Action::Select(sel) => app.selection = Some(sel),
        Action::Edit(sel) => app.open_editor_for(sel),
        Action::Delete(sel) => {
            app.confirm = Some(match sel {
                Selection::Group(id) => Confirm::DeleteGroup(id),
                Selection::Timeline(id) => Confirm::DeleteTimeline(id),
                Selection::Biography(id) => Confirm::DeleteBiography(id),
                Selection::Event(id) => Confirm::DeleteEvent(id),
            })
        }
        Action::ToggleVisible(id) => app.mutate(|doc| {
            if let Some(t) = doc.timeline_mut(id) {
                t.visible = !t.visible;
            }
        }),
        Action::Move(id, delta) => app.mutate(|doc| reorder(doc, id, delta)),
        Action::SetDisplay(id, d) => app.mutate(|doc| {
            if let Some(b) = doc.biography_mut(id) {
                b.display = d;
            }
        }),
        Action::AddEventTo(owner) => app.dialog = Dialog::Event(EventForm::new(owner)),
        Action::AddNestedEventTo(owner, parent) => {
            app.dialog = Dialog::Event(EventForm::new_nested(owner, parent))
        }
        Action::ToggleGroupVisible(id) => app.mutate(|doc| {
            if let Some(g) = doc.group_mut(id) {
                g.visible = !g.visible;
            }
        }),
        Action::ToggleCollapsed(id) => app.mutate(|doc| {
            if let Some(g) = doc.group_mut(id) {
                g.collapsed = !g.collapsed;
            }
        }),
        Action::NewGroupUnder(parent) => {
            let color = app.doc.next_palette_color();
            let mut form = GroupForm::new(color);
            form.parent = parent;
            app.dialog = Dialog::Group(form);
        }
    }
}

/// Move a timeline up or down among its siblings, renumbering so order stays
/// dense. Reordering is scoped to the timeline's own group: a "move up" should
/// never silently jump it into a neighbouring group.
fn reorder(doc: &mut Document, id: Id, delta: i32) {
    let Some(group) = doc.timelines.iter().find(|t| t.id == id).map(|t| t.group) else {
        return;
    };
    let mut siblings: Vec<Id> = doc.timelines_in(group).iter().map(|t| t.id).collect();
    let Some(pos) = siblings.iter().position(|s| *s == id) else {
        return;
    };
    let target = pos as i32 + delta;
    if target < 0 || target >= siblings.len() as i32 {
        return;
    }
    siblings.swap(pos, target as usize);
    for (i, sid) in siblings.iter().enumerate() {
        if let Some(t) = doc.timeline_mut(*sid) {
            t.order = i as u32;
        }
    }
}

fn section_header(ui: &mut egui::Ui, title: &str, count: usize) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.weak(format!("({count})"));
    });
}

fn color_chip(ui: &mut egui::Ui, color: Rgb) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(10.0, 14.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(2), to_color(color));
}

fn timelines_section(app: &TimelineApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Timelines").strong());
        ui.weak(format!(
            "({} in {} group(s))",
            app.doc.timelines.len(),
            app.doc.groups.len()
        ));
    });
    ui.weak("Groups collapse into a single band so you can compare whole civilisations.");
    ui.add_space(2.0);

    group_tree(app, ui, None, 0, actions, &mut 0);

    if app.doc.timelines.is_empty() && app.doc.groups.is_empty() {
        ui.weak("None yet - add one with + Timeline above.");
    }
    ui.add_space(2.0);
    if ui.small_button("+ group at top level").clicked() {
        actions.push(Action::NewGroupUnder(None));
    }
}

/// Render one level of the group tree, then recurse.
///
/// `guard` bounds the walk: a hand-edited file could contain a parent cycle,
/// and the sidebar must not hang because of it.
fn group_tree(
    app: &TimelineApp,
    ui: &mut egui::Ui,
    parent: Option<Id>,
    depth: usize,
    actions: &mut Vec<Action>,
    guard: &mut usize,
) {
    *guard += 1;
    if *guard > 512 || depth > 12 {
        return;
    }
    let indent = depth as f32 * 12.0;

    for g in app.doc.child_groups(parent) {
        let selected = app.selection == Some(Selection::Group(g.id));
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let mut vis = g.visible;
            if ui
                .checkbox(&mut vis, "")
                .on_hover_text("Show this group")
                .changed()
            {
                actions.push(Action::ToggleGroupVisible(g.id));
            }
            if ui
                .small_button(if g.collapsed { "+" } else { "-" })
                .on_hover_text(if g.collapsed {
                    "Expand: show the timelines inside"
                } else {
                    "Collapse: show one band for the whole group"
                })
                .clicked()
            {
                actions.push(Action::ToggleCollapsed(g.id));
            }
            color_chip(ui, g.color);
            let count = app.doc.group_timelines(g.id).len();
            if ui
                .selectable_label(selected, egui::RichText::new(&g.name).strong())
                .on_hover_text(format!("{count} timeline(s) inside"))
                .clicked()
            {
                actions.push(Action::Select(Selection::Group(g.id)));
            }
        });
        if selected {
            ui.horizontal(|ui| {
                ui.add_space(indent + 20.0);
                if ui.small_button("edit").clicked() {
                    actions.push(Action::Edit(Selection::Group(g.id)));
                }
                if ui.small_button("+ subgroup").clicked() {
                    actions.push(Action::NewGroupUnder(Some(g.id)));
                }
                if ui.small_button("Remove").clicked() {
                    actions.push(Action::Delete(Selection::Group(g.id)));
                }
            });
        }
        if !g.collapsed {
            group_tree(app, ui, Some(g.id), depth + 1, actions, guard);
        }
    }

    for t in app.doc.timelines_in(parent) {
        timeline_row(app, ui, t, indent, actions);
    }
}

fn timeline_row(
    app: &TimelineApp,
    ui: &mut egui::Ui,
    t: &Timeline,
    indent: f32,
    actions: &mut Vec<Action>,
) {
    let selected = app.selection == Some(Selection::Timeline(t.id));
    ui.horizontal(|ui| {
        ui.add_space(indent);
        let mut vis = t.visible;
        if ui
            .checkbox(&mut vis, "")
            .on_hover_text("Show this timeline")
            .changed()
        {
            actions.push(Action::ToggleVisible(t.id));
        }
        color_chip(ui, t.color);
        let count = app.doc.events_of(OwnerRef::Timeline(t.id)).count();
        if ui
            .selectable_label(selected, &t.name)
            .on_hover_text(format!("{count} event(s)"))
            .clicked()
        {
            actions.push(Action::Select(Selection::Timeline(t.id)));
        }
    });
    if selected {
        ui.horizontal(|ui| {
            ui.add_space(indent + 20.0);
            if ui.small_button("+ event").clicked() {
                actions.push(Action::AddEventTo(OwnerRef::Timeline(t.id)));
            }
            if ui.small_button("edit").clicked() {
                actions.push(Action::Edit(Selection::Timeline(t.id)));
            }
            if ui.small_button("Up").on_hover_text("Move up").clicked() {
                actions.push(Action::Move(t.id, -1));
            }
            if ui.small_button("Down").on_hover_text("Move down").clicked() {
                actions.push(Action::Move(t.id, 1));
            }
            if ui.small_button("Delete").clicked() {
                actions.push(Action::Delete(Selection::Timeline(t.id)));
            }
        });
    }
}

fn biographies_section(app: &TimelineApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    section_header(ui, "Biographies", app.doc.biographies.len());
    ui.weak("Inline nests under a culture; Own lane runs alongside them.");

    let mut sorted: Vec<&Biography> = app.doc.biographies.iter().collect();
    sorted.sort_by(|a, b| {
        a.birth
            .decimal()
            .partial_cmp(&b.birth.decimal())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for b in sorted {
        let selected = app.selection == Some(Selection::Biography(b.id));
        ui.horizontal(|ui| {
            color_chip(ui, app.doc.bio_color(b));
            if ui
                .selectable_label(selected, &b.name)
                .on_hover_text(b.life_label())
                .clicked()
            {
                actions.push(Action::Select(Selection::Biography(b.id)));
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            for d in [BioDisplay::Hidden, BioDisplay::Inline, BioDisplay::Lane] {
                let enabled = d != BioDisplay::Inline || b.timeline.is_some();
                let resp = ui.add_enabled(
                    enabled,
                    egui::Button::selectable(b.display == d, d.name()),
                );
                if resp.clicked() {
                    actions.push(Action::SetDisplay(b.id, d));
                }
            }
        });
        if selected {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                if ui.small_button("+ event").clicked() {
                    actions.push(Action::AddEventTo(OwnerRef::Biography(b.id)));
                }
                if ui.small_button("edit").clicked() {
                    actions.push(Action::Edit(Selection::Biography(b.id)));
                }
                if ui.small_button("Delete").clicked() {
                    actions.push(Action::Delete(Selection::Biography(b.id)));
                }
            });
        }
    }

    if app.doc.biographies.is_empty() {
        ui.weak("None yet — add one with + Biography above.");
    }
}

/// One level of the category tree in the sidebar filter, then recurse.
///
/// Ticking a parent category is enough to also cover its subcategories when
/// filtering (see `Document::effective_filters`) — the checkbox here still
/// only reflects a category's own membership in `selected`, so a
/// subcategory's box does not appear ticked just because its parent's is.
#[allow(clippy::too_many_arguments)]
fn category_filter_tree(
    ui: &mut egui::Ui,
    doc: &Document,
    parent: Option<Id>,
    depth: usize,
    guard: &mut usize,
    selected: &mut BTreeSet<Id>,
    changed: &mut bool,
) {
    *guard += 1;
    if *guard > 512 || depth > 12 {
        return;
    }
    let indent = depth as f32 * 14.0;

    for c in doc.child_categories(parent) {
        let mut on = selected.contains(&c.id);
        ui.horizontal(|ui| {
            ui.add_space(indent);
            color_chip(ui, c.color);
            if ui.checkbox(&mut on, &c.name).changed() {
                if on {
                    selected.insert(c.id);
                } else {
                    selected.remove(&c.id);
                }
                *changed = true;
            }
        });
        category_filter_tree(ui, doc, Some(c.id), depth + 1, guard, selected, changed);
    }
}

fn filters_section(app: &mut TimelineApp, ui: &mut egui::Ui) {
    section_header(ui, "Categories & filter", app.doc.categories.len());

    let mut changed = false;
    let mut mode = app.doc.view.filters.mode;
    ui.horizontal(|ui| {
        for m in FilterMode::ALL {
            if ui.selectable_label(mode == m, m.name()).clicked() {
                mode = m;
                changed = true;
            }
        }
    });
    match mode {
        FilterMode::Off => ui.weak("Everything is shown."),
        FilterMode::Include => ui.weak("Only ticked categories are shown."),
        FilterMode::Exclude => ui.weak("Ticked categories are hidden."),
    };

    let mut selected = app.doc.view.filters.selected.clone();
    let mut keep_uncat = app.doc.view.filters.keep_uncategorised;

    ui.add_space(4.0);
    if app.doc.categories.is_empty() {
        ui.weak("No categories yet.");
    } else {
        let mut guard = 0usize;
        category_filter_tree(ui, &app.doc, None, 0, &mut guard, &mut selected, &mut changed);
    }

    ui.add_space(4.0);
    if ui
        .checkbox(&mut keep_uncat, "Always keep uncategorised entries")
        .changed()
    {
        changed = true;
    }

    ui.horizontal(|ui| {
        if ui.small_button("Clear filter").clicked() {
            selected.clear();
            mode = FilterMode::Off;
            changed = true;
        }
        if ui.small_button("Edit categories…").clicked() {
            app.dialog = Dialog::Categories(CategoryEditor::default());
        }
    });

    if changed {
        app.doc.view.filters.mode = mode;
        app.doc.view.filters.selected = selected;
        app.doc.view.filters.keep_uncategorised = keep_uncat;
        app.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Inspector
// ---------------------------------------------------------------------------

pub fn inspector(app: &mut TimelineApp, ui: &mut egui::Ui) {
    let Some(sel) = app.selection else { return };
    let mut actions: Vec<Action> = Vec::new();

    egui::ScrollArea::vertical().show(ui, |ui| match sel {
        Selection::Group(id) => group_inspector(app, ui, id, &mut actions),
        Selection::Event(id) => event_inspector(app, ui, id, &mut actions),
        Selection::Timeline(id) => timeline_inspector(app, ui, id, &mut actions),
        Selection::Biography(id) => biography_inspector(app, ui, id, &mut actions),
    });

    for a in actions {
        apply(app, a);
    }
}

fn header_row(ui: &mut egui::Ui, sel: Selection, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        if ui.button("Edit").clicked() {
            actions.push(Action::Edit(sel));
        }
        if ui.button("Delete").clicked() {
            actions.push(Action::Delete(sel));
        }
    });
}

fn field(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    let value = value.into();
    if value.trim().is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.weak(format!("{label}: "));
        ui.label(value);
    });
}

fn group_inspector(app: &TimelineApp, ui: &mut egui::Ui, id: Id, actions: &mut Vec<Action>) {
    let Some(g) = app.doc.group(id) else { return };
    ui.horizontal(|ui| {
        color_chip(ui, g.color);
        ui.heading(&g.name);
    });
    ui.add_space(4.0);
    header_row(ui, Selection::Group(id), actions);
    ui.separator();

    if let Some(p) = g.parent.and_then(|p| app.doc.group(p)) {
        field(ui, "Inside", p.name.clone());
    }
    field(
        ui,
        "State",
        if g.collapsed {
            "collapsed - drawn as one band"
        } else {
            "expanded"
        },
    );
    let timelines = app.doc.group_timelines(id);
    let events = app
        .doc
        .events
        .iter()
        .filter(|e| match e.owner {
            OwnerRef::Timeline(t) => timelines.contains(&t),
            OwnerRef::Biography(b) => app
                .doc
                .biography(b)
                .and_then(|bio| bio.timeline)
                .is_some_and(|t| timelines.contains(&t)),
        })
        .count();
    field(ui, "Contains", format!("{} timeline(s), {events} event(s)", timelines.len()));
    if !g.notes.trim().is_empty() {
        ui.add_space(4.0);
        ui.label(&g.notes);
    }

    ui.add_space(8.0);
    section_header(ui, "Timelines", timelines.len());
    for tid in timelines {
        if let Some(t) = app.doc.timeline(tid) {
            if ui
                .selectable_label(app.selection == Some(Selection::Timeline(tid)), &t.name)
                .clicked()
            {
                actions.push(Action::Select(Selection::Timeline(tid)));
            }
        }
    }
}

fn event_inspector(app: &TimelineApp, ui: &mut egui::Ui, id: Id, actions: &mut Vec<Action>) {
    let Some(ev) = app.doc.event(id) else { return };
    ui.heading(&ev.title);
    ui.add_space(4.0);
    header_row(ui, Selection::Event(id), actions);

    // Nested inside another event: make the containment explicit and
    // navigable, rather than a fact only visible by hunting through dates.
    if let Some(parent) = ev.parent.and_then(|p| app.doc.event(p)) {
        ui.horizontal(|ui| {
            ui.weak("Nested inside:");
            if ui.link(&parent.title).clicked() {
                actions.push(Action::Select(Selection::Event(parent.id)));
            }
        });
    }
    if ev.span.is_range() {
        if ui.button("+ Add nested event").clicked() {
            actions.push(Action::AddNestedEventTo(ev.owner, id));
        }
    }
    ui.separator();

    field(ui, "Date", ev.span.label());
    field(ui, "On", app.doc.owner_name(ev.owner));
    field(ui, "Importance", importance_name(ev.importance));
    field(ui, "Categories", app.doc.category_names(&ev.categories));
    if !ev.description.trim().is_empty() {
        ui.add_space(6.0);
        ui.separator();
        ui.label(&ev.description);
    }

    let children = app.doc.child_events(id);
    if !children.is_empty() {
        ui.add_space(6.0);
        ui.separator();
        ui.weak(format!("Contains {} nested event(s):", children.len()));
        for child in children {
            if ui.link(&child.title).clicked() {
                actions.push(Action::Select(Selection::Event(child.id)));
            }
        }
    }
}

fn timeline_inspector(app: &TimelineApp, ui: &mut egui::Ui, id: Id, actions: &mut Vec<Action>) {
    let Some(tl) = app.doc.timeline(id) else { return };
    ui.horizontal(|ui| {
        color_chip(ui, tl.color);
        ui.heading(&tl.name);
    });
    ui.add_space(4.0);
    header_row(ui, Selection::Timeline(id), actions);
    if ui.button("+ Add event here").clicked() {
        actions.push(Action::AddEventTo(OwnerRef::Timeline(id)));
    }
    ui.separator();

    if let Some(s) = tl.span {
        field(ui, "Span", s.label());
    } else {
        field(ui, "Span", "inferred from its events");
    }
    if let Some(j) = &tl.origin {
        field(
            ui,
            "Splits from",
            format!(
                "{} at {}{}",
                app.doc
                    .timeline(j.other)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "(missing)".into()),
                j.date.label(),
                if j.label.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", j.label)
                }
            ),
        );
    }
    if let Some(j) = &tl.merge {
        field(
            ui,
            "Merges into",
            format!(
                "{} at {}{}",
                app.doc
                    .timeline(j.other)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "(missing)".into()),
                j.date.label(),
                if j.label.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", j.label)
                }
            ),
        );
    }
    if !tl.notes.trim().is_empty() {
        ui.add_space(4.0);
        ui.label(&tl.notes);
    }

    ui.add_space(8.0);
    event_list(app, ui, OwnerRef::Timeline(id), actions);
}

fn biography_inspector(app: &TimelineApp, ui: &mut egui::Ui, id: Id, actions: &mut Vec<Action>) {
    let Some(bio) = app.doc.biography(id) else { return };
    ui.horizontal(|ui| {
        color_chip(ui, app.doc.bio_color(bio));
        ui.heading(&bio.name);
    });
    ui.add_space(4.0);
    header_row(ui, Selection::Biography(id), actions);
    if ui.button("+ Add life event").clicked() {
        actions.push(Action::AddEventTo(OwnerRef::Biography(id)));
    }
    ui.separator();

    field(ui, "Lived", bio.life_label());
    if let Some(t) = bio.timeline.and_then(|t| app.doc.timeline(t)) {
        field(ui, "Culture", t.name.clone());
    }
    field(ui, "Shown as", bio.display.name());
    field(ui, "Importance", importance_name(bio.importance));
    field(ui, "Categories", app.doc.category_names(&bio.categories));
    if !bio.notes.trim().is_empty() {
        ui.add_space(4.0);
        ui.label(&bio.notes);
    }

    ui.add_space(8.0);
    event_list(app, ui, OwnerRef::Biography(id), actions);
}

/// Chronological list of an owner's events — the fastest way to review and
/// correct a stretch of data that was entered over several sittings.
fn event_list(app: &TimelineApp, ui: &mut egui::Ui, owner: OwnerRef, actions: &mut Vec<Action>) {
    let mut events: Vec<&Event> = app.doc.events_of(owner).collect();
    events.sort_by(|a, b| {
        a.span
            .t0()
            .partial_cmp(&b.span.t0())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    section_header(ui, "Events", events.len());
    if events.is_empty() {
        ui.weak("Nothing here yet.");
        return;
    }
    for ev in events {
        let selected = app.selection == Some(Selection::Event(ev.id));
        let label = format!("{} — {}", ev.span.start.label(), ev.title);
        if ui
            .selectable_label(selected, label)
            .on_hover_text(importance_name(ev.importance))
            .clicked()
        {
            actions.push(Action::Select(Selection::Event(ev.id)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(names: &[&str]) -> Document {
        let mut doc = Document::default();
        for (i, n) in names.iter().enumerate() {
            let id = doc.new_id();
            doc.timelines.push(Timeline {
                id,
                name: (*n).into(),
                color: [0, 0, 0],
                visible: true,
                group: None,
                order: i as u32,
                span: None,
                origin: None,
                merge: None,
                notes: String::new(),
                epochs: Vec::new(),
            });
        }
        doc
    }

    fn order_of(doc: &Document) -> Vec<String> {
        let mut v: Vec<&Timeline> = doc.timelines.iter().collect();
        v.sort_by_key(|t| (t.order, t.id.0));
        v.iter().map(|t| t.name.clone()).collect()
    }

    #[test]
    fn moving_a_timeline_up_swaps_it_with_its_neighbour() {
        let mut doc = doc_with(&["A", "B", "C"]);
        let b = doc.timelines[1].id;
        reorder(&mut doc, b, -1);
        assert_eq!(order_of(&doc), vec!["B", "A", "C"]);
    }

    #[test]
    fn moving_a_timeline_down_swaps_it_with_its_neighbour() {
        let mut doc = doc_with(&["A", "B", "C"]);
        let b = doc.timelines[1].id;
        reorder(&mut doc, b, 1);
        assert_eq!(order_of(&doc), vec!["A", "C", "B"]);
    }

    #[test]
    fn moving_past_either_end_is_a_no_op() {
        let mut doc = doc_with(&["A", "B", "C"]);
        let first = doc.timelines[0].id;
        let last = doc.timelines[2].id;
        reorder(&mut doc, first, -1);
        assert_eq!(order_of(&doc), vec!["A", "B", "C"]);
        reorder(&mut doc, last, 1);
        assert_eq!(order_of(&doc), vec!["A", "B", "C"]);
    }

    #[test]
    fn reordering_keeps_order_values_dense_and_unique() {
        let mut doc = doc_with(&["A", "B", "C", "D"]);
        // Start from a document with duplicated and sparse order values, as a
        // hand-edited file could easily contain.
        doc.timelines[0].order = 7; // A
        doc.timelines[1].order = 7; // B
        doc.timelines[2].order = 90; // C
        doc.timelines[3].order = 3; // D
        // Sorted by (order, id) that reads D, A, B, C. Move C up one.
        let c = doc.timelines[2].id;
        reorder(&mut doc, c, -1);

        assert_eq!(order_of(&doc), vec!["D", "A", "C", "B"]);
        let mut orders: Vec<u32> = doc.timelines.iter().map(|t| t.order).collect();
        orders.sort_unstable();
        assert_eq!(orders, vec![0, 1, 2, 3], "orders should be renumbered densely");
    }

    #[test]
    fn reordering_an_unknown_id_changes_nothing() {
        let mut doc = doc_with(&["A", "B"]);
        reorder(&mut doc, Id(999), 1);
        assert_eq!(order_of(&doc), vec!["A", "B"]);
    }
}
