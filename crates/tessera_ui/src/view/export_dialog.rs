//! Choosing what an export produces, and remembering the choice.
//!
//! **A preset is not a convenience here, it is the point.** A studio sends the
//! same three kinds of file for years — a screen proof, a job for the coated
//! press, a job for the newsprint one — and re-choosing a standard, a set of
//! marks and a resolution threshold each time is how a file goes out as an
//! RGB proof to a printer expecting X-1a. Naming the combination once and
//! picking it by name is the whole defence against that.
//!
//! The presets travel with the person, not with the document: they are how *this
//! studio* sends work, and opening somebody else's layout must not change it.

use egui::Ui;

use tessera_pdf::{ExportOptions, Marks, Standard};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// A named set of export choices.
///
/// The intent is deliberately **not** part of a preset. Which press a job is for
/// belongs to the document — it travels in the file and is what the printer
/// needs — and a preset that carried one would silently re-target somebody's job
/// to the press they last used.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    pub standard: Standard,
    pub crop: bool,
    pub bleed: bool,
    pub registration: bool,
    pub colour_bar: bool,
    pub offset: f64,
}

impl Preset {
    /// The marks this preset asks for.
    pub fn marks(&self) -> Marks {
        Marks {
            crop: self.crop,
            bleed: self.bleed,
            registration: self.registration,
            colour_bar: self.colour_bar,
            offset: self.offset,
        }
    }

    /// The ones a studio actually uses, so the list is never empty.
    ///
    /// An empty preset list teaches somebody that presets are a thing they have
    /// to build before they can be useful, and they never do.
    pub fn usual() -> Vec<Preset> {
        vec![
            Preset {
                name: "Screen proof".to_string(),
                standard: Standard::Plain,
                crop: false,
                bleed: false,
                registration: false,
                colour_bar: false,
                offset: Marks::default().offset,
            },
            Preset {
                name: "Press, PDF/X-4".to_string(),
                standard: Standard::X4,
                crop: true,
                bleed: true,
                registration: true,
                colour_bar: true,
                offset: Marks::default().offset,
            },
            Preset {
                name: "Press, PDF/X-1a".to_string(),
                standard: Standard::X1a,
                crop: true,
                bleed: true,
                registration: true,
                colour_bar: true,
                offset: Marks::default().offset,
            },
        ]
    }
}

/// The dialog's own state.
#[derive(Default)]
pub struct ExportWindow {
    pub open: bool,
    /// The choices as they stand, which start from a preset and may be nudged.
    pub standard: Standard,
    pub marks: Marks,
    /// Which preset was last picked, so the list can show it selected.
    pub chosen: Option<String>,
}

impl ExportWindow {
    /// Take a preset's choices as the current ones.
    pub fn adopt(&mut self, preset: &Preset) {
        self.standard = preset.standard;
        self.marks = preset.marks();
        self.chosen = Some(preset.name.clone());
    }

    /// The options an export would run with.
    ///
    /// The intent comes from the document rather than from here, which is what
    /// keeps "which press" in the file where a printer can read it.
    pub fn options(&self, state: &TesseraApp) -> ExportOptions {
        ExportOptions {
            standard: self.standard,
            marks: self.marks,
            intent: state.active().document().output_intent.clone(),
        }
    }
}

/// The dialog, if it is open.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.export.open {
        return;
    }

    let mut open = true;
    egui::Window::new("Export PDF")
        .open(&mut open)
        .resizable(false)
        .default_width(400.0)
        .show(ctx, |ui| body(ui, state));

    if !open {
        state.export.open = false;
    }
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    // The presets first, because picking one is what most exports are.
    ui.label("Preset");
    let presets = state.prefs.export_presets.clone();
    for preset in &presets {
        let chosen = state.export.chosen.as_deref() == Some(preset.name.as_str());
        if ui.selectable_label(chosen, &preset.name).clicked() {
            state.export.adopt(preset);
        }
    }

    ui.separator();

    // Then what the preset chose, adjustable. Shown rather than hidden behind an
    // "advanced" disclosure: somebody about to send a job to a press wants to
    // see what they are sending, and a summary they have to open is a summary
    // they do not read.
    let mut standard = state.export.standard;
    crate::view::panels::field(ui, "Standard", |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("export-standard")
                .selected_text(standard.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for choice in Standard::ALL {
                        ui.selectable_value(&mut standard, choice, choice.label());
                    }
                })
                .response,
            "Standard",
            egui::WidgetType::ComboBox,
            None,
        );
    });
    state.export.standard = standard;

    ui.add_space(Theme::SPACE_2);
    ui.label("Marks");
    ui.checkbox(&mut state.export.marks.crop, "Crop marks");
    ui.checkbox(&mut state.export.marks.bleed, "Bleed marks");
    ui.checkbox(&mut state.export.marks.registration, "Registration marks");
    ui.checkbox(&mut state.export.marks.colour_bar, "Colour bar");

    if state.export.marks.any() {
        let unit = state.prefs.unit;
        crate::view::panels::field(ui, "Offset", |ui| {
            crate::view::panels::measure_bare(ui, &mut state.export.marks.offset, unit)
        });
    }

    ui.separator();

    // What this export will actually be, in a sentence. **The most important
    // thing in the window**: a person choosing "PDF/X-4" wants to know it is
    // going to a coated press in CMYK, and finding out from the file afterwards
    // is finding out too late.
    summary(ui, state);

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Export...").clicked() {
            state.export.open = false;
            crate::file_ops::export_pdf(state);
        }
        if ui.button("Cancel").clicked() {
            state.export.open = false;
        }
    });
}

/// What this export will be, and why it might be refused.
fn summary(ui: &mut Ui, state: &mut TesseraApp) {
    let options = state.export.options(state);
    let resolved = state.resolve_active().clone();
    let transparency = resolved
        .items
        .iter()
        .any(|item| !item.blend.is_plain() || item.shadow.is_some());

    // Artwork that will actually be embedded. An empty picture box puts no RGB
    // in the file, so refusing an X-1a export over one would be refusing over
    // something that is not there.
    let artwork = resolved.items.iter().any(|item| {
        matches!(
            &item.kind,
            tessera_layout::resolve::ResolvedKind::Graphic {
                source: Some(_),
                ..
            }
        )
    });

    let refusals = options.refusals(transparency, artwork);
    if !refusals.is_empty() {
        for reason in &refusals {
            ui.colored_label(Theme::error(), reason);
        }
        return;
    }

    match &options.intent {
        Some(intent) => {
            let ink = tessera_pdf::Ink::for_intent(Some(intent));
            let space = if ink.is_cmyk() { "CMYK" } else { "RGB" };
            ui.colored_label(
                Theme::text_muted(),
                format!("{space} for {}", intent.description),
            );
        }
        None => {
            ui.colored_label(
                Theme::text_muted(),
                "RGB. No press is named, so nothing is converted.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preset_list_is_never_empty() {
        // An empty list teaches somebody that presets are a thing they have to
        // build before they are useful, and they never do.
        assert!(!Preset::usual().is_empty());
        assert!(Preset::usual().iter().any(|p| p.standard == Standard::X4));
    }

    #[test]
    fn a_preset_does_not_carry_which_press_a_job_is_for() {
        // That belongs to the document: it travels in the file and is what the
        // printer needs. A preset carrying one would silently re-target
        // somebody's job to the press they last used.
        let mut window = ExportWindow::default();
        let mut state = TesseraApp::headless();
        window.adopt(&Preset::usual()[1]);

        assert_eq!(window.standard, Standard::X4);
        assert!(
            window.options(&state).intent.is_none(),
            "a preset supplied a press"
        );

        let profile = tessera_color::managed::OutputProfile::screen().expect("a profile");
        state.active_mut().document_mut().output_intent =
            Some(tessera_document::intent::OutputIntent {
                description: profile.description().to_string(),
                profile: profile.bytes().to_vec(),
                rendering: tessera_document::intent::Rendering::default(),
            });
        assert!(
            window.options(&state).intent.is_some(),
            "the document's press did not reach the export"
        );
    }

    #[test]
    fn adopting_a_preset_takes_its_marks_as_well_as_its_standard() {
        let mut window = ExportWindow::default();
        window.adopt(&Preset::usual()[1]);
        assert!(window.marks.crop);
        assert!(window.marks.colour_bar);

        window.adopt(&Preset::usual()[0]);
        assert!(!window.marks.crop, "a screen proof asked for crop marks");
    }

    #[test]
    fn the_screen_proof_claims_no_standard() {
        let proof = &Preset::usual()[0];
        assert_eq!(proof.standard, Standard::Plain);
        assert!(!proof.marks().any());
    }

    #[test]
    fn a_preset_round_trips_through_json() {
        // They live in the preferences, so they are written and read like every
        // other preference.
        let preset = &Preset::usual()[2];
        let text = serde_json::to_string(preset).expect("write");
        let back: Preset = serde_json::from_str(&text).expect("read");
        assert_eq!(&back, preset);
    }

    #[test]
    fn the_dialog_starts_shut() {
        assert!(!TesseraApp::headless().export.open);
    }
}
