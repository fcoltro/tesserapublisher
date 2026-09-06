//! Tessera Publisher.
//!
//! Thin by design: this binary wires things together and owns nothing.

// A console window alongside the app is for debugging, not for users.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icon;
mod platform;

use tessera_ui::TesseraApp;

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 840.0])
        .with_min_inner_size([720.0, 480.0])
        .with_title("Tessera Publisher")
        // Take focus on launch. Without this the window can open behind
        // whatever the user clicked while it was starting.
        .with_active(true)
        // Maximised. A layout application is what Alan Cooper calls a
        // sovereign application — one a person works inside for hours at a
        // time, with nothing else competing for the screen — and his guidance
        // for those is to take the whole of it. The inner size above stays as
        // the size the window restores to.
        .with_maximized(true);

    // Only when there is one. An empty `IconData` is not "no icon", it is a
    // zero-by-zero icon, and the window manager is entitled to make a mess
    // of it.
    if let Some(mark) = icon::load() {
        viewport = viewport.with_icon(mark);
    }

    let options = eframe::NativeOptions {
        viewport,
        // No WgpuConfiguration: the Task 1 spike established that Vello runs
        // on eframe's stock device, with no extra features and no raised
        // limits. See docs/superpowers/notes/2026-09-01-vello-egui-spike.md.
        ..Default::default()
    };

    eframe::run_native(
        "Tessera Publisher",
        options,
        Box::new(|cc| {
            tessera_ui::theme::apply(&cc.egui_ctx);

            let render_state = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("Tessera needs the wgpu backend, which failed to start")?;
            tessera_ui::view::vello_host::install(render_state)?;

            let mut app = TesseraApp::headless();
            app.load_preferences();
            // Before the first frame: work from a session that did not close
            // is offered rather than quietly discarded.
            tessera_ui::recovery::offer_pending(&mut app);

            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )
}
