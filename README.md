# Timeline Explorer

A local, offline Windows tool for building and comparing parallel historical
timelines. Built from `timeline_app_planning.md`.

Ships as **one self-contained `.exe`**: no installer, no runtime to install,
and no network access unless you explicitly ask for it via the "Von URL
laden" button in the import dialog (used to pull a table straight off a
page, e.g. Wikipedia). Nothing else in the app ever goes online.

## Download

Prebuilt binaries are published on the [Releases page](../../releases) for
every tagged version.

## Build

```bash
cargo build --release
```

The result is `target/release/timeline_explorer.exe` (~9 MB). Copy it anywhere
and run it — nothing else is needed.

```bash
cargo test
```

## Tech stack, and why

The single-executable constraint drove the choice.

**Rust + `egui`/`eframe`, MSVC target, statically linked CRT.**

- **Single exe.** Compiles to one native binary. `+crt-static` (in
  `.cargo/config.toml`) links the C runtime in, so not even the VC++
  redistributable is required. Verified against the PE import table: the only
  DLLs it imports are ones that ship with Windows (`kernel32`, `user32`,
  `gdi32`, `opengl32`, `shell32`, …). No .NET, no WebView2, no VC++ redist.
- **Offline by default.** The only network code in the binary is
  `import::fetch_url`, used solely by the import dialog's opt-in "Von URL
  laden" button — nothing else ever makes a request. Fonts are compiled in;
  rendering goes straight to OpenGL.
- **Rendering.** The app is essentially one large custom canvas — converging
  bands, importance-scaled type, free zoom and pan. egui is an immediate-mode
  painter, so that geometry is drawn directly rather than fought against a
  widget or DOM layer.

Alternatives considered: .NET/WPF was rejected because no .NET SDK was present
(runtimes only) and it would have meant a ~200 MB SDK install first; Tauri was
rejected for its WebView2 dependency, which the planning document flagged as the
key risk; PyInstaller for its unpack-on-start cost and one-file fragility.

## Layout of the code

| File | Responsibility |
| --- | --- |
| `model.rs` | Data model: dates, spans, timelines, groups, biographies, events, categories, filters. All serde-serialised to one JSON file. |
| `store.rs` | Load/save: atomic replace, `fsync`, ten rotating backups. |
| `layout.rs` | Geometry and visibility rules — no painting, so the tricky parts are unit-testable. |
| `theme.rs` | Palette and the importance→size/opacity encoding. |
| `canvas.rs` | Painting the ruler, bands, junctions, markers and labels; canvas input. |
| `panels.rs` | Sidebar (group tree, biographies, filters) and inspector. |
| `forms.rs` | Modal editors. |
| `example.rs` | The optional worked example dataset. |
| `app.rs` | App state, undo/redo, autosave, top-level layout. |

## Design decisions worth knowing

**Dates.** `HDate` stores a historical year (negative = BC, no year zero), with
optional month/day, a qualifier (`exact`/`circa`/`before`/`after`) and a ± in
years. The continuous axis maps AD year *y* to `[y-1, y)` and BC year *y* to
`[-y, -y+1)`, which keeps the BC/AD boundary contiguous despite there being no
year zero. Dates are entered as free text (`44 BC`, `-44`, `c. 250 BC`,
`1789-07-14`, `14 Jul 1789`, `44 v. Chr.`, any of them with `±20`) and the form
echoes back its interpretation live, so a misread date is immediately visible.

**Convergence/divergence** is a first-class visual, not a marker. A timeline may
carry an `origin` junction (splits from another timeline) and a `merge` junction
(merges into another and ends there). Within a transition window before a merge,
the band eases off its own lane onto the target's lane with a smoothstep curve.
The window is defined in *pixels*, so the curve keeps its shape at every zoom.

**Lanes are laid out in two passes.** First the lane stack is *planned*, then the
canvas *measures* how many rows of labels each lane actually needs at the current
zoom, then lanes are *placed*. So a dense stretch grows to fit its labels instead
of silently dropping them, and a lane with nothing in the visible window
collapses to a slim dimmed row. That is what makes it practical to zoom to
single-year resolution on one culture without scrolling past empty lanes.

**Groups** are super-categories ("European history", "Greek antiquity") that
nest arbitrarily. Collapsed, a group draws a single hatched band spanning
everything beneath it and shows its members' events — so whole civilisations can
be compared without unfolding them. Expanded, it is a heading over its members.
Deleting a group keeps its contents and lifts them up a level.

**Visual encoding** keeps two channels separate: colour carries *identity*
(which timeline), while font size, marker size and opacity carry *significance*.
A marker's ring shows its first category. Note that egui's bundled fonts have no
bold face, so weight differentiation is done through size and opacity rather
than boldness.

**Data safety.** Saves write to a temp file, `fsync`, rotate a numbered backup,
then atomically rename over the live file — the library is never absent from
disk. Ten backups are kept and restorable from *File ▸ Restore backup*. A file
that fails to parse is never overwritten; the app starts empty and says so.
A UTF-8 BOM (as Notepad writes) is tolerated on load. Undo/redo covers 60 steps.

## Where the data lives

One human-readable JSON file. Portable-first: if the executable's folder is
writable, the library sits next to the exe, so the whole tool works from a USB
stick. Otherwise it falls back to `%APPDATA%\TimelineExplorer\library.json`.
*File ▸ Show data folder* opens it.

## Open questions from the planning document

Section 5 of the brief listed questions to flag rather than guess at. These were
answered with reversible defaults so the tool was usable immediately:

- **Starting categories** — ten are seeded (Military, Politics, Religion,
  Philosophy, Literature, Science, Art, Economy, Law, Personal). They are fully
  user-editable: rename, recolour, add, delete. Nothing in the code depends on a
  particular set existing.
- **Importance** — five named tiers (Detail, Minor, Notable, Major, Epochal),
  always assigned manually, defaulting to Notable. Zoom level sets which tiers
  are visible; the Detail slider biases that either way.
- **Preloaded data** — the app starts empty. An example library (Rome vs. the
  Hellenistic kingdoms, classical Athens and Sparta, four biographies) is
  *offered* under *File ▸ Load example library*, never forced.
- **Visual style** — dark by default, light theme under *View*.
- **Export to image/PDF** — **not implemented.** This was left open pending a
  decision on whether it is wanted, and if so in what form.
