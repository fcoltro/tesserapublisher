//! The Tessera interface: theme, tools, commands, panels and the viewport.
//!
//! Structure follows oxiDRAFT's conventions — design tokens in `theme`, icons
//! painted through `egui::Painter`, a single `command` layer, and a `view`
//! module that only draws.

pub mod app;
pub mod camera;
pub mod command;
pub mod file_ops;
pub mod icons;
pub mod pen;
pub mod selection;
pub mod theme;
pub mod tools;
pub mod transform;
pub mod view;

pub use app::{Status, TesseraApp};
pub use command::{Command, apply};
pub use tools::Tool;

/// The `eframe::App` implementation.
///
/// eframe 0.35 has no `update` method: it is `logic` (which may not paint)
/// plus `ui` (which does nothing else). Tessera adopts that split
/// deliberately rather than putting everything in one place.
impl eframe::App for TesseraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        view::show(ui, frame, self);
    }
}
