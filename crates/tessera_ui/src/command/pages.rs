//! Pages, spreads, parent pages, the document's setup, and guides.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::AddPage => {
            state.active_mut().document_mut().add_page();
        }

        Command::RemovePage { id } => {
            state.active_mut().document_mut().remove_page(id);
        }

        Command::DuplicatePage { id } => {
            state.active_mut().document_mut().duplicate_page(id);
        }

        Command::MoveSpread { from, to } => {
            state.active_mut().document_mut().move_spread(from, to);
        }

        Command::MovePage { id, to } => {
            state.active_mut().document_mut().move_page(id, to);
        }

        Command::InsertPages {
            after,
            count,
            parent,
        } => {
            state
                .active_mut()
                .document_mut()
                .insert_pages(after, count, parent);
        }

        Command::InsertPage { after } => {
            state.active_mut().document_mut().insert_page_after(after);
        }

        Command::DuplicatePages { ids } => {
            state.active_mut().document_mut().duplicate_pages(&ids);
        }

        Command::RemovePages { ids } => {
            state.active_mut().document_mut().remove_pages(&ids);
            // What stood on them has gone with them; a selection still
            // holding it would draw handles round nothing.
            state.active_mut().retain_existing_selection();
        }

        Command::MovePages { ids, to } => {
            state.active_mut().document_mut().move_pages(&ids, to);
        }

        Command::ApplyMasterToPages { pages, master } => {
            state
                .active_mut()
                .document_mut()
                .apply_master_to(&pages, master);
        }

        Command::AddMaster => {
            let name = state.active().document().unused_master_name();
            state.active_mut().document_mut().add_master(name);
        }

        Command::RemoveMaster { id } => {
            state.active_mut().document_mut().remove_master(id);
            // Its pages and their frames are gone; a selection still holding
            // one would draw handles round nothing.
            state.active_mut().retain_existing_selection();
        }

        Command::RenameMaster { id, name } => {
            if let Some(master) = state.active_mut().document_mut().masters.get_mut(id) {
                master.name = name;
            }
            state.active_mut().document_mut().touch();
        }

        Command::ApplyMaster { page, master } => {
            state.active_mut().document_mut().apply_master(page, master);
        }

        Command::ApplyMasterToAll { master } => {
            // One command for the whole document, so applying a master to
            // twenty pages is one undo entry rather than twenty.
            let pages: Vec<PageId> = state.active().document().page_ids().collect();
            for page in pages {
                state.active_mut().document_mut().apply_master(page, master);
            }
        }

        Command::OverrideMasterItem { page, item } => {
            if let Some(local) = state
                .active_mut()
                .document_mut()
                .override_master_item(page, item)
            {
                // Selected, because overriding an item is what you do in order
                // to change it.
                state.active_mut().selection.set(local);
            }
        }

        Command::RemoveOverrides { page } => {
            state.active_mut().document_mut().remove_overrides(page);
            state.active_mut().retain_existing_selection();
        }

        Command::SetDocumentSetup(setup) => {
            state.active_mut().document_mut().set_setup(setup);
        }

        Command::SetPageSize { width, height } => {
            // Every page, in one command so it is one undo entry.
            let follow = state.prefs.objects_follow_page_edges;
            state
                .active_mut()
                .document_mut()
                .resize_every_page(width, height, follow);
        }

        Command::SetPageSizeOf {
            page,
            width,
            height,
        } => {
            let follow = state.prefs.objects_follow_page_edges;
            state
                .active_mut()
                .document_mut()
                .resize_page(page, width, height, follow);
        }

        Command::AddGuide { spread, guide } => {
            state.active_mut().document_mut().add_guide(spread, guide);
        }

        Command::MoveGuide {
            spread,
            index,
            position,
        } => {
            let doc = state.active_mut().document_mut();
            if let Some(s) = doc.spreads.get_mut(spread)
                && let Some(guide) = s.guides.get_mut(index)
            {
                guide.position = position;
                doc.touch();
            }
        }

        Command::RemoveGuide { spread, index } => {
            state
                .active_mut()
                .document_mut()
                .remove_guide(spread, index);
        }
        _ => unreachable!("not a command for pages"),
    }
}
