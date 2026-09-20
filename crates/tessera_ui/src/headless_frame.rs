//! One egui frame with no window behind it, for tests.
//!
//! A real frame hands its texture deltas to the renderer, which uploads
//! them. A test has no renderer, and since egui 0.36 a `TexturesDelta`
//! dropped with deltas still in it is a panic in debug builds — the check
//! exists to catch a backend that forgot to upload, and a test that forgot
//! looks the same to it. So every headless frame goes through here, where
//! the deltas are cleared on purpose rather than dropped by accident.

/// Run one frame of `ui` against `ctx` and return its output with the
/// texture deltas already cleared. A test that wants the deltas — the glyph
/// atlas tests do — reads `textures_delta` from the returned output before
/// this clears it, by calling [`egui::Context::run_ui`] itself and clearing
/// afterwards.
pub(crate) fn frame(
    ctx: &egui::Context,
    input: egui::RawInput,
    ui: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, ui);
    output.textures_delta.clear();
    output
}
