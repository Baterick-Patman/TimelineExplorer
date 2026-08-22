//! Geometry and visibility rules for the canvas.
//!
//! Deliberately free of any painting so the tricky parts — the BC/AD axis
//! mapping, the converging-band curves, and the zoom/importance rules — can be
//! reasoned about and tested without a window on screen.

use crate::model::*;
use std::collections::{BTreeSet, HashMap};

// --- Tunables ---------------------------------------------------------------

/// Thickness of a culture band.
pub const BAND_THICKNESS: f32 = 16.0;
/// Thickness of a biography's lifeline — deliberately closer to a nested
/// range event's own bar (`range_bar_height`) than to a full culture band,
/// so a biography reads as its own slim lifeline rather than another "band".
pub const BIO_BAND_THICKNESS: f32 = 6.0;
/// Floor for a biography lane's thickness when fully zoomed out — with many
/// stacked lifelines (a dozen Roman emperors) something has to give, and
/// shrinking is preferable to overlapping or paging through a huge list.
pub const BIO_BAND_THICKNESS_MIN: f32 = 3.0;
/// Pixels-per-year at or above which a biography lane already has its normal
/// thickness; below it, thickness eases down toward `BIO_BAND_THICKNESS_MIN`.
const BIO_ZOOM_REFERENCE_PPY: f64 = 15.0;
/// A biography pinned open — click, or Ctrl+click to pin several at once —
/// stays at this thickness regardless of zoom, so it keeps standing out.
pub const BIO_BAND_THICKNESS_ENLARGED: f32 = 20.0;

/// A biography lane's thickness at the current zoom. Eases down toward a
/// legible minimum as the view zooms out, so a crowd of biographies does not
/// turn into an unreadable wall of thin colour; a lane the user has pinned
/// open (see `TimelineApp::enlarged_biographies`) ignores zoom entirely and
/// stays large so it can be picked out even from far away.
pub fn bio_thickness(ppy: f64, enlarged: bool) -> f32 {
    if enlarged {
        return BIO_BAND_THICKNESS_ENLARGED;
    }
    let t = (ppy / BIO_ZOOM_REFERENCE_PPY).clamp(0.0, 1.0) as f32;
    BIO_BAND_THICKNESS_MIN + (BIO_BAND_THICKNESS - BIO_BAND_THICKNESS_MIN) * t
}
/// Horizontal length, in pixels, of a merge/split curve. Expressed in pixels so
/// the curve keeps the same shape at every zoom level.
pub const TRANSITION_PX: f64 = 110.0;
/// Vertical gap left above a band for its long-event slots.
pub const LABEL_BAND_TOP: f32 = 6.0;
/// Vertical gap left below a band for its plain-event labels — mirrors
/// `LABEL_BAND_TOP`.
pub const LABEL_BAND_BOTTOM: f32 = 6.0;

/// Zoom limits: from ~4000 years across a 1000px viewport, in to individual
/// days — `tick_step`'s finest step (a single day) only actually gets
/// reached once ticks 110px apart would need to be closer than a day apart,
/// which needs `ppy` upwards of ~40,000; the cap is set with headroom above
/// that so a few day-ticks are comfortably visible at once, not just barely
/// reachable at the very edge of the zoom range.
pub const MIN_PPY: f64 = 0.02;
pub const MAX_PPY: f64 = 60_000.0;

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

/// A "nice" tick step, in years (a fraction of a year for anything finer
/// than a whole year), for the current zoom.
///
/// The finest steps — days, months, a season (three months), a half-year —
/// only ever get reached at very high `ppy`, since the search always picks
/// the *smallest* step that still keeps ticks roughly `110px` apart; at
/// ordinary zoom the coarser, year-and-up steps win exactly as before.
pub fn tick_step(ppy: f64) -> f64 {
    const STEPS: [f64; 25] = [
        1.0 / 365.0,
        2.0 / 365.0,
        5.0 / 365.0,
        10.0 / 365.0,
        15.0 / 365.0,
        1.0 / 12.0,
        2.0 / 12.0,
        3.0 / 12.0,
        6.0 / 12.0,
        1.0,
        2.0,
        5.0,
        10.0,
        20.0,
        25.0,
        50.0,
        100.0,
        200.0,
        250.0,
        500.0,
        1000.0,
        2000.0,
        2500.0,
        5000.0,
        10000.0,
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

/// Below this on-screen width, a range event's own span stops being
/// meaningful to look at.
pub const RANGE_COLLAPSE_PX: f64 = 18.0;

/// Has this range event zoomed out far enough that it should collapse to a
/// point (just a marker and its label) instead of painting its own bar?
///
/// A years- or decades-long war is worth its own visible span up close, but
/// once it has shrunk to a sliver a handful of pixels wide, a bar with
/// nothing legible inside it just adds visual noise; falling back to the
/// same point-style rendering an ordinary event gets keeps it readable as
/// "this happened here", with the detail deferred to zooming back in. Nested
/// events inside a collapsed range are not painted either — there is no bar
/// left to visually hang them off, and the whole point of collapsing is that
/// this level of detail is not what the current zoom is for. Always `false`
/// for a point event, which has no span to collapse.
pub fn range_collapsed(event: &Event, ppy: f64) -> bool {
    event.span.is_range() && (event.span.t1() - event.span.t0()) * ppy < RANGE_COLLAPSE_PX
}

/// Spreads point events that share the exact same year — and have no month
/// of their own to place them within it — evenly across that year's width,
/// ordered by id (their creation order, which for an imported table is the
/// order rows appeared in). Without this, every "year-only" event in the
/// same year piles onto the exact same pixel, since `HDate::decimal`
/// resolves a bare year to its very start — 23 events sharing one year (a
/// real case: a war's individual engagements, each only dated "429 BC")
/// would otherwise all draw on top of each other.
///
/// Returns each affected event's adjusted position on the continuous axis,
/// keyed by id. An event with its own month, a range event, or the lone
/// event in its year is left out entirely — callers fall back to the
/// event's own `span.t0()` for anything not present in the map.
pub fn fan_out_year_only_events<'a>(events: impl IntoIterator<Item = &'a Event>) -> HashMap<Id, f64> {
    let mut by_year: HashMap<i32, Vec<Id>> = HashMap::new();
    for e in events {
        if e.span.is_range() || e.span.start.month.is_some() {
            continue;
        }
        by_year.entry(e.span.start.year).or_default().push(e.id);
    }

    let mut out = HashMap::new();
    for (year, mut ids) in by_year {
        if ids.len() < 2 {
            continue;
        }
        ids.sort_by_key(|id| id.0);
        let start = HDate::year(year).decimal();
        let end = HDate::year(year).decimal_end();
        let n = ids.len() as f64;
        for (i, id) in ids.into_iter().enumerate() {
            out.insert(id, start + (i as f64 + 0.5) / n * (end - start));
        }
    }
    out
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
    /// Stacked slots of space reserved *above* the band for "long events" —
    /// a range event with its own visible nested content, drawn as its own
    /// small parallel mini-timeline. Several long events overlapping in
    /// time each claim their own slot rather than drawing on top of one
    /// another; see `canvas::paint_lane_events`.
    pub above_slots: usize,
    /// Rows of label space reserved *below* the band, for every other
    /// (plain point, or childless range) event's marker-plus-label.
    pub below_rows: usize,
}

impl Lane {
    pub fn is_nested(&self) -> bool {
        self.depth > 0
    }
}

/// What a lane needs at the current zoom, measured before placement.
#[derive(Clone, Copy, Debug, Default)]
pub struct LaneDemand {
    /// Rows of labels the lane's plain events want below the band.
    pub below_rows: usize,
    /// Stacked slots the lane's long events (see `Lane::above_slots`) want
    /// above the band.
    pub above_slots: usize,
    /// Whether the lane has anything at all in the visible window.
    pub active: bool,
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
/// Must clear the tallest label line — at the largest a label can get, once
/// zoom growth is added on top of its per-importance baseline — or a
/// high-importance title overflows into the row beneath it. Rows are a
/// fixed height regardless of current zoom, so this is sized for the
/// *maximum* a label can ever reach, not the common case; see the guard
/// test in `theme`.
pub const LABEL_ROW_HEIGHT: f32 = 25.0;
/// Padding under a band before the next lane starts.
pub const LANE_BOTTOM_PAD: f32 = 10.0;
/// Never grow a lane beyond this many label rows, however dense the data.
pub const MAX_LABEL_ROWS: usize = 14;
/// Height reserved for one stacked "long event" slot above a band: room for
/// the event's own title, a nested child's label floating above its bar,
/// and the bar itself. Sized for the worst case (a two-line title plus a
/// full `MAX_NESTED_LABEL_ROWS` stack of nested labels), the same
/// size-for-the-maximum approach `LABEL_ROW_HEIGHT` already takes.
pub const LONG_EVENT_SLOT_HEIGHT: f32 = 140.0;
/// However many long events overlap in time at once, never stack more than
/// this many slots — a hand-edited file with more than a handful of
/// overlapping wars is an extreme edge case; further ones share the
/// topmost slot rather than growing the lane without bound.
pub const MAX_LONG_EVENT_STACK: usize = 4;

/// Height a lane needs for the given demand.
///
/// Nested events (an event inside a range event) reserve no extra space of
/// their own here — they paint directly onto their parent's own bar as
/// colour-coded segments/markers (see `paint_nested_events` in `canvas.rs`),
/// exactly like an epoch on a timeline's band. What *is* reserved here is
/// room for that parent bar itself: one `LONG_EVENT_SLOT_HEIGHT` per
/// overlapping "long event" above the band, and one `LABEL_ROW_HEIGHT` per
/// row of plain-event labels below it.
pub fn lane_height(plan: &LanePlan, demand: LaneDemand) -> f32 {
    if plan.header_only {
        return 26.0;
    }
    if !demand.active {
        return DORMANT_LANE_HEIGHT;
    }
    let above = demand.above_slots as f32 * LONG_EVENT_SLOT_HEIGHT;
    let below = demand.below_rows as f32 * LABEL_ROW_HEIGHT;
    above + LABEL_BAND_TOP + plan.thickness + LABEL_BAND_BOTTOM + below + LANE_BOTTOM_PAD
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
        demand.below_rows = if demand.below_rows == 0 {
            0
        } else {
            demand.below_rows.max(plan.min_rows).min(MAX_LABEL_ROWS)
        };
        demand.above_slots = demand.above_slots.min(MAX_LONG_EVENT_STACK);
        let h = lane_height(plan, demand);
        let above_slots = if demand.active { demand.above_slots } else { 0 };
        let below_rows = if demand.active { demand.below_rows } else { 0 };
        let center = if plan.header_only || !demand.active {
            y + h * 0.5
        } else {
            y + above_slots as f32 * LONG_EVENT_SLOT_HEIGHT + LABEL_BAND_TOP + plan.thickness * 0.5
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
            above_slots,
            below_rows,
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

/// A best-effort reordering of the groups that share `parent` (siblings —
/// this does not reach into subgroups), greedily placing each one next to
/// whichever other sibling its timelines have the most origin/merge
/// connections with. Cuts down on a merge curve crossing through unrelated
/// bands the way "Griechische Antike" sitting far from "Griechische
/// Bronzezeit" would, even though nothing between them is actually related.
///
/// This is **not** a general crossing-minimisation solver — that is a hard
/// graph-layout problem in general, and the docstring on the caller should
/// say so — but a simple greedy chain already helps the common case of a
/// handful of connected cultures. Siblings with no cross-group connection at
/// all keep their existing relative order, so this never scrambles an
/// otherwise-unrelated group list for no reason.
pub fn suggest_group_order(doc: &Document, parent: Option<Id>) -> Vec<Id> {
    let siblings: Vec<Id> = doc.child_groups(parent).iter().map(|g| g.id).collect();
    if siblings.len() < 2 {
        return siblings;
    }

    let subtrees: Vec<(Id, BTreeSet<Id>)> = siblings
        .iter()
        .map(|&g| (g, doc.group_timelines(g).into_iter().collect()))
        .collect();
    let group_of_timeline = |t: Id| -> Option<Id> {
        subtrees.iter().find(|(_, members)| members.contains(&t)).map(|(g, _)| *g)
    };

    let mut weight: HashMap<(Id, Id), u32> = HashMap::new();
    let key = |a: Id, b: Id| if a.0 < b.0 { (a, b) } else { (b, a) };
    for (g, members) in &subtrees {
        for &tid in members {
            let Some(tl) = doc.timeline(tid) else { continue };
            for j in [&tl.origin, &tl.merge].into_iter().flatten() {
                if let Some(other_g) = group_of_timeline(j.other) {
                    if other_g != *g {
                        *weight.entry(key(*g, other_g)).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut placed: Vec<Id> = Vec::with_capacity(siblings.len());
    let mut remaining = siblings.clone();
    while !remaining.is_empty() {
        let next_connected = placed.last().and_then(|&last| {
            remaining
                .iter()
                .copied()
                .filter(|&cand| weight.get(&key(last, cand)).copied().unwrap_or(0) > 0)
                .max_by_key(|&cand| weight[&key(last, cand)])
        });
        let pick = next_connected.unwrap_or(remaining[0]);
        remaining.retain(|&id| id != pick);
        placed.push(pick);
    }
    placed
}

/// Apply `suggest_group_order` at *every* level of the group tree, not just
/// the top. The "Verbundene Gruppen zusammenrücken" button used to only
/// tidy top-level siblings — two connected cultures sitting side by side as
/// subgroups of a shared parent (e.g. "Griechische Antike" and "Griechische
/// Bronzezeit", both under "Antike") were never touched, since neither one
/// is a top-level group itself. Recursing into every subgroup's own
/// siblings fixes that without changing the underlying heuristic at all.
pub fn tidy_all_group_levels(doc: &mut Document) {
    tidy_group_level(doc, None);
}

fn tidy_group_level(doc: &mut Document, parent: Option<Id>) {
    let order = suggest_group_order(doc, parent);
    for (i, id) in order.iter().enumerate() {
        if let Some(g) = doc.group_mut(*id) {
            g.order = i as u32;
        }
    }
    for child in doc.child_groups(parent).iter().map(|g| g.id).collect::<Vec<_>>() {
        tidy_group_level(doc, Some(child));
    }
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
/// The easing window (in years) an origin/merge transition gets at this
/// zoom — normally a fixed `TRANSITION_PX` in screen space, so the curve
/// keeps the same shape at every zoom level.
///
/// If a timeline has both an origin and a merge, the two windows must not
/// overlap — otherwise, once zoomed out far enough that a fixed 110px
/// window exceeds the gap between the two dates (routine for a short-lived
/// timeline like a few-decade successor kingdom), the merge easing would
/// start interpolating from a `y` the origin easing had already pulled away
/// from `own_center`, instead of from `own_center` itself. The band would
/// curve out from its parent, immediately curve again into its target, and
/// never actually look "connected" to either — reported as a connection
/// that "runs too far" or "isn't attached." Capping each window at half the
/// origin-merge gap keeps the two easings from ever touching, at any zoom.
fn transition_window(tl: &Timeline, ppy: f64) -> f64 {
    let raw_window = (TRANSITION_PX / ppy).max(f64::MIN_POSITIVE);
    match (&tl.origin, &tl.merge) {
        (Some(o), Some(m)) => {
            let gap = (m.date.decimal() - o.date.decimal()).abs();
            raw_window.min(gap / 2.0)
        }
        _ => raw_window,
    }
}

pub fn band_center_at(
    tl: &Timeline,
    own_center: f32,
    centers: &HashMap<Id, f32>,
    t: f64,
    ppy: f64,
) -> f32 {
    let window = transition_window(tl, ppy);
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
    let window = transition_window(tl, axis.ppy);
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
/// The fourth element is the epoch's name, `None` for a base-colour gap —
/// callers use this (rather than comparing colours) to tell an epoch segment
/// from a gap, since an epoch could coincidentally be given the timeline's
/// own colour.
///
/// Epochs need not be sorted or disjoint on input. Where two overlap, the
/// later-starting one wins: an earlier epoch's painted end is capped at the
/// next one's start, regardless of its own configured end date. That makes
/// the common case — entering each era's start date and a rough, possibly
/// stale end date — behave the way it reads: "Classical starts in 500 BC"
/// unambiguously ends the Archaic era there too.
pub fn band_color_segments(tl: &Timeline, from: f64, to: f64) -> Vec<(f64, f64, Rgb, Option<&str>)> {
    color_segments(&tl.epochs, tl.color, from, to)
}

/// Same gap-filling logic as [`band_color_segments`], generalised to any list
/// of colour-coded sub-ranges and a base colour — reused for a biography's
/// `life_phases`, which recolour stretches of a lifeline the same way epochs
/// recolour stretches of a timeline's band.
pub fn color_segments(
    epochs: &[Epoch],
    base_color: Rgb,
    from: f64,
    to: f64,
) -> Vec<(f64, f64, Rgb, Option<&str>)> {
    if to <= from {
        return Vec::new();
    }
    let mut epochs: Vec<&Epoch> = epochs.iter().collect();
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
            segments.push((cursor, e0, base_color, None));
        }
        segments.push((e0, e1, e.color, Some(e.name.as_str())));
        cursor = e1;
    }
    if cursor < to {
        segments.push((cursor, to, base_color, None));
    }
    segments
}

/// A group member's `origin`/`merge` junction, when the *other* end of it is
/// outside the group — the ones a collapsed group's single summary band
/// needs to draw a curve for itself, or the connection to something outside
/// the group silently disappears the moment it is collapsed. `is_merge`
/// tells the caller which of the two curve directions to synthesize
/// (`false` = origin, easing in from the target; `true` = merge, easing out
/// to it).
pub fn group_external_junctions(doc: &Document, group: Id) -> Vec<(Junction, bool)> {
    let members: std::collections::BTreeSet<Id> = doc.group_timelines(group).into_iter().collect();
    let mut out = Vec::new();
    for tid in &members {
        let Some(tl) = doc.timeline(*tid) else { continue };
        if let Some(j) = &tl.origin {
            if !members.contains(&j.other) {
                out.push((j.clone(), false));
            }
        }
        if let Some(j) = &tl.merge {
            if !members.contains(&j.other) {
                out.push((j.clone(), true));
            }
        }
    }
    out
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

    /// Claim `rows_needed` *consecutive* rows for a label spanning
    /// `[x_min, x_max]` — a title that wraps onto a second line needs both
    /// rows it actually occupies reserved, not just the one its first line
    /// starts in, or the packer could still hand the row directly above it
    /// to some other label and the two would overlap.
    ///
    /// Returns the index of the first (nearest-the-band) row claimed, or
    /// `None` if no run of `rows_needed` consecutive free rows exists within
    /// `max_rows`.
    pub fn place_rows(&mut self, x_min: f32, x_max: f32, rows_needed: usize, max_rows: usize) -> Option<usize> {
        if rows_needed == 0 || rows_needed > max_rows {
            return None;
        }
        let pad = 6.0;
        'start: for start in 0..=(max_rows - rows_needed) {
            for row in start..start + rows_needed {
                if self.rows.len() <= row {
                    self.rows.push(Vec::new());
                }
                let free = self.rows[row]
                    .iter()
                    .all(|(a, b)| x_max + pad < *a || x_min - pad > *b);
                if !free {
                    continue 'start;
                }
            }
            for row in start..start + rows_needed {
                self.rows[row].push((x_min, x_max));
            }
            return Some(start);
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
    fn tick_step_reaches_day_level_at_the_zoom_cap() {
        // At MAX_PPY there must be enough room for a label roughly every
        // 110px to actually reach the finest (single-day) step, or the
        // whole point of raising the zoom cap for day-level ticks is moot.
        assert_eq!(tick_step(MAX_PPY), 1.0 / 365.0);
    }

    #[test]
    fn tick_step_passes_through_month_and_season_before_reaching_a_whole_year() {
        // Comfortably inside each band, not right at a boundary.
        assert_eq!(tick_step(110.0 / (0.6 / 12.0)), 1.0 / 12.0);
        assert_eq!(tick_step(110.0 / 0.24), 3.0 / 12.0);
        assert!(tick_step(0.001) >= 1.0, "far zoomed out should still land on whole years or coarser");
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

    /// A short-lived successor timeline: splits off from `parent` at -323 and
    /// merges into `target` at -283, a 40-year gap — deliberately shorter
    /// than `TRANSITION_PX` worth of years at the zoomed-out `ppy` the test
    /// below uses, so the origin and merge windows would overlap without the
    /// cap in `transition_window`.
    fn short_lived_successor_doc() -> (Timeline, HashMap<Id, f32>) {
        let parent = Id(1);
        let target = Id(2);
        let successor = Id(3);
        let tl = Timeline {
            id: successor,
            name: "Kurzlebiges Nachfolgereich".into(),
            color: [150, 150, 150],
            visible: true,
            group: None,
            order: 0,
            span: Some(Span::range(HDate::year(-323), HDate::year(-283))),
            origin: Some(Junction {
                other: parent,
                date: HDate::year(-323),
                label: String::new(),
            }),
            merge: Some(Junction {
                other: target,
                date: HDate::year(-283),
                label: String::new(),
            }),
            notes: String::new(),
            epochs: Vec::new(),
        };
        let centers = HashMap::from([(parent, 100.0f32), (target, 300.0f32)]);
        (tl, centers)
    }

    #[test]
    fn an_origin_and_merge_close_together_do_not_overlap_when_zoomed_far_out() {
        let (tl, centers) = short_lived_successor_doc();
        let own = 200.0;
        // Zoomed out enough that a bare `TRANSITION_PX / ppy` (110 years at
        // ppy=1.0) would more than cover the whole 40-year gap on its own,
        // let alone both windows combined.
        let ppy = 1.0;

        // Right at the origin, still on the parent's lane.
        let at_origin = band_center_at(&tl, own, &centers, -323.0, ppy);
        assert!((at_origin - 100.0).abs() < 0.01, "expected to start on the parent, got {at_origin}");

        // Right at the merge, arrived on the target's lane.
        let at_merge = band_center_at(&tl, own, &centers, -283.0, ppy);
        assert!((at_merge - 300.0).abs() < 0.01, "expected to end on the target, got {at_merge}");

        // Exactly at the midpoint, the origin easing must have *fully*
        // completed (reaching `own`) before the merge easing takes over —
        // if the two windows overlapped, this would land somewhere between
        // the parent and `own` instead, never actually reaching its own lane.
        let mid = band_center_at(&tl, own, &centers, -303.0, ppy);
        assert!((mid - own).abs() < 0.01, "expected to reach its own lane at the midpoint, got {mid}");

        // The whole span must be monotonic (parent -> own -> target), not an
        // overshoot or a double-back caused by compounding interpolations.
        let mut prev = f32::NEG_INFINITY;
        for i in 0..=40 {
            let t = -323.0 + 40.0 * (i as f64 / 40.0);
            let y = band_center_at(&tl, own, &centers, t, ppy);
            assert!(y >= prev - 1e-3, "band should rise steadily, got {y} after {prev}");
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

    /// The Diadochi split: several successor kingdoms fanning out from the
    /// same empire at the same moment, then each merging into a different
    /// conqueror at its own later date — Seleucid Syria falls to Rome in 63
    /// BC, decades before Ptolemaic Egypt does in 30 BC. Nothing about
    /// `origin`/`merge` is exclusive to one child or one merge target: both
    /// fields live on the child timeline itself, so this "one parent, many
    /// independent children, each converging on its own schedule" shape
    /// needs no special-casing anywhere in the geometry.
    #[test]
    fn several_timelines_can_split_from_one_parent_and_merge_into_different_targets() {
        let alexanders_empire = Id(1);
        let rome = Id(2);
        let ptolemaic_egypt = Timeline {
            id: Id(3),
            name: "Ptolemaic Egypt".into(),
            color: [0, 0, 0],
            visible: true,
            group: None,
            order: 0,
            span: Some(Span::range(HDate::year(-305), HDate::year(-30))),
            origin: Some(Junction {
                other: alexanders_empire,
                date: HDate::year(-305),
                label: "Diadochi".into(),
            }),
            merge: Some(Junction {
                other: rome,
                date: HDate::year(-30),
                label: "Antony and Cleopatra".into(),
            }),
            notes: String::new(),
            epochs: Vec::new(),
        };
        let seleucid_empire = Timeline {
            id: Id(4),
            name: "Seleucid Empire".into(),
            color: [0, 0, 0],
            visible: true,
            group: None,
            order: 1,
            span: Some(Span::range(HDate::year(-305), HDate::year(-63))),
            // Same parent, same split date as Ptolemaic Egypt above.
            origin: Some(Junction {
                other: alexanders_empire,
                date: HDate::year(-305),
                label: "Diadochi".into(),
            }),
            // Same target as Ptolemaic Egypt, but decades earlier.
            merge: Some(Junction {
                other: rome,
                date: HDate::year(-63),
                label: "Pompey's settlement".into(),
            }),
            notes: String::new(),
            epochs: Vec::new(),
        };

        let centers = HashMap::from([
            (alexanders_empire, 50.0f32),
            (ptolemaic_egypt.id, 150.0f32),
            (seleucid_empire.id, 250.0f32),
            (rome, 350.0f32),
        ]);
        // Zoomed in enough (110px transition / 20 px-per-year = 5.5-year
        // window) that the two merges, 33 years apart, don't visually blend
        // into each other — at 2 px/year the transition windows are wide
        // enough to overlap, which is correct rendering, just not what this
        // test wants to isolate.
        let ppy = 20.0;

        // Both still ride the shared parent's lane right at the split.
        for child in [&ptolemaic_egypt, &seleucid_empire] {
            let at_split = band_center_at(child, centers[&child.id], &centers, -305.0, ppy);
            assert!(
                (at_split - 50.0).abs() < 0.01,
                "{} should start on the parent lane, got {at_split}",
                child.name
            );
        }

        // Seleucid Empire has already converged onto Rome...
        let seleucid_after_63 = band_center_at(&seleucid_empire, 250.0, &centers, -63.0, ppy);
        assert!((seleucid_after_63 - 350.0).abs() < 0.01);
        // ...33 years before Ptolemaic Egypt, which at that same date is
        // still living its own independent life on its own lane.
        let ptolemaic_at_50 = band_center_at(&ptolemaic_egypt, 150.0, &centers, -50.0, ppy);
        assert!((ptolemaic_at_50 - 150.0).abs() < 0.01);
        let ptolemaic_after_30 = band_center_at(&ptolemaic_egypt, 150.0, &centers, -30.0, ppy);
        assert!((ptolemaic_after_30 - 350.0).abs() < 0.01);
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
        assert_eq!(segs, vec![(-800.0, -300.0, tl.color, None)]);
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
                (-800.0, -500.0, [1, 1, 1], Some("Archaic")),
                (-500.0, -322.0, [2, 2, 2], Some("Classical")),
                (-322.0, -300.0, tl.color, None),
            ]
        );
    }

    #[test]
    fn an_epoch_outside_the_range_is_dropped_entirely() {
        let tl = timeline_with_epochs(vec![epoch("Bronze age", [1, 1, 1], -2000, -1200)]);
        let segs = band_color_segments(&tl, -800.0, -300.0);
        assert_eq!(segs, vec![(-800.0, -300.0, tl.color, None)]);
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
                (-800.0, -500.0, [1, 1, 1], Some("Archaic")),
                (-500.0, -322.0, [2, 2, 2], Some("Classical")),
                (-322.0, -300.0, tl.color, None),
            ]
        );
    }

    #[test]
    fn bio_thickness_eases_from_the_minimum_up_to_the_normal_size_as_you_zoom_in() {
        assert_eq!(bio_thickness(0.0, false), BIO_BAND_THICKNESS_MIN);
        assert_eq!(bio_thickness(BIO_ZOOM_REFERENCE_PPY, false), BIO_BAND_THICKNESS);
        // Zooming in further must not overshoot the normal thickness.
        assert_eq!(bio_thickness(BIO_ZOOM_REFERENCE_PPY * 10.0, false), BIO_BAND_THICKNESS);
        let mid = bio_thickness(BIO_ZOOM_REFERENCE_PPY * 0.5, false);
        assert!(mid > BIO_BAND_THICKNESS_MIN && mid < BIO_BAND_THICKNESS);
    }

    #[test]
    fn a_pinned_open_biography_ignores_zoom_entirely() {
        assert_eq!(bio_thickness(0.0, true), BIO_BAND_THICKNESS_ENLARGED);
        assert_eq!(bio_thickness(1000.0, true), BIO_BAND_THICKNESS_ENLARGED);
    }

    #[test]
    fn an_empty_range_produces_no_segments() {
        let tl = timeline_with_epochs(vec![epoch("Archaic", [1, 1, 1], -800, -500)]);
        assert!(band_color_segments(&tl, -300.0, -300.0).is_empty());
        assert!(band_color_segments(&tl, -300.0, -400.0).is_empty());
    }

    #[test]
    fn color_segments_works_without_a_timeline_for_biography_life_phases() {
        // The generalised helper behind `band_color_segments` — a
        // biography's life phases recolour stretches of its lifeline the
        // same way, but there is no `Timeline` to hang them off.
        let phases = vec![epoch("Als Kaiser", [9, 9, 9], -27, -14)];
        let segs = color_segments(&phases, [5, 5, 5], -63.0, -10.0);
        assert_eq!(
            segs,
            vec![
                (-63.0, -27.0, [5, 5, 5], None),
                (-27.0, -13.0, [9, 9, 9], Some("Als Kaiser")),
                (-13.0, -10.0, [5, 5, 5], None),
            ]
        );
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

    fn ranged_event(id: Id, start: i32, end: i32) -> Event {
        Event {
            id,
            owner: OwnerRef::Timeline(Id(1)),
            title: "war".into(),
            description: String::new(),
            span: Span::range(HDate::year(start), HDate::year(end)),
            importance: 3,
            categories: vec![],
            parent: None,
        }
    }

    #[test]
    fn a_point_event_never_collapses() {
        assert!(!range_collapsed(&nested_event(Id(1), None, 3), 0.0001));
    }

    #[test]
    fn a_range_collapses_once_its_on_screen_width_drops_below_the_threshold() {
        // 27 years (the Peloponnesian War, roughly) at these two zoom levels.
        let war = ranged_event(Id(1), -431, -404);
        assert!(!range_collapsed(&war, 5.0), "27 years * 5 px/yr = 135px, well above the threshold");
        assert!(range_collapsed(&war, 0.1), "27 years * 0.1 px/yr = 2.7px, a sliver");
    }

    fn year_only_point_event(id: Id, year: i32) -> Event {
        Event {
            id,
            owner: OwnerRef::Timeline(Id(1)),
            title: "e".into(),
            description: String::new(),
            span: Span::point(HDate::year(year)),
            importance: 3,
            categories: vec![],
            parent: None,
        }
    }

    #[test]
    fn a_lone_year_only_event_is_left_at_its_own_position() {
        let e = year_only_point_event(Id(1), -429);
        let fanned = fan_out_year_only_events([&e]);
        assert!(fanned.is_empty(), "a single event in its year needs no spreading");
    }

    #[test]
    fn events_with_a_real_month_are_never_moved() {
        let mut e = year_only_point_event(Id(1), -429);
        e.span.start.month = Some(6);
        let other = year_only_point_event(Id(2), -429);
        let fanned = fan_out_year_only_events([&e, &other]);
        assert!(!fanned.contains_key(&Id(1)), "a dated event has its own position already");
    }

    #[test]
    fn year_only_events_sharing_a_year_spread_across_it_in_id_order() {
        // Three engagements of a war, each only ever dated "429 BC" — exactly
        // the shape a real ancient-history import produces.
        let a = year_only_point_event(Id(3), -429);
        let b = year_only_point_event(Id(1), -429);
        let c = year_only_point_event(Id(2), -429);
        let fanned = fan_out_year_only_events([&a, &b, &c]);
        assert_eq!(fanned.len(), 3);

        let year_start = HDate::year(-429).decimal();
        let year_end = HDate::year(-429).decimal_end();
        for &t in fanned.values() {
            assert!(t > year_start && t < year_end, "must stay within the year");
        }
        // Ordered by id, not by the order passed in.
        assert!(fanned[&Id(1)] < fanned[&Id(2)]);
        assert!(fanned[&Id(2)] < fanned[&Id(3)]);
    }

    #[test]
    fn different_years_are_never_mixed_together() {
        let a = year_only_point_event(Id(1), -429);
        let b = year_only_point_event(Id(2), -429);
        let c = year_only_point_event(Id(3), -400);
        let fanned = fan_out_year_only_events([&a, &b, &c]);
        // -400 is a lone event in its year, so it is not spread at all.
        assert!(!fanned.contains_key(&Id(3)));
        assert_eq!(fanned.len(), 2);
    }

    // --- Lanes -------------------------------------------------------------

    /// Plan + place with no measured label demand, i.e. minimum lane sizes.
    fn build_lanes(doc: &Document, top: f32, filters: &Filters) -> Vec<Lane> {
        let plans = plan_lanes(doc, filters);
        let demands = vec![LaneDemand { below_rows: 0, above_slots: 0, active: true }; plans.len()];
        place_lanes(&plans, &demands, top)
    }

    fn demands(n: usize, rows: usize) -> Vec<LaneDemand> {
        vec![LaneDemand { below_rows: rows, above_slots: 0, active: true }; n]
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
            life_phases: Vec::new(),
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
            life_phases: Vec::new(),
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
            life_phases: Vec::new(),
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
            parent: None,
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
    fn group_external_junctions_skips_connections_within_the_group() {
        let mut doc = grouped_doc();
        let greek = doc.groups[0].id;
        let sparta = doc.timelines[0].id;
        let athens = doc.timelines[1].id;
        let rome = doc.timelines[2].id;

        // Athens merges out to Rome — outside the group, must be reported.
        doc.timeline_mut(athens).unwrap().merge = Some(Junction {
            other: rome,
            date: HDate::year(-146),
            label: "Corinth".into(),
        });
        // Sparta "splits from" Athens — its own group sibling, so collapsing
        // the group hides nothing that isn't already inside the one summary
        // band, and this must not be reported.
        doc.timeline_mut(sparta).unwrap().origin = Some(Junction {
            other: athens,
            date: HDate::year(-800),
            label: String::new(),
        });

        let external = group_external_junctions(&doc, greek);
        assert_eq!(external.len(), 1);
        assert_eq!(external[0].0.other, rome);
        assert!(external[0].1, "Athens' connection to Rome is a merge");
    }

    #[test]
    fn suggest_group_order_chains_connected_groups_together() {
        // A, B, C, D at the top level; only B and D are actually related
        // (B's timeline merges into D's) — the suggestion should pull them
        // next to each other without disturbing A and C's relative order.
        let mut doc = Document::default();
        let group_a = doc.new_id();
        let group_b = doc.new_id();
        let group_c = doc.new_id();
        let group_d = doc.new_id();
        for (id, name) in [(group_a, "A"), (group_b, "B"), (group_c, "C"), (group_d, "D")] {
            doc.groups.push(Group {
                id,
                name: name.into(),
                color: [0, 0, 0],
                parent: None,
                order: doc.groups.len() as u32,
                collapsed: false,
                visible: true,
                notes: String::new(),
            });
        }
        let in_b = doc.new_id();
        let in_d = doc.new_id();
        doc.timelines.push(Timeline {
            id: in_d,
            name: "D's timeline".into(),
            color: [0, 0, 0],
            visible: true,
            group: Some(group_d),
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });
        doc.timelines.push(Timeline {
            id: in_b,
            name: "B's timeline".into(),
            color: [0, 0, 0],
            visible: true,
            group: Some(group_b),
            order: 0,
            span: None,
            origin: None,
            merge: Some(Junction { other: in_d, date: HDate::year(1), label: String::new() }),
            notes: String::new(),
            epochs: Vec::new(),
        });

        let order = suggest_group_order(&doc, None);
        let pos = |id: Id| order.iter().position(|&x| x == id).unwrap();
        assert!(
            (pos(group_b) as i32 - pos(group_d) as i32).abs() == 1,
            "B and D should end up adjacent, got order {order:?}"
        );
    }

    #[test]
    fn suggest_group_order_leaves_unconnected_siblings_in_their_original_order() {
        let mut doc = Document::default();
        let ids: Vec<Id> = ["A", "B", "C"]
            .into_iter()
            .map(|name| {
                let id = doc.new_id();
                doc.groups.push(Group {
                    id,
                    name: name.into(),
                    color: [0, 0, 0],
                    parent: None,
                    order: doc.groups.len() as u32,
                    collapsed: false,
                    visible: true,
                    notes: String::new(),
                });
                id
            })
            .collect();
        assert_eq!(suggest_group_order(&doc, None), ids);
    }

    #[test]
    fn tidy_all_group_levels_reaches_into_subgroups_too() {
        // The reported bug: two connected cultures sitting as *subgroups* of
        // a shared parent ("Griechische Antike" and "Griechische Bronzezeit"
        // under "Antike") were never nudged together, because the old
        // top-level-only tidy only ever looked at `suggest_group_order(doc,
        // None)` — neither subgroup is a top-level group itself, so nothing
        // about their connection was visible from the top.
        let mut doc = Document::default();
        let antike = doc.new_id();
        doc.groups.push(Group {
            id: antike,
            name: "Antike".into(),
            color: [0, 0, 0],
            parent: None,
            order: 0,
            collapsed: false,
            visible: true,
            notes: String::new(),
        });
        let bronze = doc.new_id();
        let greek = doc.new_id();
        let unrelated = doc.new_id();
        // Deliberately ordered so the connected pair starts apart, with an
        // unrelated subgroup between them.
        for (i, (id, name)) in [(bronze, "Bronzezeit"), (unrelated, "Unrelated"), (greek, "Griechische Antike")]
            .into_iter()
            .enumerate()
        {
            doc.groups.push(Group {
                id,
                name: name.into(),
                color: [0, 0, 0],
                parent: Some(antike),
                order: i as u32,
                collapsed: false,
                visible: true,
                notes: String::new(),
            });
        }
        let bronze_tl = doc.new_id();
        let greek_tl = doc.new_id();
        doc.timelines.push(Timeline {
            id: bronze_tl,
            name: "Bronze age culture".into(),
            color: [0, 0, 0],
            visible: true,
            group: Some(bronze),
            order: 0,
            span: None,
            origin: None,
            merge: Some(Junction { other: greek_tl, date: HDate::year(-1200), label: String::new() }),
            notes: String::new(),
            epochs: Vec::new(),
        });
        doc.timelines.push(Timeline {
            id: greek_tl,
            name: "Greek antiquity".into(),
            color: [0, 0, 0],
            visible: true,
            group: Some(greek),
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });

        tidy_all_group_levels(&mut doc);

        let siblings = doc.child_groups(Some(antike));
        let pos = |id: Id| siblings.iter().position(|g| g.id == id).unwrap();
        assert!(
            (pos(bronze) as i32 - pos(greek) as i32).abs() == 1,
            "the connected subgroups should end up adjacent, got {:?}",
            siblings.iter().map(|g| &g.name).collect::<Vec<_>>()
        );
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
        assert_eq!(dense[0].below_rows, 8);
    }

    #[test]
    fn lane_growth_is_capped_so_one_dense_lane_cannot_fill_the_screen() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let lanes = place_lanes(&plans, &demands(plans.len(), 9_999), 0.0);
        assert_eq!(lanes[0].below_rows, MAX_LABEL_ROWS);
    }

    #[test]
    fn a_lane_with_labels_keeps_a_minimum_of_breathing_room() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        // One label's worth of demand still gets the timeline minimum.
        let lanes = place_lanes(&plans, &demands(plans.len(), 1), 0.0);
        assert!(lanes[0].below_rows >= 2, "timelines reserve label space");
    }

    #[test]
    fn a_lane_with_no_labels_reserves_no_label_space() {
        // A band that exists in the window but has no events in it should not
        // leave a tall empty gap below itself.
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let empty = place_lanes(&plans, &demands(plans.len(), 0), 0.0);
        let labelled = place_lanes(&plans, &demands(plans.len(), 1), 0.0);
        assert_eq!(empty[0].below_rows, 0);
        assert!(empty[0].bottom - empty[0].top < labelled[0].bottom - labelled[0].top);
    }

    #[test]
    fn label_space_sits_below_the_band_in_every_lane() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let lanes = place_lanes(&plans, &demands(plans.len(), 4), 0.0);
        for l in &lanes {
            let band_bottom = l.center + l.thickness * 0.5;
            assert!(
                l.bottom - band_bottom >= l.below_rows as f32 * LABEL_ROW_HEIGHT,
                "lane {} does not reserve room for its labels",
                l.name
            );
            assert!(l.bottom >= l.center + l.thickness * 0.5);
        }
    }

    fn above_demands(n: usize, slots: usize) -> Vec<LaneDemand> {
        vec![LaneDemand { below_rows: 0, above_slots: slots, active: true }; n]
    }

    #[test]
    fn lanes_grow_to_fit_stacked_long_events_above_the_band() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let one = place_lanes(&plans, &above_demands(plans.len(), 1), 0.0);
        let two = place_lanes(&plans, &above_demands(plans.len(), 2), 0.0);
        assert!(
            two[0].bottom - two[0].top > one[0].bottom - one[0].top,
            "a second overlapping long event must get its own slot, not share the first's"
        );
        assert_eq!(one[0].above_slots, 1);
        assert_eq!(two[0].above_slots, 2);
    }

    #[test]
    fn long_event_stacking_is_capped_so_it_cannot_grow_without_bound() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let lanes = place_lanes(&plans, &above_demands(plans.len(), 9_999), 0.0);
        assert_eq!(lanes[0].above_slots, MAX_LONG_EVENT_STACK);
    }

    #[test]
    fn stacked_slot_space_sits_above_the_band_in_every_lane() {
        let doc = lane_doc();
        let plans = plan_lanes(&doc, &Filters::default());
        let lanes = place_lanes(&plans, &above_demands(plans.len(), 2), 0.0);
        for l in &lanes {
            let band_top = l.center - l.thickness * 0.5;
            assert!(
                band_top - l.top >= l.above_slots as f32 * LONG_EVENT_SLOT_HEIGHT,
                "lane {} does not reserve room for its stacked long events",
                l.name
            );
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
            &vec![LaneDemand { below_rows: 3, above_slots: 0, active: false }; plans.len()],
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
            .map(|i| LaneDemand { below_rows: i * 3, above_slots: 0, active: i % 2 == 0 })
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
        assert_eq!(p.place_rows(0.0, 100.0, 1, 3), Some(0));
        assert_eq!(p.place_rows(50.0, 150.0, 1, 3), Some(1));
        assert_eq!(p.place_rows(60.0, 160.0, 1, 3), Some(2));
        assert_eq!(p.place_rows(70.0, 170.0, 1, 3), None, "should run out of rows");
    }

    #[test]
    fn non_overlapping_labels_share_the_first_row() {
        let mut p = LabelPacker::new();
        assert_eq!(p.place_rows(0.0, 100.0, 1, 3), Some(0));
        assert_eq!(p.place_rows(200.0, 300.0, 1, 3), Some(0));
    }

    #[test]
    fn a_two_row_label_reserves_both_rows_it_occupies() {
        let mut p = LabelPacker::new();
        // A wrapped, two-line label claims rows 0 and 1 together...
        assert_eq!(p.place_rows(0.0, 100.0, 2, 4), Some(0));
        // ...so an overlapping single-line label must skip past both of
        // them rather than landing on row 1 and colliding with the second
        // line of the label above it.
        assert_eq!(p.place_rows(0.0, 100.0, 1, 4), Some(2));
        // A non-overlapping label is free to still use row 0 or row 1.
        assert_eq!(p.place_rows(200.0, 300.0, 1, 4), Some(0));
    }

    #[test]
    fn a_two_row_label_is_rejected_once_too_few_consecutive_rows_remain() {
        let mut p = LabelPacker::new();
        assert_eq!(p.place_rows(0.0, 100.0, 1, 3), Some(0));
        assert_eq!(p.place_rows(0.0, 100.0, 1, 3), Some(1));
        // Only row 2 is left free — not enough for a two-row label.
        assert_eq!(p.place_rows(0.0, 100.0, 2, 3), None);
    }
}
