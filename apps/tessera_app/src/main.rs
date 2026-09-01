//! Tessera Publisher.
//!
//! Thin by design: this binary wires things together and owns nothing.

// A console window alongside the app is for debugging, not for users.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod platform;

use tessera_ui::TesseraApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("Tessera Publisher")
            // Take focus on launch. Without this the window can open behind
            // whatever the user clicked while it was starting.
            .with_active(true),
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

            Ok(Box::new(TesseraApp::headless()) as Box<dyn eframe::App>)
        }),
    )
}
