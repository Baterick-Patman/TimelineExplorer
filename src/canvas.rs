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
            let importance = app.doc.biography(id).map_or(3, |b| b.importance);
            plan.thickness = bio_thickness(axis.ppy, app.enlarged_biographies.contains(&id), importance);
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

    // A biography's own name and its life-phase names both float centred on
    // the band rather than sitting in a fixed gutter, so painting names
    // first and phase labels after means a phase label always wins where
    // the two would otherwise overlap — the phase is the more specific,
    // more informative of the two at that exact spot.
    paint_lane_names(
        &clip, &app.doc, &lanes, content_rect, &axis, view_from, view_to, &theme, &mut hits,
    );
    paint_segment_labels(&clip, &app.doc, &lanes, &centers, &axis, view_from, view_to, &theme);
    paint_junction_labels(&clip, &app.doc, &lanes, &centers, &axis, view_from, view_to, &theme);
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
            axis_tick_label(t, step),
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
    // follows the curve through a merge/origin transition. Their *names*
    // are deliberately not painted here — see `paint_segment_labels`.
    if !tl.epochs.is_empty() {
        if let Some((from, to)) = band_visible_range(doc, tl, view_from, view_to) {
            for (seg_from, seg_to, seg_color, name) in band_color_segments(tl, from, to) {
                if name.is_none() {
                    continue;
                }
                let seg_pts = band_curve(tl, lane.center, centers, axis, seg_from, seg_to);
                if seg_pts.len() < 2 {
                    continue;
                }
                let seg_points: Vec<Pos2> = seg_pts.iter().map(|(x, y)| Pos2::new(*x, *y)).collect();
                p.add(egui::Shape::line(
                    seg_points,
                    Stroke::new(lane.thickness, with_alpha(to_color(seg_color), 235)),
                ));
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
        }
    }
    // Junction *labels* are painted in a separate, later pass — see
    // `paint_junction_labels` — so several timelines merging into a tight
    // cluster of dates (as at the end of a Successor kingdom, say) can
    // stagger their labels instead of drawing over one another, and so the
    // labels always win over an epoch tag crossing the same stretch of band.
}

/// Never stack more than this many junction labels on top of one another —
/// several timelines merging within a few years of each other is already an
/// edge case; beyond this they share the bottom row rather than growing the
/// gap under the band without bound.
const JUNCTION_LABEL_MAX_ROWS: usize = 4;
/// Height of one stacked junction-label row.
const JUNCTION_LABEL_ROW_HEIGHT: f32 = 16.0;

/// Every visible timeline's origin/merge junction label, in one pass after
/// everything else — including `paint_segment_labels` — so a junction label
/// always wins where it would otherwise sit under an epoch tag's opaque
/// background, and so several timelines merging into a tight cluster of
/// dates (the end of a Successor kingdom, say) get staggered into their own
/// rows via a shared `LabelPacker` instead of drawing over one another.
/// Previously each label sat at a single fixed offset below the band with no
/// awareness of anything else on screen — reliably too little clearance from
/// an epoch tag centred on that same band, and no clearance at all from a
/// second junction landing nearby.
#[allow(clippy::too_many_arguments)]
fn paint_junction_labels(
    p: &egui::Painter,
    doc: &Document,
    lanes: &[Lane],
    centers: &std::collections::HashMap<Id, f32>,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
    theme: &Theme,
) {
    let mut packer = LabelPacker::new();
    for lane in lanes.iter().filter(|l| l.active) {
        let LaneKind::Timeline(id) = lane.kind else { continue };
        let Some(tl) = doc.timeline(id) else { continue };
        let r = lane.thickness * 0.5;
        for j in [&tl.merge, &tl.origin].into_iter().flatten() {
            if j.label.trim().is_empty() {
                continue;
            }
            let jt = j.date.decimal();
            if jt < view_from || jt > view_to {
                continue;
            }
            let x = axis.x(jt) + r + 5.0;
            let y_base = band_center_at(tl, lane.center, centers, jt, axis.ppy) + r;
            junction_label(p, &mut packer, &j.label, x, y_base, theme);
        }
    }
}

fn junction_label(p: &egui::Painter, packer: &mut LabelPacker, label: &str, x: f32, y_base: f32, theme: &Theme) {
    let galley = p.layout_no_wrap(
        label.to_owned(),
        FontId::proportional(11.0),
        theme.text_dim,
    );
    let w = galley.size().x;
    let row = packer
        .place_rows(x, x + w, 1, JUNCTION_LABEL_MAX_ROWS)
        .unwrap_or(JUNCTION_LABEL_MAX_ROWS - 1);
    // The base offset (8px past the band's own edge) is deliberately more
    // than the old fixed `+3.0` — enough to clear an epoch tag's pill, which
    // is centred on the band and a couple of pixels taller than the band
    // itself, rather than just barely touching it.
    let pos = Pos2::new(x, y_base + 8.0 + row as f32 * JUNCTION_LABEL_ROW_HEIGHT);
    // Fully opaque — a semi-transparent label background lets whatever is
    // behind it (band colour, an event marker scrolled underneath) bleed
    // through and look like a rendering glitch rather than a deliberate
    // overlay. See the same fix on the lane-name gutter tag for the bug this
    // was actually caught by.
    p.rect_filled(
        Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(3.0, 1.0)),
        CornerRadius::same(3),
        theme.canvas_bg,
    );
    p.galley(pos, galley, theme.text_dim);
}

/// Below this on-screen width, an epoch or life-phase segment no longer gets
/// a label at all — a fixed value, deliberately independent of any
/// particular name's length, so *whether* a label shows is governed purely
/// by the segment's own duration on screen. Otherwise "Spätminoische Zeit"
/// and "Frühminoische Zeit" — same name length, different real durations —
/// would disappear at the same zoom regardless of which one actually lasted
/// longer, just because they happen to measure the same width.
const SEGMENT_LABEL_MIN_PX: f32 = 34.0;

/// An event title never grows wider than this on screen, on either of its
/// (at most two — see `wrap_two_lines`) lines — a long title ("Untergang des
/// Weströmischen Reichs - Einfall der Langobarden") wraps or, failing that,
/// ellipsises instead, so one event's label cannot dwarf the date it
/// actually marks or crowd out its neighbours.
const EVENT_LABEL_MAX_PX: f32 = 260.0;

/// Shortens `text` with a trailing "…" until it measures within `max_width`,
/// re-measuring at each step. A name too long for its segment degrades
/// gracefully this way instead of either overflowing into a neighbouring
/// segment or vanishing outright the moment it no longer fits verbatim.
fn fit_text(p: &egui::Painter, text: &str, font: &FontId, color: Color32, max_width: f32) -> String {
    if p.layout_no_wrap(text.to_owned(), font.clone(), color).size().x <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for len in (0..chars.len()).rev() {
        let candidate: String = chars[..len].iter().collect::<String>() + "…";
        if p.layout_no_wrap(candidate.clone(), font.clone(), color).size().x <= max_width {
            return candidate;
        }
    }
    "…".to_owned()
}

/// Wraps `text` onto at most two lines, each within `max_width`, breaking at
/// word boundaries. A title merely too long for one line used to lose its
/// tail to an ellipsis immediately; splitting across a second line first
/// keeps it fully readable, falling back to ellipsising only that second
/// line if even two lines still are not enough. A single word wider than
/// `max_width` on its own still gets its own line rather than looping
/// forever trying to shrink it.
fn wrap_two_lines(p: &egui::Painter, text: &str, font: &FontId, color: Color32, max_width: f32) -> Vec<String> {
    let measure = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), color).size().x;
    if measure(text) <= max_width {
        return vec![text.to_owned()];
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![String::new()];
    }
    let mut line1 = String::new();
    let mut split = 0;
    for (i, word) in words.iter().enumerate() {
        let candidate = if line1.is_empty() { (*word).to_owned() } else { format!("{line1} {word}") };
        if line1.is_empty() || measure(&candidate) <= max_width {
            line1 = candidate;
            split = i + 1;
        } else {
            break;
        }
    }
    // The very first word can still land on `line1` above even when it alone
    // is wider than `max_width` — that's what keeps this loop from placing
    // zero words and looping forever, but it means `line1` itself can still
    // overflow at this point and needs the same ellipsis treatment as an
    // overflowing second line would get.
    if measure(&line1) > max_width {
        line1 = fit_text(p, &line1, font, color, max_width);
    }
    if split >= words.len() {
        return vec![line1];
    }
    let rest = words[split..].join(" ");
    vec![line1, fit_text(p, &rest, font, color, max_width)]
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
    let seg_px = (axis.x(seg_to) - axis.x(seg_from)).abs();
    // A segment shorter than this, on screen, keeps its colour coding but
    // loses its name — the same zoomed-out-enough-and-detail-drops-away
    // idea events already follow, just driven by duration instead of an
    // explicit importance value.
    if seg_px < SEGMENT_LABEL_MIN_PX {
        return;
    }

    let mid = (seg_from + seg_to) * 0.5;
    let center = Pos2::new(
        axis.x(mid),
        band_center_at(tl, own_center, centers, mid, axis.ppy),
    );

    let font = FontId::proportional(10.5);
    let fitted = fit_text(p, name, &font, theme.text, (seg_px - 12.0).max(0.0));
    let size = p.layout_no_wrap(fitted.clone(), font.clone(), theme.text).size();

    let pill = Rect::from_center_size(center, size + Vec2::new(10.0, 4.0));
    // Fully opaque — see the comment on `junction_label`'s identical fix.
    p.rect_filled(pill, CornerRadius::same(3), theme.canvas_bg);
    p.rect_stroke(
        pill,
        CornerRadius::same(3),
        Stroke::new(1.0, with_alpha(theme.text_dim, 100)),
        StrokeKind::Outside,
    );
    p.text(center, Align2::CENTER_CENTER, &fitted, font, theme.text);
}

/// Every visible timeline epoch's and biography life-phase's name, painted
/// in their own pass *after* every band (including any other timeline's
/// curve) is already on screen.
///
/// These names used to be painted inline as part of `paint_timeline_band`/
/// `paint_biography_band`, in the same pass and lane order as every other
/// band. A curve travelling several lanes to reach a distant merge target is
/// just another band drawn in that same pass — if it happened to be painted
/// after a nearer timeline's epoch label, it painted directly over that
/// label along the way, exactly as if the label had never been there (and,
/// for a biography's life-phase name, likewise reported as unreadable —
/// something else on screen was simply painted on top of it afterwards).
/// Repainting every name in its own later pass means it always sits on top
/// of whatever else crosses through that stretch of screen, not just its
/// own band.
#[allow(clippy::too_many_arguments)]
fn paint_segment_labels(
    p: &egui::Painter,
    doc: &Document,
    lanes: &[Lane],
    centers: &std::collections::HashMap<Id, f32>,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
    theme: &Theme,
) {
    for lane in lanes {
        if !lane.active {
            continue;
        }
        match lane.kind {
            LaneKind::Timeline(id) => {
                let Some(tl) = doc.timeline(id) else { continue };
                if tl.epochs.is_empty() {
                    continue;
                }
                let Some((from, to)) = band_visible_range(doc, tl, view_from, view_to) else {
                    continue;
                };
                for (seg_from, seg_to, _, name) in band_color_segments(tl, from, to) {
                    let Some(name) = name else { continue };
                    epoch_segment_label(p, tl, lane.center, centers, axis, seg_from, seg_to, name, theme);
                }
            }
            LaneKind::Biography(id) => {
                let Some(bio) = doc.biography(id) else { continue };
                if bio.life_phases.is_empty() {
                    continue;
                }
                let span = bio.span();
                let seg_from = span.t0().max(view_from);
                let seg_to = span.t1().min(view_to);
                if seg_to <= seg_from {
                    continue;
                }
                let fill = doc.bio_color(bio);
                for (s0, s1, _, name) in color_segments(&bio.life_phases, fill, seg_from, seg_to) {
                    let Some(name) = name else { continue };
                    let seg_rect = Rect::from_min_max(
                        Pos2::new(axis.x(s0), lane.center - lane.thickness * 0.5),
                        Pos2::new(axis.x(s1), lane.center + lane.thickness * 0.5),
                    );
                    phase_segment_label(p, seg_rect, name, theme);
                }
            }
            LaneKind::Group(_) => {}
        }
    }
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
    // The phase *name* is deliberately not painted here — see
    // `paint_segment_labels`, same reasoning as a timeline's epoch names.
    if !bio.life_phases.is_empty() {
        let seg_from = span.t0().max(view_from);
        let seg_to = span.t1().min(view_to);
        if seg_to > seg_from {
            for (s0, s1, seg_color, name) in color_segments(&bio.life_phases, fill, seg_from, seg_to) {
                if name.is_none() {
                    continue;
                }
                let seg_rect = Rect::from_min_max(
                    Pos2::new(axis.x(s0), r.top()),
                    Pos2::new(axis.x(s1), r.bottom()),
                );
                p.rect_filled(seg_rect, 0.0, with_alpha(to_color(seg_color), 210));
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
    if seg_rect.width() < SEGMENT_LABEL_MIN_PX {
        return;
    }
    let center = seg_rect.center();
    let font = FontId::proportional(10.0);
    let fitted = fit_text(p, name, &font, theme.text, (seg_rect.width() - 10.0).max(0.0));
    let size = p.layout_no_wrap(fitted.clone(), font.clone(), theme.text).size();
    let pill = Rect::from_center_size(center, size + Vec2::new(8.0, 3.0));
    // Fully opaque — see the comment on `junction_label`'s identical fix.
    p.rect_filled(pill, CornerRadius::same(3), theme.canvas_bg);
    p.text(center, Align2::CENTER_CENTER, &fitted, font, theme.text);
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

/// A "long event" — a range with its own visible nested content — gets a
/// dedicated stacked slot above the band (its own title, sections, nested
/// event labels; see `paint_nested_events`) rather than sharing the plain
/// below-band label rows every other event uses. Shared between
/// `measure_lanes` and `paint_lane_events` so the two agree on exactly which
/// events fall in which bucket.
fn is_long_event(doc: &Document, filters: &Filters, ppy: f64, ev: &Event) -> bool {
    ev.span.is_range()
        && !range_collapsed(ev, ppy)
        && doc.child_events(ev.id).into_iter().any(|c| event_visible(c, filters, ppy))
}

/// Measure what each planned lane needs at the current zoom.
///
/// Runs the same packing the painter will, but with the row/slot limit
/// raised, so a lane can be sized to hold everything rather than dropping
/// it. This is what keeps a cluster of events in a single year readable, and
/// what lets several overlapping long events each get their own slot
/// instead of drawing on top of one another.
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
            // Deliberately *not* `plan.header_only || ...` — an expanded
            // group's header row used to always report itself active
            // regardless of whether anything beneath it was actually in
            // view, which meant its sticky name never disappeared while
            // scrolling the way a dormant timeline's already does (both
            // `lane_height` and `paint_lane_events` already check
            // `header_only` on their own first, so this doesn't change
            // anything for them — it only fixes what "active" itself means).
            let active = lane_active(doc, plan.kind, filters, axis.ppy, view_from, view_to);
            if plan.header_only || !active {
                return LaneDemand { below_rows: 0, above_slots: 0, above_slot_height: 0.0, active };
            }

            let roots = visible_events(doc, plan.kind, filters, axis, view_from, view_to);

            // Long events stack purely by time overlap — reusing
            // `LabelPacker` again, just claiming an event's own date span
            // instead of a label's pixel width, exactly the way
            // `paint_lane_events` will when it actually draws them. Every
            // slot in the lane shares one height, so it has to be tall
            // enough for whichever long event ends up needing the most —
            // no lane pays for the deep tier just because *one* event on it
            // happens to nest two levels while the rest only nest one.
            let mut stack_packer = LabelPacker::new();
            let mut above_slots = 0usize;
            let mut above_slot_height = 0.0f32;
            for ev in roots.iter().filter(|e| is_long_event(doc, filters, axis.ppy, e)) {
                let x0 = axis.x(ev.span.t0());
                let x1 = axis.x(ev.span.t1()).max(x0 + 3.0);
                if let Some(row) = stack_packer.place_rows(x0, x1, 1, MAX_LONG_EVENT_STACK) {
                    above_slots = above_slots.max(row + 1);
                }
                above_slot_height = above_slot_height.max(long_event_slot_height(p, doc, filters, axis.ppy, ev));
            }

            if !doc.view.show_labels {
                return LaneDemand { below_rows: 0, above_slots, above_slot_height, active };
            }

            let mut packer = LabelPacker::new();
            let mut used = 0usize;

            let mut claim = |text: &str, importance: u8, at: f32| {
                let galley = p.layout_no_wrap(
                    text.to_owned(),
                    FontId::proportional(label_font_size(importance, axis.ppy)),
                    Color32::WHITE,
                );
                // A title within one line's width reserves exactly the space
                // it needs; a longer one wraps onto a second line in the real
                // paint pass (see `wrap_two_lines`), so it needs two rows
                // reserved here too, or that second line would land on
                // whatever row the packer next hands out to someone else.
                let (w, rows_needed) = if galley.size().x <= EVENT_LABEL_MAX_PX {
                    (galley.size().x, 1)
                } else {
                    (EVENT_LABEL_MAX_PX, 2)
                };
                let lx = (at - w * 0.5)
                    .max(rect.left() + 2.0)
                    .min(rect.right() - w - 2.0);
                if let Some(row) = packer.place_rows(lx, lx + w, rows_needed, MAX_LABEL_ROWS) {
                    used = used.max(row + rows_needed);
                }
            };

            // Same fan-out the real paint pass applies, so a lane doesn't
            // under-reserve rows for events that will visually spread out.
            // Only a *plain* event's label lives in these below-band rows —
            // a long event's own title has its dedicated slot above instead.
            let fanned = fan_out_year_only_events(roots.iter().copied());
            for ev in roots.iter().filter(|e| !is_long_event(doc, filters, axis.ppy, e)) {
                let t0 = fanned.get(&ev.id).copied().unwrap_or_else(|| ev.span.t0());
                claim(&ev.title, ev.importance, axis.x(t0));
            }
            LaneDemand { below_rows: used, above_slots, above_slot_height, active }
        })
        .collect()
}

/// Vertical space one stacked slot needs for `ev`'s own content: `ev`'s own
/// title measured at however many lines it actually wraps to (see
/// `wrap_two_lines`; most titles are one line, so this is usually much less
/// than the two-line worst case), plus one nested-label row block per
/// labelled depth `ev` actually reaches (`MAX_LABELED_NESTED_DEPTH`, both if
/// `ev` has a labelled grandchild, just depth-1's own otherwise), plus the
/// bar itself. Called only from `measure_lanes`, which stores the result on
/// `Lane::above_slot_height` for `paint_lane_events`/`paint_long_event` to
/// read back later in the same frame — the two never call this function
/// independently, so they can't disagree on the value.
fn long_event_slot_height(p: &egui::Painter, doc: &Document, filters: &Filters, ppy: f64, ev: &Event) -> f32 {
    let nested = nested_reservation_for(doc, filters, ppy, ev);
    // This event's *own* title, measured rather than assumed — most titles
    // are one line, so reserving for a worst-case two-line title on every
    // long event regardless of what it's actually called wasted real space
    // in a library with several stacked long events.
    let title_font = FontId::proportional(label_font_size(ev.importance, ppy));
    let title_w = p.layout_no_wrap(ev.title.clone(), title_font, Color32::WHITE).size().x;
    let title_rows = if title_w <= EVENT_LABEL_MAX_PX { 1.0 } else { 2.0 };
    let bar_h = range_bar_height(ev.importance) + 10.0; // the "has_children" bonus `paint_range` always adds here.
    title_rows * LABEL_ROW_HEIGHT + TITLE_TO_NESTED_GAP + nested + bar_h + 9.0 + 6.0
}

/// How much nested-label row space `ev` itself actually needs above its bar:
/// both labelled depths' row blocks if it has a labelled grandchild, just
/// depth-1's own block otherwise. Shared by `long_event_slot_height` (sizing
/// the lane's reserved slot) and `paint_long_event` (placing this event's own
/// title) so the two never disagree about how far a title sits from content
/// that, for this particular event, was never going to be there.
fn nested_reservation_for(doc: &Document, filters: &Filters, ppy: f64, ev: &Event) -> f32 {
    let has_grandchild = doc
        .child_events(ev.id)
        .into_iter()
        .filter(|c| event_visible(c, filters, ppy))
        .any(|c| doc.child_events(c.id).into_iter().any(|gc| event_visible(gc, filters, ppy)));
    if has_grandchild {
        max_nested_label_reserved_height()
    } else {
        NESTED_LABEL_ROWS_PER_DEPTH as f32 * nested_label_style(1).1
    }
}

/// Paints every root event on a lane, split into two groups that live on
/// opposite sides of the band:
///
/// - A **long event** — a range with its own visible nested content — gets a
///   dedicated stacked slot *above* the band: its bar (with sections/markers
///   for its children, see `paint_nested_events`), and its own title above
///   that. Several long events overlapping in time stack into further slots
///   (`paint_long_event`) instead of drawing over one another.
/// - Every other event — a plain point, or a range with nothing nested in
///   it — gets its marker exactly on the band as always, but its label now
///   floats *below* the band instead of above it. This is what actually
///   frees up the space a long event's own dedicated slot needs, and keeps
///   a crowd of ordinary events from competing with a war's own section
///   headers for the same real estate.
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
    // Several year-only-dated events sharing one year would otherwise all
    // sit on the exact same pixel — see `fan_out_year_only_events`.
    let fanned = fan_out_year_only_events(events.iter().copied());

    let max_below_rows = lane.below_rows.max(1);
    let max_above_slots = lane.above_slots.max(1);
    let mut below_packer = LabelPacker::new();
    let mut stack_packer = LabelPacker::new();

    let lane_color = to_color(lane.color);
    for ev in events {
        let t0 = fanned.get(&ev.id).copied().unwrap_or_else(|| ev.span.t0());
        let y = match lane.kind {
            LaneKind::Timeline(id) => match doc.timeline(id) {
                Some(tl) => band_center_at(tl, lane.center, centers, t0, axis.ppy),
                None => lane.center,
            },
            LaneKind::Biography(_) | LaneKind::Group(_) => lane.center,
        };
        let alpha = importance_alpha(ev.importance);
        let selected = app.selection == Some(Selection::Event(ev.id));
        // Category identity as a ring around the band-coloured marker: colour
        // still means "which timeline", the ring adds "what kind".
        let ring = ev
            .categories
            .first()
            .and_then(|c| doc.category(*c))
            .map(|c| to_color(c.color));

        if is_long_event(doc, filters, axis.ppy, ev) {
            paint_long_event(
                p,
                app,
                doc,
                filters,
                ev,
                axis,
                y,
                lane.top,
                lane_color,
                alpha,
                ring,
                selected,
                &mut stack_packer,
                max_above_slots,
                lane.above_slot_height,
                content_rect,
                view_from,
                view_to,
                theme,
                hits,
            );
            continue;
        }

        // The band may be mid-curve (origin/merge transition) at this event's
        // own date, so the label's anchor has to track the same curved `y` as
        // the marker rather than the lane's flat resting position.
        let band_bottom = y + lane.thickness * 0.5;
        let x = axis.x(t0);

        // A range zoomed down to a sliver stops looking like its own bar and
        // falls back to the same point-style marker an ordinary event gets —
        // see `range_collapsed` for why. (A range with visible children never
        // reaches here at all — see `is_long_event` above.)
        let shown_as_range = ev.span.is_range() && !range_collapsed(ev, axis.ppy);
        let marker_rect = if shown_as_range {
            paint_range(p, ev, axis, y, true, 0.0, lane_color, alpha, ring, selected, false, content_rect, theme)
        } else {
            paint_point(p, ev, axis, x, y, lane_color, alpha, ring, selected, theme)
        };
        hits.push(Hit {
            rect: marker_rect.expand(2.0),
            sel: Selection::Event(ev.id),
        });

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

        let font = FontId::proportional(label_font_size(ev.importance, axis.ppy));
        // A neutral colour, not a tint of the lane's own hue — a light band
        // (e.g. cyan) tinted the same way its label was coloured produced
        // low-contrast, barely-legible text once the two sat close together
        // at a cramped zoom. Which lane a label belongs to is already clear
        // from its position and the marker beside it.
        let color = with_alpha(theme.text, alpha);
        // A title too long for one line wraps onto a second rather than
        // losing its tail to an ellipsis right away — only a title that
        // still doesn't fit across two lines falls back to ellipsising, and
        // only on that second line. Either way the marker itself stays
        // exactly on the band; only the label below it grows taller.
        let lines = wrap_two_lines(p, &ev.title, &font, color, EVENT_LABEL_MAX_PX);
        let line_galleys: Vec<_> = lines.into_iter().map(|l| p.layout_no_wrap(l, font.clone(), color)).collect();
        let rows_needed = line_galleys.len();
        let w = line_galleys.iter().fold(0.0_f32, |w, g| w.max(g.size().x));
        // A range's own bar can be wider than the whole screen once zoomed
        // in — its label should stay centred over whatever portion of it is
        // actually on screen and scroll along with it, the same way an
        // epoch's name already tracks its visible segment, rather than
        // staying anchored to the start date and scrolling off-screen the
        // moment you pan into the middle of the range.
        let label_x = if shown_as_range {
            let visible_from = t0.max(view_from);
            let visible_to = ev.span.t1().min(view_to);
            axis.x((visible_from + visible_to) * 0.5)
        } else {
            x
        };
        let lx = (label_x - w * 0.5)
            .max(content_rect.left() + 2.0)
            .min(content_rect.right() - w - 2.0);
        let Some(row) = below_packer.place_rows(lx, lx + w, rows_needed, max_below_rows) else {
            continue;
        };
        let ly_top = band_bottom + LABEL_BAND_BOTTOM + row as f32 * LABEL_ROW_HEIGHT;
        if ly_top + rows_needed as f32 * LABEL_ROW_HEIGHT > lane.bottom + LABEL_ROW_HEIGHT {
            continue;
        }
        let lrect = Rect::from_min_size(Pos2::new(lx, ly_top), Vec2::new(w, rows_needed as f32 * LABEL_ROW_HEIGHT));

        if selected {
            p.rect_filled(
                lrect.expand2(Vec2::new(4.0, 2.0)),
                CornerRadius::same(3),
                with_alpha(theme.selection, 40),
            );
        }
        // A leader line ties the label back to its marker (or, for a wide
        // range, up to whatever point on its own bar sits directly above
        // the label's now-centred, scroll-tracking position) when offset.
        p.line_segment(
            [Pos2::new(label_x, band_bottom + 2.0), Pos2::new(label_x, lrect.top())],
            Stroke::new(1.0, with_alpha(lane_color, 70)),
        );
        // Each line centred within the shared block rather than sharing one
        // left edge — a two-line title rarely wraps into two equal-width
        // lines, and left-aligning the shorter one under the wider one reads
        // as off-centre from the marker it belongs to.
        for (i, galley) in line_galleys.into_iter().enumerate() {
            let line_x = lx + (w - galley.size().x) * 0.5;
            p.galley(Pos2::new(line_x, ly_top + i as f32 * LABEL_ROW_HEIGHT), galley, theme.text);
        }
        hits.push(Hit {
            rect: lrect,
            sel: Selection::Event(ev.id),
        });
    }
}

/// Paints one "long event" — a range with its own visible nested content —
/// in its own stacked slot above the band: assigns a stack level purely by
/// time overlap against every other long event on this lane (`stack_packer`,
/// shared across the whole lane), draws its bar and everything nested on it
/// (`paint_nested_events`), then its own title above the reserved nested-
/// label area. A degenerate overlap of more long events than
/// `MAX_LONG_EVENT_STACK` allows falls back to sharing the topmost slot
/// rather than dropping the event outright.
#[allow(clippy::too_many_arguments)]
fn paint_long_event(
    p: &egui::Painter,
    app: &TimelineApp,
    doc: &Document,
    filters: &Filters,
    ev: &Event,
    axis: &TimeAxis,
    y: f32,
    lane_top: f32,
    lane_color: Color32,
    alpha: u8,
    ring: Option<Color32>,
    selected: bool,
    stack_packer: &mut LabelPacker,
    max_above_slots: usize,
    above_slot_height: f32,
    content_rect: Rect,
    view_from: f64,
    view_to: f64,
    theme: &Theme,
    hits: &mut Vec<Hit>,
) {
    let x0 = axis.x(ev.span.t0());
    let x1 = axis.x(ev.span.t1()).max(x0 + 3.0);
    // A genuine slot keeps this event visually distinct from every other
    // long event it overlaps in time; degenerately overlapping more of them
    // than `MAX_LONG_EVENT_STACK` allows falls back to sharing the topmost
    // slot with whatever is already there. The bar still paints in that
    // shared slot — better a rare visual overlap than a dropped event
    // outright — but neither its title nor its nested content does: two
    // titles, or two independent sets of nested labels (each with its own
    // `paint_nested_events` call and thus its own `LabelPacker`, with no
    // visibility into the other), landing on the exact same spot renders as
    // illegible, run-together text, worse than the one that lost the slot
    // simply staying available via a click or the hover tooltip instead.
    let (slot, got_own_slot) = match stack_packer.place_rows(x0, x1, 1, max_above_slots) {
        Some(row) => (row, true),
        None => (max_above_slots.saturating_sub(1), false),
    };
    let stack_offset = slot as f32 * above_slot_height;

    let marker_rect =
        paint_range(p, ev, axis, y, false, stack_offset, lane_color, alpha, ring, selected, true, content_rect, theme);
    hits.push(Hit {
        rect: marker_rect.expand(2.0),
        sel: Selection::Event(ev.id),
    });

    paint_nested_events(p, app, doc, filters, ev, marker_rect, axis, lane_color, theme, 1, got_own_slot, hits);

    if !doc.view.show_labels || !got_own_slot {
        return;
    }
    let on_screen = axis.x(ev.span.t1()) >= content_rect.left() && axis.x(ev.span.t0()) <= content_rect.right();
    if !on_screen {
        return;
    }

    let font = FontId::proportional(label_font_size(ev.importance, axis.ppy));
    let color = with_alpha(theme.text, alpha);
    let lines = wrap_two_lines(p, &ev.title, &font, color, EVENT_LABEL_MAX_PX);
    let line_galleys: Vec<_> = lines.into_iter().map(|l| p.layout_no_wrap(l, font.clone(), color)).collect();
    let rows_needed = line_galleys.len();
    let w = line_galleys.iter().fold(0.0_f32, |w, g| w.max(g.size().x));

    let visible_from = ev.span.t0().max(view_from);
    let visible_to = ev.span.t1().min(view_to);
    let label_x = axis.x((visible_from + visible_to) * 0.5);
    let lx = (label_x - w * 0.5)
        .max(content_rect.left() + 2.0)
        .min(content_rect.right() - w - 2.0);

    // Only as much nested-label space as this event itself actually reaches
    // — a long event with just direct children sits closer to its own bar
    // than one whose nesting goes a level deeper, exactly like the slot
    // height above already accounts for.
    let title_bottom = marker_rect.top() - nested_reservation_for(doc, filters, axis.ppy, ev) - TITLE_TO_NESTED_GAP;
    let ly_top = title_bottom - rows_needed as f32 * LABEL_ROW_HEIGHT;
    if ly_top < lane_top - LABEL_ROW_HEIGHT {
        return;
    }
    let lrect = Rect::from_min_size(Pos2::new(lx, ly_top), Vec2::new(w, rows_needed as f32 * LABEL_ROW_HEIGHT));

    if selected {
        p.rect_filled(
            lrect.expand2(Vec2::new(4.0, 2.0)),
            CornerRadius::same(3),
            with_alpha(theme.selection, 40),
        );
    }
    p.line_segment(
        [Pos2::new(label_x, y - 2.0), Pos2::new(label_x, lrect.bottom())],
        Stroke::new(1.0, with_alpha(lane_color, 70)),
    );
    // Each line centred within the shared block — see the identical fix in
    // `paint_lane_events`.
    for (i, galley) in line_galleys.into_iter().enumerate() {
        let line_x = lx + (w - galley.size().x) * 0.5;
        p.galley(Pos2::new(line_x, ly_top + i as f32 * LABEL_ROW_HEIGHT), galley, theme.text);
    }
    hits.push(Hit {
        rect: lrect,
        sel: Selection::Event(ev.id),
    });
}

/// However deep a hand-edited file nests events, never recurse the on-band
/// segment painting below this many levels — a chain that deep is unreadable
/// at this scale regardless, and every level shares the very same bar height
/// so deeper segments would be indistinguishable from their parent anyway.
const MAX_NESTED_SEGMENT_DEPTH: usize = 4;
/// How many nesting levels below the parent get their own floating label at
/// all — "Peloponnesischer Krieg" → "Archidamischer Krieg" (depth 1) →
/// "Schlacht bei Solygeia" (depth 2) both get one. Any deeper and a label
/// would have nowhere left to sit without colliding with its own parent's —
/// it still paints (and stays clickable) but falls back to the hover
/// tooltip for its name, the same "dense clusters" tradeoff the top level
/// already accepts.
const MAX_LABELED_NESTED_DEPTH: usize = 2;
/// A floating nested-child label that would collide with a sibling steps up
/// to a further row within its own depth's block instead of overlapping it,
/// up to this many rows — a small, fixed cap since there is no lane-height
/// reservation backing this the way top-level label rows have; run out and
/// the label is dropped, same as a top-level label that runs out of
/// `MAX_LABEL_ROWS`.
const NESTED_LABEL_ROWS_PER_DEPTH: usize = 2;
/// Gap between a long event's own title and the nested-label block right
/// below it (or the bar itself, if it has no labelled children at all).
/// Shared by `long_event_slot_height` (reserving the room) and
/// `paint_long_event` (placing the title within it) so the two can't drift
/// apart. A hover tooltip for whatever is on the bar — a nested child's
/// own name, say — floats independently of this layout and can still land
/// close by, but a title sitting right on top of its own nested block with
/// next to no breathing room made an already-tight reading harder still.
const TITLE_TO_NESTED_GAP: f32 = 10.0;

/// Font size and row height for a nested label at the given depth. A
/// shallower depth (closer to the parent's own title) reads as a heading
/// over the deeper depth's more numerous, smaller event labels — the same
/// visual hierarchy the top-level title already sets up over everything
/// nested beneath it (e.g. a section name like "Archidamischer Krieg"
/// written a size larger than the individual events inside it).
fn nested_label_style(depth: usize) -> (f32, f32) {
    if depth <= 1 {
        (11.0, 16.0)
    } else {
        (9.5, 13.0)
    }
}

/// How far above the bar a given depth's own row block starts: every depth
/// *deeper* than it reserves its own block of rows closer to the bar
/// first, so a shallower depth's rows sit above all of those combined
/// rather than overlapping them.
fn nested_label_base_offset(depth: usize) -> f32 {
    ((depth + 1)..=MAX_LABELED_NESTED_DEPTH)
        .map(|d| NESTED_LABEL_ROWS_PER_DEPTH as f32 * nested_label_style(d).1)
        .sum()
}

/// Total vertical space every labelled nesting depth's rows need together —
/// what a long event's own title must clear above the bar.
fn max_nested_label_reserved_height() -> f32 {
    (1..=MAX_LABELED_NESTED_DEPTH)
        .map(|d| NESTED_LABEL_ROWS_PER_DEPTH as f32 * nested_label_style(d).1)
        .sum()
}

/// Paint events nested inside `parent` — "Archidamischer Krieg" inside
/// "Peloponnesischer Krieg" — directly on the parent's own bar rather than in
/// a row below it: a nested range event becomes a colour-coded segment
/// spanning `parent_rect`'s own height, exactly like an epoch painted on a
/// timeline's band; a nested point event becomes a small marker sitting on
/// that same bar. The parent's bar behaves like its own small, exactly
/// parallel mini-timeline. Recurses into a range child's own segment rect for
/// grandchildren (capped at `MAX_NESTED_SEGMENT_DEPTH`), and the first
/// `MAX_LABELED_NESTED_DEPTH` levels each get their own floating label, one
/// row block per depth, shallower depths sitting above deeper ones — any
/// deeper still and a label would have nowhere left to sit without colliding
/// with its own parent's, so it still paints (and is still clickable) but
/// falls back to the hover tooltip for its name, the same "dense clusters"
/// tradeoff the top level accepts.
/// `labels_allowed` additionally suppresses every label at every depth when
/// `false` — used when `parent` itself lost the race for its own stacked
/// slot (see `paint_long_event`) and shares one with an unrelated event, so
/// this call's own independent `LabelPacker` cannot tell the two apart.
/// Markers and segments still paint and stay clickable either way; only the
/// text is dropped.
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
    theme: &Theme,
    depth: usize,
    labels_allowed: bool,
    hits: &mut Vec<Hit>,
) {
    if depth > MAX_NESTED_SEGMENT_DEPTH {
        return;
    }
    let children: Vec<&Event> = doc
        .child_events(parent.id)
        .into_iter()
        .filter(|e| event_visible(e, filters, axis.ppy))
        // Only a child whose own span actually overlaps the parent's is drawn
        // on the parent's bar — one that doesn't belongs to a data mistake,
        // not a "start/end of the mini-timeline" edge case worth clipping to.
        .filter(|e| e.span.t1() >= parent.span.t0() && e.span.t0() <= parent.span.t1())
        .collect();
    if children.is_empty() {
        return;
    }

    // The parent's own bar is already lightened (`shade(lane_color, 0.15)` in
    // `paint_range`) — a child needs to go the *other* way, darker, or it
    // would paint in the exact same colour and disappear into the bar behind
    // it. Each nesting level darkens a little further, so a grandchild
    // segment still reads as visually "deeper" than its parent even though
    // both sit at the very same bar height.
    let shade_amount = -0.3 - (depth - 1) as f32 * 0.15;
    // Scoped to exactly this call's own children (i.e. one sibling group at
    // a time) — only ever actually consulted at depth 1, since deeper
    // labels are never drawn, but harmless to always create.
    let mut label_packer = LabelPacker::new();

    for child in children {
        let alpha = importance_alpha(child.importance);
        let selected = app.selection == Some(Selection::Event(child.id));
        let fill = with_alpha(shade(lane_color, shade_amount), alpha);
        let show_label = doc.view.show_labels && depth <= MAX_LABELED_NESTED_DEPTH && labels_allowed;

        if child.span.is_range() && !range_collapsed(child, axis.ppy) {
            let x0 = axis.x(child.span.t0()).max(parent_rect.left());
            let x1 = axis.x(child.span.t1()).min(parent_rect.right()).max(x0 + 2.0);
            let rect = Rect::from_min_max(Pos2::new(x0, parent_rect.top()), Pos2::new(x1, parent_rect.bottom()));
            if rect.right() <= parent_rect.left() || rect.left() >= parent_rect.right() {
                continue; // Scrolled entirely past the parent's own visible bar.
            }

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
            hits.push(Hit { rect: rect.expand(2.0), sel: Selection::Event(child.id) });

            if show_label {
                nested_child_label(
                    p,
                    &mut label_packer,
                    &child.title,
                    rect.center().x,
                    (rect.width() - 4.0).max(20.0),
                    parent_rect.top(),
                    alpha,
                    depth,
                    lane_color,
                    theme,
                );
            }

            paint_nested_events(p, app, doc, filters, child, rect, axis, lane_color, theme, depth + 1, labels_allowed, hits);
        } else {
            let cx = axis.x(child.span.t0());
            if cx < parent_rect.left() - 20.0 || cx > parent_rect.right() + 20.0 {
                continue; // Scrolled well off screen — not worth painting.
            }
            let center = Pos2::new(cx, parent_rect.center().y);
            let r = (parent_rect.height() * 0.5).clamp(2.5, 5.0);

            if selected {
                p.circle_filled(center, r + 3.0, with_alpha(theme.selection, 110));
            }
            p.circle_filled(center, r + 1.0, with_alpha(theme.canvas_bg, 235));
            p.circle_filled(center, r, fill);
            p.circle_stroke(center, r, Stroke::new(1.0, with_alpha(shade(lane_color, -0.4), alpha)));

            hits.push(Hit {
                rect: Rect::from_center_size(center, Vec2::splat(r * 2.0 + 4.0)),
                sel: Selection::Event(child.id),
            });

            if show_label {
                nested_child_label(
                    p,
                    &mut label_packer,
                    &child.title,
                    cx,
                    170.0,
                    parent_rect.top(),
                    alpha,
                    depth,
                    lane_color,
                    theme,
                );
            }
        }
    }
}

/// A short title floated above `top_y`, centred on `anchor_x` — used for both
/// a nested point-child's marker and a nested range-child's own segment, so a
/// title never has to fit inside a bar only a few pixels tall (the same idea
/// as an epoch's name overlaid on its timeline band, just floated entirely
/// above it rather than centred within it, since a nested bar is far thinner
/// than a timeline's own). `depth` picks this label's own row block — see
/// `nested_label_style`/`nested_label_base_offset` — so a depth-1 "section"
/// label reads as a heading sitting above every depth-2 "event" label rather
/// than colliding with them. `packer` — shared across every sibling *at this
/// depth* in the same call — pushes a label that would otherwise overlap its
/// neighbour onto a further row within that same block instead, the same
/// idea as top-level labels stacking in `LabelPacker` rows, just with a small
/// fixed cap of its own rather than a lane-height reservation behind it; a
/// title that still doesn't fit within `NESTED_LABEL_ROWS_PER_DEPTH` is
/// silently dropped and falls back to the hover tooltip. A thin leader line
/// from the label straight down to `top_y` — the same idea `paint_lane_events`
/// and a long event's own title already use — ties the label back to
/// whichever marker or segment sits at `anchor_x`; with several siblings
/// packed close together (a cluster of battles a few years apart, say), the
/// horizontal centring alone stopped being enough to tell which label
/// belonged to which dot once labels started stacking into further rows.
#[allow(clippy::too_many_arguments)]
fn nested_child_label(
    p: &egui::Painter,
    packer: &mut LabelPacker,
    name: &str,
    anchor_x: f32,
    max_width: f32,
    top_y: f32,
    alpha: u8,
    depth: usize,
    lane_color: Color32,
    theme: &Theme,
) {
    if name.trim().is_empty() {
        return;
    }
    let (font_size, row_h) = nested_label_style(depth);
    let font = FontId::proportional(font_size);
    let color = with_alpha(theme.text_dim, alpha);
    let fitted = fit_text(p, name, &font, color, max_width);
    let galley = p.layout_no_wrap(fitted, font, color);
    let w = galley.size().x;
    let x_min = anchor_x - w * 0.5;
    let Some(row) = packer.place_rows(x_min, x_min + w, 1, NESTED_LABEL_ROWS_PER_DEPTH) else {
        return;
    };
    let offset = nested_label_base_offset(depth) + row as f32 * row_h;
    let pos = Pos2::new(x_min, top_y - galley.size().y - 2.0 - offset);
    p.line_segment(
        [Pos2::new(anchor_x, top_y), Pos2::new(anchor_x, pos.y + galley.size().y + 1.0)],
        Stroke::new(1.0, with_alpha(lane_color, 70)),
    );
    // Fully opaque — see the comment on `junction_label`'s identical fix.
    p.rect_filled(
        Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(2.0, 1.0)),
        CornerRadius::same(2),
        theme.canvas_bg,
    );
    p.galley(pos, galley, color);
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

/// Paints a range event's own bar, either above the band (a "long event"
/// with its own nested content, possibly pushed further up by
/// `stack_offset` if it shares the lane with an overlapping long event) or
/// below it (an ordinary childless range, exactly mirroring the above case).
/// `y` is always the actual band centre — needed regardless of `below`, so
/// the connecting ticks at the range's start/end always reach the real band
/// line rather than just the bar's own edge.
#[allow(clippy::too_many_arguments)]
fn paint_range(
    p: &egui::Painter,
    ev: &Event,
    axis: &TimeAxis,
    y: f32,
    below: bool,
    stack_offset: f32,
    lane_color: Color32,
    alpha: u8,
    ring: Option<Color32>,
    selected: bool,
    has_children: bool,
    content_rect: Rect,
    theme: &Theme,
) -> Rect {
    // A bare bar only needs to read as "a range, not a point"; one with
    // nested content on it needs enough of its own height for a child
    // segment's fill and a marker to actually sit on, comfortably clear of
    // the label rows beyond it (see the comment at this function's call
    // site) — only ever the case for a bar above the band, since a
    // below-the-band bar is by definition a childless, "plain" event.
    let h = range_bar_height(ev.importance) + if has_children { 10.0 } else { 0.0 };
    // Clamped to just past the visible edges rather than the raw (possibly
    // enormous) pixel position a far-off-screen date maps to — a years-wide
    // war zoomed in far enough that its own span is many screens wide used
    // to hand the renderer a rect thousands of pixels past the clip rect on
    // one or both sides, which reproducibly failed to paint *anything* at
    // all rather than just clipping visibly, on this eframe/glow version.
    // Clamping first sidesteps that rather than depending on a fix (or an
    // explanation) landing upstream; a bar's edges rounding off screen is
    // invisible to the user either way, since nothing out there was drawn
    // precisely regardless.
    let margin = 100.0;
    let x0 = axis.x(ev.span.t0()).max(content_rect.left() - margin);
    let x1 = axis.x(ev.span.t1()).min(content_rect.right() + margin).max(x0 + 3.0);
    // The edge nearest the band sits a fixed gap away, pushed further out by
    // `stack_offset` when this bar shares its lane with an overlapping long
    // event stacked in front of it; the far edge is `h` beyond that, away
    // from the band either way.
    let dir: f32 = if below { 1.0 } else { -1.0 };
    let near_edge = y + dir * (9.0 + stack_offset);
    let far_edge = near_edge + dir * h;
    let (top, bottom) = if below { (near_edge, far_edge) } else { (far_edge, near_edge) };
    let r = Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x1, bottom));
    let cr = CornerRadius::same((h * 0.5) as u8);

    if selected {
        p.rect_filled(r.expand(3.0), cr, with_alpha(theme.selection, 100));
    }
    p.rect_filled(r, cr, with_alpha(shade(lane_color, 0.15), alpha));
    if let Some(rc) = ring {
        p.rect_stroke(r, cr, Stroke::new(1.5, with_alpha(rc, alpha)), StrokeKind::Outside);
    }
    // Ticks to the band mark where the range starts and ends.
    let band_edge = y + dir * 2.0;
    for x in [x0, x1] {
        p.line_segment(
            [Pos2::new(x, near_edge), Pos2::new(x, band_edge)],
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
            // `lane_active` now also retracts a biography once the zoomed-out
            // view no longer wants its importance level — previously the
            // *only* way this lane went dormant was scrolling past its own
            // lifespan, which `paint_biography_name`'s own view-window check
            // below already caught on its own, so this check went unneeded
            // until now: without it, an importance-dormant lane still painted
            // a name with no band under it.
            if lane.active {
                if let Some(bio) = doc.biography(id) {
                    paint_biography_name(p, bio, lane, axis, view_from, view_to, theme, hits);
                }
            }
            continue;
        }
        // Nothing on this lane — band or events — falls in the visible
        // window, so its name shouldn't either: seeing "Seleukidenreich"
        // pinned to the left edge while looking at 200 AD, centuries past
        // where it ever existed, is exactly the kind of stale label a
        // biography's on-band name already avoids by disappearing once
        // scrolled past. A dormant lane still reserves its (slim) row for
        // scroll-position consistency — only the name is skipped here.
        if !lane.active {
            continue;
        }
        let size = if lane.is_nested() { 11.5 } else { 13.5 };
        // Neutral text colour — see the comment in `paint_lane_events` on
        // why this is no longer tinted by the lane's own colour.
        let color = theme.text;
        let galley = p.layout_no_wrap(lane.name.clone(), FontId::proportional(size), color);
        let pos = Pos2::new(
            rect.left() + GUTTER + lane.depth as f32 * 13.0,
            lane.center - galley.size().y * 0.5,
        );
        let bg = Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(6.0, 3.0));
        // Fully opaque — this tag is pinned to the left edge regardless of
        // scroll, so whatever content is scrolled underneath it (band, an
        // event marker) must be cleanly hidden rather than bleeding through
        // at partial alpha. The bleed-through used to look exactly like a
        // displaced, ghostly event marker wherever one happened to scroll
        // under the tag — a real, reported bug, not a hypothetical one.
        p.rect_filled(bg, CornerRadius::same(4), theme.canvas_bg);
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

    // Same per-importance/zoom scale an event's own title uses, so a more
    // significant life reads at a visibly larger name — an inline biography
    // (nested under its timeline) stays a touch smaller than one promoted to
    // its own lane, same as before this took importance into account.
    let base = label_font_size(bio.importance, axis.ppy);
    let size = if lane.is_nested() { base - 2.0 } else { base };
    // Neutral text colour — see the comment in `paint_lane_events`; doubly
    // so here, since this name sits directly on top of the band's own fill.
    let color = theme.text;
    let font = FontId::proportional(size);
    let galley_size = p.layout_no_wrap(bio.name.clone(), font.clone(), color).size();
    // Too little room to show the name without clipping or overlapping the
    // band's own edges — better to just leave it off than crowd it in.
    if galley_size.x + 12.0 > seg_px {
        return;
    }

    let pos = Pos2::new(center.x - galley_size.x * 0.5, center.y - galley_size.y * 0.5);
    let bg = Rect::from_min_size(pos, galley_size).expand2(Vec2::new(6.0, 3.0));
    // Fully opaque — see the comment on the lane-name gutter tag's identical fix.
    p.rect_filled(bg, CornerRadius::same(4), theme.canvas_bg);
    let galley = p.layout_no_wrap(bio.name.clone(), font, color);
    p.galley(pos, galley, theme.text);
    hits.push(Hit {
        rect: bg,
        sel: Selection::Biography(bio.id),
    });
}
