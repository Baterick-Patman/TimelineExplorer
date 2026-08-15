//! Loading and saving the library.
//!
//! This is a dataset the user grows over years, so the write path is
//! deliberately defensive: write to a temporary file, fsync it, rotate a
//! numbered backup, and only then replace the live file. A crash or a full disk
//! can cost the newest edit, never the accumulated document.

use crate::model::Document;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub const FILE_EXTENSION: &str = "json";
pub const DEFAULT_FILE_NAME: &str = "library.json";
/// How many rotating backups to keep alongside the library.
pub const BACKUP_COUNT: usize = 10;

/// Directory the library lives in by default.
///
/// Portable first: if a `library.json` sits next to the executable, or the
/// executable's folder is writable, use that, so the whole tool can live on a
/// USB stick. Otherwise fall back to `%APPDATA%\TimelineExplorer`.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = portable_dir() {
        return dir;
    }
    appdata_dir()
}

fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();

    // Never treat a cargo build directory as a portable install; during
    // development that would scatter libraries through target/.
    if dir
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("target"))
    {
        return None;
    }

    if dir.join(DEFAULT_FILE_NAME).is_file() || is_writable(&dir) {
        Some(dir)
    } else {
        None
    }
}

fn appdata_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("TimelineExplorer")
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".tl_write_probe");
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Full path to the library used when the user has not chosen one.
pub fn default_path() -> PathBuf {
    default_data_dir().join(DEFAULT_FILE_NAME)
}

/// Read a document. A missing file is not an error: it means "new library".
pub fn load(path: &Path) -> Result<Option<Document>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{} konnte nicht gelesen werden: {e}", path.display()))?;
    // Notepad and PowerShell write a UTF-8 BOM. The library is meant to be
    // user-inspectable, so a byte-order mark must not make it unreadable.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    if text.trim().is_empty() {
        return Ok(None);
    }
    let doc: Document = serde_json::from_str(text)
        .map_err(|e| format!("{} ist keine gültige Bibliotheksdatei: {e}", path.display()))?;
    Ok(Some(doc))
}

/// Write a document, replacing the previous file only once the new one is
/// safely on disk.
pub fn save(path: &Path, doc: &Document) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("{} konnte nicht angelegt werden: {e}", parent.display()))?;
        }
    }

    let json = serde_json::to_string_pretty(doc).map_err(|e| format!("Bibliothek konnte nicht kodiert werden: {e}"))?;

    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)
            .map_err(|e| format!("{} konnte nicht geschrieben werden: {e}", tmp.display()))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("{} konnte nicht geschrieben werden: {e}", tmp.display()))?;
        // Force the bytes out before anything replaces the live file.
        f.sync_all()
            .map_err(|e| format!("{} konnte nicht synchronisiert werden: {e}", tmp.display()))?;
    }

    if path.exists() {
        rotate_backups(path);
    }

    // `fs::rename` replaces the destination atomically on Windows as well as
    // Unix. Deleting first would leave a window in which the library does not
    // exist on disk at all — a crash there would cost the whole document.
    fs::rename(&tmp, path).map_err(|e| format!("{} konnte nicht fertiggestellt werden: {e}", path.display()))?;
    Ok(())
}

/// Minimum age of the newest backup before a save starts a fresh generation
/// rather than just refreshing it in place.
///
/// Autosave fires 1.2s after the last edit, so without this a burst of small
/// edits (nudging a date, retyping a title) would shove a genuinely old backup
/// out of the 10-slot ring every few seconds. Gating on age instead means the
/// 10 slots span real time — roughly the last couple of hours of editing —
/// rather than the last couple of minutes.
const MIN_BACKUP_AGE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

fn rotate_backups(path: &Path) {
    rotate_backups_impl(path, MIN_BACKUP_AGE);
}

/// Shift `library.bak1..bakN` down by one and copy the current file into slot
/// 1 — unless slot 1 is younger than `min_age`, in which case the backups are
/// left untouched entirely. That makes slot 1 a stable checkpoint of "the
/// state before this editing burst started" for as long as the burst
/// continues, rather than sliding forward with every single save (which would
/// leave it barely different from the live file). `min_age` is a parameter
/// (rather than always reading `MIN_BACKUP_AGE`) so tests can force either
/// behaviour deterministically instead of racing the wall clock.
fn rotate_backups_impl(path: &Path, min_age: std::time::Duration) {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "library".into());
    let slot = |n: usize| dir.join(format!("{stem}.bak{n}.json"));

    let slot1_is_fresh = fs::metadata(slot(1))
        .and_then(|m| m.modified())
        .and_then(|t| t.elapsed().map_err(std::io::Error::other))
        .is_ok_and(|age| age < min_age);

    if slot1_is_fresh {
        return;
    }

    let _ = fs::remove_file(slot(BACKUP_COUNT));
    for n in (1..BACKUP_COUNT).rev() {
        if slot(n).exists() {
            let _ = fs::rename(slot(n), slot(n + 1));
        }
    }
    // Best-effort: a failed backup must not block the save itself.
    let _ = fs::copy(path, slot(1));
}

/// Existing backups, newest first, as (path, human label).
pub fn backups(path: &Path) -> Vec<(PathBuf, String)> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "library".into());
    let mut out = Vec::new();
    for n in 1..=BACKUP_COUNT {
        let p = dir.join(format!("{stem}.bak{n}.json"));
        if p.is_file() {
            let label = match fs::metadata(&p).and_then(|m| m.modified()) {
                Ok(t) => format!("Sicherung {n} — {}", format_age(t)),
                Err(_) => format!("Sicherung {n}"),
            };
            out.push((p, label));
        }
    }
    out
}

/// Rough "how long ago", good enough for a restore picker without pulling in a
/// date-formatting dependency.
fn format_age(t: std::time::SystemTime) -> String {
    let Ok(elapsed) = t.elapsed() else {
        return "gerade eben".into();
    };
    let secs = elapsed.as_secs();
    if secs < 90 {
        "gerade eben".into()
    } else if secs < 3600 {
        format!("vor {} Min.", secs / 60)
    } else if secs < 86_400 {
        format!("vor {} Std.", secs / 3600)
    } else {
        format!("vor {} Tagen", secs / 86_400)
    }
}

/// Open the containing folder in Explorer.
pub fn reveal_in_explorer(path: &Path) {
    let target = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let _ = std::process::Command::new("explorer")
        .arg(target)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, HDate, Span, Timeline};

    fn temp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("timeline_explorer_test_{tag}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample() -> Document {
        let mut doc = Document::with_starter_categories();
        let id = doc.new_id();
        doc.timelines.push(Timeline {
            id,
            name: "Roman Republic".into(),
            color: [214, 96, 77],
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

    #[test]
    fn round_trips_through_disk() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("library.json");
        let doc = sample();
        save(&path, &doc).unwrap();
        let back = load(&path).unwrap().unwrap();
        assert_eq!(back, doc);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_means_new_library_not_an_error() {
        let dir = temp_dir("missing");
        assert!(load(&dir.join("nope.json")).unwrap().is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_re_saved_by_notepad_with_a_bom_still_loads() {
        // Editing the library by hand is an advertised feature; Windows editors
        // add a UTF-8 BOM, which serde_json would otherwise choke on.
        let dir = temp_dir("bom");
        let path = dir.join("library.json");
        save(&path, &sample()).unwrap();
        let body = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("\u{feff}{body}")).unwrap();

        let doc = load(&path).expect("a BOM must not break loading").unwrap();
        assert_eq!(doc.timelines[0].name, "Roman Republic");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_reports_an_error_and_does_not_pretend_to_be_empty() {
        let dir = temp_dir("corrupt");
        let path = dir.join("library.json");
        fs::write(&path, "{ this is not json").unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_repeatedly_rotates_backups_and_keeps_the_previous_content() {
        let dir = temp_dir("backups");
        let path = dir.join("library.json");

        let mut doc = sample();
        save(&path, &doc).unwrap();

        doc.timelines[0].name = "Second".into();
        save(&path, &doc).unwrap();

        let list = backups(&path);
        assert!(!list.is_empty(), "expected a backup after the second save");
        // bak1 holds what was live before the most recent save.
        let prev = load(&list[0].0).unwrap().unwrap();
        assert_eq!(prev.timelines[0].name, "Roman Republic");
        assert_eq!(load(&path).unwrap().unwrap().timelines[0].name, "Second");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backups_are_capped() {
        // Forces every save to start a fresh generation (min_age = 0), which
        // is what a real ring of *spaced-out* saves looks like — this is the
        // scenario `rotate_backups` degrades to once each backup is old
        // enough to no longer be "fresh".
        let dir = temp_dir("cap");
        let path = dir.join("library.json");
        let doc = sample();
        save(&path, &doc).unwrap();
        for _ in 0..(BACKUP_COUNT + 5) {
            rotate_backups_impl(&path, std::time::Duration::ZERO);
        }
        assert!(backups(&path).len() <= BACKUP_COUNT);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rapid_saves_coalesce_into_the_newest_backup_slot_instead_of_flooding_it() {
        // A burst of small edits (autosave fires 1.2s after each one) used to
        // shove a fresh backup into the ring every time, so 10 slots covered
        // only the last few minutes of editing instead of real history. Back
        // to back saves within MIN_BACKUP_AGE of each other must leave the
        // backups untouched instead of rotating the chain each time.
        let dir = temp_dir("coalesce");
        let path = dir.join("library.json");
        let mut doc = sample();
        save(&path, &doc).unwrap();

        for i in 0..5 {
            doc.timelines[0].name = format!("Edit {i}");
            save(&path, &doc).unwrap();
        }

        let list = backups(&path);
        assert_eq!(
            list.len(),
            1,
            "rapid saves should share one backup slot, not rotate a new one each time"
        );
        // Slot 1 must hold the content from just before the *first* of the
        // rapid edits — the pre-burst snapshot, not overwritten with a
        // half-way state and not lost either.
        let prev = load(&list[0].0).unwrap().unwrap();
        assert_eq!(prev.timelines[0].name, "Roman Republic");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn overwriting_never_leaves_the_library_missing() {
        // Guards the atomic replace: at no point may saving over an existing
        // library remove it. Losing the file to a crash mid-save would be the
        // worst possible failure for a dataset built up over years.
        let dir = temp_dir("atomic");
        let path = dir.join("library.json");
        save(&path, &sample()).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let mut doc = sample();
        doc.timelines[0].name = "Changed".into();
        save(&path, &doc).unwrap();

        assert!(path.is_file(), "library must exist after an overwrite");
        let after = fs::read_to_string(&path).unwrap();
        assert_ne!(before, after);
        assert_eq!(load(&path).unwrap().unwrap().timelines[0].name, "Changed");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_temp_file_is_left_behind() {
        let dir = temp_dir("tmp");
        let path = dir.join("library.json");
        save(&path, &sample()).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_fields_and_missing_fields_still_load() {
        // Forward/backward compatibility: a file from another build version.
        let dir = temp_dir("compat");
        let path = dir.join("library.json");
        fs::write(
            &path,
            r#"{"version":1,"next_id":5,"timelines":[
                 {"id":2,"name":"Sparta","color":[1,2,3],"future_field":true}
               ]}"#,
        )
        .unwrap();
        let doc = load(&path).unwrap().unwrap();
        assert_eq!(doc.timelines.len(), 1);
        assert!(doc.timelines[0].visible, "visible should default to true");
        assert!(doc.events.is_empty());
        fs::remove_dir_all(&dir).ok();
    }
}
