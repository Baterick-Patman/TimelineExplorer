//! The timeline canvas: ruler, bands, junctions, events and labels.

use crate::app::{Selection, TimelineApp};
use crate::layout::*;
use crate::model::*;
use crate::theme::*;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

pub const RULER_HEIGHT: f32 = 30.0;
/// Width reserved at the left edge for sticky lane names.
pub const GUTTER: f32 = 8.0;

/// A clickable thing on the canvas, recorded while painting.
pub struct Hit {
    pub rect: Rect,
    pub sel: Selection,
}

pub fn draw(app: &mut TimelineApp, ui: &mut egui::Ui) {
    let theme = Theme::new(app.doc.view.dark_mode);
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = resp.rect;
    if rect.width() < 4.0 || rect.height() < 4.0 {
        return;
    }
    painter.rect_filled(rect, 0.0, theme.canvas_bg);

    handle_input(app, ui, &resp, rect);

    let axis = TimeAxis::new(
        rect.left(),
        app.doc.view.left_year,
        app.doc.view.pixels_per_year,
    );
    // Write the clamped zoom back so the stored view never drifts out of range.
    app.doc.view.pixels_per_year = axis.ppy;
    let (view_from, view_to) = axis.visible_range(rect.width());

    let content_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + RULER_HEIGHT),
        rect.max,
    );
    let content_top = content_rect.top() + 10.0;

    // Expanded so ticking a parent category also covers its subcategories —
    // the sidebar checkboxes themselves still reflect the raw, un-expanded
    // selection.
    let filters = app.doc.effective_filters();
    // Two passes: plan the lanes, measure how many label rows each one really
    // needs at this zoom, then place them so dense stretches get the room they
    // need instead of silently dropping labels.
    let mut plans = plan_lanes(&app.doc, &filters);
    for plan in &mut plans {
        // Many stacked biographies (a dozen Roman emperors) get unreadable at
        // a fixed size, so their lanes ease narrower as you zoom out — unless
        // pinned open (click, or Ctrl+click for several) to stay prominent.
        if let LaneKind::Biography(id) = plan.kind {
            plan.thickness = bio_thickness(axis.ppy, app.enlarged_biographies.contains(&id));
        }
    }
    let demands =
        measure_lanes(&app.doc, &plans, &axis, &painter, &filters, rect, view_from, view_to);
    let lanes = place_lanes(&plans, &demands, content_top - app.y_offset);
    let total_height = lanes_height(&lanes, content_top - app.y_offset);
    app.max_y_offset = (total_height - content_rect.height() + 24.0).max(0.0);
    app.y_offset = app.y_offset.clamp(0.0, app.max_y_offset);

    let clip = painter.with_clip_rect(content_rect);
    let mut hits: Vec<Hit> = Vec::new();

    paint_grid(&clip, &axis, content_rect, &theme, view_from, view_to);
    paint_lane_stripes(&clip, &lanes, content_rect, &theme);

    let centers = timeline_centers(&lanes);

    // Bands first, so every marker and label sits on top of them.
    for lane in lanes.iter().filter(|l| l.active) {
        match lane.kind {
            LaneKind::Timeline(id) => {
                if let Some(tl) = app.doc.timeline(id) {
                    paint_timeline_band(
                        &clip, &app.doc, tl, lane, &centers, &axis, view_from, view_to, &theme,
                        app.selection,
                    );
                }
            }
            LaneKind::Biography(id) => {
                if let Some(bio) = app.doc.biography(id) {
                    paint_biography_band(
                        &clip, &app.doc, bio, lane, &axis, &theme, app.selection, view_from,
                        view_to, &mut hits,
                    );
                }
            }
            LaneKind::Group(id) => {
                paint_group_lane(
                    &clip, &app.doc, id, lane, &centers, &axis, view_from, view_to, content_rect, &theme,
                );
            }
        }
    }

    for lane in &lanes {
        paint_lane_events(
            &clip, app, lane, &centers, &axis, content_rect, &theme, &filters, view_from, view_to,
            &mut hits,
        );
    }

    paint_lane_names(
        &clip, &app.doc, &lanes, content_rect, &axis, view_from, view_to, &theme, &mut hits,
    );
    paint_ruler(&painter, &axis, rect, &theme);
    paint_scroll_indicator(&painter, content_rect, app, &theme);

    if lanes.is_empty() {
        paint_empty_state(&painter, content_rect, &theme, app.doc.is_empty());
    }

    // Remembered so double-click-to-add knows which lane and date was clicked.
    app.last_axis = Some(axis);
    app.last_width = Some(rect.width());
    app.last_lanes = lanes;

    handle_picking(app, ui, &resp, hits);
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

fn handle_input(app: &mut TimelineApp, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
    let mut axis = TimeAxis::new(
        rect.left(),
        app.doc.view.left_year,
        app.doc.view.pixels_per_year,
    );

    let hovered = resp.hovered();
    let (scroll, modifiers, pinch, pointer) = ui.input(|i| {
        (
            i.smooth_scroll_delta,
            i.modifiers,
            i.zoom_delta(),
            i.pointer.hover_pos(),
        )
    });
    let pivot = pointer
        .filter(|p| rect.contains(*p))
        .map(|p| p.x)
        .unwrap_or(rect.center().x);

    if hovered {
        if modifiers.alt {
            // Alt + wheel scrolls the lane stack vertically.
            app.y_offset = (app.y_offset - scroll.y).clamp(0.0, app.max_y_offset);
        } else if modifiers.shift {
            // Shift + wheel pans along time.
            axis.left_year -= scroll.y as f64 / axis.ppy;
        } else if scroll.y != 0.0 {
            axis.zoom_about(pivot, (scroll.y as f64 * 0.0025).exp());
        }
        if pinch != 1.0 {
            axis.zoom_about(pivot, pinch as f64);
        }
        // Trackpad horizontal scroll pans time.
        if scroll.x != 0.0 {
            axis.left_year -= scroll.x as f64 / axis.ppy;
        }
    }

    if resp.dragged() {
        let d = resp.drag_delta();
        axis.left_year -= d.x as f64 / axis.ppy;
        app.y_offset = (app.y_offset - d.y).clamp(0.0, app.max_y_offset);
    }
    if resp.dragged() || resp.hovered() {
        ui.ctx().set_cursor_icon(if resp.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Default
        });
    }

    // Keyboard: pan, zoom, fit.
    if !ui.ctx().egui_wants_keyboard_input() {
        let (left, right, plus, minus, home) = ui.input(|i| {
            (
                i.key_down(egui::Key::ArrowLeft),
                i.key_down(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals),
                i.key_pressed(egui::Key::Minus),
                i.key_pressed(egui::Key::Home),
            )
        });
        let step = 12.0 / axis.ppy;
        if left {
            axis.left_year -= step;
        }
        if right {
            axis.left_year += step;
        }
        if plus {
            axis.zoom_about(rect.center().x, 1.25);
        }
        if minus {
            axis.zoom_about(rect.center().x, 0.8);
        }
        if home {
            app.fit_to_content(rect.width());
            return;
        }
    }

    app.doc.view.left_year = axis.left_year;
    app.doc.view.pixels_per_year = axis.ppy;
}

fn handle_picking(app: &mut TimelineApp, ui: &egui::Ui, resp: &egui::Response, hits: Vec<Hit>) {
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let Some(pos) = pointer else { return };
    if !resp.rect.contains(pos) {
        return;
    }
    // Later hits are painted on top, so search backwards.
    let hit = hits.iter().rev().find(|h| h.rect.contains(pos));

    if let Some(h) = hit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if let Selection::Event(id) = h.sel {
            if let Some(ev) = app.doc.event(id) {
                let text = tooltip_text(&app.doc, ev);
                egui::Tooltip::for_widget(resp)
                    .at_pointer()
                    .show(|ui| ui.label(text));
            }
        }
    }

    if resp.clicked() {
        app.selection = hit.map(|h| h.sel);
        if let Some(Selection::Biography(id)) = app.selection {
            let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
            if multi {
                // Toggle just this one, so several can be pinned open at once.
                if !app.enlarged_biographies.remove(&id) {
                    app.enlarged_biographies.insert(id);
                }
            } else {
                app.enlarged_biographies.clear();
                app.enlarged_biographies.insert(id);
            }
        }
    }
    if resp.double_clicked() {
        match hit.map(|h| h.sel) {
            Some(sel) => app.open_editor_for(sel),
            None => {
                // Double-clicking empty space is a fast way to add an event at
                // that date on that lane.
                app.quick_add_at(pos, resp.rect);
            }
        }
    }
}

fn tooltip_text(doc: &Document, ev: &Event) -> String {
    let mut s = format!("{}\n{}", ev.title, ev.span.label());
    s.push_str(&format!(
        "\n{} · {}",
        doc.owner_name(ev.owner),
        importance_name(ev.importance)
    ));
    if !ev.categories.is_empty() {
        s.push_str(&format!("\n{}", doc.category_names(&ev.categories)));
    }
    if !ev.description.trim().is_empty() {
        s.push_str("\n\n");
        s.push_str(ev.description.trim());
    }
    s
}

// ---------------------------------------------------------------------------
// Background
// ---------------------------------------------------------------------------

fn paint_grid(
    p: &egui::Painter,
    axis: &TimeAxis,
    rect: Rect,
    theme: &Theme,
    from: f64,
    to: f64,
) {
    let step = tick_step(axis.ppy);
    for t in ticks(from, to, step) {
        let x = axis.x(t);
        if x < rect.left() - 1.0 || x > rect.right() + 1.0 {
            continue;
        }
        // Every fifth tick reads as a major gridline.
        let major = ((t / step).round() as i64).rem_euclid(5) == 0;
        let color = if major { theme.grid_major } else { theme.grid_minor };
        p.vline(x, rect.y_range(), Stroke::new(1.0, color));
    }

    // The BC/AD boundary is worth calling out on a historical chart.
    let x0 = axis.x(0.0);
    if x0 >= rect.left() && x0 <= rect.right() {
        p.vline(x0, rect.y_range(), Stroke::new(1.5, theme.era_line));
    }
}

fn paint_lane_stripes(p: &egui::Painter, lanes: &[Lane], rect: Rect, theme: &Theme) {
    for (i, lane) in lanes.iter().enumerate() {
        if i % 2 == 1 {
            continue;
        }
        let r = Rect::from_min_max(
            Pos2::new(rect.left(), lane.top),
            Pos2::new(rect.right(), lane.bottom),
        );
        if r.bottom() < rect.top() || r.top() > rect.bottom() {
            continue;
        }
        p.rect_filled(r, 0.0, theme.lane_stripe);
    }
}

fn paint_empty_state(p: &egui::Painter, rect: Rect, theme: &Theme, doc_empty: bool) {
    let msg = if doc_empty {
        "Noch keine Zeitstrahlen.\nMit “+ Zeitstrahl” eine anlegen, oder die Beispielbibliothek laden."
    } else {
        "Nichts sichtbar.\nAlles ist entweder ausgeblendet oder herausgefiltert."
    };
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        msg,
        FontId::proportional(15.0),
        theme.text_dim,
    );
}

// ---------------------------------------------------------------------------
// Ruler
// ---------------------------------------------------------------------------

fn paint_ruler(p: &egui::Painter, axis: &TimeAxis, rect: Rect, theme: &Theme) {
    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), RULER_HEIGHT));
    p.rect_filled(bar, 0.0, theme.ruler_bg);
    p.hline(
        rect.x_range(),
        bar.bottom(),
        Stroke::new(1.0, theme.grid_major),
    );

    let (from, to) = axis.visible_range(rect.width());
    let step = tick_step(axis.ppy);
    for t in ticks(from, to, step) {
        let x = axis.x(t);
        if x < rect.left() - 40.0 || x > rect.right() + 40.0 {
            continue;
        }
        p.vline(
            x,
            egui::Rangef::new(bar.bottom() - 6.0, bar.bottom()),
            Stroke::new(1.0, theme.text_dim),
        );
        p.text(
            Pos2::new(x + 4.0, bar.top() + 4.0),
            Align2::LEFT_TOP,
            axis_year_label(t),
            FontId::proportional(11.5),
            theme.text_dim,
        );
    }
}

/// A slim bar on the right showing where the lane stack is scrolled to.
fn paint_scroll_indicator(p: &egui::Painter, rect: Rect, app: &TimelineApp, theme: &Theme) {
    if app.max_y_offset <= 0.5 {
        return;
    }
    let track_h = rect.height() - 12.0;
    let visible_fraction = rect.height() / (rect.height() + app.max_y_offset);
    let thumb_h = (track_h * visible_fraction).max(24.0);
    let pos = app.y_offset / app.max_y_offset;
    let y = rect.top() + 6.0 + (track_h - thumb_h) * pos;
    let r = Rect::from_min_size(Pos2::new(rect.right() - 7.0, y), Vec2::new(4.0, thumb_h));
    p.rect_filled(r, CornerRadius::same(2), with_alpha(theme.text_dim, 110));
}

// ---------------------------------------------------------------------------
// Bands
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn paint_timeline_band(
    p: &egui::Painter,
    doc: &Document,
    tl: &Timeline,
    lane: &Lane,
    centers: &std::collections::HashMap<Id, f32>,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
    theme: &Theme,
    selection: Option<Selection>,
) {
    let pts = band_polyline(doc, tl, lane.center, centers, axis, view_from, view_to);
    if pts.len() < 2 {
        return;
    }
    let color = to_color(tl.color);
    let selected = selection == Some(Selection::Timeline(tl.id));
    let points: Vec<Pos2> = pts.iter().map(|(x, y)| Pos2::new(*x, *y)).collect();

    if selected {
        p.add(egui::Shape::line(
            points.clone(),
            Stroke::new(lane.thickness + 5.0, with_alpha(theme.selection, 90)),
        ));
    }
    p.add(egui::Shape::line(
        points.clone(),
        Stroke::new(lane.thickness, with_alpha(color, 235)),
    ));

    // Colour-coded eras — "Archaic", "Classical" — painted as separate
    // strokes over the base band. Each is sampled independently so it still
    // follows the curve through a merge/origin transition.
    if !tl.epochs.is_empty() {
        if let Some((from, to)) = band_visible_range(doc, tl, view_from, view_to) {
            for (seg_from, seg_to, seg_color, name) in band_color_segments(tl, from, to) {
                let Some(name) = name else { continue };
                let seg_pts = band_curve(tl, lane.center, centers, axis, seg_from, seg_to);
                if seg_pts.len() < 2 {
                    continue;
                }
                let seg_points: Vec<Pos2> = seg_pts.iter().map(|(x, y)| Pos2::new(*x, *y)).collect();
                p.add(egui::Shape::line(
                    seg_points,
                    Stroke::new(lane.thickness, with_alpha(to_color(seg_color), 235)),
                ));
                epoch_segment_label(
                    p, tl, lane.center, centers, axis, seg_from, seg_to, name, theme,
                );
            }
        }
    }

    // Rounded caps, plus emphasis at a junction so the merge point reads as an
    // event rather than the line just stopping.
    let r = lane.thickness * 0.5;
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        p.circle_filled(*first, r, with_alpha(color, 235));
        p.circle_filled(*last, r, with_alpha(color, 235));
    }

    if let Some(j) = &tl.merge {
        let jt = j.date.decimal();
        if jt >= view_from && jt <= view_to {
            let x = axis.x(jt);
            let y = band_center_at(tl, lane.center, centers, jt, axis.ppy);
            p.circle_filled(Pos2::new(x, y), r + 2.0, shade(color, 0.35));
            p.circle_stroke(
                Pos2::new(x, y),
                r + 2.0,
                Stroke::new(1.5, with_alpha(theme.text, 160)),
            );
            junction_label(p, &j.label, x + r + 5.0, y + r, theme);
        }
    }
    if let Some(j) = &tl.origin {
        let jt = j.date.decimal();
        if jt >= view_from && jt <= view_to {
            let x = axis.x(jt);
            let y = band_center_at(tl, lane.center, centers, jt, axis.ppy);
            junction_label(p, &j.label, x + r + 5.0, y + r, theme);
        }
    }
}

/// Junction labels sit just *below* the band. Centring them on the band put
/// them straight over the ribbon, where they were unreadable.
fn junction_label(p: &egui::Painter, label: &str, x: f32, y: f32, theme: &Theme) {
    if label.trim().is_empty() {
        return;
    }
    let galley = p.layout_no_wrap(
        label.to_owned(),
        FontId::proportional(11.0),
        theme.text_dim,
    );
    let pos = Pos2::new(x, y + 3.0);
    p.rect_filled(
        Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(3.0, 1.0)),
        CornerRadius::same(3),
        with_alpha(theme.canvas_bg, 200),
    );
    p.galley(pos, galley, theme.text_dim);
}

/// An epoch's name, sat directly on its band segment rather than in the
/// label rows above — that placement is the point: colour alone told
/// "Archaic" from "Classical" apart, this gives it a name too, and keeping it
/// *inside* the ribbon (own pill, own row) reads as clearly distinct from
/// event titles, which always live above the band.
///
/// `seg_from`/`seg_to` are already clipped to the visible window, so
/// centring on their midpoint keeps the label on screen while a long era is
/// scrolled through rather than pinning it to a point that may be off screen.
#[allow(clippy::too_many_arguments)]
fn epoch_segment_label(
    p: &egui::Painter,
    tl: &Timeline,
    own_center: f32,
    centers: &std::collections::HashMap<Id, f32>,
    axis: &TimeAxis,
    seg_from: f64,
    seg_to: f64,
    name: &str,
    theme: &Theme,
) {
    if name.trim().is_empty() {
        return;
    }
    let mid = (seg_from + seg_to) * 0.5;
    let center = Pos2::new(
        axis.x(mid),
        band_center_at(tl, own_center, centers, mid, axis.ppy),
    );
    let seg_px = (axis.x(seg_to) - axis.x(seg_from)).abs();

    let font = FontId::proportional(10.5);
    let size = p
        .layout_no_wrap(name.to_owned(), font.clone(), theme.text)
        .size();
    // A segment too narrow for its own name just keeps the colour coding —
    // better than crowding it with a clipped or overlapping label.
    if size.x + 12.0 > seg_px {
        return;
    }

    let pill = Rect::from_center_size(center, size + Vec2::new(10.0, 4.0));
    p.rect_filled(pill, CornerRadius::same(3), with_alpha(theme.canvas_bg, 220));
    p.rect_stroke(
        pill,
        CornerRadius::same(3),
        Stroke::new(1.0, with_alpha(theme.text_dim, 100)),
        StrokeKind::Outside,
    );
    p.text(center, Align2::CENTER_CENTER, name, font, theme.text);
}

#[allow(clippy::too_many_arguments)]
fn paint_biography_band(
    p: &egui::Painter,
    doc: &Document,
    bio: &Biography,
    lane: &Lane,
    axis: &TimeAxis,
    theme: &Theme,
    selection: Option<Selection>,
    view_from: f64,
    view_to: f64,
    hits: &mut Vec<Hit>,
) {
    let span = bio.span();
    let x0 = axis.x(span.t0());
    let x1 = axis.x(span.t1());
    let (fill, border) = doc.bio_colors(bio);
    let color = to_color(fill);
    let h = lane.thickness;
    let r = Rect::from_min_max(
        Pos2::new(x0, lane.center - h * 0.5),
        Pos2::new(x1.max(x0 + 2.0), lane.center + h * 0.5),
    );

    if selection == Some(Selection::Biography(bio.id)) {
        p.rect_filled(
            r.expand(3.0),
            CornerRadius::same((h * 0.5) as u8 + 3),
            with_alpha(theme.selection, 90),
        );
    }
    let corner = CornerRadius::same((h * 0.5) as u8);
    p.rect_filled(r, corner, with_alpha(color, 210));

    // Life phases — "became emperor" — recolour a stretch of the lifeline,
    // the same idea as a timeline's epochs. The band is flat (biographies
    // never curve), so unlike `band_color_segments` this needs no per-segment
    // sampling: each phase is just a straight sub-rect over the base fill.
    if !bio.life_phases.is_empty() {
        let seg_from = span.t0().max(view_from);
        let seg_to = span.t1().min(view_to);
        if seg_to > seg_from {
            for (s0, s1, seg_color, name) in color_segments(&bio.life_phases, fill, seg_from, seg_to) {
                let Some(name) = name else { continue };
                let seg_rect = Rect::from_min_max(
                    Pos2::new(axis.x(s0), r.top()),
                    Pos2::new(axis.x(s1), r.bottom()),
                );
                p.rect_filled(seg_rect, 0.0, with_alpha(to_color(seg_color), 210));
                phase_segment_label(p, seg_rect, name, theme);
            }
        }
    }
    // The culture's own colour as a border, so a fill driven by category
    // (see `Document::bio_colors`) does not hide which culture this person
    // belongs to.
    if let Some(border) = border {
        p.rect_stroke(
            r,
            corner,
            Stroke::new(1.5, with_alpha(to_color(border), 235)),
            StrokeKind::Inside,
        );
    }

    // An open-ended life (no death date) fades out rather than stopping hard.
    if bio.death.is_none() {
        for i in 0..8 {
            let a = 180 - i * 22;
            let x = r.right() + i as f32 * 4.0;
            p.rect_filled(
                Rect::from_min_size(Pos2::new(x, r.top()), Vec2::new(4.0, h)),
                0.0,
                with_alpha(color, a.max(0) as u8),
            );
        }
    }

    // Clicking anywhere along the band selects it — previously only the
    // fixed left-gutter name tab was clickable, which is being replaced by a
    // name that rides along the band itself (see `paint_lane_names`).
    hits.push(Hit {
        rect: r,
        sel: Selection::Biography(bio.id),
    });
}

/// A life phase's name, sat directly on its coloured stretch of the band.
/// Mirrors `epoch_segment_label`, minus the curve sampling a biography's
/// flat band never needs.
fn phase_segment_label(p: &egui::Painter, seg_rect: Rect, name: &str, theme: &Theme) {
    if name.trim().is_empty() {
        return;
    }
    let center = seg_rect.center();
    let font = FontId::proportional(10.0);
    let size = p
        .layout_no_wrap(name.to_owned(), font.clone(), theme.text)
        .size();
    if size.x + 10.0 > seg_rect.width() {
        return;
    }
    let pill = Rect::from_center_size(center, size + Vec2::new(8.0, 3.0));
    p.rect_filled(pill, CornerRadius::same(3), with_alpha(theme.canvas_bg, 220));
    p.text(center, Align2::CENTER_CENTER, name, font, theme.text);
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Events belonging on a lane that survive the filters and fall in view,
/// ordered most-important-first so the scarce label rows go to what matters.
///
/// Only root events (no `parent`) are returned — an event nested inside
/// another is drawn in its own nested row instead, by [`paint_nested_events`].
fn visible_events<'a>(
    doc: &'a Document,
    kind: LaneKind,
    filters: &Filters,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
) -> Vec<&'a Event> {
    let owners = lane_owners(doc, kind);
    let mut events: Vec<&Event> = doc
        .events
        .iter()
        .filter(|e| owners.contains(&e.owner))
        .filter(|e| e.parent.is_none())
        .filter(|e| event_visible(e, filters, axis.ppy))
        .filter(|e| e.span.t1() >= view_from && e.span.t0() <= view_to)
        .collect();
    events.sort_by(|a, b| {
        b.importance.cmp(&a.importance).then(
            a.span
                .t0()
                .partial_cmp(&b.span.t0())
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    events
}

/// Measure what each planned lane needs at the current zoom.
///
/// Runs the same packing the painter will, but with the row limit raised, so a
/// lane can be sized to hold its labels rather than dropping them. This is what
/// keeps a cluster of events in a single year readable.
#[allow(clippy::too_many_arguments)]
fn measure_lanes(
    doc: &Document,
    plans: &[LanePlan],
    axis: &TimeAxis,
    p: &egui::Painter,
    filters: &Filters,
    rect: Rect,
    view_from: f64,
    view_to: f64,
) -> Vec<LaneDemand> {
    plans
        .iter()
        .map(|plan| {
            let active = plan.header_only
                || lane_active(doc, plan.kind, filters, axis.ppy, view_from, view_to);
            if plan.header_only || !active {
                return LaneDemand { rows: 0, active, nested_rows: 0 };
            }

            let roots = visible_events(doc, plan.kind, filters, axis, view_from, view_to);
            // Nested rows are markers, not just labels, so they need room
            // below the band whether or not text labels are switched on.
            let nested_rows = roots
                .iter()
                .filter(|e| e.span.is_range() && !range_collapsed(e, axis.ppy))
                .map(|e| nested_depth(doc, filters, axis.ppy, e.id, 0))
                .max()
                .unwrap_or(0);

            if !doc.view.show_labels {
                return LaneDemand { rows: 0, active, nested_rows };
            }

            let mut packer = LabelPacker::new();
            let mut used = 0usize;

            let mut claim = |text: &str, importance: u8, at: f32| {
                let galley = p.layout_no_wrap(
                    text.to_owned(),
                    FontId::proportional(label_font_size(importance)),
                    Color32::WHITE,
                );
                let w = galley.size().x;
                let lx = (at - w * 0.5)
                    .max(rect.left() + 2.0)
                    .min(rect.right() - w - 2.0);
                if let Some(row) = packer.place(lx, lx + w, MAX_LABEL_ROWS) {
                    used = used.max(row + 1);
                }
            };

            for ev in roots {
                claim(&ev.title, ev.importance, axis.x(ev.span.t0()));
            }
            LaneDemand { rows: used, active, nested_rows }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn paint_lane_events(
    p: &egui::Painter,
    app: &TimelineApp,
    lane: &Lane,
    centers: &std::collections::HashMap<Id, f32>,
    axis: &TimeAxis,
    content_rect: Rect,
    theme: &Theme,
    filters: &Filters,
    view_from: f64,
    view_to: f64,
    hits: &mut Vec<Hit>,
) {
    // Skip lanes scrolled off screen entirely.
    if lane.bottom < content_rect.top() - 40.0 || lane.top > content_rect.bottom() + 40.0 {
        return;
    }

    // An expanded group is only a heading; its members draw their own events.
    // A dormant lane has nothing in this window.
    if lane.header_only || !lane.active {
        return;
    }

    let doc = &app.doc;

    let mut events: Vec<&Event> = visible_events(doc, lane.kind, filters, axis, view_from, view_to);
    // Most important first, so the scarce label rows go to what matters.
    events.sort_by(|a, b| {
        b.importance
            .cmp(&a.importance)
            .then(a.span.t0().partial_cmp(&b.span.t0()).unwrap_or(std::cmp::Ordering::Equal))
    });

    let max_rows = lane.label_rows.max(1);
    let mut packer = LabelPacker::new();

    let lane_color = to_color(lane.color);
    for ev in events {
        let t0 = ev.span.t0();
        let y = match lane.kind {
            LaneKind::Timeline(id) => match doc.timeline(id) {
                Some(tl) => band_center_at(tl, lane.center, centers, t0, axis.ppy),
                None => lane.center,
            },
            LaneKind::Biography(_) | LaneKind::Group(_) => lane.center,
        };
        // The band may be mid-curve (origin/merge transition) at this event's
        // own date, so the label's anchor has to track the same curved `y` as
        // the marker rather than the lane's flat resting position.
        let band_top = y - lane.thickness * 0.5;
        let x = axis.x(t0);
        let alpha = importance_alpha(ev.importance);
        let selected = app.selection == Some(Selection::Event(ev.id));

        // Category identity as a ring around the band-coloured marker: colour
        // still means "which timeline", the ring adds "what kind".
        let ring = ev
            .categories
            .first()
            .and_then(|c| doc.category(*c))
            .map(|c| to_color(c.color));

        // A range zoomed down to a sliver stops looking like its own bar and
        // falls back to the same point-style marker an ordinary event gets —
        // see `range_collapsed` for why.
        let shown_as_range = ev.span.is_range() && !range_collapsed(ev, axis.ppy);
        let marker_rect = if shown_as_range {
            paint_range(p, ev, axis, y, lane_color, alpha, ring, selected, theme)
        } else {
            paint_point(p, ev, axis, x, y, lane_color, alpha, ring, selected, theme)
        };
        hits.push(Hit {
            rect: marker_rect.expand(2.0),
            sel: Selection::Event(ev.id),
        });

        if shown_as_range {
            paint_nested_events(
                p,
                app,
                doc,
                filters,
                ev,
                marker_rect,
                axis,
                lane_color,
                lane.bottom - LANE_BOTTOM_PAD,
                theme,
                1,
                hits,
            );
        }

        if !doc.view.show_labels {
            continue;
        }

        // Clamping a label into view is right for a range whose bar is on
        // screen, but for a point event (or a collapsed range, painted the
        // same way) whose marker is off screen it would park the title at a
        // date it has nothing to do with.
        let on_screen = if shown_as_range {
            axis.x(ev.span.t1()) >= content_rect.left() && x <= content_rect.right()
        } else {
            x >= content_rect.left() && x <= content_rect.right()
        };
        if !on_screen {
            continue;
        }

        let font = FontId::proportional(label_font_size(ev.importance));
        let color = with_alpha(label_color(lane_color, theme.dark), alpha);
        let galley = p.layout_no_wrap(ev.title.clone(), font, color);
        let w = galley.size().x;
        let lx = (x - w * 0.5)
            .max(content_rect.left() + 2.0)
            .min(content_rect.right() - w - 2.0);
        let Some(row) = packer.place(lx, lx + w, max_rows) else {
            continue;
        };
        let ly = band_top - LABEL_BAND_TOP - (row as f32 + 1.0) * LABEL_ROW_HEIGHT;
        if ly < lane.top - LABEL_ROW_HEIGHT {
            continue;
        }
        let pos = Pos2::new(lx, ly);
        let lrect = Rect::from_min_size(pos, galley.size());

        if selected {
            p.rect_filled(
                lrect.expand2(Vec2::new(4.0, 2.0)),
                CornerRadius::same(3),
                with_alpha(theme.selection, 40),
            );
        }
        // A leader line ties the label back to its marker when they are offset.
        p.line_segment(
            [Pos2::new(x, band_top - 2.0), Pos2::new(x, lrect.bottom())],
            Stroke::new(1.0, with_alpha(lane_color, 70)),
        );
        p.galley(pos, galley, theme.text);
        hits.push(Hit {
            rect: lrect,
            sel: Selection::Event(ev.id),
        });
    }
}

/// Paint events nested inside `parent` — "Peace of Nicias" inside
/// "Peloponnesian War" — as a row of small bars/markers directly below its
/// own bar, with a tether line back up to it so the containment reads at a
/// glance. Recurses one row further down for grandchildren.
#[allow(clippy::too_many_arguments)]
fn paint_nested_events(
    p: &egui::Painter,
    app: &TimelineApp,
    doc: &Document,
    filters: &Filters,
    parent: &Event,
    parent_rect: Rect,
    axis: &TimeAxis,
    lane_color: Color32,
    lane_bottom_limit: f32,
    theme: &Theme,
    depth: usize,
    hits: &mut Vec<Hit>,
) {
    if depth > MAX_NESTED_ROWS {
        return;
    }
    let children: Vec<&Event> = doc
        .child_events(parent.id)
        .into_iter()
        .filter(|e| event_visible(e, filters, axis.ppy))
        .collect();
    if children.is_empty() {
        return;
    }

    let row_top = parent_rect.bottom() + 3.0;
    let row_h = (NESTED_ROW_HEIGHT - 5.0).max(6.0);
    if row_top + row_h > lane_bottom_limit {
        return; // Out of reserved room — deeper nesting is dropped, not overlapped.
    }

    for child in children {
        let alpha = importance_alpha(child.importance);
        let selected = app.selection == Some(Selection::Event(child.id));
        let fill = with_alpha(shade(lane_color, 0.2), alpha);

        let rect = if child.span.is_range() {
            let x0 = axis.x(child.span.t0());
            let x1 = axis.x(child.span.t1()).max(x0 + 3.0);
            Rect::from_min_max(Pos2::new(x0, row_top), Pos2::new(x1, row_top + row_h))
        } else {
            let cx = axis.x(child.span.t0());
            Rect::from_center_size(
                Pos2::new(cx, row_top + row_h * 0.5),
                Vec2::splat(row_h * 0.8),
            )
        };

        // A tether ties the child back to the parent bar it belongs to.
        let tether_x = rect.center().x.clamp(parent_rect.left(), parent_rect.right());
        p.line_segment(
            [Pos2::new(tether_x, parent_rect.bottom()), Pos2::new(tether_x, rect.top())],
            Stroke::new(1.0, with_alpha(lane_color, 100)),
        );

        if selected {
            p.rect_filled(rect.expand(2.0), CornerRadius::same(2), with_alpha(theme.selection, 100));
        }
        p.rect_filled(rect, CornerRadius::same(2), fill);
        p.rect_stroke(
            rect,
            CornerRadius::same(2),
            Stroke::new(1.0, with_alpha(shade(lane_color, -0.3), alpha)),
            StrokeKind::Outside,
        );

        hits.push(Hit {
            rect: rect.expand(2.0),
            sel: Selection::Event(child.id),
        });

        // A short title next to the marker when there is obviously room for
        // one; dense clusters fall back to the hover tooltip instead of
        // fighting over space the way top-level labels do.
        if doc.view.show_labels {
            let font = FontId::proportional((row_h - 2.0).max(9.0));
            let color = with_alpha(theme.text_dim, alpha);
            let galley = p.layout_no_wrap(child.title.clone(), font, color);
            p.galley(Pos2::new(rect.right() + 3.0, rect.top()), galley, theme.text_dim);
        }

        // Same collapse rule as the top level: a nested range event zoomed
        // down to a sliver stops offering up its own further sub-detail —
        // e.g. once "Archidamischer Krieg" itself is too thin to read, its
        // "429 v. Chr.: Einfall der Spartaner in Attika" sub-event should not
        // still be drawn in an even tinier row underneath it.
        if !range_collapsed(child, axis.ppy) {
            paint_nested_events(
                p,
                app,
                doc,
                filters,
                child,
                rect,
                axis,
                lane_color,
                lane_bottom_limit,
                theme,
                depth + 1,
                hits,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_point(
    p: &egui::Painter,
    ev: &Event,
    axis: &TimeAxis,
    x: f32,
    y: f32,
    lane_color: Color32,
    alpha: u8,
    ring: Option<Color32>,
    selected: bool,
    theme: &Theme,
) -> Rect {
    let r = marker_radius(ev.importance);
    let center = Pos2::new(x, y);

    // Uncertainty drawn as a soft horizontal whisker.
    if ev.span.start.plus_minus > 0 {
        let half = ev.span.start.plus_minus as f64 * axis.ppy;
        p.line_segment(
            [
                Pos2::new(x - half as f32, y),
                Pos2::new(x + half as f32, y),
            ],
            Stroke::new(2.0, with_alpha(lane_color, 90)),
        );
    }

    if selected {
        p.circle_filled(center, r + 4.0, with_alpha(theme.selection, 110));
    }
    // A background-tinted halo separates the marker from the band behind it —
    // without it, a marker in the band's own colour nearly disappears where a
    // curve (e.g. near a merge) puts band and marker at the same hue.
    p.circle_filled(center, r + 2.0, with_alpha(theme.canvas_bg, 235));
    p.circle_filled(center, r, with_alpha(shade(lane_color, 0.25), alpha));
    if let Some(rc) = ring {
        p.circle_stroke(center, r + 1.5, Stroke::new(2.0, with_alpha(rc, alpha)));
    } else {
        p.circle_stroke(
            center,
            r,
            Stroke::new(1.0, with_alpha(shade(lane_color, -0.4), alpha)),
        );
    }
    // Approximate dates get a hollow centre to distinguish them at a glance.
    if ev.span.start.qualifier != DateQualifier::Exact {
        p.circle_filled(center, (r * 0.4).max(1.2), theme.canvas_bg);
    }
    Rect::from_center_size(center, Vec2::splat(r * 2.0 + 6.0))
}

#[allow(clippy::too_many_arguments)]
fn paint_range(
    p: &egui::Painter,
    ev: &Event,
    axis: &TimeAxis,
    y: f32,
    lane_color: Color32,
    alpha: u8,
    ring: Option<Color32>,
    selected: bool,
    theme: &Theme,
) -> Rect {
    let h = range_bar_height(ev.importance);
    let x0 = axis.x(ev.span.t0());
    let x1 = axis.x(ev.span.t1()).max(x0 + 3.0);
    // Sits just above the band so it never hides the band itself.
    let top = y - h - 9.0;
    let r = Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x1, top + h));
    let cr = CornerRadius::same((h * 0.5) as u8);

    if selected {
        p.rect_filled(r.expand(3.0), cr, with_alpha(theme.selection, 100));
    }
    p.rect_filled(r, cr, with_alpha(shade(lane_color, 0.15), alpha));
    if let Some(rc) = ring {
        p.rect_stroke(r, cr, Stroke::new(1.5, with_alpha(rc, alpha)), StrokeKind::Outside);
    }
    // Ticks down to the band mark where the range starts and ends.
    for x in [x0, x1] {
        p.line_segment(
            [Pos2::new(x, r.bottom()), Pos2::new(x, y - 2.0)],
            Stroke::new(1.0, with_alpha(lane_color, 120)),
        );
    }
    r
}

/// A group row. Expanded it is just a heading with a bracket; collapsed it
/// becomes a single band spanning everything beneath it, so whole
/// civilisations can be compared without unfolding them.
#[allow(clippy::too_many_arguments)]
fn paint_group_lane(
    p: &egui::Painter,
    doc: &Document,
    id: Id,
    lane: &Lane,
    centers: &std::collections::HashMap<Id, f32>,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
    content_rect: Rect,
    theme: &Theme,
) {
    let color = to_color(lane.color);

    if lane.header_only {
        // A rule across the row marks where the group starts.
        p.hline(
            egui::Rangef::new(content_rect.left(), content_rect.right()),
            lane.bottom - 2.0,
            Stroke::new(1.0, with_alpha(color, 70)),
        );
        return;
    }

    // Collapsed: one band covering the union of every member's extent.
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for tid in doc.group_timelines(id) {
        if let Some(tl) = doc.timeline(tid) {
            if let Some((a, b)) = timeline_band_range(doc, tl) {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return;
    }
    let r = Rect::from_min_max(
        Pos2::new(axis.x(lo), lane.center - lane.thickness * 0.5),
        Pos2::new(axis.x(hi).max(axis.x(lo) + 3.0), lane.center + lane.thickness * 0.5),
    );
    p.rect_filled(r, CornerRadius::same((lane.thickness * 0.5) as u8), with_alpha(color, 190));
    // Hatching hints that this band stands for several timelines at once.
    let mut x = r.left() + 6.0;
    while x < r.right() {
        p.line_segment(
            [Pos2::new(x, r.top() + 2.0), Pos2::new(x, r.bottom() - 2.0)],
            Stroke::new(1.0, with_alpha(shade(color, -0.35), 120)),
        );
        x += 9.0;
    }

    // A collapsed group would otherwise silently drop any member's
    // connection to something outside it — draw it from the flat summary
    // band itself by borrowing the same easing `band_curve` already gives a
    // single timeline, via a throwaway `Timeline` carrying just that one
    // junction.
    let (from, to) = (lo.max(view_from), hi.min(view_to));
    if to > from {
        for (junction, is_merge) in group_external_junctions(doc, id) {
            if !centers.contains_key(&junction.other) {
                continue; // Falls back to a straight band, same as any hidden target.
            }
            let mut stand_in = Timeline {
                id,
                name: String::new(),
                color: lane.color,
                visible: true,
                order: 0,
                group: None,
                span: None,
                origin: None,
                merge: None,
                notes: String::new(),
                epochs: Vec::new(),
            };
            if is_merge {
                stand_in.merge = Some(junction);
            } else {
                stand_in.origin = Some(junction);
            }
            let pts = band_curve(&stand_in, lane.center, centers, axis, from, to);
            if pts.len() < 2 {
                continue;
            }
            let points: Vec<Pos2> = pts.iter().map(|(x, y)| Pos2::new(*x, *y)).collect();
            p.add(egui::Shape::line(points, Stroke::new(lane.thickness, with_alpha(color, 235))));
        }
    }

    let _ = theme;
}

// ---------------------------------------------------------------------------
// Lane names
// ---------------------------------------------------------------------------

/// Sticky names pinned to the left edge, so you always know which band is which
/// however far along the axis you have panned. Biographies are the
/// exception — with many of them stacked (a dozen Roman emperors), a
/// permanent left-side tab per lane gets unreadable, so a person's name
/// instead rides along their own band and disappears once you have scrolled
/// past their lifespan; see `paint_biography_name`.
#[allow(clippy::too_many_arguments)]
fn paint_lane_names(
    p: &egui::Painter,
    doc: &Document,
    lanes: &[Lane],
    rect: Rect,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
    theme: &Theme,
    hits: &mut Vec<Hit>,
) {
    for lane in lanes {
        if lane.bottom < rect.top() || lane.top > rect.bottom() {
            continue;
        }
        if let LaneKind::Biography(id) = lane.kind {
            if let Some(bio) = doc.biography(id) {
                paint_biography_name(p, bio, lane, axis, view_from, view_to, theme, hits);
            }
            continue;
        }
        let size = if lane.is_nested() { 11.5 } else { 13.5 };
        let mut color = label_color(to_color(lane.color), theme.dark);
        if !lane.active {
            // Nothing on this lane falls in the visible window.
            color = with_alpha(color, 105);
        }
        let galley = p.layout_no_wrap(lane.name.clone(), FontId::proportional(size), color);
        let pos = Pos2::new(
            rect.left() + GUTTER + lane.depth as f32 * 13.0,
            lane.center - galley.size().y * 0.5,
        );
        let bg = Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(6.0, 3.0));
        p.rect_filled(bg, CornerRadius::same(4), with_alpha(theme.canvas_bg, 215));
        // A colour chip repeats the band identity next to the name.
        p.rect_filled(
            Rect::from_min_size(Pos2::new(bg.left() - 5.0, bg.top() + 2.0), Vec2::new(3.0, bg.height() - 4.0)),
            CornerRadius::same(1),
            to_color(lane.color),
        );
        p.galley(pos, galley, theme.text);
        hits.push(Hit {
            rect: bg,
            sel: match lane.kind {
                LaneKind::Timeline(id) => Selection::Timeline(id),
                LaneKind::Biography(id) => Selection::Biography(id),
                LaneKind::Group(id) => Selection::Group(id),
            },
        });
    }
}

/// A biography's name, sat on its own band rather than in a fixed left-side
/// tab — centred on whatever portion of their life is currently on screen,
/// so it simply disappears once you have scrolled past it instead of
/// piling up in a permanent list the way a dozen Roman emperors would.
#[allow(clippy::too_many_arguments)]
fn paint_biography_name(
    p: &egui::Painter,
    bio: &Biography,
    lane: &Lane,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
    theme: &Theme,
    hits: &mut Vec<Hit>,
) {
    let span = bio.span();
    let from = span.t0().max(view_from);
    let to = span.t1().min(view_to);
    if to <= from {
        return; // Scrolled entirely out of view.
    }
    let seg_px = (axis.x(to) - axis.x(from)).max(0.0);
    let center = Pos2::new(axis.x((from + to) * 0.5), lane.center);

    let size = if lane.is_nested() { 11.5 } else { 13.5 };
    let color = label_color(to_color(lane.color), theme.dark);
    let font = FontId::proportional(size);
    let galley_size = p.layout_no_wrap(bio.name.clone(), font.clone(), color).size();
    // Too little room to show the name without clipping or overlapping the
    // band's own edges — better to just leave it off than crowd it in.
    if galley_size.x + 12.0 > seg_px {
        return;
    }

    let pos = Pos2::new(center.x - galley_size.x * 0.5, center.y - galley_size.y * 0.5);
    let bg = Rect::from_min_size(pos, galley_size).expand2(Vec2::new(6.0, 3.0));
    p.rect_filled(bg, CornerRadius::same(4), with_alpha(theme.canvas_bg, 215));
    let galley = p.layout_no_wrap(bio.name.clone(), font, color);
    p.galley(pos, galley, theme.text);
    hits.push(Hit {
        rect: bg,
        sel: Selection::Biography(bio.id),
    });
}
