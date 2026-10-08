//! Help > About Tessera: what this is, which build, under what licence, and
//! whose work ships inside it.
//!
//! The credits are not decoration. Several of the things Tessera ships ask
//! that their notice travel with them — SCOWL's dictionaries, the ICC
//! profiles, Adobe's Spectrum icons — and a page a person can actually find
//! is where a notice is read.

use egui::{Color32, Ui};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// The build: the version, and the commit it was made from when the build
/// knew it.
pub fn build() -> String {
    let version = env!("CARGO_PKG_VERSION");
    match option_env!("TESSERA_COMMIT").filter(|c| !c.is_empty()) {
        Some(commit) => format!("Version {version} ({commit})"),
        None => format!("Version {version}"),
    }
}

/// Who made what ships inside Tessera, and under what terms.
pub const CREDITS: &[(&str, &str)] = &[
    (
        "Spelling dictionaries",
        "SCOWL and friends, by Kevin Atkinson and others; permissive licence",
    ),
    (
        "Interface icons",
        "Adobe Spectrum 2 workflow icons; Apache License 2.0",
    ),
    (
        "Colour management",
        "Little CMS, by Marti Maria; MIT licence",
    ),
    (
        "Colour profiles",
        "sRGB and the CGATS 21 print profiles, by the International Color Consortium; free to share unaltered",
    ),
    (
        "PostScript and EPS",
        "Ghostscript, by Artifex Software, run as a separate program; GNU AGPL",
    ),
    (
        "Drawing",
        "Vello, kurbo and resvg, by the Linebender community; Apache 2.0 or MIT",
    ),
    (
        "Interface",
        "egui and wgpu, by Emil Ernerfeldt and the gfx-rs community; Apache 2.0 or MIT",
    ),
    (
        "Text layout",
        "Parley, HarfRust and Skrifa, by the Linebender, HarfBuzz and Fontations projects; Apache 2.0 or MIT",
    ),
];

/// Show the box, when it is open.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.about_open {
        return;
    }
    let newer = state.update_check.newer().map(|(v, url)| {
        (
            format!("{}.{}.{}", v.major, v.minor, v.patch),
            url.to_string(),
        )
    });
    let mut close = false;
    let response = egui::Modal::new(egui::Id::new("about-tessera"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 440.0));
            ui.vertical_centered(|ui| {
                logo(ui, 72.0);
                ui.add_space(Theme::space_2());
                ui.heading("Tessera Publisher");
                ui.colored_label(Theme::text_muted(), build());
                ui.colored_label(
                    Theme::text_muted(),
                    "Free software under the GNU General Public License, version 3 or later.",
                );
                if let Some((version, url)) = &newer {
                    ui.add_space(Theme::space_1());
                    ui.hyperlink_to(format!("Version {version} is available"), url);
                }
            });
            ui.add_space(Theme::space_3());
            ui.label(egui::RichText::new("Made with").strong());
            ui.add_space(Theme::space_1());
            egui::Grid::new("about-credits")
                .num_columns(2)
                .spacing([Theme::space_2(), Theme::space_1()])
                .show(ui, |ui| {
                    for (what, who) in CREDITS {
                        ui.colored_label(Theme::text_muted(), *what);
                        ui.add(egui::Label::new(*who).wrap());
                        ui.end_row();
                    }
                });
            ui.add_space(Theme::space_3());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.add(super::primary_button("Close")).clicked();
            });
        });
    if close || response.should_close() {
        state.about_open = false;
    }
}

/// The logotype, `side` points tall, drawn from its SVG at the pixels it
/// covers.
fn logo(ui: &mut Ui, side: f32) {
    const SVG: &str = include_str!("../../../../assets/tessera-publisher-logotype.svg");
    let ppp = ui.ctx().pixels_per_point();
    let pixels = (side * ppp).round().max(1.0) as u32;
    let id = egui::Id::new(("tessera-about-logo", pixels));
    let texture = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(id));
    let texture = match texture {
        Some(t) => t,
        None => {
            let Some(image) = rasterise(SVG, pixels) else {
                return;
            };
            let t = ui
                .ctx()
                .load_texture("about-logo", image, egui::TextureOptions::LINEAR);
            ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
            t
        }
    };
    ui.add(egui::Image::new((texture.id(), egui::vec2(side, side))));
}

/// An SVG drawn into a square of `pixels`.
fn rasterise(svg: &str, pixels: u32) -> Option<egui::ColorImage> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels)?;
    let scale = pixels as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba: Vec<Color32> = pixmap
        .pixels()
        .iter()
        .map(|p| Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha()))
        .collect();
    Some(egui::ColorImage::new(
        [pixels as usize, pixels as usize],
        rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_says_its_version() {
        assert!(build().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn the_logo_draws() {
        let image = rasterise(
            include_str!("../../../../assets/tessera-publisher-logotype.svg"),
            64,
        )
        .expect("the logo parses");
        assert!(image.pixels.iter().any(|p| p.a() > 0), "it has ink");
    }

    #[test]
    fn every_credit_names_its_terms() {
        for (what, who) in CREDITS {
            assert!(who.contains(';'), "{what} says whose and on what terms");
        }
    }
}
