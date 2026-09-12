//! Bundled interface fonts, independent of document fonts and OS installations.

use egui::epaint::text::{FontTweak, HintingTarget, SmoothHinting, VariationCoords};
use egui::{Context, FontData, FontDefinitions, FontFamily};

const NOTO_SANS: &[u8] = include_bytes!("../../../assets/fonts/NotoSansVariable.ttf");
/// The user's preferred light weight for 13-point interface labels.
const BODY_WEIGHT: f32 = 300.0;
const HEADING_WEIGHT: f32 = 600.0;
pub const HEADING_FAMILY: &str = "Tessera UI Semibold";

fn data(weight: f32) -> FontData {
    FontData::from_static(NOTO_SANS).tweak(FontTweak {
        coords: VariationCoords::new([(b"wght", weight), (b"wdth", 100.0)]),
        // Light hinting aligns horizontal strokes vertically at small sizes
        // while preserving fractional horizontal spacing and outline coverage.
        hinting: Some(true),
        hinting_target: HintingTarget::Smooth(SmoothHinting {
            light: true,
            symmetric_rendering: true,
            preserve_linear_metrics: true,
        }),
        // Four fractional x positions make kerning and diagonal strokes
        // smoother. This is grayscale coverage AA, not LCD colour fringing.
        subpixel_binning: Some(true),
        ..Default::default()
    })
}

fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("Noto Sans UI".into(), data(BODY_WEIGHT).into());
    fonts
        .font_data
        .insert(HEADING_FAMILY.into(), data(HEADING_WEIGHT).into());
    let body = fonts.families.entry(FontFamily::Proportional).or_default();
    body.insert(0, "Noto Sans UI".into());
    let mut heading = body.clone();
    heading.insert(0, HEADING_FAMILY.into());
    fonts
        .families
        .insert(FontFamily::Name(HEADING_FAMILY.into()), heading);
    fonts
}

/// Install once at startup; theme changes must not rebuild the glyph atlas.
pub fn install(ctx: &Context) {
    let installed = egui::Id::new("tessera-ui-fonts-installed");
    if ctx.data_mut(|data| data.get_temp::<bool>(installed).unwrap_or(false)) {
        return;
    }
    ctx.set_fonts(definitions());
    ctx.data_mut(|data| data.insert_temp(installed, true));
    ctx.all_styles_mut(|style| {
        // Let fallback faces use their default smooth hinting as well.
        style.visuals.text_options.font_hinting = true;
        style.visuals.text_options.subpixel_binning = true;
    });
    ctx.tessellation_options_mut(|options| {
        options.feathering = true;
        options.feathering_size_in_pixels = 1.0;
        // Glyphs already encode fractional offsets in the atlas. Align the
        // galley origin to physical pixels to avoid filtering that coverage a
        // second time. This does not round the shaper's inter-glyph advances.
        options.round_text_to_pixels = true;
        options.round_line_segments_to_pixels = true;
        options.round_rects_to_pixels = true;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_font_supports_our_weight_and_width_settings() {
        let axes = data(400.0).variation_axes();
        for tag in [b"wght", b"wdth"] {
            assert!(axes.iter().any(|axis| axis.tag.as_ref() == tag));
        }
    }

    #[test]
    fn bundled_fonts_render_accented_ui_text_at_common_display_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = Context::default();
            install(&ctx);
            ctx.set_pixels_per_point(scale);
            let output = ctx.run_ui(Default::default(), |ui| {
                ui.label(
                    egui::RichText::new("Properties · São Paulo · Édition · 12.70 mm").size(13.0),
                );
                ui.label(
                    egui::RichText::new("Document setup")
                        .family(FontFamily::Name(HEADING_FAMILY.into())),
                );
            });
            assert!(!output.textures_delta.set.is_empty());
            assert!(
                output.textures_delta.set.iter().any(|(_, delta)| {
                    let egui::ImageData::Color(image) = &delta.image;
                    image.pixels.iter().any(|pixel| {
                        let alpha = pixel.a();
                        alpha > 0 && alpha < 255
                    })
                }),
                "glyph atlas must contain partial coverage at scale {scale}"
            );
            assert!(
                !ctx.tessellate(output.shapes, output.pixels_per_point)
                    .is_empty()
            );
        }
    }

    #[test]
    fn fractional_widget_origins_do_not_resample_glyph_textures() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = Context::default();
            install(&ctx);
            ctx.set_pixels_per_point(scale);
            let _ = ctx.run_ui(Default::default(), |_| {});
            let galley = ctx.fonts_mut(|fonts| {
                fonts.layout_no_wrap(
                    "Properties 12.70 mm".into(),
                    egui::FontId::proportional(13.0),
                    egui::Color32::WHITE,
                )
            });
            let positions = |origin| {
                ctx.tessellate(
                    vec![egui::epaint::ClippedShape {
                        clip_rect: egui::Rect::EVERYTHING,
                        shape: egui::Shape::galley(origin, galley.clone(), egui::Color32::WHITE),
                    }],
                    scale,
                )
                .into_iter()
                .flat_map(|clipped| match clipped.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => {
                        mesh.vertices.into_iter().map(|v| v.pos).collect::<Vec<_>>()
                    }
                    egui::epaint::Primitive::Callback(_) => unreachable!(),
                })
                .collect::<Vec<_>>()
            };
            let aligned = positions(egui::Pos2::ZERO);
            assert!(!aligned.is_empty());
            assert_eq!(aligned, positions(egui::pos2(0.2 / scale, 0.2 / scale)));
        }
    }

    #[test]
    fn text_rendering_keeps_fractional_antialiasing_enabled() {
        let ctx = Context::default();
        install(&ctx);
        let style = ctx.style_of(ctx.theme());
        assert!(style.visuals.text_options.font_hinting);
        assert!(style.visuals.text_options.subpixel_binning);
        assert!(ctx.tessellation_options(|options| options.round_text_to_pixels));
    }
}
