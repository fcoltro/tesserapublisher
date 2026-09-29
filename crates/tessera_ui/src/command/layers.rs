//! Layers.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::AddLayer => {
            let name = state.active().document().unused_layer_name();
            state.active_mut().document_mut().add_layer(name);
        }

        Command::RemoveLayer { id } => {
            state.active_mut().document_mut().remove_layer(id);
            // Whatever was on it is gone, so the selection cannot still name
            // it. `restore` does this for undo; a removal has to do it here.
            state.active_mut().retain_existing_selection();
        }

        Command::RenameLayer { id, name } => {
            if let Some(layer) = state.active_mut().document_mut().layers.get_mut(id) {
                layer.name = name;
            }
            state.active_mut().document_mut().touch();
        }

        Command::SetLayerVisible { id, visible } => {
            if let Some(layer) = state.active_mut().document_mut().layers.get_mut(id) {
                layer.visible = visible;
            }
            state.active_mut().document_mut().touch();
            // A hidden layer's frames cannot be selected, so a selection that
            // was standing on one has to let go — otherwise handles float over
            // nothing and a drag moves what cannot be seen.
            drop_the_untouchable(state);
        }

        Command::SetLayerColour { id, colour } => {
            if let Some(layer) = state.active_mut().document_mut().layers.get_mut(id) {
                layer.colour = colour;
            }
            state.active_mut().document_mut().touch();
        }

        Command::SetLayerLocked { id, locked } => {
            if let Some(layer) = state.active_mut().document_mut().layers.get_mut(id) {
                layer.locked = locked;
            }
            state.active_mut().document_mut().touch();
            drop_the_untouchable(state);
        }

        Command::MoveLayer { from, to } => {
            state.active_mut().document_mut().move_layer(from, to);
        }

        Command::SetActiveLayer(id) => {
            if state.active().document().layers.contains_key(id) {
                state.active_mut().document_mut().set_active_layer(id);
            }
        }

        Command::MoveSelectionToLayer(id) => {
            let frames = state.active().selection.as_slice().to_vec();
            state
                .active_mut()
                .document_mut()
                .move_frames_to_layer(&frames, id);
        }

        Command::SetLayersVisible { layers } => {
            for (id, visible) in layers {
                if let Some(layer) = state.active_mut().document_mut().layers.get_mut(id) {
                    layer.visible = visible;
                }
            }
            state.active_mut().document_mut().touch();
            drop_the_untouchable(state);
        }

        Command::SetLayersLocked { layers } => {
            for (id, locked) in layers {
                if let Some(layer) = state.active_mut().document_mut().layers.get_mut(id) {
                    layer.locked = locked;
                }
            }
            state.active_mut().document_mut().touch();
            drop_the_untouchable(state);
        }
        _ => unreachable!("not a command for layers"),
    }
}
