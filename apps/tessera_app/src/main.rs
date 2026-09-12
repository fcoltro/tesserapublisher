//! Tessera Publisher.
//!
//! Thin by design: this binary wires things together and owns nothing.

// A console window alongside the app is for debugging, not for users.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icon;
mod platform;
mod releases;

use tessera_ui::TesseraApp;

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 840.0])
        .with_min_inner_size([720.0, 480.0])
        .with_title("Tessera Publisher")
        // Take focus on launch. Without this the window can open behind
        // whatever the user clicked while it was starting.
        .with_active(true)
        // Keep the inner size above for restoring the maximized window.
        // Maximize after native creation applies the initial size and DPI.
        // Creating already maximized lets that size request shrink the window
        // while Windows still reports it as maximized.
        .with_maximized(false);

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
            // After the preferences, because the switch deciding whether to
            // look for a newer version is one of them. On a thread of its own,
            // so a slow server delays nothing — and the fetch is passed in
            // because `tessera_ui` deliberately has no HTTP client.
            app.begin_update_check(releases::newest);
            // Before the first frame: work from a session that did not close
            // is offered rather than quietly discarded.
            tessera_ui::recovery::offer_pending(&mut app);
            // And if there is nothing to come back to, ask what to make. A page
            // size, a bleed and a press are decisions a job is built on, and a
            // document that appears without being asked for has already made
            // all three on somebody's behalf.
            app.ask_what_to_make();
            // And on a first run, offer to say where everything is. Offered
            // rather than started: it waits until the dialog above has been
            // dealt with, because a tour of the interface behind a modal is a
            // tour of something nobody can look at.
            app.offer_tour_on_first_run();

            Ok(Box::new(NativeApp {
                app,
                maximize_pending: true,
                fit_after_maximize: true,
            }) as Box<dyn eframe::App>)
        }),
    )
}

/// Native startup policy lives in the binary rather than the headless UI state.
struct NativeApp {
    app: TesseraApp,
    maximize_pending: bool,
    fit_after_maximize: bool,
}

impl eframe::App for NativeApp {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.maximize_pending) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            ctx.request_repaint();
        }
        if self.fit_after_maximize && ctx.input(|i| i.viewport().maximized == Some(true)) {
            // The first frame may have fitted the restored-size canvas.
            // Refit once after the OS has delivered the maximized dimensions.
            self.app.active_mut().fitted = false;
            self.fit_after_maximize = false;
        }
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
    }

    fn on_exit(&mut self) {
        self.app.on_exit();
    }
}
