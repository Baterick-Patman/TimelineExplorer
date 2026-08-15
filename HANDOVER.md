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
colour-coded epochs (now with their name painted directly on the band, not
just the colour), nestable events, and nestable categories (ticking a parent
category in the filter cascades onto its subcategories).

- **~9,000 lines** of Rust across 10 files in `src/`.
- **110 tests**, all passing, no compiler warnings.
- Release binary: `target/release/timeline_explorer.exe`, ~6.4 MB, single file.

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
| `model.rs` | 1172 | Data model + serde. Dates, spans, timelines, groups, biographies, events, categories, filters. **No UI, no geometry.** |
| `layout.rs` | 1493 | Time axis, tick steps, visibility rules, lane planning/placement, band curves, label packing. **No painting** — that's why it holds 46 of the 92 tests. |
| `store.rs` | 349 | Load/save, atomic replace, rotating backups. |
| `theme.rs` | 304 | Palette, importance→size/opacity encoding, egui `Visuals` override. |
| `canvas.rs` | 887 | All painting of the timeline surface + canvas input handling. |
| `panels.rs` | 712 | Sidebar (group tree, biographies, filters) and inspector. |
| `forms.rs` | 1262 | Modal editors for group/timeline/biography/event/categories. |
| `app.rs` | 852 | `TimelineApp` state, undo/redo, autosave, menus, keyboard shortcuts, top-level layout. |
| `example.rs` | 592 | The optional worked example dataset. |
| `main.rs` | 39 | Entry point, window options. |

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

---

## 6. Testing approach

All 92 tests are pure logic and run in ~0.2s without opening a window.

- `layout` (46) — axis maths, zoom clamping, tick steps, filters, lane stacking,
  band convergence geometry, dormant lanes, label packing.
- `example` (11) — the sample dataset is checked for referential integrity, id
  uniqueness, and that its dates are historically coherent.
- `model` (9), `store` (9), `theme` (7), `forms` (5), `panels` (5).

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
