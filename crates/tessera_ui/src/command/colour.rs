//! Colour: fills, strokes, swatches, the press, blending and shadows.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::RepointSwatch { from, to } => {
            state.active_mut().document_mut().repoint_swatch(&from, &to);
        }

        Command::SetFill { id, paint } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.fill = paint;
            }
        }

        Command::ApplyAppearance {
            id,
            format,
            corners,
            text,
        } => {
            let doc = state.active_mut().document_mut();
            doc.write_object_format(id, &format);
            if let Some(corners) = corners
                && let Some(frame) = doc.frame_mut(id)
            {
                frame.corners = corners;
            }
            if let Some(text) = text
                && let Some(FrameKind::Text { story, .. }) = doc.frame(id).map(|f| &f.kind)
            {
                let story = *story;
                let len = doc.story(story).map_or(0, |s| s.text.len());
                if let Some(s) = doc.story_mut(story) {
                    s.apply_character_format(0..len, &text);
                }
            }
            state.active_mut().document_mut().touch();
        }

        Command::SetBlending { id, blend } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.blend = blend;
            }
            state.active_mut().document_mut().touch();
        }

        Command::SetOverprint { id, overprint } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.overprint = overprint;
            }
            state.active_mut().document_mut().touch();
        }

        Command::SetShadow { id, shadow } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.shadow = shadow;
            }
            state.active_mut().document_mut().touch();
        }

        Command::SetFeather { id, feather } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.feather = feather;
            }
            state.active_mut().document_mut().touch();
        }

        Command::SetOutputIntent(intent) => {
            state.active_mut().document_mut().output_intent = intent.map(|boxed| *boxed);
            state.active_mut().document_mut().touch();
        }

        Command::SetSwatch(swatch) => {
            state.active_mut().document_mut().set_swatch(swatch);
        }

        Command::EditSwatch { old, swatch } => {
            state.active_mut().document_mut().edit_swatch(&old, swatch);
        }

        Command::RemoveSwatch { name } => {
            state.active_mut().document_mut().remove_swatch(&name);
        }

        Command::ReplaceSwatch { name, with } => {
            state
                .active_mut()
                .document_mut()
                .replace_swatch(&name, with.as_deref());
        }

        Command::MoveSwatch { name, before } => {
            state
                .active_mut()
                .document_mut()
                .move_swatch(&name, before.as_deref());
        }

        Command::SortSwatches => {
            state.active_mut().document_mut().sort_swatches();
        }

        Command::NameUnnamedColours => {
            use crate::view::swatch_editor::{BLACK_INK, PAPER_INK};
            state
                .active_mut()
                .document_mut()
                .name_unnamed_colours(&[PAPER_INK, BLACK_INK]);
        }

        Command::AddSwatches(swatches) => {
            state.active_mut().document_mut().add_swatches(swatches);
        }

        Command::SetStroke { id, stroke } => {
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.stroke = stroke;
            }
        }

        Command::SwapFillAndStroke(id) => {
            let Some(frame) = state.active().document().frame(id).cloned() else {
                return;
            };
            // A stroke carries one colour, so a gradient fill swapped onto a
            // stroke becomes one colour from the ramp. Gradient strokes are not
            // modelled, and quietly refusing the swap would be worse than doing
            // the part of it that can be done.
            let fill = frame.fill.representative();
            let (new_fill, new_stroke) = match frame.stroke {
                Some(mut stroke) => {
                    let was = stroke.color.clone();
                    stroke.color = fill;
                    (Paint::Solid(was), Some(stroke))
                }
                // With no stroke to swap with, the fill becomes one rather
                // than being discarded — a swap that silently deleted a
                // colour would be worse than one that had no effect.
                None => (
                    Paint::Solid(Color::BLACK_INK),
                    Some(tessera_document::nodes::Stroke::new(fill, 1.0)),
                ),
            };
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.fill = new_fill;
                f.stroke = new_stroke;
            }
        }

        Command::DefaultFillAndStroke(id) => {
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.fill = NO_FILL;
                f.stroke = Some(tessera_document::nodes::Stroke::new(Color::BLACK_INK, 1.0));
            }
        }

        Command::ClearFill(id) => {
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                // "No fill" is the colour the object had, at no alpha, so that
                // turning the fill back on gets the colour back rather than
                // black. A gradient keeps one colour from its ramp: there is no
                // transparent gradient to hold, and the ramp is what the person
                // would have to rebuild either way.
                let [r, g, b, _] = f.fill.representative().to_rgb_f32();
                f.fill = Paint::Solid(Color::Rgb { r, g, b, a: 0.0 });
            }
        }
        _ => unreachable!("not a command for colour"),
    }
}
