//! Geometry and visibility rules for the canvas.
//!
//! Deliberately free of any painting so the tricky parts — the BC/AD axis
//! mapping, the converging-band curves, and the zoom/importance rules — can be
//! reasoned about and tested without a window on screen.

use crate::model::*;
use std::collections::HashMap;

// --- Tunables ---------------------------------------------------------------

/// Thickness of a culture band.
pub const BAND_THICKNESS: f32 = 16.0;
/// Thickness of a biography's lifeline.
pub const BIO_BAND_THICKNESS: f32 = 8.0;
/// Horizontal length, in pixels, of a merge/split curve. Expressed in pixels so
/// the curve keeps the same shape at every zoom level.
pub const TRANSITION_PX: f64 = 110.0;
/// Vertical gap left above a band for its event labels.
pub const LABEL_BAND_TOP: f32 = 6.0;

/// Zoom limits: from ~4000 years across a 1000px viewport, in to a single year.
pub const MIN_PPY: f64 = 0.02;
pub const MAX_PPY: f64 = 4000.0;

// --- Time axis --------------------------------------------------------------

/// Maps continuous-axis years to screen x and back.
#[derive(Clone, Copy, Debug)]
pub struct TimeAxis {
    /// Screen x of `left_year`.
    pub x0: f32,
    pub left_year: f64,
    /// Zoom: screen pixels per year.
    pub ppy: f64,
}

impl TimeAxis {
    pub fn new(x0: f32, left_year: f64, ppy: f64) -> Self {
        Self {
            x0,
            left_year,
            ppy: ppy.clamp(MIN_PPY, MAX_PPY),
        }
    }

    pub fn x(&self, t: f64) -> f32 {
        (self.x0 as f64 + (t - self.left_year) * self.ppy) as f32
    }

    pub fn t(&self, x: f32) -> f64 {
        self.left_year + (x as f64 - self.x0 as f64) / self.ppy
    }

    /// Year range covered by `[x0, x0 + width]`.
    pub fn visible_range(&self, width: f32) -> (f64, f64) {
        (self.left_year, self.t(self.x0 + width))
    }

    /// Zoom by `factor`, keeping the year under `pivot_x` pinned in place.
    pub fn zoom_about(&mut self, pivot_x: f32, factor: f64) {
        let anchor = self.t(pivot_x);
        let new_ppy = (self.ppy * factor).clamp(MIN_PPY, MAX_PPY);
        if new_ppy == self.ppy {
            return;
        }
        self.ppy = new_ppy;
        // Re-derive left_year so `anchor` still lands on `pivot_x`.
        self.left_year = anchor - (pivot_x as f64 - self.x0 as f64) / self.ppy;
    }
}

// --- Axis ticks -------------------------------------------------------------

/// A "nice" tick step in years for the current zoom, plus whether months are
/// worth labelling.
pub fn tick_step(ppy: f64) -> f64 {
    const STEPS: [f64; 16] = [
        1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0,
        5000.0, 10000.0,
    ];
    // Aim for a label roughly every 110 px.
    let target_years = 110.0 / ppy;
    for s in STEPS {
        if s >= target_years {
            return s;
        }
    }
    *STEPS.last().unwrap()
}

/// Tick positions covering `[from, to]`, aligned to whole multiples of the step.
pub fn ticks(from: f64, to: f64, step: f64) -> Vec<f64> {
    if step <= 0.0 || !from.is_finite() || !to.is_finite() {
        return Vec::new();
    }
    let first = (from / step).floor() * step;
    let mut out = Vec::new();
    let mut t = first;
    // Bounded so a pathological zoom can never spin here.
    while t <= to && out.len() < 4096 {
        out.push(t);
        t += step;
    }
    out
}

// --- Visibility -------------------------------------------------------------

/// Minimum importance that survives at this zoom level.
///
/// Zoomed far out only epochal entries show; zooming in progressively reveals
/// finer detail. The user's detail slider biases this in either direction.
pub fn importance_threshold(ppy: f64, detail_bias: i32) -> u8 {
    let base: i32 = if ppy < 0.35 {
        5
    } else if ppy < 1.5 {
        4
    } else if ppy < 6.0 {
        3
    } else if ppy < 30.0 {
        2
    } else {
        1
    };
    (base - detail_bias).clamp(IMPORTANCE_MIN as i32, IMPORTANCE_MAX as i32) as u8
}

/// Does this set of categories pass the include/exclude filter?
pub fn passes_category_filter(categories: &[Id], filters: &Filters) -> bool {
    if filters.mode == FilterMode::Off || filters.selected.is_empty() {
        return true;
    }
    if categories.is_empty() {
        return filters.keep_uncategorised;
    }
    let hit = categories.iter().any(|c| filters.selected.contains(c));
    match filters.mode {
        FilterMode::Include => hit,
        FilterMode::Exclude => !hit,
        FilterMode::Off => true,
    }
}

fn matches_search(haystack: &[&str], needle: &str) -> bool {
    if needle.trim().is_empty() {
        return true;
    }
    let needle = needle.to_lowercase();
    haystack.iter().any(|h| h.to_lowercase().contains(&needle))
}

/// Should an event be drawn, given zoom, filters and search?
pub fn event_visible(event: &Event, filters: &Filters, ppy: f64) -> bool {
    // A search is an explicit request, so it overrides the zoom threshold —
    // otherwise searching for a minor event while zoomed out finds nothing.
    let searching = !filters.search.trim().is_empty();
    if !searching && event.importance < importance_threshold(ppy, filters.detail_bias) {
        return false;
    }
    if !passes_category_filter(&event.categories, filters) {
        return false;
    }
    matches_search(&[&event.title, &event.description], &filters.search)
}

/// Should a biography's lane be drawn at all?
pub fn biography_visible(bio: &Biography, filters: &Filters) -> bool {
    if bio.display == BioDisplay::Hidden {
        return false;
    }
    if !passes_category_filter(&bio.categories, filters) {
        return false;
    }
    // A lane stays open while searching even if the name itself does not match,
    // so that matching events inside it remain reachable.
    true
}

// --- Lanes ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneKind {
    Timeline(Id),
    Biography(Id),
    /// A super-category row. Collapsed, it stands in for all its members.
    Group(Id),
}

/// A lane before it has been given a vertical position.
///
/// Laying out in two phases lets the canvas measure how many rows of labels a
/// lane actually needs at the current zoom, then size the lane to fit them.
/// That matters for the dense case: a tragedian's plays lined up against the
/// events of a war, at single-year resolution.
#[derive(Clone, Debug)]
pub struct LanePlan {
    pub kind: LaneKind,
    pub color: Rgb,
    pub name: String,
    /// Indent level, from group nesting.
    pub depth: usize,
    pub thickness: f32,
    pub min_rows: usize,
    /// Expanded group: a heading only, with no band and no events of its own.
    pub header_only: bool,
}

/// A positioned lane.
#[derive(Clone, Debug)]
pub struct Lane {
    pub kind: LaneKind,
    pub color: Rgb,
    pub name: String,
    pub depth: usize,
    pub thickness: f32,
    pub header_only: bool,
    /// False when nothing on this lane falls inside the visible window.
    pub active: bool,
    /// Vertical centre of the band.
    pub center: f32,
    pub top: f32,
    pub bottom: f32,
    /// Rows of label space reserved above the band.
    pub label_rows: usize,
}

impl Lane {
    pub fn is_nested(&self) -> bool {
        self.depth > 0
    }
}

/// What a lane needs at the current zoom, measured before placement.
#[derive(Clone, Copy, Debug, Default)]
pub struct LaneDemand {
    /// Rows of labels the lane wants.
    pub rows: usize,
    /// Whether the lane has anything at all in the visible window.
    pub active: bool,
    /// Deepest chain of visible events nested inside one of this lane's
    /// events, e.g. an event inside an event inside a range event is 2.
    pub nested_rows: usize,
}

/// Height of a lane with nothing in the current window. Kept visible but slim,
/// so zooming into one century does not mean scrolling past empty lanes.
pub const DORMANT_LANE_HEIGHT: f32 = 24.0;

/// Does this lane have a band or any visible events inside the window?
pub fn lane_active(
    doc: &Document,
    kind: LaneKind,
    filters: &Filters,
    ppy: f64,
    view_from: f64,
    view_to: f64,
) -> bool {
    let overlaps = |lo: f64, hi: f64| hi >= view_from && lo <= view_to;

    let band = match kind {
        LaneKind::Timeline(id) => doc
            .timeline(id)
            .and_then(|t| timeline_band_range(doc, t))
            .is_some_and(|(lo, hi)| overlaps(lo, hi)),
        LaneKind::Biography(id) => doc.biography(id).is_some_and(|b| {
            let s = b.span();
            overlaps(s.t0(), s.t1())
        }),
        LaneKind::Group(id) => doc.group_timelines(id).iter().any(|t| {
            doc.timeline(*t)
                .and_then(|tl| timeline_band_range(doc, tl))
                .is_some_and(|(lo, hi)| overlaps(lo, hi))
        }),
    };
    if band {
        return true;
    }
    // An event outside the band still counts, e.g. a posthumous publication.
    let owners = lane_owners(doc, kind);
    doc.events
        .iter()
        .filter(|e| owners.contains(&e.owner))
        .any(|e| overlaps(e.span.t0(), e.span.t1()) && event_visible(e, filters, ppy))
}

/// Height of one row of event labels.
///
/// Must clear the tallest label line, or a high-importance title overflows into
/// the row beneath it. See the guard test in `theme`.
pub const LABEL_ROW_HEIGHT: f32 = 20.0;
/// Padding under a band before the next lane starts.
pub const LANE_BOTTOM_PAD: f32 = 10.0;
/// Never grow a lane beyond this many label rows, however dense the data.
pub const MAX_LABEL_ROWS: usize = 14;
/// Height of one row of nested events, stacked below the band.
pub const NESTED_ROW_HEIGHT: f32 = 15.0;
/// However deep a hand-edited file nests events, never reserve room for more
/// than this many rows — a chain that long is unreadable anyway.
pub const MAX_NESTED_ROWS: usize = 4;

/// Deepest chain of *visible* events nested inside `parent`, or 0 if it has
/// none. Bounded so a corrupt parent cycle cannot hang the UI.
pub fn nested_depth(doc: &Document, filters: &Filters, ppy: f64, parent: Id, guard: usize) -> usize {
    if guard > 64 {
        return 0;
    }
    doc.child_events(parent)
        .into_iter()
        .filter(|e| event_visible(e, filters, ppy))
        .map(|e| 1 + nested_depth(doc, filters, ppy, e.id, guard + 1))
        .max()
        .unwrap_or(0)
}

/// Height a lane needs for the given demand.
pub fn lane_height(plan: &LanePlan, demand: LaneDemand) -> f32 {
    if plan.header_only {
        return 26.0;
    }
    if !demand.active {
        return DORMANT_LANE_HEIGHT;
    }
    let nested = demand.nested_rows.min(MAX_NESTED_ROWS) as f32 * NESTED_ROW_HEIGHT;
    demand.rows as f32 * LABEL_ROW_HEIGHT + LABEL_BAND_TOP + plan.thickness + nested + LANE_BOTTOM_PAD
}

/// Which entities' events belong on a lane.
pub fn lane_owners(doc: &Document, kind: LaneKind) -> Vec<OwnerRef> {
    match kind {
        LaneKind::Timeline(id) => vec![OwnerRef::Timeline(id)],
        LaneKind::Biography(id) => vec![OwnerRef::Biography(id)],
        LaneKind::Group(id) => {
            // A collapsed group stands in for everything beneath it, so it
            // shows its members' events rather than going blank.
            let timelines = doc.group_timelines(id);
            let mut out: Vec<OwnerRef> = timelines.iter().map(|t| OwnerRef::Timeline(*t)).collect();
            for b in &doc.biographies {
                if b.timeline.is_some_and(|t| timelines.contains(&t)) {
                    out.push(OwnerRef::Biography(b.id));
                }
            }
            out
        }
    }
}

/// Build the ordered lane plan: groups (collapsed or expanded), the timelines
/// within them, each timeline's inline biographies, then biographies promoted
/// to their own lanes.
pub fn plan_lanes(doc: &Document, filters: &Filters) -> Vec<LanePlan> {
    let mut out = Vec::new();
    plan_group(doc, None, 0, filters, &mut out, &mut 0);

    let mut own: Vec<&Biography> = doc
        .biographies
        .iter()
        .filter(|b| b.display == BioDisplay::Lane && biography_visible(b, filters))
        .collect();
    own.sort_by(|a, b| {
        a.birth
            .decimal()
            .partial_cmp(&b.birth.decimal())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for bio in own {
        out.push(LanePlan {
            kind: LaneKind::Biography(bio.id),
            color: doc.bio_color(bio),
            name: bio.name.clone(),
            depth: 0,
            thickness: BIO_BAND_THICKNESS,
            min_rows: 1,
            header_only: false,
        });
    }
    out
}

/// Recursive walk of the group tree. `guard` bounds the recursion so a
/// corrupted parent cycle cannot hang the UI.
fn plan_group(
    doc: &Document,
    parent: Option<Id>,
    depth: usize,
    filters: &Filters,
    out: &mut Vec<LanePlan>,
    guard: &mut usize,
) {
    *guard += 1;
    if *guard > 512 || depth > 12 {
        return;
    }

    for g in doc.child_groups(parent) {
        if !g.visible {
            continue;
        }
        out.push(LanePlan {
            kind: LaneKind::Group(g.id),
            color: g.color,
            name: g.name.clone(),
            depth,
            thickness: BAND_THICKNESS,
            min_rows: if g.collapsed { 2 } else { 0 },
            header_only: !g.collapsed,
        });
        if !g.collapsed {
            plan_group(doc, Some(g.id), depth + 1, filters, out, guard);
        }
    }

    for tl in doc.timelines_in(parent) {
        if !tl.visible {
            continue;
        }
        out.push(LanePlan {
            kind: LaneKind::Timeline(tl.id),
            color: tl.color,
            name: tl.name.clone(),
            depth,
            thickness: BAND_THICKNESS,
            min_rows: 2,
            header_only: false,
        });

        let mut inline: Vec<&Biography> = doc
            .biographies
            .iter()
            .filter(|b| {
                b.timeline == Some(tl.id)
                    && b.display == BioDisplay::Inline
                    && biography_visible(b, filters)
            })
            .collect();
        inline.sort_by(|a, b| {
            a.birth
                .decimal()
                .partial_cmp(&b.birth.decimal())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for bio in inline {
            out.push(LanePlan {
                kind: LaneKind::Biography(bio.id),
                color: doc.bio_color(bio),
                name: bio.name.clone(),
                depth: depth + 1,
                thickness: BIO_BAND_THICKNESS,
                min_rows: 1,
                header_only: false,
            });
        }
    }
}

/// Stack the plans vertically according to their measured demand: lanes grow to
/// fit their labels (within `MAX_LABEL_ROWS`) and shrink to a slim row when they
/// have nothing in the visible window.
pub fn place_lanes(plans: &[LanePlan], demands: &[LaneDemand], top: f32) -> Vec<Lane> {
    let mut y = top;
    let mut lanes = Vec::with_capacity(plans.len());
    for (i, plan) in plans.iter().enumerate() {
        let mut demand = demands.get(i).copied().unwrap_or_default();
        demand.rows = if demand.rows == 0 {
            0
        } else {
            demand.rows.max(plan.min_rows).min(MAX_LABEL_ROWS)
        };
        let h = lane_height(plan, demand);
        let label_rows = if demand.active { demand.rows } else { 0 };
        let center = if plan.header_only || !demand.active {
            y + h * 0.5
        } else {
            y + label_rows as f32 * LABEL_ROW_HEIGHT + LABEL_BAND_TOP + plan.thickness * 0.5
        };
        lanes.push(Lane {
            kind: plan.kind,
            color: plan.color,
            name: plan.name.clone(),
            depth: plan.depth,
            thickness: plan.thickness,
            header_only: plan.header_only,
            active: demand.active,
            center,
            top: y,
            bottom: y + h,
            label_rows,
        });
        y += h;
    }
    lanes
}

/// Total height the lane stack occupies.
pub fn lanes_height(lanes: &[Lane], top: f32) -> f32 {
    lanes.last().map(|l| l.bottom - top).unwrap_or(0.0)
}

/// Lane centres keyed by timeline id, for resolving junction targets.
pub fn timeline_centers(lanes: &[Lane]) -> HashMap<Id, f32> {
    lanes
        .iter()
        .filter_map(|l| match l.kind {
            LaneKind::Timeline(id) => Some((id, l.center)),
            _ => None,
        })
        .collect()
}

// --- Band geometry ----------------------------------------------------------

/// The span a timeline's band actually covers, honouring junctions and falling
/// back to the extent of its own events when the user hasn't set one.
pub fn timeline_band_range(doc: &Document, tl: &Timeline) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = match tl.span {
        Some(s) => (s.t0(), s.t1()),
        None => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for e in doc.events_of(OwnerRef::Timeline(tl.id)) {
                lo = lo.min(e.span.t0());
                hi = hi.max(e.span.t1());
            }
            // Biographies belonging to this culture also anchor its band.
            for b in doc.biographies.iter().filter(|b| b.timeline == Some(tl.id)) {
                lo = lo.min(b.span().t0());
                hi = hi.max(b.span().t1());
            }
            if !lo.is_finite() || !hi.is_finite() {
                return None;
            }
            // A little breathing room so markers aren't flush with the band end.
            let pad = ((hi - lo) * 0.04).max(1.0);
            (lo - pad, hi + pad)
        }
    };

    if let Some(j) = &tl.origin {
        lo = j.date.decimal();
    }
    if let Some(j) = &tl.merge {
        hi = j.date.decimal();
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    Some((lo, hi))
}

/// Smooth S-curve, so merges ease in and out rather than kinking.
fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Vertical position of a timeline's band at time `t`.
///
/// This is what makes convergence a first-class visual: within a transition
/// window before a merge, the band eases from its own lane onto the lane of the
/// timeline it is being absorbed into, and symmetrically after a split.
pub fn band_center_at(
    tl: &Timeline,
    own_center: f32,
    centers: &HashMap<Id, f32>,
    t: f64,
    ppy: f64,
) -> f32 {
    let window = (TRANSITION_PX / ppy).max(f64::MIN_POSITIVE);
    let mut y = own_center as f64;

    // Splitting off from a parent: start on the parent's lane, ease to our own.
    if let Some(j) = &tl.origin {
        if let Some(&parent_y) = centers.get(&j.other) {
            let d = j.date.decimal();
            if t <= d {
                y = parent_y as f64;
            } else if t < d + window {
                let k = smoothstep((t - d) / window);
                y = parent_y as f64 + (own_center as f64 - parent_y as f64) * k;
            }
        }
    }

    // Merging into another timeline: ease onto its lane and end there.
    if let Some(j) = &tl.merge {
        if let Some(&target_y) = centers.get(&j.other) {
            let d = j.date.decimal();
            if t >= d {
                y = target_y as f64;
            } else if t > d - window {
                let k = smoothstep((d - t) / window);
                // k == 1 far from the merge (keep current y), 0 at the merge.
                y = target_y as f64 + (y - target_y as f64) * k;
            }
        }
    }

    y as f32
}

/// The portion of a timeline's band range that actually falls in view, or
/// `None` if it is scrolled off entirely.
pub fn band_visible_range(
    doc: &Document,
    tl: &Timeline,
    view_from: f64,
    view_to: f64,
) -> Option<(f64, f64)> {
    let (lo, hi) = timeline_band_range(doc, tl)?;
    let from = lo.max(view_from);
    let to = hi.min(view_to);
    (to > from).then_some((from, to))
}

/// Sample a band's centre line across an arbitrary sub-range `[from, to]`.
///
/// Returns screen-space points. Sampling is per-pixel-ish rather than analytic
/// so the same code handles the straight sections and the curves. Used both
/// for the whole band and for painting one coloured epoch segment within it.
pub fn band_curve(
    tl: &Timeline,
    own_center: f32,
    centers: &HashMap<Id, f32>,
    axis: &TimeAxis,
    from: f64,
    to: f64,
) -> Vec<(f32, f32)> {
    if to <= from {
        return Vec::new();
    }

    // Straight sections need only their endpoints; curves need real sampling.
    let mut cuts: Vec<f64> = vec![from, to];
    let window = TRANSITION_PX / axis.ppy;
    for (date, start) in [
        (tl.origin.as_ref().map(|j| j.date.decimal()), true),
        (tl.merge.as_ref().map(|j| j.date.decimal()), false),
    ] {
        let Some(d) = date else { continue };
        let (a, b) = if start { (d, d + window) } else { (d - window, d) };
        let steps = 24;
        for i in 0..=steps {
            let t = a + (b - a) * (i as f64 / steps as f64);
            if t > from && t < to {
                cuts.push(t);
            }
        }
    }
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cuts.dedup();

    cuts.iter()
        .map(|&t| {
            (
                axis.x(t),
                band_center_at(tl, own_center, centers, t, axis.ppy),
            )
        })
        .collect()
}

/// Sample a band's centre line across the visible range.
///
/// Returns screen-space points. Sampling is per-pixel-ish rather than analytic
/// so the same code handles the straight sections and the curves.
pub fn band_polyline(
    doc: &Document,
    tl: &Timeline,
    own_center: f32,
    centers: &HashMap<Id, f32>,
    axis: &TimeAxis,
    view_from: f64,
    view_to: f64,
) -> Vec<(f32, f32)> {
    let Some((from, to)) = band_visible_range(doc, tl, view_from, view_to) else {
        return Vec::new();
    };
    band_curve(tl, own_center, centers, axis, from, to)
}

/// Coloured sub-ranges to paint along a band within `[from, to]`: the
/// timeline's own epochs, with every gap between/around them filled by its
/// base colour so the whole range is always covered by exactly one colour.
///
/// Epochs need not be sorted or disjoint on input. Where two overlap, the
/// later-starting one wins: an earlier epoch's painted end is capped at the
/// next one's start, regardless of its own configured end date. That makes
/// the common case — entering each era's start date and a rough, possibly
/// stale end date — behave the way it reads: "Classical starts in 500 BC"
/// unambiguously ends the Archaic era there too.
pub fn band_color_segments(tl: &Timeline, from: f64, to: f64) -> Vec<(f64, f64, Rgb)> {
    if to <= from {
        return Vec::new();
    }
    let mut epochs: Vec<&Epoch> = tl.epochs.iter().collect();
    epochs.sort_by(|a, b| a.t0().partial_cmp(&b.t0()).unwrap_or(std::cmp::Ordering::Equal));

    let mut segments = Vec::new();
    let mut cursor = from;
    for (i, e) in epochs.iter().enumerate() {
        let mut e1 = e.t1().min(to);
        if let Some(next) = epochs.get(i + 1) {
            e1 = e1.min(next.t0());
        }
        let e0 = e.t0().max(cursor);
        if e0 >= to || e1 <= e0 {
            continue; // Outside the range, or fully swallowed by a neighbour.
        }
        if e0 > cursor {
            segments.push((cursor, e0, tl.color));
        }
        segments.push((e0, e1, e.color));
        cursor = e1;
    }
    if cursor < to {
        segments.push((cursor, to, tl.color));
    }
    segments
}

// --- Label placement --------------------------------------------------------

/// Greedy non-overlapping label placer, scoped to one lane.
///
/// Labels are offered slots at increasing distance from the band; the first
/// free slot wins. Callers place the most important entries first so that when
/// space runs out it is the minor entries that lose their label.
#[derive(Default)]
pub struct LabelPacker {
    /// Occupied (x_min, x_max, row) boxes.
    rows: Vec<Vec<(f32, f32)>>,
}

impl LabelPacker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim a row for a label spanning `[x_min, x_max]`.
    ///
    /// Returns the row index, or `None` if every allowed row is taken.
    pub fn place(&mut self, x_min: f32, x_max: f32, max_rows: usize) -> Option<usize> {
        let pad = 6.0;
        for row in 0..max_rows {
            if self.rows.len() <= row {
                self.rows.push(Vec::new());
            }
            let free = self.rows[row]
                .iter()
                .all(|(a, b)| x_max + pad < *a || x_min - pad > *b);
            if free {
                self.rows[row].push((x_min, x_max));
                return Some(row);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis() -> TimeAxis {
        TimeAxis::new(100.0, -300.0, 2.0)
    }

    #[test]
    fn axis_maps_years_to_pixels_and_back() {
        let a = axis();
        assert_eq!(a.x(-300.0), 100.0);
        assert_eq!(a.x(-200.0), 300.0);
        let t = a.t(300.0);
        assert!((t - -200.0).abs() < 1e-9);
    }

    #[test]
    fn zooming_keeps_the_year_under_the_cursor_pinned() {
        let mut a = axis();
        let pivot = 437.0;
        let before = a.t(pivot);
        a.zoom_about(pivot, 3.0);
        let after = a.t(pivot);
        assert!(
            (before - after).abs() < 1e-6,
            "anchor drifted: {before} -> {after}"
        );
        assert!(a.ppy > 2.0);
    }

    #[test]
    fn zoom_is_clamped_at_both_ends() {
        let mut a = axis();
        for _ in 0..200 {
            a.zoom_about(300.0, 2.0);
        }
        assert!(a.ppy <= MAX_PPY);
        for _ in 0..400 {
            a.zoom_about(300.0, 0.5);
        }
        assert!(a.ppy >= MIN_PPY);
    }

    #[test]
    fn zoom_at_the_clamp_does_not_shift_the_view() {
        let mut a = TimeAxis::new(0.0, -300.0, MAX_PPY);
        let before = a.left_year;
        a.zoom_about(500.0, 2.0);
        assert_eq!(a.left_year, before);
    }

    #[test]
    fn tick_steps_grow_as_you_zoom_out() {
        assert!(tick_step(0.05) > tick_step(1.0));
        assert!(tick_step(1.0) >= tick_step(100.0));
        assert!(tick_step(0.0001) <= 10000.0);
    }

    #[test]
    fn ticks_align_to_whole_multiples_and_terminate() {
        let t = ticks(-317.0, -200.0, 50.0);
        assert!(t.contains(&-300.0) && t.contains(&-250.0));
        assert!(t.iter().all(|v| (v / 50.0).fract().abs() < 1e-9));
        assert!(ticks(0.0, 1e12, 1.0).len() <= 4096, "must stay bounded");
    }

    #[test]
    fn zooming_out_hides_all_but_the_most_important() {
        assert_eq!(importance_threshold(0.1, 0), 5);
        assert_eq!(importance_threshold(100.0, 0), 1);
        assert!(importance_threshold(0.1, 0) > importance_threshold(10.0, 0));
    }

    #[test]
    fn detail_bias_shifts_the_threshold_within_bounds() {
        assert_eq!(importance_threshold(0.1, 2), 3);
        assert_eq!(importance_threshold(0.1, 99), IMPORTANCE_MIN);
        assert_eq!(importance_threshold(1000.0, -99), IMPORTANCE_MAX);
    }

    fn cat(n: u32) -> Id {
        Id(n)
    }

    #[test]
    fn include_filter_shows_only_selected_categories() {
        let mut f = Filters {
            mode: FilterMode::Include,
            ..Default::default()
        };
        f.selected.insert(cat(1));
        assert!(passes_category_filter(&[cat(1)], &f));
        assert!(passes_category_filter(&[cat(1), cat(2)], &f));
        assert!(!passes_category_filter(&[cat(2)], &f));
    }

    #[test]
    fn exclude_filter_hides_selected_categories() {
        let mut f = Filters {
            mode: FilterMode::Exclude,
            ..Default::default()
        };
        f.selected.insert(cat(1));
        assert!(!passes_category_filter(&[cat(1)], &f));
        // Multi-category entries are hidden if *any* category is excluded.
        assert!(!passes_category_filter(&[cat(1), cat(2)], &f));
        assert!(passes_category_filter(&[cat(2)], &f));
    }

    #[test]
    fn uncategorised_entries_follow_the_keep_flag() {
        let mut f = Filters {
            mode: FilterMode::Include,
            keep_uncategorised: true,
            ..Default::default()
        };
        f.selected.insert(cat(1));
        assert!(passes_category_filter(&[], &f));
        f.keep_uncategorised = false;
        assert!(!passes_category_filter(&[], &f));
    }

    #[test]
    fn an_empty_selection_filters_nothing() {
        let f = Filters {
            mode: FilterMode::Include,
            ..Default::default()
        };
        assert!(passes_category_filter(&[cat(9)], &f));
    }

    fn make_event(importance: u8, title: &str) -> Event {
        Event {
            id: Id(1),
            owner: OwnerRef::Timeline(Id(1)),
            title: title.into(),
            description: String::new(),
            span: Span::point(HDate::year(-44)),
            importance,
            categories: vec![],
            parent: None,
        }
    }

    #[test]
    fn search_reveals_entries_the_zoom_threshold_would_hide() {
        let e = make_event(1, "Ides of March");
        let zoomed_out = 0.1;
        let mut f = Filters::default();
        assert!(!event_visible(&e, &f, zoomed_out));
        f.search = "ides".into();
        assert!(
            event_visible(&e, &f, zoomed_out),
            "an explicit search should override the zoom threshold"
        );
    }

    #[test]
    fn search_is_case_insensitive_and_matches_description() {
        let mut e = make_event(5, "Battle");
        e.description = "Fought near Cannae".into();
        let f = Filters {
            search: "CANNAE".into(),
            ..Default::default()
        };
        assert!(event_visible(&e, &f, 10.0));
    }

    // --- Band geometry -----------------------------------------------------

    fn merging_doc() -> (Document, HashMap<Id, f32>) {
        let mut doc = Document::default();
        let rome = doc.new_id();
        let macedon = doc.new_id();
        doc.timelines.push(Timeline {
            id: rome,
            name: "Rome".into(),
            color: [200, 90, 80],
            visible: true,
            group: None,
            order: 0,
            span: Some(Span::range(HDate::year(-509), HDate::year(-27))),
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });
        doc.timelines.push(Timeline {
            id: macedon,
            name: "Macedon".into(),
            color: [80, 140, 200],
            visible: true,
            group: None,
            order: 1,
            span: Some(Span::range(HDate::year(-306), HDate::year(-168))),
            origin: None,
            merge: Some(Junction {
                other: rome,
                date: HDate::year(-168),
                label: "Battle of Pydna".into(),
            }),
            notes: String::new(),
            epochs: Vec::new(),
        });
        let centers = HashMap::from([(rome, 100.0f32), (macedon, 200.0f32)]);
        (doc, centers)
    }

    #[test]
    fn a_merging_band_converges_onto_its_target_lane() {
        let (doc, centers) = merging_doc();
        let macedon = doc.timelines[1].clone();
        let own = 200.0;
        let ppy = 2.0;
        let merge_t = -168.0;

        // Far from the merge it sits on its own lane.
        let far = band_center_at(&macedon, own, &centers, merge_t - 500.0, ppy);
        assert!((far - 200.0).abs() < 0.01);

        // At the merge it has arrived on Rome's lane.
        let at = band_center_at(&macedon, own, &centers, merge_t, ppy);
        assert!((at - 100.0).abs() < 0.01, "expected to land on Rome, got {at}");

        // Halfway through the window it is strictly between the two.
        let window = TRANSITION_PX / ppy;
        let mid = band_center_at(&macedon, own, &centers, merge_t - window * 0.5, ppy);
        assert!(mid > 100.0 && mid < 200.0, "expected a curve, got {mid}");
    }

    #[test]
    fn convergence_is_monotonic_through_the_window() {
        let (doc, centers) = merging_doc();
        let macedon = doc.timelines[1].clone();
        let ppy = 2.0;
        let window = TRANSITION_PX / ppy;
        let merge_t = -168.0;
        let mut prev = f32::INFINITY;
        for i in 0..=20 {
            let t = merge_t - window + window * (i as f64 / 20.0);
            let y = band_center_at(&macedon, 200.0, &centers, t, ppy);
            assert!(y <= prev + 1e-3, "band should descend steadily, got {y} after {prev}");
            prev = y;
        }
    }

    #[test]
    fn a_splitting_band_diverges_from_its_parent() {
        let mut doc = Document::default();
        let parent = doc.new_id();
        let child = doc.new_id();
        doc.timelines.push(Timeline {
            id: parent,
            name: "Alexander's Empire".into(),
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
        let child_tl = Timeline {
            id: child,
            name: "Ptolemaic Egypt".into(),
            color: [0, 0, 0],
            visible: true,
            group: None,
            order: 1,
            span: Some(Span::range(HDate::year(-305), HDate::year(-30))),
            origin: Some(Junction {
                other: parent,
                date: HDate::year(-305),
                label: "Diadochi".into(),
            }),
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        };
        doc.timelines.push(child_tl.clone());
        let centers = HashMap::from([(parent, 100.0f32), (child, 220.0f32)]);
        let ppy = 2.0;
        let split_t = -305.0;

        let at = band_center_at(&child_tl, 220.0, &centers, split_t, ppy);
        assert!((at - 100.0).abs() < 0.01, "should start on the parent lane");

        let later = band_center_at(&child_tl, 220.0, &centers, split_t + 400.0, ppy);
        assert!((later - 220.0).abs() < 0.01, "should settle on its own lane");
    }

    #[test]
    fn a_junction_pointing_at_a_hidden_timeline_falls_back_to_a_straight_band() {
        let (doc, _) = merging_doc();
        let macedon = doc.timelines[1].clone();
        // Rome is not in the centre map, e.g. because it was hidden.
        let centers = HashMap::new();
        let y = band_center_at(&macedon, 200.0, &centers, -168.0, 2.0);
        assert_eq!(y, 200.0);
    }

    #[test]
    fn the_merge_curve_keeps_its_pixel_width_across_zoom_levels() {
        let (doc, centers) = merging_doc();
        let macedon = doc.timelines[1].clone();
        let merge_t = -168.0;
        for ppy in [0.5, 2.0, 40.0] {
            let window = TRANSITION_PX / ppy;
            let mid = band_center_at(&macedon, 200.0, &centers, merge_t - window * 0.5, ppy);
            assert!(
                (mid - 150.0).abs() < 1.0,
                "midpoint should be the same shape at every zoom, got {mid} at ppy={ppy}"
            );
        }
    }

    #[test]
    fn band_range_is_clipped_by_its_junctions() {
        let (doc, _) = merging_doc();
        let (_, hi) = timeline_band_range(&doc, &doc.timelines[1]).unwrap();
        assert_eq!(hi, -168.0, "band must stop where it merges");
    }

    #[test]
    fn band_range_falls_back_to_the_extent_of_its_events() {
        let mut doc = Document::default();
        let id = doc.new_id();
        doc.timelines.push(Timeline {
            id,
            name: "Carthage".into(),
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
        assert!(timeline_band_range(&doc, &doc.timelines[0]).is_none());

        let eid = doc.new_id();
        doc.events.push(Event {
            id: eid,
            owner: OwnerRef::Timeline(id),
            title: "Founded".into(),
            description: String::new(),
            span: Span::point(HDate::year(-814)),
            importance: 5,
            categories: vec![],
            parent: None,
        });
        let (lo, hi) = timeline_band_range(&doc, &doc.timelines[0]).unwrap();
        assert!(lo <= -814.0 && hi >= -813.0);
    }

    #[test]
    fn band_polyline_is_empty_when_scrolled_out_of_view() {
        let (doc, centers) = merging_doc();
        let a = TimeAxis::new(0.0, 1500.0, 2.0);
        let pts = band_polyline(&doc, &doc.timelines[1], 200.0, &centers, &a, 1500.0, 1900.0);
        assert!(pts.is_empty());
    }

    #[test]
    fn band_polyline_samples_the_curve_not_just_the_endpoints() {
        let (doc, centers) = merging_doc();
        let a = TimeAxis::new(0.0, -400.0, 2.0);
        let pts = band_polyline(&doc, &doc.timelines[1], 200.0, &centers, &a, -400.0, -100.0);
        assert!(pts.len() > 10, "curve needs sampling, got {} points", pts.len());
        // x must advance monotonically or the ribbon will self-intersect.
        assert!(pts.windows(2).all(|w| w[1].0 >= w[0].0));
    }

    // --- Epoch colour segments -----------------------------------------------

    fn timeline_with_epochs(epochs: Vec<Epoch>) -> Timeline {
        Timeline {
            id: Id(1),
            name: "Greek antiquity".into(),
            color: [10, 20, 30],
            visible: true,
            group: None,
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs,
        }
    }

    fn epoch(name: &str, color: Rgb, start: i32, end: i32) -> Epoch {
        Epoch {
            name: name.into(),
            color,
            start: HDate::year(start),
            end: HDate::year(end),
        }
    }

    #[test]
    fn no_epochs_means_one_segment_in_the_base_colour() {
        let tl = timeline_with_epochs(vec![]);
        let segs = band_color_segments(&tl, -800.0, -300.0);
        assert_eq!(segs, vec![(-800.0, -300.0, tl.color)]);
    }

    #[test]
    fn epochs_split_the_band_and_fill_the_gaps_with_the_base_colour() {
        let tl = timeline_with_epochs(vec![
            epoch("Archaic", [1, 1, 1], -800, -500),
            epoch("Classical", [2, 2, 2], -500, -323),
        ]);
        let segs = band_color_segments(&tl, -800.0, -300.0);
        // Archaic's own end (500 BC's decimal_end, -499.0) is capped at
        // Classical's start (500 BC's decimal, -500.0) — the two "500 BC"
        // boundaries a year-only span produces a year apart otherwise.
        assert_eq!(
            segs,
            vec![
                (-800.0, -500.0, [1, 1, 1]),
                (-500.0, -322.0, [2, 2, 2]),
                (-322.0, -300.0, tl.color),
            ]
        );
    }

    #[test]
    fn an_epoch_outside_the_range_is_dropped_entirely() {
        let tl = timeline_with_epochs(vec![epoch("Bronze age", [1, 1, 1], -2000, -1200)]);
        let segs = band_color_segments(&tl, -800.0, -300.0);
        assert_eq!(segs, vec![(-800.0, -300.0, tl.color)]);
    }

    #[test]
    fn overlapping_epochs_let_the_later_one_win() {
        // Epochs need not be disjoint or sorted. Archaic is deliberately set
        // to run well past Classical's own end (-200 vs Classical's -323) —
        // it must still be cut short right where Classical starts.
        let tl = timeline_with_epochs(vec![
            epoch("Classical", [2, 2, 2], -500, -323),
            epoch("Archaic", [1, 1, 1], -800, -200),
        ]);
        let segs = band_color_segments(&tl, -800.0, -300.0);
        assert_eq!(
            segs,
            vec![
                (-800.0, -500.0, [1, 1, 1]),
                (-500.0, -322.0, [2, 2, 2]),
                (-322.0, -300.0, tl.color),
            ]
        );
    }

    #[test]
    fn an_empty_range_produces_no_segments() {
        let tl = timeline_with_epochs(vec![epoch("Archaic", [1, 1, 1], -800, -500)]);
        assert!(band_color_segments(&tl, -300.0, -300.0).is_empty());
        assert!(band_color_segments(&tl, -300.0, -400.0).is_empty());
    }

    // --- Nested events -------------------------------------------------------

    fn nested_event(id: Id, parent: Option<Id>, importance: u8) -> Event {
        Event {
            id,
            owner: OwnerRef::Timeline(Id(1)),
            title: "e".into(),
            description: String::new(),
            span: Span::point(HDate::year(-400)),
            importance,
            categories: vec![],
            parent,
        }
    }

    #[test]
    fn an_event_with_no_children_has_zero_nested_depth() {
        let mut doc = Document::default();
        doc.events.push(nested_event(Id(1), None, 3));
        assert_eq!(nested_depth(&doc, &Filters::default(), 2.0, Id(1), 0), 0);
    }

    #[test]
    fn nested_depth_counts_the_longest_chain() {
        let mut doc = Document::default();
        doc.events.push(nested_event(Id(1), None, 3));
        doc.events.push(nested_event(Id(2), Some(Id(1)), 3));
        doc.events.push(nested_event(Id(3), Some(Id(2)), 3));
        assert_eq!(nested_depth(&doc, &Filters::default(), 2.0, Id(1), 0), 2);
    }

    #[test]
    fn nested_depth_ignores_children_hidden_by_filters_or_zoom() {
        let mut doc = Document::default();
        doc.events.push(nested_event(Id(1), None, 3));
        // Importance 1 needs to be zoomed in a long way to survive the
        // zoom-dependent importance threshold.
        doc.events.push(nested_event(Id(2), Some(Id(1)), 1));
        assert_eq!(nested_depth(&doc, &Filters::default(), 0.1, Id(1), 0), 0);
        assert_eq!(nested_depth(&doc, &Filters::default(), 50.0, Id(1), 0), 1);
    }

    // --- Lanes -------------------------------------------------------------

    /// Plan + place with no measured label demand, i.e. minimum lane sizes.
    fn build_lanes(doc: &Document, top: f32, filters: &Filters) -> Vec<Lane> {
        let plans = plan_lanes(doc, filters);
        let demands = vec![
            LaneDemand {
                rows: 0,
                active: true,
                nested_rows: 0,
            };
            plans.len()
        ];
        place_lanes(&plans, &demands, top)
    }

    fn demands(n: usize, rows: usize) -> Vec<LaneDemand> {
        vec![LaneDemand { rows, active: true, nested_rows: 0 }; n]
    }

    fn lane_doc() -> Document {
        let mut doc = Document::default();
        let t1 = doc.new_id();
        let t2 = doc.new_id();
        for (i, id) in [t1, t2].into_iter().enumerate() {
            doc.timelines.push(Timeline {
                id,
                name: format!("T{i}"),
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
        let b_inline = doc.new_id();
        doc.biographies.push(Biography {
            id: b_inline,
            name: "Cicero".into(),
            timeline: Some(t1),
            birth: HDate::year(-106),
            death: Some(HDate::year(-43)),
            color: None,
            categories: vec![],
            importance: 4,
            display: BioDisplay::Inline,
            notes: String::new(),
        });
        let b_lane = doc.new_id();
        doc.biographies.push(Biography {
            id: b_lane,
            name: "Caesar".into(),
            timeline: Some(t1),
            birth: HDate::year(-100),
            death: Some(HDate::year(-44)),
            color: None,
            categories: vec![],
            importance: 5,
            display: BioDisplay::Lane,
            notes: String::new(),
        });
        let b_hidden = doc.new_id();
        doc.biographies.push(Biography {
            id: b_hidden,
            name: "Hidden".into(),
            timeline: Some(t1),
            birth: HDate::year(-90),
            death: None,
            color: None,
            categories: vec![],
            importance: 2,
            display: BioDisplay::Hidden,
            notes: String::new(),
        });
        doc
    }

    #[test]
    fn inline_biographies_nest_under_their_parent_culture() {
        let doc = lane_doc();
        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        let names: Vec<&str> = lanes.iter().map(|l| l.name.as_str()).collect();
        // Cicero is inline under T0; Caesar has his own lane after all cultures.
        assert_eq!(names, vec!["T0", "Cicero", "T1", "Caesar"]);
        assert!(lanes[1].is_nested());
        assert!(!lanes[3].is_nested());
    }

    #[test]
    fn hidden_biographies_get_no_lane() {
        let doc = lane_doc();
        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        assert!(!lanes.iter().any(|l| l.name == "Hidden"));
    }

    #[test]
    fn lanes_do_not_overlap_and_stack_downwards() {
        let doc = lane_doc();
        let lanes = build_lanes(&doc, 10.0, &Filters::default());
        for w in lanes.windows(2) {
            assert!(w[0].bottom <= w[1].top, "lanes must not overlap");
        }
        for l in &lanes {
            assert!(l.center > l.top && l.center < l.bottom);
        }
    }

    #[test]
    fn hiding_a_timeline_removes_its_lane_and_its_nested_biographies() {
        let mut doc = lane_doc();
        doc.timelines[0].visible = false;
        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        let names: Vec<&str> = lanes.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["T1", "Caesar"]);
    }

    #[test]
    fn category_filters_apply_to_biography_lanes() {
        let mut doc = lane_doc();
        let cat = doc.new_id();
        doc.categories.push(Category {
            id: cat,
            name: "Writers".into(),
            color: [0, 0, 0],
        });
        doc.biographies[0].categories = vec![cat];
        let mut f = Filters {
            mode: FilterMode::Exclude,
            keep_uncategorised: true,
            ..Default::default()
        };
        f.selected.insert(cat);
        let lanes = build_lanes(&doc, 0.0, &f);
        assert!(!lanes.iter().any(|l| l.name == "Cicero"));
    }

    // --- Groups ------------------------------------------------------------

    /// Two groups, "Greek antiquity" containing Sparta and Athens, plus an
    /// ungrouped timeline — the shape the feature request describes.
    fn grouped_doc() -> Document {
        let mut doc = Document::default();
        let greek = doc.new_id();
        doc.groups.push(Group {
            id: greek,
            name: "Greek antiquity".into(),
            color: [10, 20, 30],
            parent: None,
            order: 0,
            collapsed: false,
            visible: true,
            notes: String::new(),
        });
        for (i, name) in ["Sparta", "Athens"].into_iter().enumerate() {
            let id = doc.new_id();
            doc.timelines.push(Timeline {
                id,
                name: name.into(),
                color: [0, 0, 0],
                visible: true,
                group: Some(greek),
                order: i as u32,
                span: Some(Span::range(HDate::year(-800), HDate::year(-300))),
                origin: None,
                merge: None,
                notes: String::new(),
                epochs: Vec::new(),
            });
        }
        let rome = doc.new_id();
        doc.timelines.push(Timeline {
            id: rome,
            name: "Rome".into(),
            color: [0, 0, 0],
            visible: true,
            group: None,
            order: 0,
            span: Some(Span::range(HDate::year(-509), HDate::year(-27))),
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });
        doc
    }

    fn names(lanes: &[Lane]) -> Vec<&str> {
        lanes.iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn an_expanded_group_heads_its_member_timelines() {
        let doc = grouped_doc();
        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        assert_eq!(names(&lanes), vec!["Greek antiquity", "Sparta", "Athens", "Rome"]);
        // The heading itself carries no band or events.
        assert!(lanes[0].header_only);
        assert_eq!(lanes[1].depth, 1, "members indent under their group");
        assert_eq!(lanes[3].depth, 0, "ungrouped timelines stay at top level");
    }

    #[test]
    fn a_collapsed_group_replaces_its_members_with_one_band() {
        let mut doc = grouped_doc();
        doc.groups[0].collapsed = true;
        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        assert_eq!(names(&lanes), vec!["Greek antiquity", "Rome"]);
        assert!(!lanes[0].header_only, "collapsed groups draw a real band");
    }

    #[test]
    fn a_collapsed_group_shows_its_members_events() {
        let mut doc = grouped_doc();
        doc.groups[0].collapsed = true;
        let sparta = doc.timelines[0].id;
        let owners = lane_owners(&doc, LaneKind::Group(doc.groups[0].id));
        assert!(
            owners.contains(&OwnerRef::Timeline(sparta)),
            "a collapsed group must stand in for its members, not go blank"
        );
    }

    #[test]
    fn hiding_a_group_hides_everything_under_it() {
        let mut doc = grouped_doc();
        doc.groups[0].visible = false;
        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        assert_eq!(names(&lanes), vec!["Rome"]);
    }

    #[test]
    fn groups_nest_to_several_levels() {
        let mut doc = grouped_doc();
        let outer = doc.new_id();
        doc.groups.push(Group {
            id: outer,
            name: "European history".into(),
            color: [1, 1, 1],
            parent: None,
            order: 0,
            collapsed: false,
            visible: true,
            notes: String::new(),
        });
        // Greek antiquity now sits inside European history.
        doc.groups[0].parent = Some(outer);
        doc.groups[0].order = 0;

        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        assert_eq!(
            names(&lanes),
            vec!["European history", "Greek antiquity", "Sparta", "Athens", "Rome"]
        );
        assert_eq!(lanes[1].depth, 1);
        assert_eq!(lanes[2].depth, 2, "Sparta is two levels deep");
    }

    #[test]
    fn group_timelines_gathers_the_whole_subtree() {
        let mut doc = grouped_doc();
        let outer = doc.new_id();
        doc.groups.push(Group {
            id: outer,
            name: "European history".into(),
            color: [1, 1, 1],
            parent: None,
            order: 0,
            collapsed: false,
            visible: true,
            notes: String::new(),
        });
        let greek = doc.groups[0].id;
        doc.groups[0].parent = Some(outer);
        assert_eq!(doc.group_timelines(outer).len(), 2);
        assert_eq!(doc.group_timelines(greek).len(), 2);
    }

    #[test]
    fn a_cyclic_group_tree_terminates_instead_of_hanging() {
        // A hand-edited file could contain this; the UI must survive it.
        let mut doc = grouped_doc();
        let a = doc.groups[0].id;
        let b = doc.new_id();
        doc.groups.push(Group {
            id: b,
            name: "Loop".into(),
            color: [0, 0, 0],
            parent: Some(a),
            order: 0,
            collapsed: false,
            visible: true,
            notes: String::new(),
        });
        doc.groups[0].parent = Some(b);

        // Neither of these may loop forever.
        let _ = doc.group_timelines(a);
        let lanes = build_lanes(&doc, 0.0, &Filters::default());
        assert!(lanes.iter().any(|l| l.name == "Rome"));
    }

    // --- Adaptive lane sizing ----------------------------------------------

    #[test]
    fn lanes_grow_to_fit_the_labels_they_are_given() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let sparse = place_lanes(&plans, &demands(plans.len(), 0), 0.0);
        let dense = place_lanes(&plans, &demands(plans.len(), 8), 0.0);
        assert!(
            dense[0].bottom - dense[0].top > sparse[0].bottom - sparse[0].top,
            "a lane with many labels must get more room, not drop them"
        );
        assert_eq!(dense[0].label_rows, 8);
    }

    #[test]
    fn lane_growth_is_capped_so_one_dense_lane_cannot_fill_the_screen() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let lanes = place_lanes(&plans, &demands(plans.len(), 9_999), 0.0);
        assert_eq!(lanes[0].label_rows, MAX_LABEL_ROWS);
    }

    #[test]
    fn a_lane_with_labels_keeps_a_minimum_of_breathing_room() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        // One label's worth of demand still gets the timeline minimum.
        let lanes = place_lanes(&plans, &demands(plans.len(), 1), 0.0);
        assert!(lanes[0].label_rows >= 2, "timelines reserve label space");
    }

    #[test]
    fn a_lane_with_no_labels_reserves_no_label_space() {
        // A band that exists in the window but has no events in it should not
        // leave a tall empty gap above itself.
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let empty = place_lanes(&plans, &demands(plans.len(), 0), 0.0);
        let labelled = place_lanes(&plans, &demands(plans.len(), 1), 0.0);
        assert_eq!(empty[0].label_rows, 0);
        assert!(empty[0].bottom - empty[0].top < labelled[0].bottom - labelled[0].top);
    }

    #[test]
    fn label_space_sits_above_the_band_in_every_lane() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let lanes = place_lanes(&plans, &demands(plans.len(), 4), 0.0);
        for l in &lanes {
            let band_top = l.center - l.thickness * 0.5;
            assert!(
                band_top - l.top >= l.label_rows as f32 * LABEL_ROW_HEIGHT,
                "lane {} does not reserve room for its labels",
                l.name
            );
            assert!(l.bottom >= l.center + l.thickness * 0.5);
        }
    }

    #[test]
    fn a_lane_with_nothing_in_the_window_is_marked_dormant() {
        // Zooming into the Peloponnesian War should not mean scrolling past
        // lanes for empires that did not exist yet.
        let (doc, _) = merging_doc();
        let filters = Filters::default();
        let macedon = LaneKind::Timeline(doc.timelines[1].id);
        // Macedon runs 306..168 BC.
        assert!(
            lane_active(&doc, macedon, &filters, 20.0, -320.0, -160.0),
            "should be active across its own lifespan"
        );
        assert!(
            !lane_active(&doc, macedon, &filters, 20.0, -450.0, -400.0),
            "should be dormant a century before it existed"
        );
    }

    #[test]
    fn a_dormant_lane_collapses_to_a_slim_row() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let awake = place_lanes(&plans, &demands(plans.len(), 3), 0.0);
        let asleep = place_lanes(
            &plans,
            &vec![LaneDemand { rows: 3, active: false, nested_rows: 0 }; plans.len()],
            0.0,
        );
        assert!(asleep[0].bottom - asleep[0].top < awake[0].bottom - awake[0].top);
        assert_eq!(asleep[0].bottom - asleep[0].top, DORMANT_LANE_HEIGHT);
        assert!(!asleep[0].active);
    }

    #[test]
    fn an_event_outside_the_band_still_wakes_its_lane() {
        // A posthumous work sits past the end of a biography's lifeline; the
        // lane must not go dormant and hide it.
        let mut doc = lane_doc();
        let bio = doc.biographies[0].id; // Cicero, died 43 BC
        let eid = doc.new_id();
        doc.events.push(Event {
            id: eid,
            owner: OwnerRef::Biography(bio),
            title: "Posthumous edition".into(),
            description: String::new(),
            span: Span::point(HDate::year(-20)),
            importance: 5,
            categories: vec![],
            parent: None,
        });
        assert!(lane_active(
            &doc,
            LaneKind::Biography(bio),
            &Filters::default(),
            20.0,
            -25.0,
            -15.0
        ));
    }

    #[test]
    fn lanes_still_stack_without_overlapping_when_sizes_vary() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let rows: Vec<LaneDemand> = (0..plans.len())
            .map(|i| LaneDemand {
                rows: i * 3,
                active: i % 2 == 0,
                nested_rows: 0,
            })
            .collect();
        let lanes = place_lanes(&plans, &rows, 5.0);
        for w in lanes.windows(2) {
            assert!(w[0].bottom <= w[1].top, "lanes must not overlap");
        }
    }

    // --- Label packing -----------------------------------------------------

    #[test]
    fn overlapping_labels_are_pushed_to_further_rows() {
        let mut p = LabelPacker::new();
        assert_eq!(p.place(0.0, 100.0, 3), Some(0));
        assert_eq!(p.place(50.0, 150.0, 3), Some(1));
        assert_eq!(p.place(60.0, 160.0, 3), Some(2));
        assert_eq!(p.place(70.0, 170.0, 3), None, "should run out of rows");
    }

    #[test]
    fn non_overlapping_labels_share_the_first_row() {
        let mut p = LabelPacker::new();
        assert_eq!(p.place(0.0, 100.0, 3), Some(0));
        assert_eq!(p.place(200.0, 300.0, 3), Some(0));
    }
}
