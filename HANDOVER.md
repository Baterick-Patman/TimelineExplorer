# Handover — Timeline Explorer

Written for whoever picks this up next in VS Code. [README.md](README.md) covers
*what* the app is and *why* the stack was chosen; this file covers *how to work
on it* — environment, gotchas, and the traps that cost time the first time round.

---

## 1. State of play

Working and complete against `timeline_app_planning.md`, all four phases:
multiple parallel timelines, biographies (inline and own-lane), fuzzy dates,
categories with include/exclude filtering, zoom-dependent importance, and
converging/diverging timelines. Several later requests are also in: nestable
super-categories ("groups"), single-year-resolution work on biographies,
colour-coded epochs (with their name painted directly on the band, not just
the colour), nestable events, nestable categories (ticking a parent category
in the filter cascades onto its subcategories), European `14.07.1789`-style
date entry alongside ISO, biography bands with a category-driven fill and a
culture-driven border, a sidebar Timelines/Biographies search with
biographies collapsible by culture or by category — each cluster also
getting a bundled "alle anzeigen"/"alle ausblenden" pair, the same bulk
show/hide a `Group`'s own visibility checkbox already gives timelines — a
full German UI, and export of a chosen slice (timelines, date range, minimum
importance, with or without biographies) to PNG or PDF — item 1 from the
original planning document's open questions, previously deliberately left
undone.

**One-parent-many-children splits/merges were already fully supported and
needed no new code** — `Timeline.origin`/`.merge` are per-timeline fields, so
several timelines can split from the same parent (at the same or different
dates) and each independently merge into a different target at a different
date; `layout::tests::several_timelines_can_split_from_one_parent_and_merge_into_different_targets`
now pins this down explicitly (it wasn't before — only the single-child case
had a test). Only gotcha found while writing that test: at zoomed-out views,
two nearby transition dates' easing windows (`TRANSITION_PX`, 110px) can
visually overlap — correct, not a bug, but worth knowing before assuming
something is broken from a screenshot taken zoomed far out.

A further batch on top of that: a real bug fix (a collapsed group used to
silently drop a member's connection to something outside it — see below),
group reordering (siblings, same Up/Down shape as timelines already had)
plus a best-effort "tidy" heuristic that nudges connected groups next to
each other, search suggestions that jump the canvas straight to the match
(reveal + pan + zoom, not just filter the current list), a real Google-style
autocomplete dropdown under all three search fields, and bulk import of
events or biographies from a pasted table or a URL — the one place in the
app that now touches the network, and only when the user explicitly asks it
to.

Latest batch, driven by hands-on use of the import feature and a crowded
biography lane (many Roman emperors): the import dialog is now scrollable
on a small screen (only the middle content scrolls, the Abbrechen/Importieren
buttons stay outside it and always reachable); `HDate::parse` accepts a
spelled-out month in either order (`"14 July 1789"` / `"July 14, 1789"`) —
a real pre-existing bug, not just a missing feature, since the old parser
silently mis-parsed that order; import gained a "Kategorie für alle" bulk
category (Events and Biographies both), layered on top of — not replacing —
any per-row column-mapped category; a real bug fix where an event's label
and leader line stayed anchored to a lane's flat resting position instead of
following the band's curve near an origin/merge transition (see below); the
"+" toolbar buttons now default a new group/timeline's parent group, or a
new biography's timeline, to whatever is currently selected in the sidebar;
and a biography-rendering overhaul — see below — covering on-band names,
epoch-style life-phase colouring, and zoom-responsive lane thickness with a
click-to-pin-open mechanism.

One more small batch on top of that: the "Verbundene Gruppen zusammenrücken"
tidy button turned out to only ever reorder *top-level* groups — connected
cultures sitting as subgroups of a shared parent (the far more common case,
e.g. two Greek-antiquity cultures both nested under "Antike") were silently
never touched, since `suggest_group_order(doc, None)` alone has no way to see
into a subgroup's own sibling list. Fixed by recursing the same heuristic
into every level (`layout::tidy_all_group_levels`). Also: nested/range events
now collapse to a plain point-and-label once their own span has zoomed down
to a sliver (`layout::range_collapsed`), so a years-long war does not sit
there as an unreadable pixel-wide bar with its own sub-phases crammed
underneath — see below; and table import can now target an existing range
event directly (`ImportForm::nest_under`), so a table of a war's phases can
be imported straight into that war's own event instead of the timeline's top
level. Two tests lock in that the whole save-file schema stays additive-only
going forward (see the dedicated section on this below) — this shipped as
v0.7.0.

Immediately on top of *that* release, four more fixes from the next round of
hands-on use: `HDate::parse` now understands German seasons (`Sommer`,
`Herbst`, with `früh`/`spät` prefixes for early/late) and calendar
quarters/halves (`1. Quartal`, `Q1`, `1. Hälfte`, `Halbjahr` — see
`month_from_period`/`fuse_period_tokens`), each mapped to a representative
month and automatically qualified `Circa`; selecting a range event and
adding a new one now defaults to nesting the new one inside it
(`TimelineApp::default_parent_event`), since forgetting the "Verschachtelt
in:" field was an easy way to end up with a sub-event that silently wasn't
nested at all; every on-canvas label switched from a tint of its own lane's
colour to the theme's plain neutral text colour, since a light band (cyan,
say) tinted the same way its label was produced barely-legible text once the
two sat close together (`theme::label_color` is gone entirely, along with
the now-untrue `Theme.dark` field it was the last reader of); and an
epoch's or life phase's label now disappears purely by its own on-screen
duration rather than by whether its *particular name* happens to fit —
see the entry below on `SEGMENT_LABEL_MIN_PX`/`fit_text`.

Same-day follow-up, once real data with real epoch counts and real
origin/merge chains started exposing the next layer of rough edges: **every**
modal dialog (event/group/timeline/biography/category/export, not just
import) now caps its middle content in a height-responsive `ScrollArea`, the
same fix import got earlier — a timeline with nine epochs used to overflow
the window with no way to reach "Speichern." An origin and a merge close
together in time (a short-lived successor timeline) used to produce a
visibly wrong curve once zoomed out far enough for their two 110px-in-years
easing windows to overlap and compound — `layout::transition_window` now
caps each at half the gap between the two dates, so they never touch (see
its test, `an_origin_and_merge_close_together_do_not_overlap_when_zoomed_far_out`).
Epoch names are now repainted in their own pass, `canvas::paint_epoch_labels`,
strictly after every band (including any other timeline's curve) — they used
to be painted inline as part of the same first "bands" pass as everything
else, so a curve travelling several lanes to a distant merge target could
paint directly over a nearer timeline's epoch label along the way, purely
because of lane iteration order. A long event title no longer grows
arbitrarily wide; it ellipsises past `EVENT_LABEL_MAX_PX` the same way an
epoch name does. And a lane's sticky name tag now disappears entirely
(instead of just dimming) once nothing on that lane — band or events — is in
the current view, so a short-lived timeline's name doesn't stay pinned to
the screen centuries after it stopped existing. **v0.8.0.**

Another same-day round, once real data with real density (23 events packed
into a 10-year window, dozens of Egyptian dynasties) exposed the next layer:
event label font size now grows with zoom on top of its per-importance
baseline (capped so importance still reads as a size hierarchy — see
`theme::label_font_size`'s `LABEL_ZOOM_GROWTH_*` constants — and
`LABEL_ROW_HEIGHT` bumped to stay tall enough for the new maximum); several
events sharing one bare year (no month of their own) used to all draw on the
exact same pixel, since `HDate::decimal` resolves a year-only date to its
very start — `layout::fan_out_year_only_events` now spreads them evenly
across that year, ordered by id (creation order); a nested event's title
had no background at all, so on a range event whose bar sits close to its
own timeline's band, the title painted directly over the band's colour with
nothing guaranteeing contrast; biographies are now visually closer to a
nested range event's own slim bar than to a full culture band; a range
event's label ("Peloponnesischer Krieg") now stays centred on whatever
portion of its span is actually on screen and scrolls with it, the same way
an epoch's name already tracks its visible segment, instead of anchoring to
the start date and scrolling off-screen the moment you pan into the middle
of a wide range; and the ruler now labels ticks down to individual days —
`model::date_from_decimal` (the inverse of `HDate::decimal`) plus
`axis_tick_label` switch precision to match the current tick step, and
`tick_step`'s ladder gained day/month/season/half-year rungs below its
original whole-year floor. `MAX_PPY` raised from 4,000 to 60,000 so day-level
ticks are actually reachable, not just mathematically defined.

**Deliberately not done in that pass** — each was a substantial feature in
its own right, and squeezing it in alongside everything above risked doing
it half-right: nested events that are themselves ranges (e.g. "Archidamischer
Krieg" inside "Peloponnesischer Krieg") still rendered as small rows below
their parent rather than as an epoch-style coloured segment *on* the
parent's own bar; there was no dedicated fast-zoom slider next to the detail
bias slider; and the top-left search did not yet parse a typed date and
jump to it directly. The first and third of those are done now (next
paragraph); the fast-zoom slider is still the one open item — see §7.

The very next round of hands-on testing (still same major version, no
release cut between the two) produced an eight-item batch, all now done:
the sidebar group-label bug turned out to still reproduce — `measure_lanes`
computed `active` as `plan.header_only || lane_active(...)`, which forced an
*expanded* group's own header row to always report itself active regardless
of whether anything under it was actually in view, so its sticky name never
disappeared the way a dormant timeline's already did; removing the
`header_only ||` short-circuit was the whole fix (`lane_height` and
`paint_lane_events` already gate on `header_only` independently, so nothing
else needed to change). Nested events now render directly **on** their
parent's own bar instead of in rows underneath it — the redesign promised
above — see the dedicated section below. Import gained "Anfang"/"Beginn"/
"Ende" + a year (placing the date at that year's first or last month,
`Circa`-qualified, the same pattern as seasons/quarters) and a visual
red-tint + tooltip on any preview row that fails to parse, so a bad row can
be found and fixed in the pasted text without guessing which of possibly
hundreds of rows it was. The top-left search now parses a typed date
directly (leaning entirely on the existing `HDate::parse` — no new format
support was needed, just a place to feed it a date instead of only a name)
and can also find and jump to an epoch or a biography's life phase, not just
events/timelines/biographies/groups. A long event title now wraps onto a
second line instead of immediately ellipsising once it will not fit
verbatim — see the section below on `LabelPacker::place_rows`.

The screenshots that came back from testing that batch were all still taken
against the *previous* release — every one of them showed exactly the
pre-fix behaviour (the group label still not disappearing, the nested event
still drawn in a row below rather than as a segment on the parent's own bar,
search-by-date visibly doing nothing), which is expected since nothing above
had been released yet. Genuinely new out of that same round, though: two
real, previously-unnoticed date-parsing bugs, a biography life-phase label
visibility fix, and two follow-on refinements to the nested-events-on-band
redesign once a closer look at a denser dataset showed it needed them —
covered in their own dedicated sections below.

Asked to "implement everything" immediately after, including the one item
flagged above as explicitly not attempted: the canvas is now reorganised so
a long event with its own sub-structure gets a dedicated stacked slot
*above* its timeline's band, plain single events moved *below* it, and
several overlapping long events stack progressively higher, each pushing
`layout::lane_height`/`place_lanes` to reserve more room, which — for free,
since lanes already stack sequentially — pushes every following
timeline/group lane further down to make room. This did end up touching
`LaneDemand`/`lane_height`/`place_lanes` exactly as predicted; see the
dedicated section below for the full design and its tradeoffs.

With the feature list essentially complete, the round after that was an
explicit pre-1.0 QA pass rather than a new request: a deliberately
stress-heavy throwaway document (deep nesting, several mutually-overlapping
long events, a single absurdly long word, a zero-duration range, the BC/AD
boundary, a collapsed group, an open-ended biography) screenshotted across
many zoom levels and pan positions. Found and fixed four more real bugs, all
of them in the two features built earlier this session under load neither
had actually been tested against — see the dedicated section below,
"A pre-1.0 QA pass found four more real bugs." Light mode was checked once,
found readable, and explicitly deprioritised for further scrutiny per
direct instruction — don't read the lack of deeper light-mode testing here
as "it's broken," just as "not this round's focus." **v1.0.0.**

- **~13,600 lines** of Rust across 14 files in `src/`.
- **163 tests**, all passing, no compiler warnings (5 pre-existing clippy
  style lints — `derivable_impls`, `collapsible_if`,
  `field_reassign_with_default` — left as-is, not regressions).
- Release binary: `target/release/timeline_explorer.exe`, single file, ~9 MB
  (image encoding, `ureq`+`rustls` for the optional URL fetch, and
  `scraper`+`html5ever` for HTML table parsing account for the growth from
  the ~6.4 MB the first release shipped at — still one file, no installer).

### Export reuses the real canvas painter — it does not re-render anything

`export.rs` + `TimelineApp::start_export`/`tick_export`/`finish_export` (in
`app.rs`) do **not** re-implement any drawing. The only way to get pixel-perfect
parity with the live app — same fonts, same curve/label code, no risk of the
export drifting from what `canvas.rs` actually draws — is to let `canvas.rs`
draw it and capture that. The flow, across several frames:

1. `start_export` swaps `app.doc` for an already-filtered clone
   (`export::build_export_document`) and points the view at the chosen range
   (`export::export_axis`), bypassing `mutate`/`mark_dirty` — this is a
   transient render, not an edit, and must never enter the undo stack or get
   autosaved over the real library.
2. While `app.export_job` is `Some`, `TimelineApp::ui()` **replaces the whole
   panel layout** with just the canvas — no toolbar, sidebar, inspector, or
   status bar — and skips `autosave_if_due` and `show_dialogs`/`show_confirm`
   entirely. The eventual screenshot must contain nothing else.
3. `ExportStage::Preparing` exists solely to eat one frame of lag: the
   document swap happens *inside* a button click handler, which runs *after*
   that frame's canvas draw, so `last_lanes` on that exact frame still
   reflects the *old* document. Only from the next frame on is `last_lanes`
   trustworthy for the new one.
4. `Measuring` reads `last_lanes` (now correct) to get the exact content
   height, then sends `ViewportCommand::InnerSize` to resize the OS window to
   fit it precisely — no cropping needed since the canvas already fills the
   whole (panel-less) window.
5. `Settling` waits a few frames for the resize to actually land (window
   managers do not apply `InnerSize` synchronously).
6. `Capturing` sends `ViewportCommand::Screenshot` and polls
   `ctx.input(|i| i.events)` for the matching `Event::Screenshot` each frame
   until it arrives.
7. `finish_export` encodes the returned `ColorImage` — PNG directly via the
   `image` crate, PDF by JPEG-encoding it and wrapping that in a **hand-rolled**
   single-page PDF (`export::wrap_jpeg_as_pdf`) rather than pulling in a
   PDF-writing crate; a one-image PDF is a small, fixed structure, and
   `printpdf`'s page/content-stream API was still enough of a moving target
   across recent versions (checked against its 0.12.5 docs while building
   this) that hand-writing it was the lower-risk choice. Verified by writing
   a real gradient test image through both paths and opening the resulting
   files directly — both render correctly.
8. Either way, the real document, `y_offset`, `selection`, and window size are
   restored, whether the capture succeeded or the format was Png/Pdf.

**Not verified end-to-end in this environment**: steps 2–6 (the window-resize
→ settle → screenshot dance) could not be exercised by actually driving the
UI — this dev sandbox's input automation is blocked from taking real
foreground focus (by design; a forced-focus attempt was flagged and blocked
by antivirus, correctly). Everything **outside** that interactive loop is
covered by unit tests and by writing real files through both output paths
and opening them (`export::tests`, all passing) — the document filtering,
axis math, JPEG encoding, and PDF structure are all confirmed correct. If
export produces a blank, wrongly-sized, or stale-content image the first time
someone actually runs it, look at steps 2–6 first, in particular whether
`Settling`'s frame count (currently 4) is enough on slower machines/drivers —
it is a guess, not a measurement.

### The UI is German-only, by request — not a translation *layer*

Every user-facing string (menus, dialogs, toasts, tooltips, the ruler's
`v. Chr.`/`n. Chr.`-style year labels, `STARTER_CATEGORIES`) was translated
in place. This was a deliberate choice over building an i18n system: the app
has exactly one intended UI language, so a string catalogue plus a language
switch would have been pure overhead. If English (or a togglable second
language) is ever wanted back, that is a real feature to design, not a
revert — search each file for the German strings and reintroduce a catalogue
rather than trying to mechanically undo this.

**Scope drawn deliberately:** code identifiers, comments, doc comments, and
test fixture data (event/timeline names like `"Second Punic War"` in
`example.rs` and in unit tests) were left in English — only what a user
actually sees was translated. `STARTER_CATEGORIES` is the one exception
worth knowing about: those names are also looked up by exact string in
`example::build()` (`cat(&doc, "Politik")` etc.), so renaming a starter
category requires updating both places together, or `example::build()`
silently fails to tag anything.

**If you touch date formatting**, `year_label`/`axis_year_label` produce
`"{n} v. Chr."` for BC/BCE years now, not `"{n} BC"` — the corresponding
assertions in `model.rs`'s tests were updated to match. `HDate::parse` still
accepts English input (`44 BC`, `1789-07-14`) unchanged; only *display*
moved to German. Curly quotes (`"`/`"`) are reused everywhere a quoted name
appears in German text, deliberately **not** the German-typographic `„`/`"`
pair — `"`/`"` were already verified safe in egui's bundled font (see the
font caveat below); `„` was never tested and there was no way to verify it
renders without a live screenshot loop, so it was avoided rather than risking
tofu boxes in every confirmation dialog.

**Not implemented:** export to image/PDF (open question 5 in the planning
document, deliberately left for a decision rather than guessed at), and
day/month-precision zoom (the ruler bottoms out at whole-year ticks even
though events carry day precision internally — deferred deliberately, see
item 2 in §7 below, as the display could get crowded at that resolution).

### Git

There is a git repo at the project root but **no commits yet** — the working
tree is entirely untracked. `.gitignore` contains only `/target`. First job is
probably:

```bash
git add -A && git commit -m "Timeline Explorer: initial implementation"
```

Do check `Cargo.lock` goes in (it should — this is a binary, not a library).

---

## 2. VS Code setup

### Extensions

- **rust-analyzer** (`rust-lang.rust-analyzer`) — essential.
- **CodeLLDB** (`vadimcn.vscode-lldb`) — debugging. On Windows/MSVC the
  Microsoft **C/C++** extension (`ms-vscode.cpptools`) is the more reliable
  debugger; pick either, config for both below.
- **Even Better TOML** (`tamasfe.even-better-toml`) — for `Cargo.toml`.

### `.vscode/settings.json`

```json
{
  "rust-analyzer.check.command": "clippy",
  "rust-analyzer.cargo.features": "all",
  "files.trimTrailingWhitespace": true,
  "[rust]": {
    "editor.formatOnSave": true,
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

### `.vscode/launch.json`

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "cppvsdbg",
      "request": "launch",
      "name": "Debug Timeline Explorer",
      "program": "${workspaceFolder}/target/debug/timeline_explorer.exe",
      "args": [],
      "cwd": "${workspaceFolder}",
      "preLaunchTask": "cargo build"
    }
  ]
}
```

### `.vscode/tasks.json`

```json
{
  "version": "2.0.0",
  "tasks": [
    { "label": "cargo build", "type": "shell", "command": "cargo build", "group": "build",
      "problemMatcher": ["$rustc"] },
    { "label": "cargo test", "type": "shell", "command": "cargo test", "group": "test",
      "problemMatcher": ["$rustc"] },
    { "label": "cargo build --release", "type": "shell", "command": "cargo build --release",
      "problemMatcher": ["$rustc"] }
  ]
}
```

### Gotcha: there is no console

`src/main.rs` starts with `#![windows_subsystem = "windows"]` so no console
window appears when the user double-clicks the exe. **`println!`/`dbg!` output
goes nowhere in a normal run.** Options while debugging:

- Comment that line out temporarily, or
- Run under `cargo run` from an already-open terminal (output still suppressed
  on Windows — the attribute is compile-time), so realistically: comment it out,
  or write to a file, or surface the value in the UI status bar.

The status bar already prints the live axis (`showing 560 BC – 58 BC (2.20
px/yr)`), which was added precisely because it was the fastest way to see
internal state without a console. Extend it the same way when debugging.

---

## 3. Build, test, run

```bash
cargo build            # debug
cargo test             # 159 tests, ~0.2s, all pure logic — no window needed
cargo build --release  # the shippable single exe
```

Release builds take ~60s (LTO + single codegen unit). Debug builds are fast.

The release profile in `Cargo.toml` sets `panic = "abort"`, `lto = true`,
`strip = true`. `.cargo/config.toml` adds `-C target-feature=+crt-static`, which
is what removes the VC++ redistributable dependency. **Don't delete
`.cargo/config.toml`** — without it the exe needs `vcruntime140.dll` and the
single-file promise breaks.

To verify the single-exe property after a change, check the PE import table
contains only Windows-shipped DLLs (no `vcruntime`, `msvcp`, `api-ms-win-crt`).

---

## 4. Code map

Rough dependency order — `model` and `layout` are the load-bearing parts.

| File | Lines | What it owns |
| --- | --- | --- |
| `layout.rs` | 2540 | Time axis, tick steps, visibility rules, lane planning/placement, band curves, label packing. **No painting** — that's why it holds the largest share of the tests. |
| `forms.rs` | 2299 | Modal editors for group/timeline/biography/event/categories/export/import. |
| `model.rs` | 2052 | Data model + serde. Dates, spans, timelines, groups, biographies, events, categories, filters. **No UI, no geometry.** |
| `canvas.rs` | 1818 | All painting of the timeline surface + canvas input handling. **No test module at all** — see §6. |
| `app.rs` | 1392 | `TimelineApp` state, undo/redo, autosave, menus, keyboard shortcuts, top-level layout, jump targets. |
| `panels.rs` | 1255 | Sidebar (group tree, biographies, filters) and inspector. |
| `example.rs` | 604 | The optional worked example dataset. |
| `store.rs` | 477 | Load/save, atomic replace, rotating backups. |
| `export.rs` | 410 | PNG/PDF export by driving the real canvas painter and capturing a screenshot. |
| `theme.rs` | 360 | Palette, importance→size/opacity encoding, egui `Visuals` override. |
| `import.rs` | 345 | Bulk import: table parsing, HTML table extraction, column-guessing, lenient draft-building. The only file with network code (`fetch_url`, opt-in). |
| `main.rs` | 44 | Entry point, window options. |

### The layering rule worth preserving

`model.rs` and `layout.rs` deliberately contain **no egui painting calls**. That
is what makes the hard parts (BC/AD arithmetic, merge curves, zoom thresholds,
lane stacking) testable without a window. If you find yourself wanting a
`Painter` in `layout.rs`, push the measurement out to `canvas.rs` and pass the
result in — that is exactly how `measure_lanes` → `place_lanes` works.

---

## 5. Traps and non-obvious decisions

### egui 0.36 is not the egui you remember

This was written against **egui/eframe 0.36.1**, which renamed a lot. If you
consult older tutorials or an LLM trained on earlier versions, expect these:

| Older egui | 0.36 |
| --- | --- |
| `App::update(&mut self, ctx, frame)` | `App::ui(&mut self, ui, frame)` |
| `TopBottomPanel` / `SidePanel` | unified `egui::Panel::top/bottom/left/right` |
| `.default_width()` / `.width_range()` | `.default_size()` / `.size_range()` |
| `.exact_height()` | `.exact_size()` |
| `SelectableLabel::new(sel, txt)` | `Button::selectable(sel, txt)` |
| `show_tooltip_at_pointer(..)` | `Tooltip::for_widget(&resp).at_pointer().show(..)` |
| `ctx.wants_keyboard_input()` | `ctx.egui_wants_keyboard_input()` |
| `ctx.screen_rect()` | `ctx.content_rect()` |
| `rounding` | `corner_radius` |
| `on_exit(&mut self)` | `on_exit(&mut self, Option<&glow::Context>)` |
| panels took `&Context` | panels take `&mut Ui` |
| `Sense::click()` etc. | still exist, but `Sense` is now bitflags (`Sense::CLICK \| Sense::DRAG`) |

**When in doubt, read the crate source** rather than guessing — it is on disk at
`~/.cargo/registry/src/index.crates.io-*/egui-0.36.1/src/`. That was faster than
trial-and-error compiling every time.

### Fonts: only what egui bundles

Non-ASCII glyphs render as tofu boxes unless they are in egui's default fonts.
Fullwidth `＋` (U+FF0B) and `－` (U+FF0D) shipped as empty boxes in the first
build and had to be replaced with ASCII. **Assume `🗑 ▲ ▼ ✕ → 👤 ▸` are unsafe**
and stick to ASCII or plain words in UI strings. Safe and verified in use:
`— · “ ” ± ≥ … • –`.

There is also **no bold face** in the bundled fonts. Importance is therefore
encoded with size + marker size + opacity, not weight. If real bold is ever
wanted, a font must be embedded via `include_bytes!` — check the licence, and
note that loading from `C:\Windows\Fonts` at runtime avoids redistribution but
adds a fallback path.

### The BC/AD axis has no year zero

`HDate::decimal()` maps AD year *y* to `[y-1, y)` and BC year *y* to
`[-y, -y+1)`. This keeps the boundary contiguous — 1 BC ends exactly where 1 AD
begins — while `year` stays in historical numbering (`-44` means 44 BC, and 0 is
normalised away to 1 BC). Tests pin this down; **don't "simplify" it to
astronomical numbering** without updating `axis_year_label` and the tests
together.

### Lane layout is two-pass, on purpose

`plan_lanes()` → `measure_lanes()` (in `canvas.rs`, needs fonts) →
`place_lanes()`. The middle pass measures how many label rows each lane actually
needs at the current zoom. This is what lets a dense stretch grow instead of
silently dropping labels, and lets a lane with nothing in the visible window
collapse to a slim dimmed row. If you add a new lane type, it needs a
`LanePlan`, an arm in `lane_owners()`, and an arm in `lane_active()`.

`LABEL_ROW_HEIGHT` must stay ≥ the largest label's line height or big titles
overlap the row above. There is a guard test for exactly this in `theme.rs`.

### Categories cascade through a second, expanded `Filters`, not the stored one

`Category.parent` makes categories nestable, the same "Inside:" pattern as
`Group`. The one behavioural effect of nesting: ticking a parent category in
the Include/Exclude filter should also cover its subcategories, so the user
does not have to tick "Domestic politics" and "Foreign politics" separately
after ticking "Politics".

This is deliberately **not** implemented by mutating `Filters.selected`
itself. `Document::effective_filters()` returns a clone with `selected`
expanded to include every descendant of a selected category;
`canvas::draw()` computes this once per frame and threads it through
`plan_lanes` / `measure_lanes` / `paint_lane_events`. `panels::filters_section`
and the sidebar checkboxes still read `doc.view.filters` (the raw, un-expanded
set) directly — a subcategory's own checkbox must not appear ticked just
because its parent's is, the same way a collapsed group does not flip its
members' own `visible` flags. **If you add a new place that decides
visibility from categories, make sure it receives the expanded `Filters`, not
`doc.view.filters` directly** — `paint_lane_events` used to fetch the raw one
itself before this was added, which would have silently ignored the cascade.

### An epoch segment's name, not its colour, is the ground truth for "is this a gap"

`band_color_segments` returns `(from, to, colour, name)`, where `name` is
`None` for the base-colour filler between/around epochs and `Some` for an
actual epoch. Painting used to skip a segment by comparing `colour == tl.color`
— that breaks if a user ever colours an epoch to match its timeline's own
colour (unlikely, but not invalid), silently dropping both the stroke *and*
the label for it. Compare on `name.is_none()` instead if you touch this code.

The epoch name itself is painted with `canvas::epoch_segment_label`, centred
on the segment and skipped if the segment is narrower than the label —
deliberately placed *on* the band (its own pill, `theme.canvas_bg` behind it)
rather than in the label rows above, so it reads as structural (like a lane
name) rather than as an event title.

### Sidebar search strings live on `TimelineApp`, not `Document`

`timeline_search` / `bio_search` / `bio_group_by` are deliberately session-only
UI state on `TimelineApp`, not part of `Document` — they narrow a long
sidebar list while you work, not something worth remembering between
launches (unlike the canvas's own `view.filters.search`, which *is*
persisted). `panels::sidebar()` takes them out of `app` with `mem::take`
(or a plain copy, for the `Copy` `bio_group_by`) before the section
functions run, and writes them back after — that is what lets
`timelines_section` / `biographies_section` keep taking `app: &TimelineApp`
like every other panel function here, instead of needing `&mut TimelineApp`
just to update two strings and an enum.

Biography clustering (`panels::bio_cluster`) is keyed on `(id_salt, Id)`,
passed to `egui::collapsing_header::CollapsingState::load_with_default_open`
— egui persists the open/closed state per id across frames automatically, no
extra bookkeeping needed. It uses `CollapsingState` directly (not the
higher-level `CollapsingHeader`) specifically so the header row can carry
the "alle anzeigen"/"alle ausblenden" buttons alongside the label —
`CollapsingHeader` only exposes that via `.show_header()`, which is actually
a `CollapsingState` method reached through a type coercion, not something
`CollapsingHeader` itself has; reaching for `CollapsingState` up front avoids
that trap. Category clusters are **not** a partition: a biography with
several categories appears in each matching cluster, unlike culture
clustering where each biography has exactly one (or none).

### A collapsed group used to silently drop a member's outside connection

If timeline A (inside a collapsed group) had `merge.other` pointing at
something outside the group, the merge curve simply vanished the moment the
group collapsed — `paint_group_lane`'s collapsed branch drew one flat summary
rectangle and nothing else, with no idea any of its members had an
`origin`/`merge` at all. Fixed by `layout::group_external_junctions`, which
walks every member and reports each origin/merge whose *other end* is not
itself inside the group; `canvas::paint_group_lane` then synthesizes a
throwaway `Timeline` carrying just that one junction and feeds it through the
same `band_curve` a normal band uses, so the curve eases out of the flat
summary band exactly the way it would out of the member's own lane if the
group were expanded. No new curve maths — same easing function, same
`TRANSITION_PX` window, just fed a stand-in `Timeline` instead of a real one.

### Group ordering is manual by default, with a best-effort "tidy" on top

`panels::reorder_group` mirrors the timeline `reorder` that already existed
(Up/Down among same-parent siblings, dense renumbering) — this was a real
gap, not a design choice; groups had an `order` field since the original
implementation but nothing in the UI ever changed it.

`layout::suggest_group_order` is a separate, opt-in "Verbundene Gruppen
zusammenrücken" action (a button, not automatic background behaviour) that
greedily chains top-level groups so ones connected by an origin/merge end up
adjacent — cuts down on a merge curve visually crossing through unrelated
bands. **This is deliberately not a general crossing-minimisation solver**
(that's a hard graph-layout problem) — just a greedy nearest-neighbour chain,
and it only reorders siblings at one level (called on the top level from the
UI; it takes a `parent` argument, so recursing into subgroups is a small
follow-up if it's ever wanted, not a redesign). Groups with no cross-group
connection at all keep their existing relative order rather than being
shuffled for no reason — see the two tests next to it in `layout.rs` for the
exact guarantee.

### Search suggestions jump the canvas, they don't just filter

`app::JumpTarget` (Event/Biography/Timeline/Group) is deliberately a
separate type from `Selection`, even though the variants line up one to
one — a jump target is "what to reveal and frame," a selection is "what the
inspector shows right now," and keeping them apart leaves room for a jump
target later that isn't a valid `Selection` (an epoch, say) without having
to touch `Selection` itself.

`TimelineApp::jump_to` does two genuinely separate things and is careful to
route them through the app's two different "this changed" channels
correctly: revealing whatever stands between the target and being visible
(un-hiding the timeline, expanding every ancestor group, restoring a
`Hidden` biography to `Inline`/`Lane`) goes through `mutate` — a real,
undoable document edit, the same as any other visibility toggle in this
app — while the resulting pan/zoom goes through `mark_dirty` only, same as
`fit_to_content`, since view state was already established as "persists,
but doesn't clutter undo." Getting this split wrong in either direction
would be a real regression: routing the reveal through `mark_dirty` only
would make an unwanted "oh, and it un-collapsed three groups" impossible to
undo; routing the pan/zoom through `mutate` would spam the undo stack with
one entry per search.

`reveal_jump_target` is pure (`&mut Document`, no `TimelineApp`) and has its
own tests in `app.rs` — cheaper to verify this way than by driving the real
UI, and it already caught the one subtlety worth knowing: revealing a
biography must only touch its `display` if it was `Hidden`. An
already-visible one (shown as `Lane` despite having a culture, say) must not
be silently switched to `Inline` just because jumping to it noticed it has
one.

### `JumpTarget` grew an `Epoch` and a `Date` variant — neither is a `Selection`

The doc comment above `JumpTarget` had, from the start, explicitly left room
for "a jump target later that isn't a valid `Selection` (an epoch, say)" —
this is that. `Epoch(OwnerRef, usize)` addresses one of a timeline's
`epochs` or a biography's `life_phases` by its owner plus index (`Epoch`
itself carries no `Id` — it is only ever edited as a `Vec<EpochRow>` inside
its owner's form), and `Date(HDate)` represents a date typed straight into
the search field rather than any existing name match.

The old `impl From<JumpTarget> for Selection` could not survive this as-is —
a bare typed date has no sensible `Selection` at all. It became an inherent
`JumpTarget::selection(self) -> Option<Selection>` instead: an `Epoch`
selects its *owning* timeline/biography (there being no dedicated inspector
view for an epoch on its own), `Date` selects nothing (`None`, which clears
whatever was selected before — deliberate, since jumping to a bare date
while an unrelated inspector stays open would be confusing). `jump_anchor`
and `reveal_jump_target` both grew matching arms — a `Date` needs no
revealing at all (nothing is hidden about a bare point in time) and anchors
directly at `d.decimal()`; an `Epoch` reveals its owner exactly like an
`Event` does. `JumpTarget` also lost its `Eq` derive (kept `PartialEq`) —
`HDate` itself doesn't derive `Eq`, so `Date(HDate)` would have broken it;
nothing in the codebase compared `JumpTarget`s for equality outside its own
struct-literal tests, so this cost nothing.

The top-left search field's candidate list (`app.rs`, inside the toolbar's
`ui.horizontal` closure building `Suche:`) now also chains in every
timeline's epochs and every biography's life phases, each mapped to
`JumpTarget::Epoch`. Separately — outside the name-matching `suggestions()`
popup entirely — if nothing matched by name at all, the trimmed query is
tried through `HDate::parse` directly; on success a small hint
(`↵ Enter: springe zu …`) appears and Enter jumps straight to that date via
`JumpTarget::Date`. Deliberately gated on "no name matches" rather than
always overriding the name-match path: a short numeric query like `14` is
both a plausible bare-year date *and* plausibly a substring someone is
searching for in a longer title (`"...1914..."`), and preferring the
existing name-suggestion behaviour whenever it has anything to offer avoids
silently changing what Enter already did for every non-date search. No new
date-format parsing was needed for this — `HDate::parse` already understood
every format asked for (spelled/numeric month, seasons, `v. Chr.`/`n. Chr.`
with or without a space, `BC`/`AD`); the field just never tried feeding it a
date before.

### The suggestion dropdown is a real `egui::Popup`, not an inline list

`panels::suggestions` anchors a floating popup below the search field via
`egui::Popup::from_response(resp).align(RectAlign::BOTTOM_START)`, forcing
`.open(true)` explicitly rather than relying on the popup's own
click-tracked memory (which is what `from_response` alone would leave it to,
and does not match "open exactly when this field has focus and the query
has matches"). It reuses one generic helper across all three search fields
(top canvas search, sidebar timeline/group search, sidebar biography search)
by being generic over the payload type — the top search's candidates are a
`JumpTarget` (heterogeneous: events, biographies, timelines, groups all in
one list), the sidebar ones are also `JumpTarget` now so that picking a
sidebar suggestion jumps the canvas too, for consistency, even though the
sidebar's own live-filtered tree already narrows things down on its own.
Pressing Enter while the field has focus picks the top match without
needing to click it — checked via `resp.ctx.input(|i| i.key_pressed(...))`
inside the helper itself, so every call site gets it for free.

### Import: network access is opt-in, and lives in exactly one function

`import::fetch_url` is the *only* function in the entire codebase that
touches the network, and it only ever runs when the user clicks "Von URL
laden" inside the import dialog — nothing calls it on startup, on a timer,
or as a side effect of anything else. If you are auditing this app for
"does it phone home", that one function (and its one call site in
`forms.rs`'s `import_dialog`) is the whole answer.

Deliberately **not** a full table-layout engine:
`import::extract_first_table_as_tsv` does not reconstruct `rowspan`/
`colspan` — a cell spanning several rows only ends up attributed to the
first of them. Wikipedia's simple "list of monarchs" style tables are
usually spanless and import cleanly; a table that does use spans needs a
touch-up pass after import rather than a perfect parse. This was a
deliberate scope cut, not an oversight — reconstructing spans correctly
(propagating a spanning cell down/across the grid it visually covers) is
real complexity for a feature whose fallback (fix it by hand afterwards) is
already available and cheap.

The column-mapping guess (`import::guess_column`, matching header text
against a keyword list per field) only fills in a field that is still
`None` — `guess_columns` is called every frame once headers exist, but
`.or_else(...)` means a column the user already picked by hand is never
silently overwritten by a re-guess triggered by, say, pasting the same table
again with one row appended.

Row parsing is lenient by design: a row with an unparseable date or an empty
required field is skipped, not treated as a reason to abort the whole
import — `import::build_event_drafts`/`build_biography_drafts` return
`(Vec<Draft>, Vec<(row_number, reason)>)` so the dialog can report exactly
how many rows landed and why any others didn't, rather than an all-or-nothing
"table wasn't quite right, so nothing happened."

### An event's label used to be anchored to the lane's flat resting position, not the curve

`paint_lane_events` (canvas.rs) computes each event's marker at a genuinely
curve-aware `y` — `band_center_at(tl, ...)` at that event's own date, correct
even mid-transition on an origin/merge curve. But the label above it, and the
leader line tying the two together, used to be anchored to `band_top`, a
value computed **once per lane, outside the event loop**, from the lane's
flat resting centre (`lane.center - thickness * 0.5`). Near a curve, the
marker would sit at the curved position while its label and leader line
stayed pinned to where the band would be if it weren't curving — reported as
an event "floating in empty space, not holding onto the timeline." Fixed by
computing `band_top` **inside** the loop, from each event's own curve-aware
`y` (`y - lane.thickness * 0.5`), instead of once from `lane.center`. If you
touch label placement in that function again, make sure whatever anchors a
label is derived from that same per-event `y`, not from `lane.center`
directly — `lane.center` is only correct where the band happens to be flat.

### Biography rendering: on-band names, epoch-style life phases, zoom-responsive width

A biography lane used to work exactly like a timeline lane: a name pinned in
a fixed left-side gutter tab (`paint_lane_names`), fixed thickness
(`BIO_BAND_THICKNESS`). That falls apart once there are a dozen of them
stacked (a dynasty of Roman emperors) — three related changes address it,
all scoped to `LaneKind::Biography` only; timelines and groups keep the
original gutter-tab behaviour:

- **Name rides the band, not a fixed gutter.** `paint_lane_names` now
  special-cases `LaneKind::Biography` and delegates to
  `canvas::paint_biography_name`, which centres the name on whatever portion
  of the person's lifespan is currently on screen (clip `span` against
  `view_from`/`view_to`, bail out if the clipped range is empty or too
  narrow for the text) — so it naturally disappears once you scroll past
  someone instead of piling up in a permanent list. This runs in the same
  painting pass as before (after events, so it stays on top), just at a
  different position.
- **Life phases** (`Biography.life_phases: Vec<Epoch>`) reuse the *exact*
  `Epoch` type a timeline's `epochs` already used — same fields, same
  gap-filling logic. `layout::band_color_segments` (timeline-specific,
  curve-aware) was split into a thin wrapper plus a new generic
  `layout::color_segments(epochs, base_color, from, to)` that needs no
  `Timeline` at all; `canvas::paint_biography_band` calls it directly since a
  biography's band is flat and needs no curve sampling. The Biography form
  gained its own epoch-style editor (`BiographyForm::life_phases: Vec<EpochRow>`),
  a close copy of the Timeline form's epoch editor.
- **Zoom-responsive thickness + pin-open.** `layout::bio_thickness(ppy,
  enlarged)` eases a biography lane's thickness from `BIO_BAND_THICKNESS_MIN`
  (zoomed out) up to the normal `BIO_BAND_THICKNESS` (at/above
  `BIO_ZOOM_REFERENCE_PPY`) — applied by mutating `LanePlan.thickness` for
  Biography lanes right after `plan_lanes()` returns in `canvas::draw()`,
  deliberately **not** by changing `plan_lanes`'s signature, to avoid
  touching its half-dozen existing test call sites. Clicking a biography (its
  band or its name — both push the same `Hit`) pins it at
  `BIO_BAND_THICKNESS_ENLARGED` regardless of zoom via
  `TimelineApp::enlarged_biographies: BTreeSet<Id>`; Ctrl+click toggles that
  one biography in/out of the set without clearing the others, a plain click
  clears the set and pins just the one clicked. This set is session-only
  view state (like `y_offset`/`timeline_search`), not part of `Document`.
- Clicking anywhere along a biography's band now selects it, not just its
  name label — `paint_biography_band` pushes its own `Hit` for the whole
  band rect. Previously the gutter name tab was the *only* way to
  click-select a biography or a timeline; that hit-testing gap still exists
  for timelines/groups, just not for biographies any more.

### New-item forms default their parent to the current sidebar selection

`TimelineApp::default_group()` / `default_timeline_for_biography()` (mirroring
the older `default_owner()` used for new events) inspect `self.selection` and
return what a fresh group/timeline/biography should default to: a selected
`Group` becomes the default parent group; a selected `Timeline` contributes
its own `.group` (for a new group/timeline) or itself (for a new biography);
a selected `Biography` contributes its own `.timeline`. Wired into the
toolbar's "+ Gruppe"/"+ Zeitstrahl"/"+ Biografie" buttons only — the
sidebar's own contextual "+ subgroup" action (`Action::NewGroupUnder`)
already threaded its parent through explicitly and needed no change.

### A range event zoomed to a sliver collapses to a point, and so do its nested children

`layout::range_collapsed(event, ppy)` — `true` once a range event's own
on-screen width (`(t1 - t0) * ppy`) drops below `RANGE_COLLAPSE_PX` (18px).
`canvas::paint_lane_events` uses it to decide, per event, whether to paint
the elaborate `paint_range` bar (rounded caps, ticks, plus whatever nests
inside it — see the next section) or fall back to the plain `paint_point`
marker every ordinary event gets — and, critically, whether to descend into
`paint_nested_events` at all. A years-long war is worth its own visible bar
up close; zoomed out far enough that the bar would be a couple of pixels
wide, showing it (and its own sub-phases) adds noise instead of clarity, so
it instead reads exactly like any other single event: a marker and a label.
The same collapse check also gates the *recursive* call inside
`paint_nested_events` — a nested range event (e.g. "Archidamischer Krieg"
inside "Peloponnesischer Krieg") stops offering up its own further
sub-detail once **it** has zoomed down far enough, independent of whatever
its parent is doing. This is orthogonal to the existing zoom-dependent
*importance* threshold (`event_visible`) that already hides low-importance
events entirely at low zoom — that one is about "is this worth showing at
all," this one is about "is this specific bar's own duration still legible."

### Nested events render on their parent's own bar now, not in rows below it

This replaces the "small rows stacked underneath the parent bar" approach
described in the previous section's earlier form. It was reported twice as
not matching how the app already treats epochs: "Archidamischer Krieg"
nested inside "Peloponnesischer Krieg" should show as a coloured segment
*on* the war's own bar, the same way a timeline's epochs sit on its band,
and a plain nested point event should show as a small marker on that same
bar — the parent behaving like its own small, exactly parallel mini-timeline,
for both its range-children and its point-children, at whatever depth they
nest.

`canvas::paint_nested_events` was rewritten around that idea rather than
patched. A nested range child is painted as a filled rect spanning exactly
`parent_rect`'s own height (`Rect::from_min_max` at `parent_rect.top()`/
`bottom()`, x clamped to the parent's own visible span), so at every nesting
depth the segment sits at the *same* height as the top-level bar — recursion
for a grandchild reuses the child's own rect as the new `parent_rect`,
capped at `MAX_NESTED_SEGMENT_DEPTH` (4). A nested point child is a small
circle centred on `parent_rect.center().y`, radius derived from the bar's
own height rather than the child's importance (`(parent_rect.height() *
0.5).clamp(2.5, 5.0)`) since importance-scaled marker sizes assume a full
band's worth of room a nested bar does not have. A child whose own span does
not overlap the parent's real span at all is skipped outright, rather than
clipped to the parent's edge and drawn as a misleading sliver — that only
happens from a genuine data mistake, not a "the mini-timeline's start/end"
edge case worth rendering.

**The one bug this went through before it looked right**: a nested child's
fill was first written as `shade(lane_color, 0.15)` — the exact same amount
`paint_range` already uses for the *parent's own* bar fill. `theme::shade`'s
sign convention is "positive lightens, negative darkens" (see its own doc
comment/test), so identical positive amounts on both parent and child
produced identical colours — the child segment was painted, at the right
position, with a title floating correctly above it, but was completely
invisible against the parent bar behind it, because the two rects were
pixel-for-pixel the same colour. Confirmed by a local screenshot showing the
title with nothing visibly under it. Fixed by darkening children instead —
`shade(lane_color, -0.3 - (depth-1) * 0.15)` — so a nested segment always
reads as visually "cut into" the lighter parent bar, and each further
nesting level darkens a bit more on top of that. **If you touch nested-event
colouring again, double check the sign** — it is easy to reach for the same
magnitude as the parent's own shade and get an invisible result rather than
an obviously-wrong one, since nothing crashes and the hit-testing/label
still work; only a screenshot catches it.

Titles only float above the bar for **depth-1** children (`show_label = ...
&& depth == 1`) via the new `nested_child_label` (shared between the segment
and point branches) — a grandchild's title is deliberately not drawn, since
every nesting level shares the exact same `parent_rect.top()` as its
floating anchor, so a child's own label and its grandchild's label would
land at the same height and collide. A depth-2+ item is still painted, still
clickable, and still shows its title via the hover tooltip (`tooltip_text`,
already wired through `handle_picking` for any `Selection::Event` hit) — the
same "dense clusters fall back to the tooltip" tradeoff the top level
already accepted, just pushed one level deeper.

Because nesting no longer needs any vertical space *below* the band,
`LaneDemand.nested_rows`, `layout::nested_depth`, and the
`NESTED_ROW_HEIGHT`/`MAX_NESTED_ROWS` constants were removed outright rather
than left in place unused — `layout::lane_height` no longer reserves
anything beyond a lane's label rows and its own band thickness for a range
event's nested content, whatever depth it goes to.

Verified with a throwaway local `Document` (a war spanning 431–404 BC with a
nested range child 431–421 BC, a nested point child one year into it, a
nested-inside-the-nested-range point child, and a second nested range near
the parent's end) written to a temp `library.json`, launched, and screenshotted
via `PrintWindow` — this is not covered by an automated test, since
`canvas.rs` paints and has no test module at all (see §6); if you change this
code again, reach for the same throwaway-document-plus-screenshot approach
rather than trusting it compiles and calling it done.

**Two follow-on refinements**, both found by the same screenshot-based
verification once tried against a denser dataset (a war with half a dozen
nested children, one a nested range of its own):

- **A range event with visible children now gets a taller bar.**
  `range_bar_height(importance)` alone (4–9px) was tuned for a bare bar with
  nothing on it; a nested segment's own fill plus a marker circle sitting on
  that same height had no breathing room. `paint_lane_events` now checks
  whether the event has any visible children (`doc.child_events` filtered
  through `event_visible`) and, if so, passes `has_children: true` into
  `paint_range`, which adds a flat `+10.0` to the bar height. This is a
  fixed bonus, not proportional to how many children there are or how deep
  they nest — deliberately conservative, since the bar's `top` moving
  further up (`y - h - 9.0`) eats into the gap between it and row 0 of the
  lane's own top-level label stack (`band_top - LABEL_BAND_TOP -
  1 * LABEL_ROW_HEIGHT`); at the sizes involved there is still a
  comfortable margin, but a much larger bonus would need that margin
  checked explicitly rather than assumed.
- **Nested-child labels now stagger onto further rows instead of
  overlapping each other.** Several children close together in time — the
  common case, since that is exactly when nesting is useful — used to all
  float their titles at the identical height above the parent's bar
  (`parent_rect.top()`), so titles ran into each other and became
  unreadable fragments (confirmed directly in a screenshot: "Archidamischer
  Krieg" clipped mid-word by the neighbouring "Schlacht von Pylos").
  `nested_child_label` now takes a `&mut LabelPacker` — one freshly created
  per call to `paint_nested_events`, so it is scoped to exactly one set of
  siblings — and calls `place_rows(x_min, x_max, 1, MAX_NESTED_LABEL_ROWS)`
  before drawing, stacking a colliding label `NESTED_LABEL_ROW_HEIGHT` (13px)
  higher per row, up to `MAX_NESTED_LABEL_ROWS` (3) before giving up on that
  particular label and falling back to the hover tooltip — the same
  precedent already accepted for a dense cluster of top-level labels. This
  reuses the *existing* `LabelPacker`/`place_rows` machinery (see the next
  section) rather than inventing a second one; the only new thing here is
  wiring a packer instance into a code path that didn't have one before.
  **Not fixed**: a nested label can still collide with the parent event's
  own top-level title, since that title is placed by a *different*
  `LabelPacker` instance scoped to the whole lane, with no visibility into
  what the nested-label packer is doing directly below it. Observed as
  "Sizilische Expedition" (a nested child) touching "Peloponnesischer Krieg"
  (the parent's own title) in the same screenshot pass. Unifying the two
  would mean threading one shared packer through both `measure_lanes` and
  `paint_nested_events`, which reaches further than this pass had scope for.

### A long event title wraps onto a second line before it ellipsises

Previously `fit_text` ellipsised a title the instant it exceeded
`EVENT_LABEL_MAX_PX` on one line — readable, but a title that was merely a
little too long lost its second half unnecessarily. `canvas::wrap_two_lines`
greedily fills a first line at word boundaries, puts whatever's left on a
second line, and only reaches for `fit_text`'s ellipsis if that *second*
line still overflows; a single word wider than the max width on its own
still gets a line to itself rather than looping forever trying to shrink it.

The marker itself never moves — only its label can now be two lines tall —
which is what makes this safe to add without touching any positioning math
for the marker/band itself, only the label's own row bookkeeping. (Written
before the below-the-band reorganisation in the very next section: a plain
event's label now floats *below* the band rather than above it, but the
two-line wrapping described here works identically either way — only the
direction the rows stack changed, not this mechanism.) That bookkeeping
needed a real
extension, not just a taller label: `layout::LabelPacker::place` (single-row)
became
`place_rows(x_min, x_max, rows_needed, max_rows)`, claiming `rows_needed`
*consecutive* rows atomically (all-or-nothing — if any row in the run is
occupied, it tries the next starting row rather than partially claiming).
Both real call sites (`canvas::measure_lanes`'s reservation pass and
`paint_lane_events`'s real placement) now pass `rows_needed = 1` for a title
that fits on one line, `2` for one that needs to wrap — computed from the
*unwrapped* text's measured width in both places, so the two passes agree on
how many rows a given title needs without `measure_lanes` having to actually
perform the word-wrap itself. The old single-row `place` wrapper was deleted
rather than kept around once nothing outside its own tests called it —
callers needing exactly one row now just pass `rows_needed = 1` to
`place_rows` directly.

### Plain events moved below the band, long events stack above it in their own slots

The most structurally significant change in this project so far. Requested
with a hand-drawn sketch: a long event with its own nested sub-structure
(a war with phases and battles) should get dedicated space *above* its
timeline's band — its own title, its bar with colour-coded sections, event
markers with their own labels — while a plain event (no nested content)
sits *below* the band instead of competing with that space above it.
Several long events overlapping in time should stack progressively higher
rather than drawing over one another, and every other lane should
automatically make room.

**The core insight that made this tractable**: "several long events
overlapping in time claim non-overlapping stack levels" is exactly the same
problem `LabelPacker` already solves for text labels — an interval-packing
problem, just claiming an event's *date span* instead of a label's *pixel
width*. No new packing algorithm was needed, just a second `LabelPacker`
instance per lane, fed spans instead of label boxes. Likewise, "every other
lane makes room automatically" needed no new mechanism at all: lanes already
stack sequentially top-to-bottom by height (`place_lanes`), so a lane that
now reports it needs more vertical space simply pushes every lane after it
further down for free — the *only* thing that had to change was correctly
computing how much space one lane needs.

**`is_long_event(doc, filters, ppy, ev)`** (`canvas.rs`) is the single
predicate everything else is built on: a range, not currently
`range_collapsed`, with at least one currently-visible child. Whether an
event counts as "long" is therefore zoom-dependent by construction — as you
zoom out, a war's children fall below the importance threshold one by one,
and the moment none are left visible any more the war itself demotes back to
a plain event (bar below the band, no stacking, no title-above-sections
treatment) purely as a side effect of this one predicate re-evaluating, with
no special "collapse" logic written for it. This is also what makes the
sketch's "sections disappear on zoom-out, the bar goes back to one colour"
requirement fall out for free — that was already `paint_nested_events`'s and
`range_collapsed`'s job from the earlier redesign; nothing new was needed
here beyond routing through the same predicate consistently.

**Layout (`layout.rs`)**: `LaneDemand.rows` split into two differently-typed
demands, and `Lane.label_rows` likewise:

- `below_rows` — plain-event label rows, exactly the old `rows`/`label_rows`
  concept, just anchored below the band now instead of above.
- `above_slots` — stacked long-event slots above the band, each
  `LONG_EVENT_SLOT_HEIGHT` (140px — sized for the worst case: a two-line
  title, a full `MAX_NESTED_LABEL_ROWS` stack of nested labels, and the bar
  itself, the same "size for the maximum" approach `LABEL_ROW_HEIGHT` already
  took), capped at `MAX_LONG_EVENT_STACK` (4).

`lane_height` sums both: `above_slots * LONG_EVENT_SLOT_HEIGHT +
LABEL_BAND_TOP + thickness + LABEL_BAND_BOTTOM + below_rows *
LABEL_ROW_HEIGHT + LANE_BOTTOM_PAD` (`LABEL_BAND_BOTTOM` is new, mirroring
the existing `LABEL_BAND_TOP`). `place_lanes` derives `center` from
`above_slots` the same way it used to derive it from `rows` — the band still
sits right after however much "above" space is reserved, with "below" space
simply falling in the remainder before the next lane starts. `LanePlan`'s
existing `min_rows` (a timeline's guaranteed breathing room even with just
one event) now clamps `below_rows`, since that is where an ordinary
timeline's own event labels live now.

**Measuring (`canvas::measure_lanes`)**: partitions each lane's root events
through `is_long_event`, runs a `LabelPacker` over the long ones' *spans* to
count `above_slots` (`place_rows(x0, x1, 1, MAX_LONG_EVENT_STACK)`,
following the exact same importance/date sort order `visible_events` already
produces), and runs the existing text-label packing exactly as before but
now scoped to only the plain events, to produce `below_rows`. Both passes
must iterate the *same* events in the *same* order as the real paint pass
below, or the stack level a long event gets measured for and the one it
actually paints at would disagree — same requirement measuring text labels
already had, just extended to event spans too.

**Painting (`canvas::paint_lane_events` / `paint_long_event`)**: the main
loop now branches on `is_long_event` per root event. A plain event is
painted almost exactly as before, mirrored: `paint_point` is unchanged
(the marker was always exactly on the band and still is), but a childless
range's bar and every label's row math both flip from "grow upward from
`band_top`" to "grow downward from `band_bottom`," including the leader
line, which now points up from the label to the marker instead of down.
A long event is painted by the new `paint_long_event`, which:

1. Claims a stack slot from a `LabelPacker` shared across the whole lane
   (created once per `paint_lane_events` call, matching `measure_lanes`'s
   own instance) via the event's own `[x0, x1]` span. A degenerate overlap
   of more long events than `MAX_LONG_EVENT_STACK` allows falls back to
   sharing the topmost slot rather than dropping the event — a rare visual
   overlap is preferable to an event vanishing outright.
2. Paints the bar via a generalised `paint_range` (see below) at that
   slot's height, then everything nested on it exactly as the earlier
   nested-events-on-band redesign already does — `paint_nested_events`
   needed no changes at all, since it was already relative to "the bar's own
   rect," which is all that moved.
3. Paints the event's own title *above* a fixed, worst-case reservation for
   the nested-label area (`MAX_NESTED_LABEL_ROWS * NESTED_LABEL_ROW_HEIGHT`)
   above the bar — deliberately not measured from how many nested labels
   this particular event actually used, so the title never needs that
   number to know where it sits, at the cost of some wasted space when an
   event has fewer nested labels than the worst case.

**`paint_range` generalised** to serve both directions from one
implementation rather than duplicating it: it now takes `below: bool` and
`stack_offset: f32` alongside the actual band centre `y` (still needed in
both cases, so the connecting ticks at the range's start/end always reach
the real band line, not just the bar's own edge). `dir = if below { 1.0 }
else { -1.0 }` turns "which side" into a sign, and the bar's near edge
(closest to the band) sits at `y + dir * (9.0 + stack_offset)` — `9.0` is
the original fixed gap, unchanged for an unstacked long event or any plain
range, so this is an exact behavioural no-op for every case that existed
before stacking was added; `stack_offset` (always `0.0` below the band,
since a plain range never stacks) is the only thing that pushes a bar
further out.

**Known limitations, accepted rather than chased further**: a long event's
own title can still collide with a *different* long event's nested-child
label if they sit in adjacent stack levels near a shared boundary — the two
are positioned by entirely separate mechanisms (the title by
`paint_long_event` directly, nested labels by their own `LabelPacker` inside
`paint_nested_events`) with no visibility into each other. `LONG_EVENT_SLOT_HEIGHT`
is a flat 140px regardless of how simple or complex a given long event's own
content actually is — a war with a single nested point event reserves the
same worst-case space as one with a dozen nested ranges. Both are the same
kind of tradeoff already accepted in the nested-events-on-band redesign
itself (see the note there about a nested label colliding with the parent's
own top-level title) — unifying every one of these into a single shared
packer would be a real project of its own, not a quick follow-up.

Verified with a throwaway local `Document` (two overlapping "long" wars
sharing one timeline, plus a plain point event and a plain childless range
scrolled into view separately) written to a temp `library.json`, launched,
and screenshotted via `PrintWindow` — confirmed the second war stacks
visibly above the first rather than overlapping it, and that both plain
events render below the band with their labels below them. Not covered by
an automated test beyond the pure `layout.rs` sizing logic
(`lanes_grow_to_fit_stacked_long_events_above_the_band`,
`long_event_stacking_is_capped_so_it_cannot_grow_without_bound`,
`stacked_slot_space_sits_above_the_band_in_every_lane`) — `canvas.rs` still
has no test module at all (see §6); reach for the same
throwaway-document-plus-screenshot approach if you touch this again.

### A pre-1.0 QA pass found four more real bugs, all in the reorganisation above

Aiming for a 1.0.0 release, the next round was a deliberate stress-testing
pass rather than a specific feature request: a throwaway document built to
exercise as many edge cases at once as reasonably possible (deeply nested
events, several mutually overlapping long events, a single word too long
for any line, a zero-duration range, the BC/AD boundary, a collapsed group,
light mode, an open-ended biography), screenshotted across a range of zoom
levels and pan positions. All four findings trace back to the two features
from this same session — the on-band nested-events redesign and the
above/below reorganisation — under load neither had actually been tested
against.

- **`wrap_two_lines` didn't truncate an oversized first word.** A single
  word wider than `max_width` (with no space to break at) is deliberately
  allowed onto `line1` anyway, so the loop always places at least one word
  rather than looping forever — but the function then returned that line
  completely unchecked, so a title that was one very long word ("Donau­dampf­
  schiff­fahrts­gesellschafts­kapitäns­patent­prüfungs­ordnungs­entwurf",
  chosen precisely to be absurd) rendered at full width, running clean off
  the screen, instead of ellipsising like every other overlong label. Fixed
  by re-checking `line1`'s width after the loop and running it through
  `fit_text` if it still overflows — covers both this path and the
  structurally identical one where `line1` alone (not just the leftover
  `rest`) is the function's only return value.
- **The long-event stack-overflow fallback produced garbled, run-together
  text**, in two different ways, once a lane actually had more overlapping
  long events than `MAX_LONG_EVENT_STACK` (a genuinely realistic amount —
  five mutually-overlapping wars, deliberately chosen as a stress case,
  triggered it immediately): two full titles, or two independent sets of
  nested-child labels, landing in the exact same shared slot with no
  coordination between them read as illegible mashed-together text (e.g.
  "Nebenkrieg4Nebenkrieg 5"). `paint_long_event` now tracks whether it got a
  genuine slot or the shared fallback one; on fallback it still paints the
  bar (a rare visual overlap beats a dropped event) but skips its own title,
  and passes a new `labels_allowed: bool` into `paint_nested_events` so that
  call's nested children still paint (and stay clickable) but never draw
  their own labels either — both are still fully reachable via a click or
  the hover tooltip, same as any other "dense cluster" tradeoff already
  accepted elsewhere in this file.
- **A bar spanning far more pixels than the visible window failed to paint
  at all**, reproducibly, once zoomed in far enough that a single long
  event's own multi-decade span was several screens wide — not just
  clipped at the edges as expected, but completely absent, and this
  silently took its nested content and label down with it. Confirmed with a
  bisection: the *identical* two-event document rendered perfectly at a
  more moderate zoom, and rendered perfectly again at the same extreme zoom
  once the window was widened enough that neither bar's edges actually
  exceeded the clip rect — strongly pointing at however this particular
  eframe/glow version handles a shape whose bounds run thousands of pixels
  past its clip rect, rather than at anything specific to this app's own
  layout math (which measured out correctly at every step along the way).
  Rather than chase that upstream, `paint_range` now clamps both edges to
  `content_rect`'s bounds plus a 100px margin before ever constructing the
  rect — a bar's true edges rounding off screen were never going to be
  drawn precisely anyway, so there is no visible cost to capping how far
  past the edge the shape passed to the renderer is allowed to reach.
  Re-verified with the same bisection dataset at the same extreme zoom:
  every event, title, and nested child now renders correctly regardless of
  how far its real span extends past either edge.

None of these four are covered by an automated test (`canvas.rs` still has
no test module — see §6); each was caught and confirmed fixed the same way
as the previous round, with a throwaway `Document` and `PrintWindow`
screenshots at the specific zoom/pan combination that reproduced it.

### Table import can nest straight into an existing event, not just onto a timeline's top level

`ImportForm::nest_under: Option<Id>` — when importing Events, an optional
combo (reusing `event_parent_combo`, the same widget the single-event form
uses, with `editing: None` since these are all brand-new events) lets the
user pick an existing **range** event on the chosen timeline; every imported
row then gets `parent: nest_under` instead of `parent: None`. This is what
makes "import a table of a war's phases straight into that war's own event"
possible instead of always dropping everything at the timeline's top level.
Changing the timeline selection resets `nest_under` — the combo's own
`selected_text` looks the chosen event up by id regardless of which
timeline is currently selected, so leaving it set across a timeline switch
would silently show a stale, unrelated event's name.

### The group tidy heuristic has to be applied at every level, not just the top

`suggest_group_order(doc, parent)` (see the entry on group ordering above)
was always general — it takes a `parent` and only reorders siblings that
share it — but the sidebar button wired it to `suggest_group_order(doc,
None)` alone, i.e. top-level groups only. Two connected cultures sitting as
*subgroups* of a shared parent (a far more typical arrangement than two
unrelated top-level groups) were therefore never nudged together — reported
as "the tidy button doesn't work." `layout::tidy_all_group_levels` fixes
this by recursing the same call down through every subgroup's own sibling
list, top to bottom. No change to the underlying heuristic itself, and
`panels.rs`'s `Action::TidyGroups` now just calls it directly
(`app.mutate(layout::tidy_all_group_levels)`) rather than inlining the
top-level-only version.

### Seasons and quarters are parsed by fusing them into ordinary month tokens, not a second code path

`model::month_from_period` extends the same one-token-at-a-time month
lookup `parse_ymd`'s token loop already used for "Jul"/"Aug" — a season
word ("sommer", "frühherbst", "spätwinter") is just another single token
that resolves to a representative month (chosen so the whole sequence,
including `früh`/`spät` variants, sorts chronologically within a year;
Winter is the one exception — see the comment on its table entry for why
it deliberately does *not* try to split across the Dec→Feb calendar-year
boundary). Quarters and halves ("1. Quartal", "Q1", "1. Hälfte",
"Halbjahr") need an extra step first: `fuse_period_tokens` turns "1.
Quartal" / "1.Quartal" / "Quartal 1" all into the single token "quartal1"
(ordinal fused onto the keyword, stray '.' turned into a space) *before*
tokenising, specifically so `month_from_period` can treat it exactly like
any other month name rather than needing a whole parallel parsing path
just to handle the separate ordinal. This fusing only ever runs when one of
`PERIOD_KEYWORDS` is actually present in the input — unconditionally
turning '.' into a space would otherwise wreck the day.month.year form
("14.07.1789") parsed just above it in `parse_ymd`. All of these are
inherently approximate (a season or a quarter is months wide), which is why
`HDate::parse` automatically upgrades the qualifier to `Circa` when
`parse_ymd` reports the date came from one of these — unless the user
already gave an explicit qualifier of their own (`vor Sommer 1789` keeps
`Before`, it is not overridden).

`"Anfang"`/`"Beginn"`/`"Ende"` + a year (`"Anfang 1789"` → January,
`"Ende 1789"` → December, both `Circa`) reuse this exact same lookup table —
three more entries in `month_from_period`'s `PERIODS` array, no new parsing
path — since they need no ordinal-fusing step at all (unlike `"1. Quartal"`,
they're already single bare words once split on whitespace). The one thing
worth knowing if you touch this array again: **its length annotation
(`[(&str, u8); N]`) is a manually-maintained count, not inferred** — adding
entries without bumping `N` is a compile error, not a silent bug, but it is
an easy one to hit typing the diff by hand.

### A German ordinal day before a spelled-out month used to be rejected outright

Reported via the new search-by-date field: typing `"1. Januar 413 v. Chr."`
did nothing at all — not "jumped to the wrong place," genuinely nothing,
because `HDate::parse` returned `None` for it. Two distinct bugs, found by
tracing `parse_ymd` by hand once a quick regression test reproduced it:

1. The day.month.year *numeric* form at the top of `parse_ymd` only handles
   a bare dotted date (`"14.07.1789"`) and correctly declines anything with
   a letter in it, so `"1. Januar 413"` falls through to the ordinary
   per-token loop below — correctly. But once there, the token `"1."` still
   carried its trailing ordinal period, and `"1.".parse::<i32>()` fails, so
   the token loop's catch-all `else { return None }` rejected the whole
   date. Fixed by extending the existing `trim_end_matches(',')` (already
   there for a trailing comma, e.g. `"July 14, 1789"`) to also strip a
   trailing `.` — safe unconditionally at this point in the function, since
   anything reaching the per-token loop with a `.` still on it is either
   this ordinal-day case or a month abbreviation's own trailing period
   (`"jul."`), which `month_from_name` already strips itself anyway.
2. That alone still wasn't enough for `"1.Januar 413 v. Chr."` (no space
   after the ordinal's period) — `split_whitespace` treats `"1.Januar"` as
   one single token, which is neither a number nor a month name, so the
   catch-all still rejects it. `model::split_ordinal_dot` now runs
   unconditionally right before the existing `PERIOD_KEYWORDS` fusing step:
   it inserts a space after any `.` that is immediately followed by a
   *letter* — never one followed by a digit, so it cannot touch the numeric
   day.month.year form (which has already had its own chance to match, and
   returned early, before this code even runs). A `.` followed by a letter
   is never legitimately a numeric separator, which is what makes this safe
   to apply unconditionally rather than gating it the way
   `fuse_period_tokens` has to.

Both are covered by
`model::tests::a_german_ordinal_day_before_a_spelled_out_month_still_parses`.
If you add another purely-textual normalisation step to `parse_ymd`, put it
*after* the numeric-form check and *before* the token loop, same as both of
these — that ordering is what lets each step assume "whatever reaches me
here is not a bare numeric date" without re-deriving it itself.

### A biography's own name used to be able to paint over its life-phase label

Reported as a life-phase name (e.g. "Stratege Athens", covering most of the
person's life in the reproduction case) needing to be reliably legible,
"im Vordergrund" — in the foreground, not obscured by anything else.
`phase_segment_label` already used an opaque background and a neutral text
colour, so the actual cause was paint order: `paint_biography_name` (the
person's own name, also centred on the band) is called from
`paint_lane_names`, which — before this fix — ran *before*
`paint_segment_labels`. Whenever the two labels' centred positions
overlapped (routine for a phase spanning most of the lifespan, since both
centre on their own span's midpoint), the person's name painted second and
covered the phase name. `draw()` now calls `paint_lane_names` *before*
`paint_segment_labels` instead, so a phase label — the more specific of the
two at that exact spot — always wins where they'd otherwise collide. This
also moved the life-phase-label call itself out of `paint_biography_band`
(the early "bands" pass) into the shared `paint_segment_labels` final pass,
renamed from `paint_epoch_labels` to reflect that it now covers both a
timeline's epochs and a biography's life phases — same reasoning as the
already-existing epoch-label pass being separate from `paint_timeline_band`
(see the section on that above): whatever paints last wins, so anything
that needs to reliably stay on top belongs in that last pass, not inline
with the band it happens to sit on.

Checked that this reorder does not reintroduce the *opposite* problem for
timelines: a timeline's own name lives in a fixed left-edge gutter tab
(`paint_lane_names`'s non-biography branch), which never shares screen
position with an epoch label floating elsewhere along the band, so
reordering the two calls has no effect there — the risk this reorder
carries is scoped to biographies only, where both labels genuinely compete
for the same centred position.

### A failing import row is now flagged directly in the preview, not just counted

`build_event_drafts`/`build_biography_drafts` already returned
`(Vec<Draft>, Vec<(row_number, reason)>)` for skipped rows — the dialog only
ever showed a *count* ("3 Zeile(n) übersprungen"), leaving the user to guess
which of possibly hundreds of pasted rows were the problem.
`forms::compute_import_skips` runs the same draft-building the "ready to
import" count already does, but keyed into a `HashMap<row_number, reason>`
that `preview_grid` uses to tint a failing row's cells red
(`Color32::from_rgba_unmultiplied(225, 120, 110, 70)`, an alpha over
`BAD_RED`) and attach the reason as a hover tooltip. Only computed once the
column mapping actually names a required field (title/date, or name/birth) —
before that, every row would trivially "fail" and the whole preview would
light up red for no useful reason. `preview_grid` also gained its own
`ScrollArea::both().max_height(220.0)` and dropped the old `.take(5)` cap, so
a flagged row further down a long pasted table is actually reachable instead
of being silently excluded from the preview entirely.

### An epoch/life-phase label's visibility is now driven by duration, not by its own name's length

`epoch_segment_label`/`phase_segment_label` used to hide a label the moment
its own *measured text* no longer fit the segment's on-screen width. That
conflates two different things: whether this era is significant enough to
still show a name at this zoom (which should depend on how long it actually
lasted) and whether this specific string happens to be short enough to fit
(which depends on nothing but how the era was named). Two eras of very
different real duration but coincidentally same-length names — "Spätminoische
Zeit" / "Frühminoische Zeit" are exactly the same length — would disappear
at exactly the same zoom under the old rule regardless of which one actually
lasted longer. `SEGMENT_LABEL_MIN_PX` (a fixed pixel width, independent of
any particular name) now gates whether a label shows *at all*; `fit_text`
then shortens the name with a trailing "…" to whatever space is actually
available, so a long name in a tight-but-still-above-the-minimum segment
degrades gracefully instead of vanishing outright the moment it stops
fitting verbatim.

### Labels are a neutral colour now, not a tint of their own lane's hue

`theme::label_color(band, dark)` used to shade a lane's own colour toward
white (dark mode) or black (light mode) for its label text. The existing
test for this only checked "lighter than the band" in every channel, which
a light cyan/blue band still satisfies while being genuinely hard to read
once its label ends up rendered close to (or, for a biography name and a
life-phase name, directly on top of) that same band — light-on-mid-blue is
low contrast even though it is technically "lighter." Every label call site
(event titles, lane-name gutter tabs, biography names) now uses `theme.text`
directly — always high-contrast against the dark/light canvas background by
construction, since it does not depend on the colour it happens to sit near.
`label_color` and its test are gone entirely; `Theme.dark` went with them,
since that field's only reader was this function. Which lane something
belongs to is still conveyed by the marker/band colour and the gutter tab's
colour chip — just not by tinting the *readable text* any more.

### A new event defaults to nesting inside whatever range event is selected

`TimelineApp::default_parent_event()` (next to the older `default_owner()`)
returns the current selection's id when it is an `Event` that is itself a
range — wired into `new_event_dialog()` so the toolbar's generic "+
Ereignis" (Strg+N) pre-fills the "Verschachtelt in:" field the same way it
already pre-filled the owner. Without this, selecting "Peloponnesischer
Krieg" and adding "Archidamischer Krieg" via the generic button produced an
event that shared its *owner* (correct) but not its *parent* — nesting only
happens if that separate combo is explicitly set, an easy step to forget,
and the result silently renders as its own independent event on the
timeline instead of a sub-segment tethered to the intended parent. The
dedicated "+ Verschachteltes Ereignis" button next to a range event already
covered this explicitly; this just extends the same convenience to the
generic path.

### Semi-transparent label backgrounds bled through into whatever scrolled underneath them

Reported as "an event marker floats disconnected, below the band, inside a notch" — for one specific event (a 5-year range that had collapsed to a point marker), at one specific scroll position, and nowhere else. That specificity was the tell: it wasn't the marker's own position that was wrong (confirmed by loading the user's actual `library.json` into a local build and reproducing pixel-for-pixel — the marker painted exactly on the band, as the code says it should), it was the **lane-name gutter tag** — pinned to the left edge of the viewport regardless of scroll — happening to sit at the same screen position as that marker at that particular scroll offset. The tag's background was `with_alpha(theme.canvas_bg, 215)`, not fully opaque, so the band's colour and the marker's own halo/ring bled through at ~84% strength instead of being cleanly hidden — which reads as a rendering glitch (a ghostly, wrongly-shaped, partially-visible circle) rather than as "oh, the label is just covering that." Fixed by making every label background that can end up overlapping scrolling content fully opaque: the lane-name gutter tag, the biography on-band name, the epoch/life-phase pill, and the origin/merge junction label. `paint_point`'s own background halo (the one *inside* a marker, there to separate it from a same-hued band) was deliberately left semi-transparent — that one blends by design, it isn't a text label with something potentially scrolling underneath it.

**Diagnostic note for next time a "something floats in the wrong place" report comes in**: before assuming the position math is wrong, check whether the described element is a *label with an opaque-looking background* — if the bug only reproduces at one specific pan/zoom and disappears at others with no code change, a fixed-position UI element (gutter tags, the scroll indicator) overlapping scrolled content is a more likely cause than the position calculation itself. Reproducing against the user's actual save file in a local build (drop it at whatever `store::default_path()` resolves to — `%APPDATA%\TimelineExplorer\library.json` when running from a `target/` build, since `portable_dir()` deliberately refuses to treat a cargo build directory as portable) and grabbing a `PrintWindow` capture settled in minutes what several rounds of asking the user to check settings could not.

### The egui hover-reflow workaround

`theme::visuals()` normalises `bg_stroke.width` across all interactive widget
states. This is **not cosmetic**: egui derives a button's inner padding by
subtracting the stroke width from the button padding, and that subtraction
clamps at zero. A `small_button` has zero vertical padding, so the 1px hover
outline made every small button 2px taller and shoved the sidebar around under
the cursor.

`theme.rs` has two tests for this. One asserts the fix works; the other
(`the_default_theme_is_what_needed_fixing`) asserts egui's *default* still has
the flaw — **if that test starts failing after an egui upgrade, the workaround is
no longer needed and can be deleted.** That is intentional, not a broken test.

### Save path is deliberately paranoid

`store::save()` writes to a temp file, `sync_all()`s it, rotates a numbered
backup, then `fs::rename`s over the live file. `fs::rename` replaces atomically
on Windows too — **do not add a `remove_file` before it.** An earlier version did
and left a window where a crash would have destroyed the whole library. There is
a test named `overwriting_never_leaves_the_library_missing` guarding this.

Backup rotation is age-gated (`MIN_BACKUP_AGE`, 10 minutes): if slot 1 is
younger than that, a save leaves the whole backup ring untouched. Without this,
autosave firing 1.2s after every small edit filled all 10 slots within minutes
of active editing, so the "history" was a few seconds deep instead of hours.
`rotate_backups_impl` takes the age threshold as a parameter precisely so the
cap and the age-gating can each be tested deterministically (`Duration::ZERO`
forces every save to rotate) without racing the wall clock.

Loading tolerates a UTF-8 BOM, because Notepad writes one and the file is
advertised as user-editable. A file that fails to parse is **never** overwritten;
the app starts empty and says so in the status bar.

### The backward-compatibility rule, and how it's enforced so it can't quietly rot

**Every field added to `Document` or anything it contains (`Group`,
`Timeline`, `Biography`, `Event`, `Category`, `Epoch`, `Junction`, `Filters`,
`SavedView`, …) must carry `#[serde(default)]` or `#[serde(default =
"fn_name")]`.** This is the entire backward-compatibility strategy for this
app — there is no version-gated migration system, and `Document.version`
(currently always `1`) is written but never read; the real guarantee is
"the schema only ever grows, and every growth is optional." A user who has
been running this app since the very first release must be able to open
their current `library.json` — and every rotating backup they've ever
made — with next month's build, indefinitely, with no manual migration
step.

This is checked by two tests in `store::tests`, not just asserted in prose:

- `a_completely_empty_file_still_loads_as_a_blank_library` — loads a bare
  `{}` and expects a valid, blank `Document`. This is the strongest form of
  the check: it only passes if *every* field on `Document` itself has a
  default, so it fails immediately if a future one doesn't.
- `a_file_holding_only_each_entitys_original_required_fields_still_loads` —
  one instance of every entity type, written with only the fields each one
  had at its very first introduction (no colour override, no `visible`, no
  `epochs`/`life_phases`, no importance, …), and asserts the now-optional
  fields fill in with sensible defaults.

**If you add a field to any of these types, extend the second test to
include it** (or add a new one alongside it) rather than treating "the
existing tests still pass" as sufficient — a field that already defaults
correctly today does not prove a *sibling* new field does too. Losing sight
of this rule is exactly the kind of mistake that would only surface as "the
app stopped opening my file" for someone who has invested real time in
their library, with no error message pointing at the cause.

---

## 6. Testing approach

All 163 tests are pure logic and run in well under a second without opening a
window — `layout` and `model` hold the largest share (axis maths, zoom
clamping, tick steps, filters, lane stacking, band convergence geometry,
dormant lanes, label packing, date parsing, colour-segment gap-filling), with
the rest spread across `example` (dataset referential integrity), `store`,
`theme`, `forms`, `panels`, `import`, and `app`.

**Rendering is verified by screenshot, not by test.** During development the app
was captured with `PrintWindow` (Win32, flag `PW_RENDERFULLCONTENT = 2`), which
grabs the window's own content even when it is occluded or unfocused — useful
because a plain `CopyFromScreen` grabs whatever is on top instead. That is how
the label overlap and the tofu glyphs were caught. Worth reusing if you change
the canvas.

Note that hover states cannot be captured that way: the app only receives mouse
input when it is genuinely under the cursor. The hover bug above was diagnosed
instead by driving a bare `egui::Context` with synthetic `PointerMoved` events
via `ctx.run_ui()` — see `theme::hover_stability`. Remember to
`full.textures_delta.clear()` or the context panics on drop.

---

## 7. If you pick up the loose ends

Roughly in order of likely value. (Export to image/PDF, month/day ticks at
extreme zoom, a group-ordering UI, and the long-event-above/plain-event-below
canvas reorganisation — all previously listed here — are done; see §1.)

1. **A dedicated fast-zoom slider**, next to the existing detail-bias slider —
   requested twice now (once alongside the day/month tick ladder, again
   folded into the batch that produced the nested-events-on-band redesign)
   and still not done. The zoom itself (`TimeAxis.ppy`, `MIN_PPY`..`MAX_PPY`,
   now a 60,000:1 range) already supports jumping straight to any level; what's
   missing is a widget to do it other than the scroll wheel/`+`/`-` keys.
2. **Drag to reorder** timelines and groups in the sidebar. Currently Up/Down
   buttons only (`panels::reorder`/`reorder_group`, which correctly scope
   movement to siblings within a group).
3. Undo depth is 60 full `Document` clones (`app::UNDO_DEPTH`). Fine for personal
   datasets; if libraries get very large, switch to a diff-based approach.
4. The two "known limitations, accepted rather than chased further" noted at
   the end of the above/below reorganisation's own section: a long event's
   title can still collide with a different long event's nested-child label
   across adjacent stack levels, and `LONG_EVENT_SLOT_HEIGHT` is a flat
   worst-case reservation regardless of how much a given long event actually
   needs. Both would need a single packer shared across everything currently
   using its own separate one.

---

## 8. Where the user's data lives

Portable-first: next to the exe if that folder is writable, otherwise
`%APPDATA%\TimelineExplorer\library.json`. *File ▸ Show data folder* opens it.

**When testing, be aware you are writing to the real user library.** Ten rotating
backups (`library.bak1.json` … `bak10.json`) sit beside it and are restorable
from *File ▸ Restore backup*. Point the app at a scratch file with *File ▸ Save
as…* if you are going to be destructive.
