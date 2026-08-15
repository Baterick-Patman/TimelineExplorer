//! Timeline Explorer — a local, offline-first tool for building and
//! comparing parallel historical timelines.
//!
//! Ships as a single self-contained Windows executable: no installer, no
//! runtime to install, and no network access unless the user explicitly
//! asks for it — the one exception is `import::fetch_url`, used only by the
//! "Von URL laden" button in the import dialog. Nothing else in the app
//! ever makes a network request.

// No console window when launched from Explorer.
#![windows_subsystem = "windows"]

mod app;
mod canvas;
mod example;
mod export;
mod forms;
mod import;
mod layout;
mod model;
mod panels;
mod store;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 880.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("Timeline Explorer"),
        ..Default::default()
    };

    eframe::run_native(
        "Timeline Explorer",
        options,
        Box::new(|cc| {
            // Slightly roomier text than the egui default; this app is read as
            // much as it is clicked.
            cc.egui_ctx.set_zoom_factor(1.05);
            Ok(Box::new(app::TimelineApp::new()))
        }),
    )
}
