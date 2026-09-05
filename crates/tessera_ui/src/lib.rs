//! The Tessera interface: theme, tools, commands, panels and the viewport.
//!
//! Structure follows oxiDRAFT's conventions — design tokens in `theme`, icons
//! painted through `egui::Painter`, a single `command` layer, and a `view`
//! module that only draws.

pub mod actions;
pub mod align;
pub mod app;
pub mod camera;
pub mod command;
pub mod cursor;
pub mod file_ops;
pub mod icons;
pub mod open_document;
pub mod pen;
pub mod prefs;
pub mod recovery;
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
        // Rides on a frame that was going to be drawn anyway; asks for none
        // of its own.
        self.autosave_if_due();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        view::show(ui, frame, self);
    }

    /// A clean quit means the work is either saved or deliberately abandoned,
    /// so the recovery copy has nothing left to recover.
    ///
    /// Without this the file survives every normal quit and the next launch
    /// offers to restore work the user may have thrown away on purpose —
    /// which teaches people to dismiss the prompt, and a prompt that is always
    /// dismissed protects nobody on the day it matters.
    ///
    /// Only a crash should leave it behind. That is the whole point of it.
    fn on_exit(&mut self) {
        recovery::Recovery::discard();
    }
}
