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
the screen centuries after it stopped existing.

- **~12,730 lines** of Rust across 14 files in `src/`.
- **150 tests**, all passing, no compiler warnings (5 pre-existing clippy
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
cargo test             # 92 tests, ~0.2s, all pure logic — no window needed
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
| `model.rs` | 1724 | Data model + serde. Dates, spans, timelines, groups, biographies, events, categories, filters. **No UI, no geometry.** |
| `layout.rs` | 2091 | Time axis, tick steps, visibility rules, lane planning/placement, band curves, label packing. **No painting** — that's why it holds the largest share of the tests. |
| `forms.rs` | 2206 | Modal editors for group/timeline/biography/event/categories/export/import. |
| `app.rs` | 1328 | `TimelineApp` state, undo/redo, autosave, menus, keyboard shortcuts, top-level layout. |
| `canvas.rs` | 1308 | All painting of the timeline surface + canvas input handling. |
| `panels.rs` | 1262 | Sidebar (group tree, biographies, filters) and inspector. |
| `example.rs` | 604 | The optional worked example dataset. |
| `store.rs` | 416 | Load/save, atomic replace, rotating backups. |
| `export.rs` | 410 | PNG/PDF export by driving the real canvas painter and capturing a screenshot. |
| `theme.rs` | 353 | Palette, importance→size/opacity encoding, egui `Visuals` override. |
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
the elaborate `paint_range` bar (rounded caps, ticks, room for nested rows
underneath) or fall back to the plain `paint_point` marker every ordinary
event gets — and, critically, whether to descend into `paint_nested_events`
at all. A years-long war is worth its own visible bar up close; zoomed out
far enough that the bar would be a couple of pixels wide, showing it (and
its own sub-phases, in even tinier rows) adds noise instead of clarity, so
it instead reads exactly like any other single event: a marker and a label.
`canvas::measure_lanes`'s nested-row reservation uses the same check, so a
lane does not keep reserving vertical space for sub-rows nobody is going to
see. The same collapse check also gates the *recursive* call inside
`paint_nested_events` — a nested range event (e.g. "Archidamischer Krieg"
inside "Peloponnesischer Krieg") stops offering up its own further
sub-detail once **it** has zoomed down far enough, independent of whatever
its parent is doing. This is orthogonal to the existing zoom-dependent
*importance* threshold (`event_visible`) that already hides low-importance
events entirely at low zoom — that one is about "is this worth showing at
all," this one is about "is this specific bar's own duration still legible."

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

All 142 tests are pure logic and run in well under a second without opening a
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

Roughly in order of likely value:

1. **Export to image/PDF** — the one open item from the planning document. An SVG
   or PNG dump of the current canvas view is the obvious form; the painting code
   is already centralised in `canvas.rs`.
2. **Month/day ticks at extreme zoom.** `tick_step()` bottoms out at 1 year. The
   data model already carries month and day, so events place correctly within a
   year, but the ruler cannot label finer than a year.
3. **Drag to reorder** timelines and groups in the sidebar. Currently Up/Down
   buttons only (`panels::reorder`, which correctly scopes movement to siblings
   within a group).
4. **Group ordering UI.** Groups have an `order` field and are sorted by it, but
   nothing in the UI changes it yet.
5. Undo depth is 60 full `Document` clones (`app::UNDO_DEPTH`). Fine for personal
   datasets; if libraries get very large, switch to a diff-based approach.

---

## 8. Where the user's data lives

Portable-first: next to the exe if that folder is writable, otherwise
`%APPDATA%\TimelineExplorer\library.json`. *File ▸ Show data folder* opens it.

**When testing, be aware you are writing to the real user library.** Ten rotating
backups (`library.bak1.json` … `bak10.json`) sit beside it and are restorable
from *File ▸ Restore backup*. Point the app at a scratch file with *File ▸ Save
as…* if you are going to be destructive.
