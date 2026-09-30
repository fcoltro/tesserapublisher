//! The Tessera interface: theme, tools, commands, panels and the viewport.
//!
//! Structure follows oxiDRAFT's conventions — design tokens in `theme`, icons
//! painted through `egui::Painter`, a single `command` layer, and a `view`
//! module that only draws.

pub mod actions;
pub mod align;
pub mod app;
pub mod background;
pub mod book_ops;
pub mod camera;
pub mod catalogue;
pub mod clock;
pub mod colour_theme;
pub mod command;
pub mod conveyor;
pub mod cursor;
pub mod docking;
pub mod file_ops;
pub mod find;
pub mod gap;
pub mod glyph_index;
#[cfg(test)]
pub(crate) mod headless_frame;
pub mod icons;
pub mod ime;
pub mod keychain;
pub mod keys;
pub mod merge_ops;
pub mod object_order;
pub mod open_document;
pub mod package;
pub mod pen;
pub mod preflight;
pub mod prefs;
pub mod print;
pub mod recovery;
pub mod reflow;
pub mod selection;
pub mod softproof;
pub mod table_ops;
pub mod theme;
pub mod tools;
pub mod transform;
pub mod ui_fonts;
pub mod update;
pub mod view;
pub mod workspace;

pub use app::{Status, TesseraApp};
pub use command::{Command, apply};
pub use tools::Tool;

/// The artwork extensions `Place` will open.
///
/// **One list.** The file dialog's filter and anything that checks a dropped
/// path have to agree, and the way they stop agreeing is by being written out
/// twice. Every one of these decodes through the same call in `tessera_render`
/// and `tessera_pdf`, so a format added here is added to the screen and to the
/// export at once.
pub const PLACEABLE: &[&str] = &[
    "png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp", "gif", "svg", "pdf", "ai", "psd", "psb",
    "eps", "epsf", "epsi",
];

/// The `eframe::App` implementation.
///
/// eframe 0.35 has no `update` method: it is `logic` (which may not paint)
/// plus `ui` (which does nothing else). Tessera adopts that split
/// deliberately rather than putting everything in one place.
impl eframe::App for TesseraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !view::quit::request(self) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));
        // The file name and date variables read these. Cheap when nothing
        // changed, which is nearly every frame.
        let now = clock::now();
        for open in self.documents.values_mut() {
            open.sync_file_facts(now);
        }
        // Rides on a frame that was going to be drawn anyway; asks for none
        // of its own.
        self.autosave_if_due();
        // Likewise: the answer to a version check arrives on whatever frame
        // it arrives on, and taking it costs a `try_recv` on the rest.
        self.settle_update_check();
        // An export running on its own thread: its word taken when it has
        // finished, and the bar kept moving while it has not.
        self.settle_job();
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
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
        if self.persists
            && let Some(dir) = crate::prefs::Preferences::directory()
        {
            self.console
                .save_to(&dir.join(crate::view::console::TRANSCRIPT_FILE));
        }
        if self.quit.confirmed || !self.documents.values().any(|doc| doc.dirty) {
            for open in self.documents.values_mut() {
                open.recovery.discard_copy();
            }
        }
    }
}
