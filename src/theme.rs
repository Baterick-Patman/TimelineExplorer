//! Palette and visual encoding rules.
//!
//! The request asks for colour to carry *identity* (which culture) while size
//! and weight carry *significance*. Keeping those two channels separate is what
//! keeps a dense chart readable, so the mapping lives here rather than being
//! scattered through the painting code.

use crate::model::{Rgb, IMPORTANCE_MAX, IMPORTANCE_MIN};
use egui::Color32;

pub struct Theme {
    pub canvas_bg: Color32,
    pub lane_stripe: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub ruler_bg: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub era_line: Color32,
    pub selection: Color32,
}

impl Theme {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                canvas_bg: Color32::from_rgb(0x15, 0x17, 0x1c),
                lane_stripe: Color32::from_rgba_unmultiplied(255, 255, 255, 6),
                grid_minor: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                grid_major: Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                ruler_bg: Color32::from_rgb(0x1c, 0x1f, 0x26),
                text: Color32::from_rgb(0xe6, 0xe8, 0xee),
                text_dim: Color32::from_rgb(0x92, 0x99, 0xa8),
                era_line: Color32::from_rgba_unmultiplied(255, 210, 130, 60),
                selection: Color32::from_rgb(0xff, 0xff, 0xff),
            }
        } else {
            Self {
                canvas_bg: Color32::from_rgb(0xfa, 0xfa, 0xf7),
                lane_stripe: Color32::from_rgba_unmultiplied(0, 0, 0, 8),
                grid_minor: Color32::from_rgba_unmultiplied(0, 0, 0, 18),
                grid_major: Color32::from_rgba_unmultiplied(0, 0, 0, 38),
                ruler_bg: Color32::from_rgb(0xef, 0xef, 0xea),
                text: Color32::from_rgb(0x20, 0x22, 0x28),
                text_dim: Color32::from_rgb(0x66, 0x6c, 0x78),
                era_line: Color32::from_rgba_unmultiplied(150, 100, 0, 70),
                selection: Color32::from_rgb(0x10, 0x12, 0x18),
            }
        }
    }
}

/// Base egui visuals for the app.
///
/// egui sizes a plain button as `content + inner_margin + 2 * bg_stroke.width`,
/// with `inner_margin` derived by *subtracting* the stroke width from the
/// button padding so that outlining a widget does not change its size. A small
/// button already has zero vertical padding, so the 1px outline the default
/// theme adds on hover has nothing to subtract from: the button grows 2px
/// taller and shoves everything below it down.
///
/// Toggle-style widgets (`selectable_label`, `Button::selectable` — used for
/// "Show only" / "Off" / "Hide" and similar) make this worse: while unselected
/// and not hovered, egui swaps in a completely blank frame that drops the
/// stroke term from the size formula rather than just using a differently
/// coloured one. So simply giving every state *the same nonzero* stroke width
/// (as opposed to zero) is not enough — the blank frame still omits it, and
/// the widget still grows by `2 * width` the moment it's hovered. The only
/// width that balances both code paths is zero. Stroke *colour* still differs
/// per state (via `bg_fill`/`weak_bg_fill`), so hovering is still visible —
/// just not via an outline that can change geometry.
pub fn visuals(dark: bool) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // `noninteractive` is deliberately left alone: it draws separators and
    // indentation lines rather than widget outlines.
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_stroke.width = 0.0;
    }
    v
}

pub fn to_color(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Blend towards white (`amount > 0`) or black (`amount < 0`).
pub fn shade(c: Color32, amount: f32) -> Color32 {
    let target = if amount >= 0.0 { 255.0 } else { 0.0 };
    let k = amount.abs().clamp(0.0, 1.0);
    let mix = |v: u8| (v as f32 + (target - v as f32) * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

// --- Importance encoding ----------------------------------------------------

/// Below this zoom, event labels sit at their plain per-importance size;
/// above it, they grow toward a capped maximum. Without this, a "Detail"
/// (importance 1) event's title stayed stuck at its smallest, barely
/// legible size even at the app's maximum zoom, since font size was
/// otherwise driven by importance alone.
const LABEL_ZOOM_GROWTH_START_PPY: f64 = 15.0;
const LABEL_ZOOM_GROWTH_END_PPY: f64 = 150.0;
const LABEL_ZOOM_GROWTH_MAX_PX: f32 = 4.0;

/// Font size for an entry of the given significance, at the given zoom.
///
/// The per-importance spread is deliberately wide: this is the channel the
/// user asked for to tell major events from footnotes at a glance. Zoom
/// only ever adds on top of that baseline, up to `LABEL_ZOOM_GROWTH_MAX_PX`
/// — it never lets a low-importance label catch up to a higher one, just
/// makes it readable once there is obviously enough room for it.
pub fn label_font_size(importance: u8, ppy: f64) -> f32 {
    let base = match importance.clamp(IMPORTANCE_MIN, IMPORTANCE_MAX) {
        5 => 16.0,
        4 => 14.0,
        3 => 12.5,
        2 => 11.0,
        _ => 10.0,
    };
    let t = ((ppy - LABEL_ZOOM_GROWTH_START_PPY) / (LABEL_ZOOM_GROWTH_END_PPY - LABEL_ZOOM_GROWTH_START_PPY))
        .clamp(0.0, 1.0) as f32;
    base + LABEL_ZOOM_GROWTH_MAX_PX * t
}

/// Marker radius for an entry of the given significance.
pub fn marker_radius(importance: u8) -> f32 {
    match importance.clamp(IMPORTANCE_MIN, IMPORTANCE_MAX) {
        5 => 7.0,
        4 => 5.5,
        3 => 4.5,
        2 => 3.5,
        _ => 2.8,
    }
}

/// Opacity for an entry of the given significance. Minor entries recede
/// without disappearing, which keeps dense stretches legible.
pub fn importance_alpha(importance: u8) -> u8 {
    match importance.clamp(IMPORTANCE_MIN, IMPORTANCE_MAX) {
        5 => 255,
        4 => 240,
        3 => 215,
        2 => 185,
        _ => 155,
    }
}

/// Thickness of the bar drawn for a date range.
pub fn range_bar_height(importance: u8) -> f32 {
    match importance.clamp(IMPORTANCE_MIN, IMPORTANCE_MAX) {
        5 => 9.0,
        4 => 7.5,
        3 => 6.0,
        2 => 5.0,
        _ => 4.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importance_encoding_is_monotonic() {
        // Every visual channel must agree on the ordering, or the encoding
        // sends mixed signals.
        for i in IMPORTANCE_MIN..IMPORTANCE_MAX {
            assert!(label_font_size(i, 1.0) < label_font_size(i + 1, 1.0));
            assert!(marker_radius(i) < marker_radius(i + 1));
            assert!(importance_alpha(i) < importance_alpha(i + 1));
            assert!(range_bar_height(i) < range_bar_height(i + 1));
        }
    }

    #[test]
    fn label_rows_are_tall_enough_for_the_largest_label() {
        // Rows are a uniform height while labels are not, so the row must clear
        // the tallest line — at the *most zoomed-in* a label can get, since
        // zoom grows font size on top of the per-importance baseline — or
        // important titles overlap their neighbours.
        let tallest = label_font_size(IMPORTANCE_MAX, LABEL_ZOOM_GROWTH_END_PPY) * 1.2;
        assert!(
            crate::layout::LABEL_ROW_HEIGHT >= tallest,
            "row height {} is too small for a {}px label",
            crate::layout::LABEL_ROW_HEIGHT,
            label_font_size(IMPORTANCE_MAX, LABEL_ZOOM_GROWTH_END_PPY)
        );
    }

    #[test]
    fn encoding_is_clamped_for_out_of_range_values() {
        assert_eq!(label_font_size(0, 1.0), label_font_size(1, 1.0));
        assert_eq!(label_font_size(99, 1.0), label_font_size(5, 1.0));
        assert_eq!(marker_radius(0), marker_radius(1));
    }

    #[test]
    fn label_font_size_grows_with_zoom_but_keeps_the_importance_ordering() {
        // Below the growth threshold, zoom changes nothing.
        assert_eq!(label_font_size(1, 1.0), label_font_size(1, LABEL_ZOOM_GROWTH_START_PPY));
        // Zoomed in past the growth window, a low-importance label grows...
        let grown = label_font_size(1, LABEL_ZOOM_GROWTH_END_PPY);
        assert!(grown > label_font_size(1, 1.0), "should have grown");
        // ...but at that *same* zoom, importance must still read as a size
        // hierarchy — the bonus is additive, not tier-catching-up.
        assert!(grown < label_font_size(2, LABEL_ZOOM_GROWTH_END_PPY));
        // Zooming in further than the growth window must not keep growing it.
        assert_eq!(grown, label_font_size(1, LABEL_ZOOM_GROWTH_END_PPY * 10.0));
    }

    #[test]
    fn shade_moves_towards_white_and_black() {
        let c = Color32::from_rgb(100, 100, 100);
        assert!(shade(c, 0.5).r() > 100);
        assert!(shade(c, -0.5).r() < 100);
        assert_eq!(shade(c, 0.0), c);
        assert_eq!(shade(c, 1.0), Color32::WHITE);
    }

}

#[cfg(test)]
mod hover_stability {
    /// Drive a bare egui context with synthetic input and report the button's
    /// rect plus the top of the widget placed directly beneath it.
    fn probe(ctx: &egui::Context, pointer: egui::Pos2) -> (egui::Rect, f32) {
        let out = std::cell::Cell::new((egui::Rect::NOTHING, 0.0f32));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            events: vec![egui::Event::PointerMoved(pointer)],
            ..Default::default()
        };
        for _ in 0..4 {
            let mut full = ctx.run_ui(input.clone(), |ui| {
                let b = ui.small_button("+ group at top level");
                let below = ui.label("below");
                out.set((b.rect, below.rect.top()));
            });
            // The context hands back texture deltas that must be consumed.
            full.textures_delta.clear();
        }
        out.get()
    }

    /// Hovering a button must not resize it or move anything below it.
    ///
    /// The default egui theme fails this for small buttons: the 1px hover
    /// outline makes them 2px taller, so a sidebar visibly jumps under the
    /// cursor. `theme::visuals` fixes it; this pins the fix down.
    fn assert_stable(dark: bool) {
        let ctx = egui::Context::default();
        ctx.set_visuals(super::visuals(dark));

        let far = egui::pos2(390.0, 390.0);
        let (rect_off, below_off) = probe(&ctx, far);
        let (rect_on, below_on) = probe(&ctx, rect_off.center());

        assert_eq!(
            rect_off.size(),
            rect_on.size(),
            "button resized on hover (dark={dark})"
        );
        assert_eq!(
            below_off, below_on,
            "widgets below the button moved on hover (dark={dark})"
        );
    }

    #[test]
    fn hovering_a_button_does_not_move_the_widgets_below_it() {
        assert_stable(true);
        assert_stable(false);
    }

    /// Same idea as [`probe`], but for a horizontal row of widgets — the
    /// shape of the "Off / Show only / Hide" filter-mode row — checking the
    /// left edge of the widget to the *right* rather than below.
    fn probe_row(ctx: &egui::Context, pointer: egui::Pos2) -> (egui::Rect, f32) {
        let out = std::cell::Cell::new((egui::Rect::NOTHING, 0.0f32));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            events: vec![egui::Event::PointerMoved(pointer)],
            ..Default::default()
        };
        for _ in 0..4 {
            let mut full = ctx.run_ui(input.clone(), |ui| {
                ui.horizontal(|ui| {
                    let b = ui.selectable_label(false, "Show only");
                    let right = ui.label("Hide");
                    out.set((b.rect, right.rect.left()));
                });
            });
            full.textures_delta.clear();
        }
        out.get()
    }

    /// Hovering an *unselected* `selectable_label` must not resize it either.
    ///
    /// This is a distinct bug from the plain-button case above: egui swaps in
    /// a blank frame (no stroke at all) while a `selectable_label` is
    /// unselected and unhovered, so merely giving every widget state the same
    /// *nonzero* stroke width — which is enough to fix plain buttons — still
    /// leaves this widget growing by `2 * width` on hover. See the comment on
    /// `visuals` for the full explanation.
    #[test]
    fn hovering_an_unselected_toggle_does_not_move_widgets_beside_it() {
        for dark in [true, false] {
            let ctx = egui::Context::default();
            ctx.set_visuals(super::visuals(dark));
            let far = egui::pos2(390.0, 390.0);
            let (rect_off, right_off) = probe_row(&ctx, far);
            let (rect_on, right_on) = probe_row(&ctx, rect_off.center());
            assert_eq!(
                rect_off.size(),
                rect_on.size(),
                "selectable_label resized on hover (dark={dark})"
            );
            assert_eq!(
                right_off, right_on,
                "widget to the right moved on hover (dark={dark})"
            );
        }
    }

    #[test]
    fn the_default_theme_is_what_needed_fixing() {
        // Documents the underlying egui behaviour this works around, so the
        // workaround can be dropped if upstream ever changes it.
        let ctx = egui::Context::default();
        let far = egui::pos2(390.0, 390.0);
        let (rect_off, _) = probe(&ctx, far);
        let (rect_on, _) = probe(&ctx, rect_off.center());
        assert_ne!(
            rect_off.size(),
            rect_on.size(),
            "egui's default theme no longer resizes small buttons on hover -              theme::visuals can lose its stroke-width normalisation"
        );
    }
}
