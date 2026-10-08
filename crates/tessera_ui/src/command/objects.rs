//! Frames and the selection: drawing, moving, grouping, arranging, deleting, pasting, and object styles.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::AddRectangle(bounds) => add(state, bounds, FrameKind::Rectangle, Look::Outline),

        Command::AddEllipse(bounds) => add(state, bounds, FrameKind::Ellipse, Look::Outline),

        Command::AddPath(bounds, path) => add(state, bounds, FrameKind::Path(path), Look::Outline),

        Command::AddGraphicFrame(bounds) => {
            add(
                state,
                bounds,
                FrameKind::Graphic { placed: None },
                Look::Bare,
            );
        }

        Command::AddTextFrame(bounds) => {
            let story = state
                .active_mut()
                .document_mut()
                .add_story(Story::default());
            // A text frame's own fill is the box behind the glyphs, so it is
            // transparent by default rather than painting a white rectangle
            // over whatever it sits on.
            add(state, bounds, FrameKind::text(story), Look::Bare);
        }

        Command::SetBounds { id, bounds } => {
            let placement = state
                .active()
                .document()
                .frame(id)
                .map_or(Transform::IDENTITY, |f| f.transform);
            retarget(state, id, bounds, placement);
        }

        Command::SetRotation { id, degrees } => {
            let Some(frame) = state.active().document().frame(id) else {
                return;
            };
            let (bounds, was) = (frame.bounds, frame.transform);
            // Normalised into -180..180 so the inspector never shows 3600 and
            // a saved document never accumulates whole turns.
            let wanted = (degrees + 180.0).rem_euclid(360.0) - 180.0;
            // Turned by the difference about where the frame really is, so a
            // scale or a shear already on the frame is preserved rather than
            // being flattened into a bare rotation.
            let turn = Transform::rotate_about(wanted - was.rotation_degrees(), frame.centre());
            retarget(state, id, bounds, was.then(turn));
        }

        Command::TranslateSelection { dx, dy } => {
            for id in state.active().selection.as_slice().to_vec() {
                // Goes through the document so a group carries its children.
                state
                    .active_mut()
                    .document_mut()
                    .translate_frame(id, dx, dy);
            }
            // Dragging an object to another page moves it to that page.
        }

        Command::SetTransforms(entries) => {
            for (id, bounds, placement) in entries {
                if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                    frame.bounds = bounds;
                    frame.transform = placement;
                }
            }
            // Nothing else to do. A frame's page is where it sits, so moving
            // it *is* changing its page — there is no ownership left to update
            // and so nothing for a caller to forget.
        }

        Command::GroupSelection => {
            state.active_mut().group_selection();
        }

        Command::UngroupSelection => {
            let freed: Vec<_> = state
                .active()
                .selection
                .as_slice()
                .to_vec()
                .into_iter()
                .flat_map(|id| state.active_mut().document_mut().ungroup(id))
                .collect();
            // Selecting the freed children is what lets a second ungroup
            // reach a nested group without re-selecting by hand.
            if !freed.is_empty() {
                state.active_mut().selection.replace_all(freed);
            }
        }

        Command::DeleteSelection => {
            for id in state.active().selection.as_slice().to_vec() {
                state.active_mut().document_mut().remove_frame(id);
            }
            state.active_mut().selection.clear();
            state.active_mut().editing = None;
        }

        Command::DuplicateSelection => {
            let roots = state.active().selection.as_slice().to_vec();
            let layer = state.default_layer();
            let copies = state.active_mut().document_mut().copy_frames(
                &roots,
                layer,
                DUPLICATE_OFFSET,
                DUPLICATE_OFFSET,
            );
            // Select the copies, so a second Ctrl+D duplicates them rather
            // than making a second copy of the originals.
            state.active_mut().selection.replace_all(copies);
        }

        Command::StepAndRepeat { copies, dx, dy } => {
            let originals: Vec<FrameId> = state.active().selection.as_slice().to_vec();
            let mut made = Vec::new();

            // **The offset accumulates.** Each copy is `n` steps from the
            // original, not one step from the copy before it: reading the
            // previous copy's position would compound any rounding, and a row of
            // forty would drift visibly by the end.
            for step in 1..=copies {
                let by = step as f64;
                let layer = state.default_layer();
                made.extend(state.active_mut().document_mut().copy_frames(
                    &originals,
                    layer,
                    dx * by,
                    dy * by,
                ));
            }

            // The copies, not the originals — the same rule Duplicate follows,
            // so stepping twice steps what was just made.
            if !made.is_empty() {
                state.active_mut().selection.replace_all(made);
            }
        }

        Command::CopySelection => {
            let source = std::sync::Arc::new(state.active().document().clone());
            let from = Some(state.active);
            let items: Vec<Clipboard> = state
                .active()
                .selection
                .iter()
                .filter(|id| source.frame(*id).is_some())
                .map(|root| Clipboard {
                    source: source.clone(),
                    root,
                    from,
                })
                .collect();
            if !items.is_empty() {
                let count = items.len();
                state.clipboard = items;
                state.status = Some(crate::app::Status::info(match count {
                    1 => "Copied".to_string(),
                    n => format!("Copied {n} objects"),
                }));
            }
        }

        Command::CutSelection => {
            apply(state, Command::CopySelection);
            apply(state, Command::DeleteSelection);
        }

        Command::Paste => {
            const OFFSET: f64 = 12.0;
            let Some(first) = state.clipboard.first() else {
                return;
            };
            let source = first.source.clone();
            let roots: Vec<_> = state.clipboard.iter().map(|item| item.root).collect();
            let layer = state.default_layer();
            let pasted = state
                .active_mut()
                .document_mut()
                .import_frames(&source, &roots, layer, OFFSET, OFFSET, false);
            match pasted {
                Ok(pasted) => state.active_mut().selection.replace_all(pasted),
                Err(message) => state.status = Some(crate::app::Status::error(message)),
            }
        }

        Command::PlaceFromConveyor { at } => {
            let Some(item) = state.conveyor.items.first() else {
                return;
            };
            let Some(corner) = crate::conveyor::top_left(item) else {
                state.conveyor.items.remove(0);
                return;
            };
            let (source, root) = (item.source.clone(), item.root);
            // Linked only back into the document it came from, and only while
            // its original is still there to follow.
            let link_to = (state.conveyor.link && item.from == Some(state.active))
                .then_some(root)
                .filter(|r| state.active().document().frame(*r).is_some());
            let layer = state.default_layer();
            let placed = state.active_mut().document_mut().import_frames(
                &source,
                &[root],
                layer,
                at.x - corner.x,
                at.y - corner.y,
                false,
            );
            match placed {
                Ok(placed) => {
                    if let (Some(original), [copy]) = (link_to, placed.as_slice()) {
                        state
                            .active_mut()
                            .document_mut()
                            .link_content(original, *copy);
                    }
                    state.active_mut().selection.replace_all(placed);
                    if !state.conveyor.keep {
                        state.conveyor.items.remove(0);
                    }
                }
                Err(message) => state.status = Some(crate::app::Status::error(message)),
            }
        }

        Command::UpdateLinkedContent { id } => {
            state.active_mut().document_mut().update_linked_content(id);
        }

        Command::UnlinkContent { id } => {
            state.active_mut().document_mut().unlink_content(id);
        }

        Command::MoveSelectionInZ(how) => {
            // Order matters, and not in the obvious way. Each frame moves
            // relative to the list as it stands, so processing the wrong end
            // first makes the selection leapfrog itself:
            //
            //   [a,b,c], raise {a,b}: a-then-b gives [a,b,c] (no change),
            //                         b-then-a gives [c,a,b] (correct)
            //   [a,b,c], front {a,b}: a-then-b gives [c,a,b] (correct),
            //                         b-then-a gives [c,b,a] (reversed)
            //
            // A one-step move must start from the end it is moving toward; a
            // move-to-the-end must start from the far end.
            let mut ids = state.active().selection.as_slice().to_vec();
            if matches!(how, ZMove::Forward | ZMove::ToBack) {
                ids.reverse();
            }
            for id in ids {
                state.active_mut().document_mut().move_in_z(id, how);
            }
        }

        Command::AddPathLike { bounds, path, from } => {
            let look = state
                .active()
                .document()
                .frame(from)
                .map(|f| (f.fill.clone(), f.stroke.clone(), f.blend));
            add(state, bounds, FrameKind::Path(path), Look::Outline);
            if let Some((fill, stroke, blend)) = look
                && let Some(id) = state.active().selection.single()
                && let Some(made) = state.active_mut().document_mut().frame_mut(id)
            {
                made.fill = fill;
                made.stroke = stroke;
                made.blend = blend;
            }
        }

        Command::SetPath { id, path } => {
            // A rectangle or ellipse edited as a path becomes one.
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id)
                && matches!(
                    frame.kind,
                    FrameKind::Path(_) | FrameKind::Rectangle | FrameKind::Ellipse
                )
            {
                // The box follows the shape. The renderer fits the stored
                // path's box onto the frame's, so a path edited past its box
                // and left there is drawn squeezed back into it.
                let (bounds, path) = tessera_document::path::normalised(&path, frame.bounds);
                frame.bounds = bounds;
                frame.kind = FrameKind::Path(path);
            }
        }

        Command::SetCorners { id, corners } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.corners = corners;
            }
        }

        Command::SetTextWrap { id, wrap } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.wrap = wrap;
            }
            state.active_mut().document_mut().touch();
        }

        Command::AddObjectStyle => {
            let name = state.active().document().unused_object_style_name();
            state
                .active_mut()
                .document_mut()
                .add_object_style(tessera_document::object_style::ObjectStyle::new(name));
        }

        Command::NameObjectStyle { id, name, based_on } => {
            let doc = state.active_mut().document_mut();
            // A style based on itself, however indirectly, is a ring. Refusing
            // it here is better than resolving it to a fixed depth and leaving
            // somebody to wonder why their style stopped inheriting.
            let ring = based_on.is_some_and(|base| doc.object_style_inherits(base, id));
            if let Some(style) = doc.object_styles.get_mut(id) {
                style.name = name;
                if !ring {
                    style.based_on = based_on;
                }
            }
            doc.touch();
        }

        Command::RestyleObjectStyle { id, format } => {
            state
                .active_mut()
                .document_mut()
                .restyle_object_style(id, *format);
        }

        Command::RemoveObjectStyle { id } => {
            state.active_mut().document_mut().remove_object_style(id);
        }

        Command::ApplyObjectStyle { id, style } => {
            state
                .active_mut()
                .document_mut()
                .apply_object_style(id, style);
        }

        Command::DetachObjectStyle { id } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.style = None;
            }
            state.active_mut().document_mut().touch();
        }

        Command::ClearObjectOverrides { id } => {
            state.active_mut().document_mut().clear_object_overrides(id);
        }

        Command::SetObjectsHidden { ids, hidden } => {
            state
                .active_mut()
                .document_mut()
                .set_frames_hidden(&ids, hidden);
            // What cannot be seen cannot stay selected: handles round
            // nothing, and a nudge moving what nobody can see.
            drop_the_untouchable(state);
        }

        Command::SetObjectsLocked { ids, locked } => {
            state
                .active_mut()
                .document_mut()
                .set_frames_locked(&ids, locked);
            drop_the_untouchable(state);
        }

        Command::ArrangeObjects { ids, layer, index } => {
            state
                .active_mut()
                .document_mut()
                .arrange_frames(&ids, layer, index);
        }

        Command::TransformAbout {
            id,
            anchor,
            scale,
            rotate,
            shear,
        } => {
            let Some(frame) = state.active().document().frame(id) else {
                return;
            };
            // The anchor is resolved where the frame really is. `bounds` says
            // only where it is in its own space, and does not move when the
            // frame does — so the anchor point has to travel through the
            // frame's own transform before anything composes onto it.
            let about = frame.transform.apply(anchor.in_rect(frame.bounds));
            let mut result = frame.transform;

            if scale != (1.0, 1.0) {
                result = result.then(Transform::scale_about(scale.0, scale.1, about));
            }
            if rotate != 0.0 {
                result = result.then(Transform::rotate_about(rotate, about));
            }
            if shear != 0.0 {
                result = result.then(Transform::shear_about(shear, about));
            }

            let bounds = frame.bounds;
            retarget(state, id, bounds, result);
        }

        Command::Align { edge, to } => {
            let ids: Vec<_> = state.active().selection.as_slice().to_vec();
            let doc = state.active().document();
            let rects: Vec<_> = ids.iter().filter_map(|id| doc.visual_bounds(*id)).collect();
            if rects.len() != ids.len() || rects.is_empty() {
                return;
            }

            let Some(target) = align_target(state, to, &rects) else {
                return;
            };
            let deltas = crate::align::align_deltas(&rects, target, edge);
            for (id, (dx, dy)) in ids.iter().zip(deltas) {
                state
                    .active_mut()
                    .document_mut()
                    .translate_frame(*id, dx, dy);
            }
        }

        Command::Distribute(axis) => {
            let ids: Vec<_> = state.active().selection.as_slice().to_vec();
            let doc = state.active().document();
            let rects: Vec<_> = ids.iter().filter_map(|id| doc.visual_bounds(*id)).collect();
            if rects.len() != ids.len() {
                return;
            }

            let deltas = crate::align::distribute_deltas(&rects, axis);
            for (id, (dx, dy)) in ids.iter().zip(deltas) {
                state
                    .active_mut()
                    .document_mut()
                    .translate_frame(*id, dx, dy);
            }
        }

        Command::FlipSelection {
            horizontal,
            vertical,
        } => {
            let anchor = state.anchor;
            for id in state.active().selection.as_slice().to_vec() {
                apply(
                    state,
                    Command::TransformAbout {
                        id,
                        anchor,
                        scale: (
                            if horizontal { -1.0 } else { 1.0 },
                            if vertical { -1.0 } else { 1.0 },
                        ),
                        rotate: 0.0,
                        shear: 0.0,
                    },
                );
            }
        }

        Command::RotateSelection90 { clockwise } => {
            let anchor = state.anchor;
            for id in state.active().selection.as_slice().to_vec() {
                apply(
                    state,
                    Command::TransformAbout {
                        id,
                        anchor,
                        scale: (1.0, 1.0),
                        rotate: if clockwise { 90.0 } else { -90.0 },
                        shear: 0.0,
                    },
                );
            }
        }
        _ => unreachable!("not a command for objects"),
    }
}
