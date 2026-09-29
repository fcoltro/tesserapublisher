//! Placed artwork and its links.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::PlaceArtwork { id, path, fit } => {
            let Some(link) = measure_link(state, &path) else {
                return;
            };
            let link = state.active_mut().document_mut().add_link(link);
            state.active_mut().document_mut().place(id, link, fit);
        }

        Command::Relink { link, path } => {
            // The page chosen stays chosen: a relink is usually the same
            // document, revised.
            let pdf = link_pdf(state, link);
            let Some(measured) = measure_link_as(state, &path, pdf) else {
                return;
            };
            let now = state.active_mut().document_mut().relink(link, measured);
            if state.links.selected == Some(link) {
                state.links.selected = Some(now);
            }
        }

        Command::UpdateLink { link } => {
            let Some(path) = state
                .active()
                .document()
                .links
                .get(link)
                .map(|l| l.path.clone())
            else {
                return;
            };
            let pdf = link_pdf(state, link);
            if let Some(measured) = measure_link_as(state, &path, pdf) {
                state.active_mut().document_mut().relink(link, measured);
            }
        }

        Command::UpdateLinks { links } => {
            for link in links {
                let path = state
                    .active()
                    .document()
                    .links
                    .get(link)
                    .map(|l| l.path.clone());
                let pdf = link_pdf(state, link);
                if let Some(path) = path
                    && let Some(measured) = measure_link_as(state, &path, pdf)
                {
                    state.active_mut().document_mut().relink(link, measured);
                }
            }
        }

        Command::RelinkMany { changes } => {
            for (link, path) in changes {
                let pdf = link_pdf(state, link);
                let Some(measured) = measure_link_as(state, &path, pdf) else {
                    continue;
                };
                let now = state.active_mut().document_mut().relink(link, measured);
                if state.links.selected == Some(link) {
                    state.links.selected = Some(now);
                }
            }
        }

        Command::RefitArtwork { id, fit } => {
            state.active_mut().document_mut().refit(id, fit);
        }

        Command::FitFrameToArtwork { id } => {
            state.active_mut().document_mut().fit_frame_to_content(id);
        }

        Command::ShowPdfPage { id, pdf } => {
            let doc = state.active().document();
            let Some(path) = doc.frame(id).and_then(|f| match &f.kind {
                FrameKind::Graphic { placed: Some(p) } => {
                    doc.links.get(p.link).map(|l| l.path.clone())
                }
                _ => None,
            }) else {
                return;
            };
            let Some(link) = measure_link_as(state, &path, pdf) else {
                return;
            };
            let link = state.active_mut().document_mut().add_link(link);
            state.active_mut().document_mut().show_link(id, link);
        }
        _ => unreachable!("not a command for artwork"),
    }
}
