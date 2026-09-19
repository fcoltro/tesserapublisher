//! Tessera Publisher.
//!
//! Thin by design: this binary wires things together and owns nothing.

// A console window alongside the app is for debugging, not for users.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icon;
mod model_http;
mod platform;
mod releases;

use tessera_ui::TesseraApp;

fn main() -> eframe::Result<()> {
    // `tessera_app --mcp`: no window, and stdin and stdout are a model's.
    // A model's client launches the process itself, which is why this is a
    // flag on the one binary rather than a second one to find and ship.
    // When a window is already up it is relayed to, so the model works on
    // the document somebody is looking at; otherwise this process is a
    // headless Tessera of its own.
    if std::env::args().skip(1).any(|a| a == "--mcp") {
        match bridge_port_file() {
            Some(file) => tessera_bridge::live::serve_or_relay_stdio(&file),
            None => tessera_bridge::serve_stdio(),
        }
        return Ok(());
    }

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

    let startup_paths: Vec<_> = std::env::args_os()
        .skip(1)
        .filter(|a| a != "--mcp")
        .map(std::path::PathBuf::from)
        .collect();
    eframe::run_native(
        "Tessera Publisher",
        options,
        Box::new(move |cc| {
            tessera_ui::theme::apply(&cc.egui_ctx);

            let render_state = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("Tessera needs the wgpu backend, which failed to start")?;
            tessera_ui::view::vello_host::install(render_state)?;
            tessera_ui::view::invert_host::install(render_state)?;

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
            tessera_ui::file_ops::open_startup_paths(&mut app, &startup_paths);
            if startup_paths.is_empty() {
                app.ask_what_to_make();
            }
            // And on a first run, offer to say where everything is. Offered
            // rather than started: it waits until the dialog above has been
            // dealt with, because a tour of the interface behind a modal is a
            // tour of something nobody can look at.
            app.offer_tour_on_first_run();

            // Listen for a model. A failure to bind is not a failure to
            // start: the window is for a person first.
            let bridge = bridge_port_file().and_then(|file| {
                let ctx = cc.egui_ctx.clone();
                tessera_bridge::live::Listener::start(file, move || ctx.request_repaint())
                    .inspect_err(|e| eprintln!("the bridge could not listen: {e}"))
                    .ok()
            });

            // The console's model, when the person has set one. The HTTP
            // is this binary's; the turns are the bridge's.
            let console = {
                let ctx = cc.egui_ctx.clone();
                tessera_bridge::console::Driver::new(
                    std::sync::Arc::new(|| Box::new(model_http::Http) as Box<_>),
                    std::sync::Arc::new(move || ctx.request_repaint()),
                )
            };

            Ok(Box::new(NativeApp {
                app,
                bridge,
                console,
                maximize_pending: true,
                fit_after_maximize: true,
            }) as Box<dyn eframe::App>)
        }),
    )
}

/// Where a running window records the port a model may reach it on.
fn bridge_port_file() -> Option<std::path::PathBuf> {
    tessera_ui::prefs::Preferences::directory().map(|d| d.join(tessera_bridge::live::PORT_FILE))
}

/// Native startup policy lives in the binary rather than the headless UI state.
struct NativeApp {
    app: TesseraApp,
    /// The socket a model reaches this window on, when one could be opened.
    bridge: Option<tessera_bridge::live::Listener>,
    /// The console's turns with the person's own model.
    console: tessera_bridge::console::Driver,
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
        // A model's requests, answered on the thread that owns the document.
        if let Some(bridge) = &self.bridge {
            bridge.pump(&mut self.app);
        }
        self.console.pump(&mut self.app);
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
    }

    fn on_exit(&mut self) {
        self.app.on_exit();
    }
}
