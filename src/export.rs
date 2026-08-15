//! Export a chosen slice of the library — a date range, a subset of
//! timelines/groups, optionally their biographies, down to a minimum
//! importance — as a PNG image or a single-page PDF.
//!
//! The approach deliberately reuses the real canvas painter rather than a
//! second rendering backend: [`ExportJob`] temporarily swaps `app.doc` for a
//! filtered clone, sizes the axis to the chosen range, hides every panel but
//! the canvas, resizes the window to fit the content exactly, then asks
//! egui for a screenshot of that frame. That is what keeps this file free of
//! any text-layout or curve-drawing code of its own — `canvas.rs` already
//! does all of that, correctly, once.

use crate::layout::{self, TimeAxis};
use crate::model::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    Png,
    Pdf,
}

/// Build a standalone document containing only the chosen slice of the
/// original: the selected timelines (and the groups on the path to each, so
/// bands still resolve `origin`/`merge` targets and collapse state renders
/// sensibly), their events at or above `min_importance`, and — if
/// `include_biographies` — biographies linked to one of those timelines or
/// with no culture at all.
pub fn build_export_document(
    doc: &Document,
    timelines: &BTreeSet<Id>,
    include_biographies: bool,
    min_importance: u8,
) -> Document {
    let mut out = doc.clone();
    out.timelines.retain(|t| timelines.contains(&t.id));

    // Every group on the path from a kept timeline up to the root, so the
    // sidebar's collapse/nest structure still makes sense around it.
    let mut keep_groups: BTreeSet<Id> = BTreeSet::new();
    for t in &out.timelines {
        let mut g = t.group;
        while let Some(id) = g {
            if !keep_groups.insert(id) {
                break;
            }
            g = doc.group(id).and_then(|gr| gr.parent);
        }
    }
    out.groups.retain(|g| keep_groups.contains(&g.id));
    // Collapsing would hide timelines the user explicitly chose to export.
    for g in &mut out.groups {
        g.collapsed = false;
        g.visible = true;
    }
    for t in &mut out.timelines {
        t.visible = true;
    }

    out.biographies.retain(|b| match b.timeline {
        Some(t) => timelines.contains(&t),
        None => include_biographies,
    });
    if !include_biographies {
        out.biographies.clear();
    }

    let biography_ids: BTreeSet<Id> = out.biographies.iter().map(|b| b.id).collect();
    out.events.retain(|e| {
        if e.importance < min_importance {
            return false;
        }
        match e.owner {
            OwnerRef::Timeline(t) => timelines.contains(&t),
            OwnerRef::Biography(b) => biography_ids.contains(&b),
        }
    });

    // The zoom-dependent detail threshold must never hide something this
    // function already decided to include — push it far past any level the
    // slider itself allows, relying on `importance_threshold`'s own clamp.
    out.view.filters = Filters {
        detail_bias: 10,
        ..Default::default()
    };
    out
}

/// Axis and pixel width for framing `[from, to]` at `width_px`, the same
/// "pad a little, clamp the zoom" shape as `TimelineApp::fit_to_content`.
pub fn export_axis(from: f64, to: f64, width_px: f32) -> (TimeAxis, f32) {
    let span = (to - from).max(1.0);
    let pad = span * 0.06;
    let ppy = (width_px as f64 / (span + pad * 2.0)).clamp(layout::MIN_PPY, layout::MAX_PPY);
    (TimeAxis::new(0.0, from - pad, ppy), width_px)
}

/// Wrap a JPEG-encoded image as a minimal single-page, single-image PDF —
/// the image scaled to fill the page exactly. Hand-written rather than
/// pulling in a PDF-writing crate: a one-image PDF is a small, fixed
/// structure, and this keeps the export path free of a dependency whose
/// page/content-stream API is a moving target across releases.
///
/// `dpi` is only used to convert the pixel size to a physical page size
/// (PDF points, 72 per inch) — the image itself is embedded at full
/// resolution regardless.
pub fn wrap_jpeg_as_pdf(jpeg: &[u8], width_px: u32, height_px: u32, dpi: f64) -> Vec<u8> {
    let w = width_px as f64 * 72.0 / dpi;
    let h = height_px as f64 * 72.0 / dpi;
    let content = format!("q {w:.2} 0 0 {h:.2} 0 0 cm /Im0 Do Q");

    let mut buf: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = vec![0]; // slot 0 is the free-list head, never a real object
    buf.extend_from_slice(b"%PDF-1.4\n");

    fn obj(buf: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]) {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", offsets.len() - 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }

    obj(&mut buf, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut buf, &mut offsets, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut buf,
        &mut offsets,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] \
             /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>"
        )
        .as_bytes(),
    );
    // Objects 4 and 5 carry binary/stream bodies, so they are written
    // directly rather than through `obj`, which assumes a plain dict body.
    offsets.push(buf.len());
    buf.extend_from_slice(b"4 0 obj\n");
    buf.extend_from_slice(
        format!(
            "<< /Type /XObject /Subtype /Image /Width {width_px} /Height {height_px} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .as_bytes(),
    );
    buf.extend_from_slice(jpeg);
    buf.extend_from_slice(b"\nendstream\nendobj\n");

    offsets.push(buf.len());
    buf.extend_from_slice(b"5 0 obj\n");
    buf.extend_from_slice(format!("<< /Length {} >>\nstream\n", content.len()).as_bytes());
    buf.extend_from_slice(content.as_bytes());
    buf.extend_from_slice(b"\nendstream\nendobj\n");

    let xref_start = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for &off in &offsets[1..] {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_start}\n%%EOF",
            offsets.len()
        )
        .as_bytes(),
    );
    buf
}

/// Convert a screenshot into JPEG bytes, dropping the alpha channel (the
/// canvas background is already opaque, and JPEG has no alpha channel to
/// carry it in anyway).
pub fn encode_jpeg(rgba: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("JPEG-Kodierung fehlgeschlagen: {e}"))?;
    Ok(out)
}

/// Save a screenshot as a PNG file.
pub fn save_png(path: &std::path::Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    image::save_buffer(path, rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("PNG konnte nicht gespeichert werden: {e}"))
}

/// Save a screenshot as a single-page PDF.
pub fn save_pdf(path: &std::path::Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    let jpeg = encode_jpeg(rgba, width, height, 90)?;
    let pdf = wrap_jpeg_as_pdf(&jpeg, width, height, 96.0);
    std::fs::write(path, pdf).map_err(|e| format!("PDF konnte nicht gespeichert werden: {e}"))
}

// ---------------------------------------------------------------------------
// The multi-frame capture job
// ---------------------------------------------------------------------------

/// Where an in-progress export is in its multi-frame dance.
///
/// `Preparing` exists only to absorb the one frame of lag between swapping
/// in the export document (mid-frame, from inside the dialog's button
/// handler) and that document actually being what `canvas::draw` painted —
/// `TimelineApp::last_lanes` on the swap's own frame still reflects the
/// *previous* document. `Measuring` is therefore the first stage allowed to
/// read `last_lanes`. `Settling` then waits a few frames for the OS to
/// actually apply the window resize before `Capturing` requests the
/// screenshot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportStage {
    Preparing,
    Measuring,
    Settling(u8),
    Capturing,
}

pub struct ExportJob {
    pub stage: ExportStage,
    pub format: ExportFormat,
    pub path: PathBuf,
    pub width_px: f32,
    /// The real document, swapped back in once the capture completes.
    pub restore_doc: Document,
    pub restore_y_offset: f32,
    pub restore_window_size: egui::Vec2,
    pub restore_selection: Option<crate::app::Selection>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline(doc: &mut Document, name: &str, group: Option<Id>) -> Id {
        let id = doc.new_id();
        doc.timelines.push(Timeline {
            id,
            name: name.into(),
            color: [0, 0, 0],
            visible: true,
            group,
            order: 0,
            span: None,
            origin: None,
            merge: None,
            notes: String::new(),
            epochs: Vec::new(),
        });
        id
    }

    fn group(doc: &mut Document, name: &str, parent: Option<Id>) -> Id {
        let id = doc.new_id();
        doc.groups.push(Group {
            id,
            name: name.into(),
            color: [0, 0, 0],
            parent,
            order: 0,
            collapsed: true,
            visible: true,
            notes: String::new(),
        });
        id
    }

    fn event(doc: &mut Document, owner: OwnerRef, title: &str, importance: u8) -> Id {
        let id = doc.new_id();
        doc.events.push(Event {
            id,
            owner,
            title: title.into(),
            description: String::new(),
            span: Span::point(HDate::year(-100)),
            importance,
            categories: vec![],
            parent: None,
        });
        id
    }

    fn bio(doc: &mut Document, name: &str, timeline: Option<Id>) -> Id {
        let id = doc.new_id();
        doc.biographies.push(Biography {
            id,
            name: name.into(),
            timeline,
            birth: HDate::year(-100),
            death: None,
            color: None,
            categories: vec![],
            importance: 3,
            display: BioDisplay::Lane,
            notes: String::new(),
        });
        id
    }

    #[test]
    fn export_keeps_only_selected_timelines_and_their_ancestor_groups() {
        let mut doc = Document::default();
        let antiquity = group(&mut doc, "Antiquity", None);
        let greek = group(&mut doc, "Greek antiquity", Some(antiquity));
        let athens = timeline(&mut doc, "Athens", Some(greek));
        let rome = timeline(&mut doc, "Rome", None);

        let mut selected = BTreeSet::new();
        selected.insert(athens);
        let out = build_export_document(&doc, &selected, false, 1);

        assert_eq!(out.timelines.len(), 1);
        assert_eq!(out.timelines[0].id, athens);
        assert!(!out.timelines.iter().any(|t| t.id == rome));
        // Both ancestor groups are kept so the exported timeline still
        // resolves its place in the hierarchy.
        assert!(out.groups.iter().any(|g| g.id == greek));
        assert!(out.groups.iter().any(|g| g.id == antiquity));
        // Collapsing would hide the very timeline that was explicitly chosen.
        assert!(out.groups.iter().all(|g| !g.collapsed));
    }

    #[test]
    fn export_drops_events_below_the_minimum_importance() {
        let mut doc = Document::default();
        let athens = timeline(&mut doc, "Athens", None);
        let major = event(&mut doc, OwnerRef::Timeline(athens), "Major", 4);
        let minor = event(&mut doc, OwnerRef::Timeline(athens), "Minor", 2);

        let mut selected = BTreeSet::new();
        selected.insert(athens);
        let out = build_export_document(&doc, &selected, false, 3);

        assert!(out.events.iter().any(|e| e.id == major));
        assert!(!out.events.iter().any(|e| e.id == minor));
    }

    #[test]
    fn export_without_biographies_drops_them_and_their_events() {
        let mut doc = Document::default();
        let athens = timeline(&mut doc, "Athens", None);
        let socrates = bio(&mut doc, "Socrates", Some(athens));
        event(&mut doc, OwnerRef::Biography(socrates), "Trial", 5);

        let mut selected = BTreeSet::new();
        selected.insert(athens);
        let out = build_export_document(&doc, &selected, false, 1);

        assert!(out.biographies.is_empty());
        assert!(out.events.is_empty());
    }

    #[test]
    fn export_with_biographies_keeps_only_those_linked_to_a_selected_timeline() {
        let mut doc = Document::default();
        let athens = timeline(&mut doc, "Athens", None);
        let rome = timeline(&mut doc, "Rome", None);
        let socrates = bio(&mut doc, "Socrates", Some(athens));
        let cicero = bio(&mut doc, "Cicero", Some(rome));
        let unlinked = bio(&mut doc, "Homer", None);

        let mut selected = BTreeSet::new();
        selected.insert(athens);
        let out = build_export_document(&doc, &selected, true, 1);

        assert!(out.biographies.iter().any(|b| b.id == socrates));
        assert!(!out.biographies.iter().any(|b| b.id == cicero));
        // Unlinked biographies ride along whenever biographies are included
        // at all — there is no timeline to have excluded them from.
        assert!(out.biographies.iter().any(|b| b.id == unlinked));
    }

    #[test]
    fn export_axis_frames_the_requested_range_with_a_small_pad() {
        let (axis, width) = export_axis(-500.0, -300.0, 1000.0);
        // The left edge starts before -500 (padding), and 1000px must not
        // all be needed for the raw 200-year span alone.
        assert!(axis.left_year < -500.0);
        assert!(axis.ppy < width as f64);
        // The requested range must fit inside the framed window.
        let axis_end = axis.left_year + width as f64 / axis.ppy;
        assert!(axis_end > -300.0);
    }

    #[test]
    fn wrapped_pdf_has_a_valid_looking_structure() {
        let fake_jpeg = vec![0xFFu8, 0xD8, 0xFF, 0xD9]; // SOI + EOI, minimal
        let pdf = wrap_jpeg_as_pdf(&fake_jpeg, 100, 50, 96.0);
        let text_prefix = String::from_utf8_lossy(&pdf[..20]);
        assert!(text_prefix.starts_with("%PDF-1.4"));
        let tail = String::from_utf8_lossy(&pdf[pdf.len().saturating_sub(400)..]);
        assert!(tail.contains("trailer"));
        assert!(tail.contains("startxref"));
        assert!(tail.contains("%%EOF"));
        // The embedded stream bytes must appear verbatim (uncompressed passthrough).
        assert!(pdf.windows(fake_jpeg.len()).any(|w| w == fake_jpeg.as_slice()));
    }

    #[test]
    fn jpeg_encoding_round_trips_through_the_image_crate() {
        // 2x2 opaque red RGBA image.
        let rgba = vec![255u8, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
        let jpeg = encode_jpeg(&rgba, 2, 2, 90).expect("jpeg encode must succeed");
        assert!(jpeg.starts_with(&[0xFF, 0xD8])); // JPEG SOI marker
    }
}
