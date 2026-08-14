//! Core data model for Timeline Explorer.
//!
//! Everything the user builds up over the years lives in [`Document`], which is
//! serialised to a single human-readable JSON file. Fields carry `#[serde(default)]`
//! so that documents written by older builds keep loading after the model grows.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Stable identifier, unique within a [`Document`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u32);

pub type Rgb = [u8; 3];

/// Average days per year in the Julian/Gregorian mix. Only used to place events
/// *within* a year, where sub-year precision is cosmetic anyway.
const DAYS_PER_MONTH: f64 = 30.437_5;
const DAYS_PER_YEAR: f64 = 365.25;

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/// How confident the user is about a date. Historical dates are frequently
/// approximate, so this is part of the model rather than something squeezed
/// into the title text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum DateQualifier {
    /// The date is known.
    #[default]
    Exact,
    /// "circa" — around this date.
    Circa,
    /// Happened at some point before this date.
    Before,
    /// Happened at some point after this date.
    After,
}

impl DateQualifier {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Exact => "",
            Self::Circa => "c. ",
            Self::Before => "before ",
            Self::After => "after ",
        }
    }

}

/// A historical date with optional month/day and an explicit uncertainty.
///
/// `year` uses historical numbering: positive is AD/CE, negative is BC/BCE
/// (`-44` means 44 BC), and there is no year zero.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct HDate {
    pub year: i32,
    #[serde(default)]
    pub month: Option<u8>,
    #[serde(default)]
    pub day: Option<u8>,
    #[serde(default)]
    pub qualifier: DateQualifier,
    /// Uncertainty in years, rendered as a fade around the marker.
    #[serde(default)]
    pub plus_minus: u16,
}

impl Default for HDate {
    fn default() -> Self {
        Self {
            year: 1,
            month: None,
            day: None,
            qualifier: DateQualifier::Exact,
            plus_minus: 0,
        }
    }
}

impl HDate {
    pub fn year(year: i32) -> Self {
        Self {
            year: normalise_year(year),
            ..Default::default()
        }
    }

    pub fn circa(year: i32) -> Self {
        Self {
            qualifier: DateQualifier::Circa,
            ..Self::year(year)
        }
    }

    /// Continuous axis position, where `0.0` is the start of 1 AD.
    ///
    /// AD year `y` occupies `[y-1, y)`; BC year `y` occupies `[-y, -y+1)`. That
    /// keeps the BC/AD boundary contiguous despite there being no year zero.
    pub fn decimal(&self) -> f64 {
        let base = year_base(self.year);
        let m = self.month.unwrap_or(1).clamp(1, 12) as f64;
        let d = self.day.unwrap_or(1).clamp(1, 31) as f64;
        base + ((m - 1.0) * DAYS_PER_MONTH + (d - 1.0)) / DAYS_PER_YEAR
    }

    /// End of the period this date denotes. A year-only date covers a whole
    /// year, a month-only date a whole month. Used so that "1789" as a range
    /// end means "through the end of 1789".
    pub fn decimal_end(&self) -> f64 {
        let base = year_base(self.year);
        match (self.month, self.day) {
            (None, _) => base + 1.0,
            (Some(m), None) => base + (m.clamp(1, 12) as f64 * DAYS_PER_MONTH) / DAYS_PER_YEAR,
            (Some(m), Some(d)) => {
                base + ((m.clamp(1, 12) as f64 - 1.0) * DAYS_PER_MONTH + d.clamp(1, 31) as f64)
                    / DAYS_PER_YEAR
            }
        }
    }

    /// Human-readable form, e.g. `c. 44 BC`, `14 Jul 1789`, `250 BC ±20`.
    pub fn label(&self) -> String {
        let mut s = String::from(self.qualifier.prefix());
        if let Some(m) = self.month {
            if let Some(d) = self.day {
                s.push_str(&format!("{d} "));
            }
            s.push_str(month_name(m));
            s.push(' ');
        }
        s.push_str(&year_label(self.year));
        if self.plus_minus > 0 {
            s.push_str(&format!(" ±{}", self.plus_minus));
        }
        s
    }

    /// Parse the free-text date entry used by the quick-add forms.
    ///
    /// Accepts `-44`, `44 BC`, `44 v. Chr.`, `c. 250 BC`, `1789`, `1789-07-14`,
    /// `Jul 1789`, `14 Jul 1789`, any of them with a trailing `±20`.
    pub fn parse(input: &str) -> Option<Self> {
        let mut s = input.trim().to_lowercase();
        if s.is_empty() {
            return None;
        }

        // Trailing uncertainty: "±20", "+/-20", "+-20".
        let mut plus_minus = 0u16;
        for marker in ["±", "+/-", "+-"] {
            if let Some(idx) = s.find(marker) {
                let tail = s[idx + marker.len()..].trim().to_string();
                let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(v) = digits.parse::<u16>() {
                    plus_minus = v;
                }
                s.truncate(idx);
                s = s.trim().to_string();
                break;
            }
        }

        // Leading qualifier.
        let mut qualifier = DateQualifier::Exact;
        for (needle, q) in [
            ("circa ", DateQualifier::Circa),
            ("ca. ", DateQualifier::Circa),
            ("ca ", DateQualifier::Circa),
            ("c. ", DateQualifier::Circa),
            ("um ", DateQualifier::Circa),
            ("~", DateQualifier::Circa),
            ("before ", DateQualifier::Before),
            ("vor ", DateQualifier::Before),
            ("<", DateQualifier::Before),
            ("after ", DateQualifier::After),
            ("nach ", DateQualifier::After),
            (">", DateQualifier::After),
        ] {
            if let Some(rest) = s.strip_prefix(needle) {
                qualifier = q;
                s = rest.trim().to_string();
                break;
            }
        }

        // Trailing era marker. Checked before parsing digits so "44 bc" works.
        let mut era_bc = false;
        let mut era_seen = false;
        for (needle, is_bc) in [
            ("bce", true),
            ("bc", true),
            ("v. chr.", true),
            ("v.chr.", true),
            ("v. chr", true),
            ("v.chr", true),
            ("ce", false),
            ("ad", false),
            ("n. chr.", false),
            ("n.chr.", false),
            ("n. chr", false),
            ("n.chr", false),
        ] {
            if let Some(rest) = s.strip_suffix(needle) {
                era_bc = is_bc;
                era_seen = true;
                s = rest.trim().trim_end_matches(',').trim().to_string();
                break;
            }
        }
        // "ad 1066" — leading era marker.
        if !era_seen {
            for (needle, is_bc) in [("ad ", false), ("ce ", false), ("bc ", true)] {
                if let Some(rest) = s.strip_prefix(needle) {
                    era_bc = is_bc;
                    era_seen = true;
                    s = rest.trim().to_string();
                    break;
                }
            }
        }

        let (year, month, day) = parse_ymd(&s)?;
        let year = if era_seen && era_bc {
            -year.abs()
        } else if era_seen {
            year.abs()
        } else {
            year
        };

        Some(Self {
            year: normalise_year(year),
            month,
            day,
            qualifier,
            plus_minus,
        })
    }
}

/// Continuous-axis start of a historical year.
fn year_base(year: i32) -> f64 {
    if year > 0 {
        (year - 1) as f64
    } else {
        year as f64
    }
}

/// There is no year zero; treat an entered `0` as 1 BC.
fn normalise_year(year: i32) -> i32 {
    if year == 0 {
        -1
    } else {
        year
    }
}

pub fn year_label(year: i32) -> String {
    if year < 0 {
        format!("{} BC", -year)
    } else {
        format!("{year}")
    }
}

/// Label for an arbitrary point on the continuous axis (used by the ruler).
pub fn axis_year_label(decimal: f64) -> String {
    let f = decimal.floor();
    if f >= 0.0 {
        format!("{}", f as i64 + 1)
    } else {
        format!("{} BC", -(f as i64))
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn month_name(m: u8) -> &'static str {
    MONTHS[(m.clamp(1, 12) - 1) as usize]
}

fn month_from_name(s: &str) -> Option<u8> {
    let s = s.trim_end_matches('.');
    // English and German abbreviations, matched on the first three characters.
    let table: [(&str, u8); 16] = [
        ("jan", 1),
        ("feb", 2),
        ("mar", 3),
        ("mär", 3),
        ("maer", 3),
        ("apr", 4),
        ("may", 5),
        ("mai", 5),
        ("jun", 6),
        ("jul", 7),
        ("aug", 8),
        ("sep", 9),
        ("okt", 10),
        ("oct", 10),
        ("nov", 11),
        ("dez", 12),
    ];
    for (name, m) in table {
        if s.starts_with(name) {
            return Some(m);
        }
    }
    if s.starts_with("dec") {
        return Some(12);
    }
    None
}

/// Parse the date body once qualifier and era have been stripped.
fn parse_ymd(s: &str) -> Option<(i32, Option<u8>, Option<u8>)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // ISO-ish: -0044-03-15 / 1789-07-14 / 1789-07
    let negative = s.starts_with('-');
    let body = if negative { &s[1..] } else { s };
    if body.contains('-') {
        let parts: Vec<&str> = body.split('-').collect();
        if parts.len() >= 2 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
            let year: i32 = parts[0].parse().ok()?;
            let year = if negative { -year } else { year };
            let month: u8 = parts[1].parse().ok()?;
            let day = parts.get(2).and_then(|p| p.parse::<u8>().ok());
            return Some((year, Some(month.clamp(1, 12)), day.map(|d| d.clamp(1, 31))));
        }
    }

    // Token form: "14 jul 1789", "jul 1789", "1789", "-44"
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut day = None;
    let mut month = None;
    let mut year = None;
    for tok in tokens {
        let tok = tok.trim_end_matches(',');
        if let Some(m) = month_from_name(tok) {
            month = Some(m);
        } else if let Ok(v) = tok.parse::<i32>() {
            // A bare number is a day only if it is small and a year is still to come.
            if (1..=31).contains(&v) && year.is_none() && day.is_none() && month.is_none() {
                day = Some(v as u8);
            } else if year.is_none() {
                year = Some(v);
            }
        } else {
            return None;
        }
    }
    // "14 1789" without a month name means the 14 was really the year.
    let year = match (year, day) {
        (Some(y), _) => y,
        (None, Some(d)) => d as i32,
        (None, None) => return None,
    };
    let day = if month.is_some() { day } else { None };
    Some((year, month, day))
}

// ---------------------------------------------------------------------------
// Spans
// ---------------------------------------------------------------------------

/// A point in time, or a range. `end == None` means a point event.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Span {
    pub start: HDate,
    #[serde(default)]
    pub end: Option<HDate>,
}

impl Default for Span {
    fn default() -> Self {
        Self {
            start: HDate::default(),
            end: None,
        }
    }
}

impl Span {
    pub fn point(start: HDate) -> Self {
        Self { start, end: None }
    }

    /// Point event at an approximate year.
    pub fn circa_point(year: i32) -> Self {
        Self::point(HDate::circa(year))
    }

    pub fn range(start: HDate, end: HDate) -> Self {
        Self {
            start,
            end: Some(end),
        }
    }

    pub fn t0(&self) -> f64 {
        self.start.decimal()
    }

    /// Exclusive end on the continuous axis.
    pub fn t1(&self) -> f64 {
        match self.end {
            Some(e) => e.decimal_end().max(self.start.decimal()),
            None => self.start.decimal_end(),
        }
    }

    pub fn is_range(&self) -> bool {
        self.end.is_some()
    }

    pub fn label(&self) -> String {
        match self.end {
            Some(e) => format!("{} – {}", self.start.label(), e.label()),
            None => self.start.label(),
        }
    }
}

// ---------------------------------------------------------------------------
// Importance
// ---------------------------------------------------------------------------

/// Significance tier, 1..=5. Drives both zoom-dependent visibility and the
/// visual weight (marker size, font size, opacity) an entry is drawn with.
pub const IMPORTANCE_MIN: u8 = 1;
pub const IMPORTANCE_MAX: u8 = 5;

pub fn importance_name(level: u8) -> &'static str {
    match level.clamp(IMPORTANCE_MIN, IMPORTANCE_MAX) {
        5 => "Epochal",
        4 => "Major",
        3 => "Notable",
        2 => "Minor",
        _ => "Detail",
    }
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// A user-defined tag. Entries may carry several.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Category {
    pub id: Id,
    pub name: String,
    #[serde(default = "default_category_color")]
    pub color: Rgb,
}

fn default_category_color() -> Rgb {
    [140, 140, 150]
}

/// A point where one timeline merges into, or splits off from, another.
///
/// This is what makes Rome absorbing a Hellenistic kingdom render as two bands
/// actually converging, rather than as a marker sitting on parallel lines.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Junction {
    /// The other timeline: the one merged into, or split from.
    pub other: Id,
    pub date: HDate,
    #[serde(default)]
    pub label: String,
}

/// A super-category holding timelines and, optionally, other groups.
///
/// This is the "Europäische vs. asiatische Geschichte" / "griechische Antike"
/// layer: a heading you can collapse to compare whole civilisations at a
/// glance, or expand to see Sparta and Athens individually. Groups nest, so
/// "European history > Greek antiquity > Sparta" works.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    /// Parent group, or `None` for a top-level group.
    #[serde(default)]
    pub parent: Option<Id>,
    #[serde(default)]
    pub order: u32,
    /// Collapsed groups draw a single summary band instead of their members.
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub notes: String,
}

/// A colour-coded sub-range within a timeline's band — "Archaic",
/// "Classical", "Hellenistic" within one Greek-antiquity band, say.
///
/// Purely cosmetic: it recolours a stretch of an existing band so eras can be
/// told apart at a glance, without requiring a separate timeline (and a
/// merge/origin junction just to mark a change of era).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Epoch {
    pub name: String,
    pub color: Rgb,
    pub start: HDate,
    pub end: HDate,
}

impl Epoch {
    pub fn t0(&self) -> f64 {
        self.start.decimal()
    }

    /// Exclusive end on the continuous axis.
    pub fn t1(&self) -> f64 {
        self.end.decimal_end().max(self.start.decimal())
    }
}

/// A culture, civilisation, institution — anything with its own band.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Timeline {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub order: u32,
    /// Super-category this timeline sits under, e.g. "Greek antiquity".
    #[serde(default)]
    pub group: Option<Id>,
    /// Explicit lifespan. `None` means "infer it from this timeline's events",
    /// so the user gets a sensible band without having to fill anything in.
    #[serde(default)]
    pub span: Option<Span>,
    /// Set when this timeline begins by splitting off from another.
    #[serde(default)]
    pub origin: Option<Junction>,
    /// Set when this timeline ends by merging into another.
    #[serde(default)]
    pub merge: Option<Junction>,
    #[serde(default)]
    pub notes: String,
    /// Colour-coded eras drawn along this timeline's own band. Ordered by
    /// start date is not required; painting sorts them.
    #[serde(default)]
    pub epochs: Vec<Epoch>,
}

fn yes() -> bool {
    true
}

/// How a biography is presented relative to the main lanes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum BioDisplay {
    /// Not shown on the canvas.
    #[default]
    Hidden,
    /// Expanded in place: a nested sub-lane tucked under its parent timeline.
    Inline,
    /// Promoted to its own full parallel lane alongside the cultures.
    Lane,
}

impl BioDisplay {
    pub fn name(self) -> &'static str {
        match self {
            Self::Hidden => "Hidden",
            Self::Inline => "Inline",
            Self::Lane => "Own lane",
        }
    }
}

/// A person, with their own life events.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Biography {
    pub id: Id,
    pub name: String,
    /// The culture this person belongs to, if any. Drives inline placement.
    #[serde(default)]
    pub timeline: Option<Id>,
    pub birth: HDate,
    #[serde(default)]
    pub death: Option<HDate>,
    /// Falls back to the parent timeline's colour when `None`.
    #[serde(default)]
    pub color: Option<Rgb>,
    #[serde(default)]
    pub categories: Vec<Id>,
    #[serde(default = "default_importance")]
    pub importance: u8,
    #[serde(default)]
    pub display: BioDisplay,
    #[serde(default)]
    pub notes: String,
}

fn default_importance() -> u8 {
    3
}

impl Biography {
    pub fn span(&self) -> Span {
        Span {
            start: self.birth,
            end: self.death,
        }
    }

    pub fn life_label(&self) -> String {
        match self.death {
            Some(d) => format!("{} – {}", self.birth.label(), d.label()),
            None => format!("b. {}", self.birth.label()),
        }
    }
}

/// What an event hangs off.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum OwnerRef {
    Timeline(Id),
    Biography(Id),
}

/// A single dated entry.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: Id,
    pub owner: OwnerRef,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub span: Span,
    #[serde(default = "default_importance")]
    pub importance: u8,
    #[serde(default)]
    pub categories: Vec<Id>,
    /// Another event this one nests under — "Peace of Nicias" inside
    /// "Peloponnesian War" inside the Classical Antiquity timeline. Nesting
    /// is otherwise unrelated to `owner`, which still names the timeline or
    /// biography the whole chain ultimately belongs to.
    #[serde(default)]
    pub parent: Option<Id>,
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Category filter mode. The request asks for both directions explicitly:
/// "show only writers" and "hide everything except battles".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum FilterMode {
    #[default]
    Off,
    /// Show only entries carrying at least one selected category.
    Include,
    /// Hide entries carrying any selected category.
    Exclude,
}

impl FilterMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::Include, Self::Exclude];

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Include => "Show only",
            Self::Exclude => "Hide",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Filters {
    #[serde(default)]
    pub mode: FilterMode,
    #[serde(default)]
    pub selected: BTreeSet<Id>,
    /// Shifts the zoom-derived importance threshold. Positive shows more.
    #[serde(default)]
    pub detail_bias: i32,
    #[serde(default)]
    pub search: String,
    /// When set, entries with no categories at all are always kept.
    #[serde(default = "yes")]
    pub keep_uncategorised: bool,
}

/// View state worth remembering between sessions.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SavedView {
    pub left_year: f64,
    pub pixels_per_year: f64,
    #[serde(default)]
    pub filters: Filters,
    #[serde(default = "yes")]
    pub dark_mode: bool,
    #[serde(default = "yes")]
    pub show_labels: bool,
}

impl Default for SavedView {
    fn default() -> Self {
        Self {
            left_year: -300.0,
            pixels_per_year: 2.0,
            filters: Filters::default(),
            dark_mode: true,
            show_labels: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

pub const DOCUMENT_VERSION: u32 = 1;

/// The whole user library. One file on disk.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Document {
    #[serde(default = "document_version")]
    pub version: u32,
    #[serde(default)]
    pub next_id: u32,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub timelines: Vec<Timeline>,
    #[serde(default)]
    pub biographies: Vec<Biography>,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub view: SavedView,
}

fn document_version() -> u32 {
    DOCUMENT_VERSION
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: DOCUMENT_VERSION,
            next_id: 1,
            groups: Vec::new(),
            timelines: Vec::new(),
            biographies: Vec::new(),
            events: Vec::new(),
            categories: Vec::new(),
            view: SavedView::default(),
        }
    }
}

/// Starting categories. The user can rename, recolour, delete and add to these
/// freely; nothing in the code depends on a particular set existing.
pub const STARTER_CATEGORIES: [(&str, Rgb); 10] = [
    ("Military", [201, 88, 79]),
    ("Politics", [92, 138, 201]),
    ("Religion", [163, 122, 196]),
    ("Philosophy", [90, 170, 160]),
    ("Literature", [214, 158, 74]),
    ("Science", [98, 178, 108]),
    ("Art", [212, 122, 168]),
    ("Economy", [150, 150, 110]),
    ("Law", [120, 140, 175]),
    ("Personal", [160, 160, 170]),
];

/// Distinct band colours, cycled when the user adds timelines.
pub const TIMELINE_PALETTE: [Rgb; 10] = [
    [214, 96, 77],
    [83, 141, 213],
    [95, 178, 130],
    [216, 160, 70],
    [163, 120, 206],
    [86, 178, 190],
    [214, 122, 168],
    [150, 165, 90],
    [188, 130, 100],
    [120, 135, 190],
];

impl Document {
    /// Allocate the next free id.
    pub fn new_id(&mut self) -> Id {
        // Guard against a hand-edited file whose next_id lags behind its content.
        let highest = self
            .groups
            .iter()
            .map(|g| g.id.0)
            .chain(self.timelines.iter().map(|t| t.id.0))
            .chain(self.biographies.iter().map(|b| b.id.0))
            .chain(self.events.iter().map(|e| e.id.0))
            .chain(self.categories.iter().map(|c| c.id.0))
            .max()
            .unwrap_or(0);
        let id = self.next_id.max(highest + 1);
        self.next_id = id + 1;
        Id(id)
    }

    pub fn with_starter_categories() -> Self {
        let mut doc = Self::default();
        for (name, color) in STARTER_CATEGORIES {
            let id = doc.new_id();
            doc.categories.push(Category {
                id,
                name: name.to_string(),
                color,
            });
        }
        doc
    }

    pub fn group(&self, id: Id) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: Id) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Direct child groups of `parent` (`None` for top level), in display order.
    pub fn child_groups(&self, parent: Option<Id>) -> Vec<&Group> {
        let mut v: Vec<&Group> = self.groups.iter().filter(|g| g.parent == parent).collect();
        v.sort_by_key(|g| (g.order, g.id.0));
        v
    }

    /// Timelines directly inside `group` (`None` for ungrouped), in order.
    pub fn timelines_in(&self, group: Option<Id>) -> Vec<&Timeline> {
        let mut v: Vec<&Timeline> = self.timelines.iter().filter(|t| t.group == group).collect();
        v.sort_by_key(|t| (t.order, t.id.0));
        v
    }

    /// Every timeline id in a group's subtree.
    ///
    /// Walks defensively: a hand-edited file could contain a parent cycle, and
    /// this must terminate rather than hang the UI.
    pub fn group_timelines(&self, group: Id) -> Vec<Id> {
        let mut groups = vec![group];
        let mut seen: BTreeSet<Id> = BTreeSet::new();
        seen.insert(group);
        let mut out = Vec::new();
        while let Some(g) = groups.pop() {
            for t in self.timelines.iter().filter(|t| t.group == Some(g)) {
                out.push(t.id);
            }
            for child in self.groups.iter().filter(|c| c.parent == Some(g)) {
                if seen.insert(child.id) {
                    groups.push(child.id);
                }
            }
        }
        out
    }

    /// Would making `group` a child of `new_parent` create a cycle?
    pub fn would_cycle(&self, group: Id, new_parent: Option<Id>) -> bool {
        let mut cursor = new_parent;
        let mut hops = 0;
        while let Some(id) = cursor {
            if id == group {
                return true;
            }
            hops += 1;
            if hops > self.groups.len() + 1 {
                // Already-corrupt data; refuse the move rather than loop.
                return true;
            }
            cursor = self.group(id).and_then(|g| g.parent);
        }
        false
    }

    /// Remove a group, lifting its contents up to its parent rather than
    /// deleting them — losing timelines to a group deletion would be brutal.
    pub fn delete_group(&mut self, id: Id) {
        let parent = self.group(id).and_then(|g| g.parent);
        for t in &mut self.timelines {
            if t.group == Some(id) {
                t.group = parent;
            }
        }
        for g in &mut self.groups {
            if g.parent == Some(id) {
                g.parent = parent;
            }
        }
        self.groups.retain(|g| g.id != id);
    }

    pub fn timeline(&self, id: Id) -> Option<&Timeline> {
        self.timelines.iter().find(|t| t.id == id)
    }

    pub fn timeline_mut(&mut self, id: Id) -> Option<&mut Timeline> {
        self.timelines.iter_mut().find(|t| t.id == id)
    }

    pub fn biography(&self, id: Id) -> Option<&Biography> {
        self.biographies.iter().find(|b| b.id == id)
    }

    pub fn biography_mut(&mut self, id: Id) -> Option<&mut Biography> {
        self.biographies.iter_mut().find(|b| b.id == id)
    }

    pub fn event(&self, id: Id) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn event_mut(&mut self, id: Id) -> Option<&mut Event> {
        self.events.iter_mut().find(|e| e.id == id)
    }

    pub fn category(&self, id: Id) -> Option<&Category> {
        self.categories.iter().find(|c| c.id == id)
    }

    pub fn category_names(&self, ids: &[Id]) -> String {
        let names: Vec<&str> = ids
            .iter()
            .filter_map(|id| self.category(*id))
            .map(|c| c.name.as_str())
            .collect();
        names.join(", ")
    }

    /// Colour a biography is drawn in: its own, else its culture's, else grey.
    pub fn bio_color(&self, bio: &Biography) -> Rgb {
        bio.color
            .or_else(|| bio.timeline.and_then(|t| self.timeline(t)).map(|t| t.color))
            .unwrap_or([170, 170, 180])
    }

    pub fn owner_name(&self, owner: OwnerRef) -> String {
        match owner {
            OwnerRef::Timeline(id) => self
                .timeline(id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "(deleted timeline)".into()),
            OwnerRef::Biography(id) => self
                .biography(id)
                .map(|b| b.name.clone())
                .unwrap_or_else(|| "(deleted biography)".into()),
        }
    }

    pub fn events_of(&self, owner: OwnerRef) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.owner == owner)
    }

    /// Direct children of `parent` — "Peace of Nicias" under "Peloponnesian
    /// War" — ordered by start date.
    pub fn child_events(&self, parent: Id) -> Vec<&Event> {
        let mut v: Vec<&Event> = self.events.iter().filter(|e| e.parent == Some(parent)).collect();
        v.sort_by(|a, b| a.span.t0().partial_cmp(&b.span.t0()).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    /// Would nesting `event` under `new_parent` make it its own ancestor?
    ///
    /// Walks defensively, as with [`Self::would_cycle`]: a hand-edited file
    /// could contain a parent cycle, and this must terminate rather than hang.
    pub fn would_cycle_event(&self, event: Id, new_parent: Option<Id>) -> bool {
        let mut cursor = new_parent;
        let mut hops = 0;
        while let Some(id) = cursor {
            if id == event {
                return true;
            }
            hops += 1;
            if hops > self.events.len() + 1 {
                return true;
            }
            cursor = self.event(id).and_then(|e| e.parent);
        }
        false
    }

    /// An event whose parent no longer exists has nothing to nest under.
    fn clear_dangling_event_parents(&mut self) {
        let ids: BTreeSet<Id> = self.events.iter().map(|e| e.id).collect();
        for e in &mut self.events {
            if e.parent.is_some_and(|p| !ids.contains(&p)) {
                e.parent = None;
            }
        }
    }

    /// Remove an event, lifting its children to its own parent rather than
    /// deleting them — the same "lift contents up" rule as [`Self::delete_group`].
    pub fn delete_event(&mut self, id: Id) {
        let parent = self.event(id).and_then(|e| e.parent);
        for e in &mut self.events {
            if e.parent == Some(id) {
                e.parent = parent;
            }
        }
        self.events.retain(|e| e.id != id);
    }

    /// Remove a timeline along with everything that points at it.
    pub fn delete_timeline(&mut self, id: Id) {
        self.timelines.retain(|t| t.id != id);
        self.events
            .retain(|e| e.owner != OwnerRef::Timeline(id));
        self.clear_dangling_event_parents();
        for t in &mut self.timelines {
            if t.origin.as_ref().is_some_and(|j| j.other == id) {
                t.origin = None;
            }
            if t.merge.as_ref().is_some_and(|j| j.other == id) {
                t.merge = None;
            }
        }
        for b in &mut self.biographies {
            if b.timeline == Some(id) {
                b.timeline = None;
                // An inline bio with no parent has nothing to nest under.
                if b.display == BioDisplay::Inline {
                    b.display = BioDisplay::Lane;
                }
            }
        }
    }

    pub fn delete_biography(&mut self, id: Id) {
        self.biographies.retain(|b| b.id != id);
        self.events
            .retain(|e| e.owner != OwnerRef::Biography(id));
        self.clear_dangling_event_parents();
    }

    pub fn delete_category(&mut self, id: Id) {
        self.categories.retain(|c| c.id != id);
        for e in &mut self.events {
            e.categories.retain(|c| *c != id);
        }
        for b in &mut self.biographies {
            b.categories.retain(|c| *c != id);
        }
        self.view.filters.selected.remove(&id);
    }

    /// Extent of everything in the document, as continuous-axis years.
    pub fn extent(&self) -> Option<(f64, f64)> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut note = |a: f64, b: f64| {
            lo = lo.min(a);
            hi = hi.max(b);
        };
        for e in &self.events {
            note(e.span.t0(), e.span.t1());
        }
        for b in &self.biographies {
            let s = b.span();
            note(s.t0(), s.t1());
        }
        for t in &self.timelines {
            if let Some(s) = t.span {
                note(s.t0(), s.t1());
            }
        }
        if lo.is_finite() && hi.is_finite() {
            Some((lo, hi))
        } else {
            None
        }
    }

    pub fn next_palette_color(&self) -> Rgb {
        TIMELINE_PALETTE[self.timelines.len() % TIMELINE_PALETTE.len()]
    }

    pub fn is_empty(&self) -> bool {
        self.timelines.is_empty() && self.biographies.is_empty() && self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc_ad_axis_is_contiguous_and_ordered() {
        // 1 BC must sit immediately before 1 AD with no gap for a year zero.
        assert_eq!(HDate::year(-1).decimal(), -1.0);
        assert_eq!(HDate::year(1).decimal(), 0.0);
        assert_eq!(HDate::year(-1).decimal_end(), 0.0);
        assert!(HDate::year(-44).decimal() < HDate::year(-43).decimal());
        assert!(HDate::year(-1).decimal() < HDate::year(1).decimal());
    }

    #[test]
    fn axis_labels_round_trip_through_the_boundary() {
        assert_eq!(axis_year_label(HDate::year(-44).decimal()), "44 BC");
        assert_eq!(axis_year_label(HDate::year(1).decimal()), "1");
        assert_eq!(axis_year_label(HDate::year(1789).decimal()), "1789");
        assert_eq!(axis_year_label(HDate::year(-1).decimal()), "1 BC");
    }

    #[test]
    fn parses_the_date_forms_the_entry_field_advertises() {
        let cases: [(&str, HDate); 6] = [
            ("-44", HDate::year(-44)),
            ("44 BC", HDate::year(-44)),
            ("44 v. Chr.", HDate::year(-44)),
            ("1789", HDate::year(1789)),
            (
                "c. 250 BC",
                HDate {
                    qualifier: DateQualifier::Circa,
                    ..HDate::year(-250)
                },
            ),
            (
                "14 Jul 1789",
                HDate {
                    month: Some(7),
                    day: Some(14),
                    ..HDate::year(1789)
                },
            ),
        ];
        for (input, want) in cases {
            assert_eq!(HDate::parse(input), Some(want), "parsing {input:?}");
        }
    }

    #[test]
    fn parses_iso_dates_and_uncertainty() {
        assert_eq!(
            HDate::parse("1789-07-14"),
            Some(HDate {
                month: Some(7),
                day: Some(14),
                ..HDate::year(1789)
            })
        );
        let d = HDate::parse("c. 1200 BC ±50").unwrap();
        assert_eq!(d.year, -1200);
        assert_eq!(d.plus_minus, 50);
        assert_eq!(d.qualifier, DateQualifier::Circa);
    }

    #[test]
    fn rejects_nonsense_rather_than_inventing_a_date() {
        assert_eq!(HDate::parse(""), None);
        assert_eq!(HDate::parse("the ides of march"), None);
        assert_eq!(HDate::parse("abc"), None);
    }

    #[test]
    fn year_zero_is_normalised_away() {
        assert_eq!(HDate::year(0).year, -1);
        assert_eq!(HDate::parse("0").unwrap().year, -1);
    }

    #[test]
    fn year_only_range_end_covers_the_whole_year() {
        let s = Span::range(HDate::year(-264), HDate::year(-241));
        assert_eq!(s.t0(), -264.0);
        assert_eq!(s.t1(), -240.0); // through the end of 241 BC
    }

    #[test]
    fn new_id_recovers_from_a_hand_edited_next_id() {
        let mut doc = Document::default();
        doc.next_id = 1;
        doc.categories.push(Category {
            id: Id(77),
            name: "x".into(),
            color: [0, 0, 0],
        });
        assert_eq!(doc.new_id(), Id(78));
    }

    #[test]
    fn deleting_a_timeline_clears_dangling_references() {
        let mut doc = Document::default();
        let a = doc.new_id();
        let b = doc.new_id();
        doc.timelines.push(Timeline {
            id: a,
            name: "A".into(),
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
        doc.timelines.push(Timeline {
            id: b,
            name: "B".into(),
            color: [0, 0, 0],
            visible: true,
            group: None,
            order: 1,
            span: None,
            origin: None,
            merge: Some(Junction {
                other: a,
                date: HDate::year(-146),
                label: String::new(),
            }),
            notes: String::new(),
            epochs: Vec::new(),
        });
        let bio = doc.new_id();
        doc.biographies.push(Biography {
            id: bio,
            name: "P".into(),
            timeline: Some(a),
            birth: HDate::year(-100),
            death: None,
            color: None,
            categories: vec![],
            importance: 3,
            display: BioDisplay::Inline,
            notes: String::new(),
        });

        doc.delete_timeline(a);

        assert!(doc.timeline(b).unwrap().merge.is_none());
        assert_eq!(doc.biography(bio).unwrap().timeline, None);
        // Inline has nothing to nest under any more, so it is promoted.
        assert_eq!(doc.biography(bio).unwrap().display, BioDisplay::Lane);
    }

    fn make_event(id: Id, owner: OwnerRef, title: &str, start: i32, end: i32) -> Event {
        Event {
            id,
            owner,
            title: title.into(),
            description: String::new(),
            span: Span::range(HDate::year(start), HDate::year(end)),
            importance: 3,
            categories: vec![],
            parent: None,
        }
    }

    #[test]
    fn child_events_are_ordered_by_start_date() {
        let mut doc = Document::default();
        let owner = OwnerRef::Timeline(Id(1));
        let war = doc.new_id();
        let later = doc.new_id();
        let earlier = doc.new_id();
        doc.events.push(make_event(war, owner, "War", -431, -404));
        doc.events.push({
            let mut e = make_event(later, owner, "Later treaty", -410, -409);
            e.parent = Some(war);
            e
        });
        doc.events.push({
            let mut e = make_event(earlier, owner, "Peace of Nicias", -421, -413);
            e.parent = Some(war);
            e
        });
        let children: Vec<Id> = doc.child_events(war).iter().map(|e| e.id).collect();
        assert_eq!(children, vec![earlier, later]);
    }

    #[test]
    fn nesting_an_event_under_its_own_descendant_is_a_cycle() {
        let mut doc = Document::default();
        let owner = OwnerRef::Timeline(Id(1));
        let grandparent = doc.new_id();
        let parent = doc.new_id();
        doc.events.push(make_event(grandparent, owner, "War", -431, -404));
        doc.events.push({
            let mut e = make_event(parent, owner, "Treaty", -421, -413);
            e.parent = Some(grandparent);
            e
        });
        // Grandparent becoming a child of its own descendant is a cycle...
        assert!(doc.would_cycle_event(grandparent, Some(parent)));
        // ...but nesting a fresh event under either of them is fine.
        let child = doc.new_id();
        assert!(!doc.would_cycle_event(child, Some(parent)));
    }

    #[test]
    fn deleting_an_event_lifts_its_children_up_a_level() {
        let mut doc = Document::default();
        let owner = OwnerRef::Timeline(Id(1));
        let war = doc.new_id();
        let treaty = doc.new_id();
        let clause = doc.new_id();
        doc.events.push(make_event(war, owner, "War", -431, -404));
        doc.events.push({
            let mut e = make_event(treaty, owner, "Peace of Nicias", -421, -413);
            e.parent = Some(war);
            e
        });
        doc.events.push({
            let mut e = make_event(clause, owner, "Return of Pylos", -421, -421);
            e.parent = Some(treaty);
            e
        });

        doc.delete_event(treaty);

        assert!(doc.event(treaty).is_none());
        // The clause moves up to the treaty's own parent — the war — rather
        // than being deleted or left dangling.
        assert_eq!(doc.event(clause).unwrap().parent, Some(war));
    }

    #[test]
    fn deleting_a_timeline_clears_dangling_event_parents() {
        let mut doc = Document::default();
        let owner = OwnerRef::Timeline(Id(1));
        let war = doc.new_id();
        doc.events.push(make_event(war, owner, "War", -431, -404));
        let other_owner = OwnerRef::Timeline(Id(2));
        let orphan = doc.new_id();
        doc.events.push({
            // Points at `war` even though it belongs to a different timeline —
            // pathological, but must not be left dangling after `war` is gone.
            let mut e = make_event(orphan, other_owner, "Orphan", -420, -410);
            e.parent = Some(war);
            e
        });

        doc.delete_timeline(Id(1));

        assert_eq!(doc.event(orphan).unwrap().parent, None);
    }
}
