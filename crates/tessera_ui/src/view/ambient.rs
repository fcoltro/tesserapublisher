//! The interface's own background, and what the glass is frosting.
//!
//! ## The correction this file is
//!
//! The first attempt had panels blurring the *document* behind them. That is a
//! different effect with a different name, and it was wrong twice over: it is
//! not what glassmorphism looks like, and in a tool where colour is judged it
//! means reading a swatch against a page that moves.
//!
//! What the reference actually shows is a frosted card over a **decorative
//! background belonging to the interface**. So the document goes back to being
//! opaque and beside the chrome, and the chrome floats over a soft coloured
//! ground that Tessera draws itself.
//!
//! ## The blur is generated, not filtered
//!
//! Because the background is ours, there is nothing to sample and blur. It is a
//! handful of radial falloffs, so a blurrier version of it is *the same function
//! evaluated more coarsely* — drawn into a smaller image and stretched back up
//! by the filter egui already samples with.
//!
//! That is the whole implementation: two small `ColorImage`s from one function,
//! one about a hundred pixels across for the open background and one about a
//! dozen for behind the glass. Both are built on the CPU in well under a
//! millisecond, uploaded once, and rebuilt only when the window changes shape or
//! the theme changes. No shader, no second render pass, and nothing that can
//! behave differently on somebody else's GPU.
//!
//! ## Why it is restrained
//!
//! The reference is a poster and can be as loud as it likes. This sits behind a
//! tool somebody uses for eight hours to judge colour, so the ground is built
//! from the theme's own accent and one complement at low saturation. It should
//! read as depth, not as decoration competing with the page.

use egui::{Color32, ColorImage};

use crate::theme::Theme;

/// How wide the sharp background is generated.
///
/// A hundred-odd pixels stretched across a window is already soft, which is
/// exactly what is wanted: the ground is meant to be atmospheric, and a crisp
/// one would compete with the document.
const SHARP: usize = 128;

/// How wide the version behind glass is generated.
///
/// This is the blur. Stretching a dozen pixels across a panel is a very wide
/// box blur, and because the source is a smooth function rather than a
/// photograph there is nothing sharp for it to lose.
const FROSTED_LEAST: usize = 6;
const FROSTED_MOST: usize = 40;

/// One soft light in the background.
///
/// Position and radius are fractions of the window, so the composition survives
/// a resize instead of drifting into a corner.
struct Light {
    at: (f32, f32),
    radius: f32,
    colour: Color32,
    strength: f32,
}

/// The lights on the ground behind the chrome. **There are none.**
///
/// There were three — two accent-family and one warm — and together they made
/// a coloured wash that read as decoration in a window whose whole job is to
/// let somebody judge colour on a page. An interface that puts a violet
/// gradient beside a proof is making a claim about the proof.
///
/// Kept as a function returning nothing rather than deleted, because the
/// machinery around it is sound and correct: this is the one place that
/// decides, and putting a light back is putting a `Light` in this list.
fn lights() -> [Light; 0] {
    []
}

/// Build the background at `width` pixels across, in the window's proportions.
///
/// A pure function of size and palette, which is what makes it testable: there
/// is no GPU here, no context, and no frame.
pub fn image(width: usize, aspect: f32) -> ColorImage {
    let width = width.max(2);
    let height = ((width as f32 / aspect.max(0.05)).round() as usize).clamp(2, 4096);
    let ground = Theme::panel_bg_solid();
    let lights = lights();

    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            // Sampled at pixel centres. Sampling at corners puts the first
            // light half a pixel off and, at this resolution, half a pixel is a
            // visible slide.
            let u = (x as f32 + 0.5) / width as f32;
            let v = (y as f32 + 0.5) / height as f32;

            let (mut r, mut g, mut b) = (ground.r() as f32, ground.g() as f32, ground.b() as f32);

            for light in &lights {
                // Distance in *window* proportions, not pixel ones, so a light
                // stays round in a wide window instead of becoming an ellipse.
                let dx = (u - light.at.0) * aspect.max(0.05);
                let dy = v - light.at.1;
                let d = (dx * dx + dy * dy).sqrt() / light.radius.max(0.01);

                // Smoothstep falloff. A linear one leaves a visible edge at the
                // radius, and an inverse-square never quite ends, so the
                // background never settles to the ground colour.
                let t = (1.0 - d).clamp(0.0, 1.0);
                let fall = t * t * (3.0 - 2.0 * t) * light.strength;

                r += (light.colour.r() as f32 - r) * fall;
                g += (light.colour.g() as f32 - g) * fall;
                b += (light.colour.b() as f32 - b) * fall;
            }

            pixels.push(Color32::from_rgb(r as u8, g as u8, b as u8));
        }
    }

    ColorImage {
        size: [width, height],
        pixels,
        source_size: egui::vec2(width as f32, height as f32),
    }
}

/// How coarsely the frosted version is generated, from the blur preference.
///
/// Inverted: a *stronger* blur means a *smaller* image. The preference reads as
/// "how blurred", which is what somebody adjusting it is thinking about, and the
/// inversion happens here once rather than in their head every time.
pub fn frosted_width(blur: u32) -> usize {
    let blur = blur.clamp(crate::prefs::BLUR_LEAST, crate::prefs::BLUR_MOST) as usize;
    let span = crate::prefs::BLUR_MOST as usize - crate::prefs::BLUR_LEAST as usize;
    let from_least = blur - crate::prefs::BLUR_LEAST as usize;
    // Least blur gives the widest image.
    FROSTED_MOST - (FROSTED_MOST - FROSTED_LEAST) * from_least / span.max(1)
}

/// The two textures, kept between frames.
///
/// Rebuilt when the window changes shape, when the theme changes, or when the
/// blur preference moves — and on no other frame. Generating them costs well
/// under a millisecond, and doing that sixty times a second for a picture that
/// has not changed would still be a millisecond a second thrown away.
#[derive(Default)]
pub struct Ambient {
    sharp: Option<egui::TextureHandle>,
    frosted: Option<egui::TextureHandle>,
    /// What the held textures were built for.
    built_for: Option<(u32, u32, crate::prefs::ThemeChoice)>,
}

impl Ambient {
    /// The two textures, built if anything they depend on has changed.
    ///
    /// The aspect is quantised to whole tenths before it is compared, because a
    /// window being dragged changes it continuously and rebuilding on every
    /// pixel of a drag is the one way this could become expensive.
    pub fn textures(
        &mut self,
        ctx: &egui::Context,
        size: egui::Vec2,
        prefs: &crate::prefs::Preferences,
    ) -> Option<(egui::TextureId, egui::TextureId)> {
        if size.x <= 0.0 || size.y <= 0.0 {
            return None;
        }
        let aspect = size.x / size.y;
        let key = (
            (aspect * 10.0).round() as u32,
            prefs.blur_divisor(),
            prefs.theme,
        );

        if self.built_for != Some(key) || self.sharp.is_none() {
            self.sharp = Some(ctx.load_texture(
                "tessera-ambient",
                image(SHARP, aspect),
                egui::TextureOptions::LINEAR,
            ));
            self.frosted = Some(ctx.load_texture(
                "tessera-ambient-frosted",
                image(frosted_width(prefs.blur_divisor()), aspect),
                egui::TextureOptions::LINEAR,
            ));
            self.built_for = Some(key);
        }

        Some((self.sharp.as_ref()?.id(), self.frosted.as_ref()?.id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_background_is_the_windows_shape() {
        let wide = image(64, 2.0);
        assert_eq!(wide.size, [64, 32]);
        let tall = image(64, 0.5);
        assert_eq!(tall.size, [64, 128]);
    }

    #[test]
    fn a_degenerate_size_does_not_divide_by_zero() {
        // A window reported as having no height happens for a frame during a
        // restore, and a background is not worth panicking over.
        let at_zero = image(32, 0.0);
        assert!(at_zero.size[1] >= 2);
        assert_eq!(at_zero.pixels.len(), at_zero.size[0] * at_zero.size[1]);
    }

    #[test]
    fn the_ground_is_flat() {
        // It used to be lit, and the lights are gone. A coloured wash behind the
        // chrome is decoration in a window whose whole job is letting somebody
        // judge colour on a page — a violet gradient beside a proof is a claim
        // about the proof.
        //
        // Asserted rather than left to `lights()` being empty, because the
        // failure this guards is somebody adding "just one" light back.
        let made = image(48, 1.5);
        let first = made.pixels[0];
        assert!(
            made.pixels.iter().all(|p| *p == first),
            "something is lighting the ground again"
        );
    }

    #[test]
    fn a_stronger_blur_is_a_smaller_picture() {
        // The inversion, which is the whole of the blur. It is done once here so
        // that nobody adjusting the preference has to do it in their head.
        let soft = frosted_width(crate::prefs::BLUR_MOST);
        let sharp = frosted_width(crate::prefs::BLUR_LEAST);
        assert!(
            soft < sharp,
            "a stronger blur produced a bigger picture: {soft} against {sharp}"
        );
        assert!(soft >= FROSTED_LEAST);
        assert!(sharp <= FROSTED_MOST);
    }

    #[test]
    fn every_blur_setting_gives_a_picture_worth_stretching() {
        // A one-pixel background is a flat colour, and a huge one is not a blur.
        for blur in crate::prefs::BLUR_LEAST..=crate::prefs::BLUR_MOST {
            let width = frosted_width(blur);
            assert!(
                (FROSTED_LEAST..=FROSTED_MOST).contains(&width),
                "blur {blur} gave {width}"
            );
        }
    }

    #[test]
    fn the_light_theme_gets_a_light_ground() {
        // Built from the palette in force, so the light theme does not inherit a
        // ground designed for the dark one.
        crate::theme::use_palette(crate::prefs::ThemeChoice::Dark);
        let dark = image(16, 1.0).pixels[0];
        crate::theme::use_palette(crate::prefs::ThemeChoice::Light);
        let light = image(16, 1.0).pixels[0];
        crate::theme::use_palette(crate::prefs::ThemeChoice::Dark);

        assert!(
            light.r() > dark.r(),
            "the light ground is not lighter: {light:?} against {dark:?}"
        );
    }
}
