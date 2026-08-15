//! Left sidebar (timelines, biographies, filters) and right inspector.

use crate::app::{BioGroupBy, Confirm, Selection, TimelineApp};
use crate::forms::{CategoryEditor, Dialog, EventForm, GroupForm};
use crate::layout;
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
    MoveGroup(Id, i32),
    TidyTopLevelGroups,
    SetDisplay(Id, BioDisplay),
    AddEventTo(OwnerRef),
    AddNestedEventTo(OwnerRef, Id),
    NewGroupUnder(Option<Id>),
    /// Bulk show/hide for every biography in one sidebar cluster (a culture
    /// or a category) — the "collapse a group" bundling `Group` already has,
    /// extended to biographies since they have no single `visible` flag to
    /// toggle, only a three-way `display`.
    ShowCluster(Vec<Id>),
    HideCluster(Vec<Id>),
    Jump(crate::app::JumpTarget),
}

pub fn sidebar(app: &mut TimelineApp, ui: &mut egui::Ui) {
    let mut actions: Vec<Action> = Vec::new();
    // Taken out for the duration of the frame so `timelines_section` and
    // `biographies_section` can keep taking `app: &TimelineApp` like every
    // other panel function here, rather than needing `&mut TimelineApp` just
    // to update two search strings.
    let mut timeline_search = std::mem::take(&mut app.timeline_search);
    let mut bio_search = std::mem::take(&mut app.bio_search);
    let mut bio_group_by = app.bio_group_by;

    egui::ScrollArea::vertical().show(ui, |ui| {
        timelines_section(app, ui, &mut actions, &mut timeline_search);
        ui.add_space(10.0);
        biographies_section(app, ui, &mut actions, &mut bio_search, &mut bio_group_by);
        ui.add_space(10.0);
        filters_section(app, ui);
    });

    app.timeline_search = timeline_search;
    app.bio_search = bio_search;
    app.bio_group_by = bio_group_by;

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
        Action::MoveGroup(id, delta) => app.mutate(|doc| reorder_group(doc, id, delta)),
        Action::TidyTopLevelGroups => app.mutate(|doc| {
            let order = layout::suggest_group_order(doc, None);
            for (i, id) in order.iter().enumerate() {
                if let Some(g) = doc.group_mut(*id) {
                    g.order = i as u32;
                }
            }
        }),
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
        Action::HideCluster(ids) => app.mutate(|doc| {
            for id in ids {
                if let Some(b) = doc.biography_mut(id) {
                    b.display = BioDisplay::Hidden;
                }
            }
        }),
        Action::ShowCluster(ids) => app.mutate(|doc| {
            for id in ids {
                if let Some(b) = doc.biography_mut(id) {
                    // Only restore ones that were hidden — an already-shown
                    // biography's own display choice (Inline vs. Lane) is
                    // left alone rather than clobbered by a bulk action.
                    if b.display == BioDisplay::Hidden {
                        b.display = if b.timeline.is_some() {
                            BioDisplay::Inline
                        } else {
                            BioDisplay::Lane
                        };
                    }
                }
            }
        }),
        Action::Jump(target) => {
            let width = app.last_width.unwrap_or(1200.0);
            app.jump_to(target, width);
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

/// Move a group up or down among its siblings (same parent group, or the
/// top level), renumbering so order stays dense. Mirrors `reorder` for
/// timelines.
fn reorder_group(doc: &mut Document, id: Id, delta: i32) {
    let Some(parent) = doc.group(id).map(|g| g.parent) else {
        return;
    };
    let mut siblings: Vec<Id> = doc.child_groups(parent).iter().map(|g| g.id).collect();
    let Some(pos) = siblings.iter().position(|s| *s == id) else {
        return;
    };
    let target = pos as i32 + delta;
    if target < 0 || target >= siblings.len() as i32 {
        return;
    }
    siblings.swap(pos, target as usize);
    for (i, sid) in siblings.iter().enumerate() {
        if let Some(g) = doc.group_mut(*sid) {
            g.order = i as u32;
        }
    }
}

fn section_header(ui: &mut egui::Ui, title: &str, count: usize) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.weak(format!("({count})"));
    });
}

/// A small "Google-style" suggestion dropdown anchored below `resp` (a
/// search field's response): up to `limit` candidates whose name contains
/// the current query, case-insensitively, each a clickable row. Returns the
/// candidate clicked, or the top match if Enter was pressed while the field
/// had focus. Stays closed while the field itself isn't focused or the
/// query is empty — an always-open empty box would just be visual noise
/// between keystrokes.
pub fn suggestions<T: Copy>(
    resp: &egui::Response,
    id_salt: &str,
    query: &str,
    candidates: impl Iterator<Item = (String, T)>,
    limit: usize,
) -> Option<T> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() || !resp.has_focus() {
        return None;
    }
    let matches: Vec<(String, T)> = candidates
        .filter(|(name, _)| name.to_lowercase().contains(&needle))
        .take(limit)
        .collect();
    if matches.is_empty() {
        return None;
    }
    if resp.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        return Some(matches[0].1);
    }

    let mut picked = None;
    egui::Popup::from_response(resp)
        .id(egui::Id::new(id_salt))
        .align(egui::RectAlign::BOTTOM_START)
        .open(true)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            for (name, value) in matches {
                if ui.selectable_label(false, name).clicked() {
                    picked = Some(value);
                }
            }
        });
    picked
}

fn color_chip(ui: &mut egui::Ui, color: Rgb) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(10.0, 14.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(2), to_color(color));
}

/// A colour chip with an optional outline — a biography's fill (its category,
/// typically) and border (its culture) shown at a glance, the same two
/// colours its band is painted with. Falls back to a plain chip when there is
/// no culture to draw a border for.
fn color_chip_bordered(ui: &mut egui::Ui, fill: Rgb, border: Option<Rgb>) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(10.0, 14.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(2), to_color(fill));
    if let Some(border) = border {
        p.rect_stroke(
            rect,
            egui::CornerRadius::same(2),
            egui::Stroke::new(1.5, to_color(border)),
            egui::StrokeKind::Inside,
        );
    }
}

/// Which groups and timelines a sidebar search leaves standing.
///
/// A timeline matches on its own name; a group matches either on its own name
/// (which pulls in its whole subtree, so a matched folder shows everything
/// inside it) or by containing a matching timeline somewhere below it (which
/// pulls in just the path down to that timeline, plus the timeline itself) —
/// otherwise a matching timeline three groups deep would have no visible way
/// to reach it once its ancestors were filtered out.
struct TreeMatch {
    timelines: BTreeSet<Id>,
    groups: BTreeSet<Id>,
}

fn ancestor_groups(doc: &Document, mut group: Option<Id>, into: &mut BTreeSet<Id>) {
    while let Some(id) = group {
        if !into.insert(id) {
            break; // Already walked this far up on an earlier match.
        }
        group = doc.group(id).and_then(|g| g.parent);
    }
}

fn compute_timeline_matches(doc: &Document, needle: &str) -> TreeMatch {
    let mut timelines = BTreeSet::new();
    let mut groups = BTreeSet::new();

    for t in &doc.timelines {
        if t.name.to_lowercase().contains(needle) {
            timelines.insert(t.id);
            ancestor_groups(doc, t.group, &mut groups);
        }
    }
    for g in &doc.groups {
        if g.name.to_lowercase().contains(needle) {
            groups.insert(g.id);
            timelines.extend(doc.group_timelines(g.id));
            ancestor_groups(doc, g.parent, &mut groups);
        }
    }
    TreeMatch { timelines, groups }
}

fn timelines_section(
    app: &TimelineApp,
    ui: &mut egui::Ui,
    actions: &mut Vec<Action>,
    search: &mut String,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Zeitstrahlen").strong());
        ui.weak(format!(
            "({} in {} Gruppe(n))",
            app.doc.timelines.len(),
            app.doc.groups.len()
        ));
    });
    ui.weak("Gruppen klappen zu einem einzigen Band zusammen, damit sich ganze Kulturen vergleichen lassen.");
    ui.add_space(2.0);
    let search_resp = ui.add(
        egui::TextEdit::singleline(search)
            .hint_text("Zeitstrahlen und Gruppen suchen…")
            .desired_width(f32::INFINITY),
    );

    let candidates = app
        .doc
        .groups
        .iter()
        .map(|g| (g.name.clone(), crate::app::JumpTarget::Group(g.id)))
        .chain(
            app.doc
                .timelines
                .iter()
                .map(|t| (t.name.clone(), crate::app::JumpTarget::Timeline(t.id))),
        );
    if let Some(target) = suggestions(&search_resp, "timeline_search_suggest", search, candidates, 8) {
        actions.push(Action::Jump(target));
    }
    ui.add_space(2.0);

    let needle = search.trim().to_lowercase();
    let filter = (!needle.is_empty()).then(|| compute_timeline_matches(&app.doc, &needle));

    group_tree(app, ui, None, 0, actions, &mut 0, filter.as_ref());

    if app.doc.timelines.is_empty() && app.doc.groups.is_empty() {
        ui.weak("Noch keine — mit + Zeitstrahl oben einen anlegen.");
    } else if let Some(f) = &filter {
        if f.timelines.is_empty() && f.groups.is_empty() {
            ui.weak("Keine Treffer.");
        }
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if ui.small_button("+ Gruppe auf oberster Ebene").clicked() {
            actions.push(Action::NewGroupUnder(None));
        }
        if app.doc.groups.iter().any(|g| g.parent.is_none())
            && ui
                .small_button("Verbundene Gruppen zusammenrücken")
                .on_hover_text(
                    "Gruppen mit \"Spaltet sich ab von\"/\"Geht auf in\"-Verbindungen zueinander \
                     nebeneinander anordnen (oberste Ebene) — bestmöglich, kein Garant gegen jede \
                     Überschneidung.",
                )
                .clicked()
        {
            actions.push(Action::TidyTopLevelGroups);
        }
    });
}

/// Render one level of the group tree, then recurse.
///
/// `guard` bounds the walk: a hand-edited file could contain a parent cycle,
/// and the sidebar must not hang because of it. `filter`, when set, hides any
/// group or timeline the current sidebar search does not match.
fn group_tree(
    app: &TimelineApp,
    ui: &mut egui::Ui,
    parent: Option<Id>,
    depth: usize,
    actions: &mut Vec<Action>,
    guard: &mut usize,
    filter: Option<&TreeMatch>,
) {
    *guard += 1;
    if *guard > 512 || depth > 12 {
        return;
    }
    let indent = depth as f32 * 12.0;

    for g in app.doc.child_groups(parent) {
        if filter.is_some_and(|f| !f.groups.contains(&g.id)) {
            continue;
        }
        let selected = app.selection == Some(Selection::Group(g.id));
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let mut vis = g.visible;
            if ui
                .checkbox(&mut vis, "")
                .on_hover_text("Diese Gruppe anzeigen")
                .changed()
            {
                actions.push(Action::ToggleGroupVisible(g.id));
            }
            if ui
                .small_button(if g.collapsed { "+" } else { "-" })
                .on_hover_text(if g.collapsed {
                    "Ausklappen: die enthaltenen Zeitstrahlen anzeigen"
                } else {
                    "Einklappen: ein Band für die ganze Gruppe anzeigen"
                })
                .clicked()
            {
                actions.push(Action::ToggleCollapsed(g.id));
            }
            color_chip(ui, g.color);
            let count = app.doc.group_timelines(g.id).len();
            if ui
                .selectable_label(selected, egui::RichText::new(&g.name).strong())
                .on_hover_text(format!("{count} Zeitstrahl(en) darin"))
                .clicked()
            {
                actions.push(Action::Select(Selection::Group(g.id)));
            }
        });
        if selected {
            ui.horizontal(|ui| {
                ui.add_space(indent + 20.0);
                if ui.small_button("bearbeiten").clicked() {
                    actions.push(Action::Edit(Selection::Group(g.id)));
                }
                if ui.small_button("+ Untergruppe").clicked() {
                    actions.push(Action::NewGroupUnder(Some(g.id)));
                }
                if ui.small_button("Hoch").on_hover_text("Nach oben verschieben").clicked() {
                    actions.push(Action::MoveGroup(g.id, -1));
                }
                if ui.small_button("Runter").on_hover_text("Nach unten verschieben").clicked() {
                    actions.push(Action::MoveGroup(g.id, 1));
                }
                if ui.small_button("Entfernen").clicked() {
                    actions.push(Action::Delete(Selection::Group(g.id)));
                }
            });
        }
        if !g.collapsed {
            group_tree(app, ui, Some(g.id), depth + 1, actions, guard, filter);
        }
    }

    for t in app.doc.timelines_in(parent) {
        if filter.is_some_and(|f| !f.timelines.contains(&t.id)) {
            continue;
        }
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
            .on_hover_text("Diesen Zeitstrahl anzeigen")
            .changed()
        {
            actions.push(Action::ToggleVisible(t.id));
        }
        color_chip(ui, t.color);
        let count = app.doc.events_of(OwnerRef::Timeline(t.id)).count();
        if ui
            .selectable_label(selected, &t.name)
            .on_hover_text(format!("{count} Ereignis(se)"))
            .clicked()
        {
            actions.push(Action::Select(Selection::Timeline(t.id)));
        }
    });
    if selected {
        ui.horizontal(|ui| {
            ui.add_space(indent + 20.0);
            if ui.small_button("+ Ereignis").clicked() {
                actions.push(Action::AddEventTo(OwnerRef::Timeline(t.id)));
            }
            if ui.small_button("bearbeiten").clicked() {
                actions.push(Action::Edit(Selection::Timeline(t.id)));
            }
            if ui.small_button("Hoch").on_hover_text("Nach oben verschieben").clicked() {
                actions.push(Action::Move(t.id, -1));
            }
            if ui.small_button("Runter").on_hover_text("Nach unten verschieben").clicked() {
                actions.push(Action::Move(t.id, 1));
            }
            if ui.small_button("Löschen").clicked() {
                actions.push(Action::Delete(Selection::Timeline(t.id)));
            }
        });
    }
}

/// Biographies clustered so each cluster can be collapsed away with one
/// click — the same declutter this sidebar already gives timelines via
/// groups. Without it, a library with hundreds of biographies was one long
/// flat scroll with no way to collapse away what you are not looking at.
///
/// Clustered by culture (their linked timeline) or by category, whichever
/// the "Group by" toggle is set to. Culture is a strict partition — one
/// culture per biography — while a category cluster ("all Philosophers")
/// can and does overlap another, since a biography may carry several
/// categories at once.
fn biographies_section(
    app: &TimelineApp,
    ui: &mut egui::Ui,
    actions: &mut Vec<Action>,
    search: &mut String,
    group_by: &mut BioGroupBy,
) {
    section_header(ui, "Biografien", app.doc.biographies.len());
    ui.weak("Eingebettet verschachtelt sich unter einer Kultur; Eigene Spur läuft parallel dazu.");
    let search_resp = ui.add(
        egui::TextEdit::singleline(search)
            .hint_text("Biografien suchen…")
            .desired_width(f32::INFINITY),
    );
    let candidates = app
        .doc
        .biographies
        .iter()
        .map(|b| (b.name.clone(), crate::app::JumpTarget::Biography(b.id)));
    if let Some(target) = suggestions(&search_resp, "bio_search_suggest", search, candidates, 8) {
        actions.push(Action::Jump(target));
    }
    ui.horizontal(|ui| {
        ui.weak("Gruppieren nach:");
        if ui.selectable_label(*group_by == BioGroupBy::Culture, "Kultur").clicked() {
            *group_by = BioGroupBy::Culture;
        }
        if ui.selectable_label(*group_by == BioGroupBy::Category, "Kategorie").clicked() {
            *group_by = BioGroupBy::Category;
        }
    });
    ui.add_space(2.0);

    let needle = search.trim().to_lowercase();
    let matches = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);
    let mut any_cluster = false;

    match group_by {
        BioGroupBy::Culture => {
            let mut ordered_timelines: Vec<&Timeline> = app.doc.timelines.iter().collect();
            ordered_timelines.sort_by_key(|t| (t.order, t.id.0));

            for t in ordered_timelines {
                let bios: Vec<&Biography> = app
                    .doc
                    .biographies
                    .iter()
                    .filter(|b| b.timeline == Some(t.id) && matches(&b.name))
                    .collect();
                if bios.is_empty() {
                    continue;
                }
                any_cluster = true;
                bio_cluster(ui, app, "bio_cluster_culture", t.id, &t.name, bios, actions);
            }

            let unculture: Vec<&Biography> = app
                .doc
                .biographies
                .iter()
                .filter(|b| b.timeline.is_none() && matches(&b.name))
                .collect();
            if !unculture.is_empty() {
                any_cluster = true;
                bio_cluster(ui, app, "bio_cluster_culture_none", (), "Keine Kultur", unculture, actions);
            }
        }
        BioGroupBy::Category => {
            for c in &app.doc.categories {
                let bios: Vec<&Biography> = app
                    .doc
                    .biographies
                    .iter()
                    .filter(|b| b.categories.contains(&c.id) && matches(&b.name))
                    .collect();
                if bios.is_empty() {
                    continue;
                }
                any_cluster = true;
                bio_cluster(ui, app, "bio_cluster_category", c.id, &c.name, bios, actions);
            }

            let uncategorised: Vec<&Biography> = app
                .doc
                .biographies
                .iter()
                .filter(|b| b.categories.is_empty() && matches(&b.name))
                .collect();
            if !uncategorised.is_empty() {
                any_cluster = true;
                bio_cluster(
                    ui,
                    app,
                    "bio_cluster_category_none",
                    (),
                    "Ohne Kategorie",
                    uncategorised,
                    actions,
                );
            }
        }
    }

    if app.doc.biographies.is_empty() {
        ui.weak("Noch keine — mit + Biografie oben eine anlegen.");
    } else if !any_cluster {
        ui.weak("Keine Treffer.");
    }
}

/// One collapsible cluster of biographies, sorted by birth year.
/// One collapsible cluster of biographies, sorted by birth year, with two
/// bundled quick actions in its header — "alle anzeigen"/"alle ausblenden"
/// set every member's `display` at once, the same bundled show/hide a
/// `Group`'s own visibility checkbox already gives timelines. A biography
/// has no single visible flag to toggle (only the three-way `display`), so
/// this needs its own bulk actions rather than reusing `ToggleGroupVisible`.
fn bio_cluster(
    ui: &mut egui::Ui,
    app: &TimelineApp,
    id_salt: &str,
    cluster_key: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    mut bios: Vec<&Biography>,
    actions: &mut Vec<Action>,
) {
    bios.sort_by(|a, b| a.birth.decimal().partial_cmp(&b.birth.decimal()).unwrap_or(std::cmp::Ordering::Equal));
    let ids: Vec<Id> = bios.iter().map(|b| b.id).collect();

    let id = ui.make_persistent_id((id_salt, cluster_key));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
        .show_header(ui, |ui| {
            ui.label(format!("{label} ({})", bios.len()));
            if ui
                .small_button("alle anzeigen")
                .on_hover_text("Jede ausgeblendete Biografie hier auf ihre normale Anzeige zurücksetzen")
                .clicked()
            {
                actions.push(Action::ShowCluster(ids.clone()));
            }
            if ui
                .small_button("alle ausblenden")
                .on_hover_text("Alle Biografien hier ausblenden")
                .clicked()
            {
                actions.push(Action::HideCluster(ids.clone()));
            }
        })
        .body(|ui| {
            for b in bios {
                biography_row(app, ui, b, actions);
            }
        });
}

fn biography_row(app: &TimelineApp, ui: &mut egui::Ui, b: &Biography, actions: &mut Vec<Action>) {
    let selected = app.selection == Some(Selection::Biography(b.id));
    ui.horizontal(|ui| {
        let (fill, border) = app.doc.bio_colors(b);
        color_chip_bordered(ui, fill, border);
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
            let resp = ui.add_enabled(enabled, egui::Button::selectable(b.display == d, d.name()));
            if resp.clicked() {
                actions.push(Action::SetDisplay(b.id, d));
            }
        }
    });
    if selected {
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            if ui.small_button("+ Ereignis").clicked() {
                actions.push(Action::AddEventTo(OwnerRef::Biography(b.id)));
            }
            if ui.small_button("bearbeiten").clicked() {
                actions.push(Action::Edit(Selection::Biography(b.id)));
            }
            if ui.small_button("Löschen").clicked() {
                actions.push(Action::Delete(Selection::Biography(b.id)));
            }
        });
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
    section_header(ui, "Kategorien & Filter", app.doc.categories.len());

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
        FilterMode::Off => ui.weak("Alles wird angezeigt."),
        FilterMode::Include => ui.weak("Nur angehakte Kategorien werden angezeigt."),
        FilterMode::Exclude => ui.weak("Angehakte Kategorien werden ausgeblendet."),
    };

    let mut selected = app.doc.view.filters.selected.clone();
    let mut keep_uncat = app.doc.view.filters.keep_uncategorised;

    ui.add_space(4.0);
    if app.doc.categories.is_empty() {
        ui.weak("Noch keine Kategorien.");
    } else {
        let mut guard = 0usize;
        category_filter_tree(ui, &app.doc, None, 0, &mut guard, &mut selected, &mut changed);
    }

    ui.add_space(4.0);
    if ui
        .checkbox(&mut keep_uncat, "Einträge ohne Kategorie immer anzeigen")
        .changed()
    {
        changed = true;
    }

    ui.horizontal(|ui| {
        if ui.small_button("Filter zurücksetzen").clicked() {
            selected.clear();
            mode = FilterMode::Off;
            changed = true;
        }
        if ui.small_button("Kategorien bearbeiten…").clicked() {
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
        if ui.button("Bearbeiten").clicked() {
            actions.push(Action::Edit(sel));
        }
        if ui.button("Löschen").clicked() {
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
        field(ui, "In", p.name.clone());
    }
    field(
        ui,
        "Status",
        if g.collapsed {
            "eingeklappt – als ein Band gezeichnet"
        } else {
            "ausgeklappt"
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
    field(ui, "Enthält", format!("{} Zeitstrahl(en), {events} Ereignis(se)", timelines.len()));
    if !g.notes.trim().is_empty() {
        ui.add_space(4.0);
        ui.label(&g.notes);
    }

    ui.add_space(8.0);
    section_header(ui, "Zeitstrahlen", timelines.len());
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
            ui.weak("Verschachtelt in:");
            if ui.link(&parent.title).clicked() {
                actions.push(Action::Select(Selection::Event(parent.id)));
            }
        });
    }
    if ev.span.is_range() {
        if ui.button("+ Verschachteltes Ereignis hinzufügen").clicked() {
            actions.push(Action::AddNestedEventTo(ev.owner, id));
        }
    }
    ui.separator();

    field(ui, "Datum", ev.span.label());
    field(ui, "Gehört zu", app.doc.owner_name(ev.owner));
    field(ui, "Bedeutung", importance_name(ev.importance));
    field(ui, "Kategorien", app.doc.category_names(&ev.categories));
    if !ev.description.trim().is_empty() {
        ui.add_space(6.0);
        ui.separator();
        ui.label(&ev.description);
    }

    let children = app.doc.child_events(id);
    if !children.is_empty() {
        ui.add_space(6.0);
        ui.separator();
        ui.weak(format!("Enthält {} verschachtelte Ereignisse:", children.len()));
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
    if ui.button("+ Ereignis hier hinzufügen").clicked() {
        actions.push(Action::AddEventTo(OwnerRef::Timeline(id)));
    }
    ui.separator();

    if let Some(s) = tl.span {
        field(ui, "Zeitraum", s.label());
    } else {
        field(ui, "Zeitraum", "aus den Ereignissen abgeleitet");
    }
    if let Some(j) = &tl.origin {
        field(
            ui,
            "Spaltet sich ab von",
            format!(
                "{} am {}{}",
                app.doc
                    .timeline(j.other)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "(fehlt)".into()),
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
            "Geht auf in",
            format!(
                "{} am {}{}",
                app.doc
                    .timeline(j.other)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "(fehlt)".into()),
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
    if ui.button("+ Lebensereignis hinzufügen").clicked() {
        actions.push(Action::AddEventTo(OwnerRef::Biography(id)));
    }
    ui.separator();

    field(ui, "Lebte", bio.life_label());
    if let Some(t) = bio.timeline.and_then(|t| app.doc.timeline(t)) {
        field(ui, "Kultur", t.name.clone());
    }
    field(ui, "Angezeigt als", bio.display.name());
    field(ui, "Bedeutung", importance_name(bio.importance));
    field(ui, "Kategorien", app.doc.category_names(&bio.categories));
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

    section_header(ui, "Ereignisse", events.len());
    if events.is_empty() {
        ui.weak("Hier ist noch nichts.");
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

    fn group_order_of(doc: &Document, parent: Option<Id>) -> Vec<String> {
        doc.child_groups(parent).iter().map(|g| g.name.clone()).collect()
    }

    #[test]
    fn moving_a_group_up_swaps_it_with_its_sibling() {
        let mut doc = Document::default();
        let _a = group(&mut doc, "A", None);
        let b = group(&mut doc, "B", None);
        let _c = group(&mut doc, "C", None);
        reorder_group(&mut doc, b, -1);
        assert_eq!(group_order_of(&doc, None), vec!["B", "A", "C"]);
    }

    #[test]
    fn moving_a_group_is_scoped_to_its_own_parent() {
        let mut doc = Document::default();
        let outer = group(&mut doc, "Outer", None);
        let a = group(&mut doc, "A", Some(outer));
        let _b = group(&mut doc, "B", Some(outer));
        let _top = group(&mut doc, "Top", None);
        // A moving up must not jump out of its parent, even though "Top" is
        // its immediate predecessor in the raw document order.
        reorder_group(&mut doc, a, -1);
        assert_eq!(group_order_of(&doc, Some(outer)), vec!["A", "B"]);
        assert_eq!(group_order_of(&doc, None), vec!["Outer", "Top"]);
    }

    // --- Sidebar search -------------------------------------------------------

    fn group(doc: &mut Document, name: &str, parent: Option<Id>) -> Id {
        let id = doc.new_id();
        doc.groups.push(Group {
            id,
            name: name.into(),
            color: [0, 0, 0],
            parent,
            order: 0,
            collapsed: false,
            visible: true,
            notes: String::new(),
        });
        id
    }

    fn timeline_in(doc: &mut Document, name: &str, group: Option<Id>) -> Id {
        let id = doc.new_id();
        doc.timelines.push(Timeline {
            id,
            name: name.into(),
            color: [0, 0, 0],
            visible: true,
            group,
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });
        id
    }

    #[test]
    fn a_timeline_match_pulls_in_its_ancestor_groups_but_not_its_siblings() {
        let mut doc = Document::default();
        let antiquity = group(&mut doc, "Antiquity", None);
        let greek = group(&mut doc, "Greek antiquity", Some(antiquity));
        let athens = timeline_in(&mut doc, "Athens", Some(greek));
        let sparta = timeline_in(&mut doc, "Sparta", Some(greek));

        let m = compute_timeline_matches(&doc, "athens");
        assert!(m.timelines.contains(&athens));
        assert!(!m.timelines.contains(&sparta));
        // Both ancestor groups must be included, or Athens would have no
        // visible path down to it in the tree.
        assert!(m.groups.contains(&greek));
        assert!(m.groups.contains(&antiquity));
    }

    #[test]
    fn a_group_name_match_pulls_in_its_whole_subtree() {
        let mut doc = Document::default();
        let greek = group(&mut doc, "Greek antiquity", None);
        let athens = timeline_in(&mut doc, "Athens", Some(greek));
        let sparta = timeline_in(&mut doc, "Sparta", Some(greek));
        let rome = timeline_in(&mut doc, "Rome", None);

        let m = compute_timeline_matches(&doc, "greek");
        assert!(m.groups.contains(&greek));
        assert!(m.timelines.contains(&athens));
        assert!(m.timelines.contains(&sparta));
        assert!(!m.timelines.contains(&rome));
    }
}
