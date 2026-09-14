//! The hyperlink box: where the selected words go when clicked.
//!
//! A URL, or a page of this document. A page link is a named destination —
//! "Page 12" — pointing at the page's id, so it follows the page if the
//! pages are reordered even though its name no longer says where it went.
//! The link rides on the character format of the selection, which is what
//! makes it travel with the words and cascade from a style.
//!
//! Nothing draws a link on the canvas: the words look as they are set. The
//! PDF carries the annotation, and that is where a link is clicked.

use tessera_document::ids::{PageId, StoryId};
use tessera_text::story::{CharacterFormat, Hyperlink};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Url,
    Page,
}

#[derive(Debug, Clone, Default)]
pub struct HyperlinkWindow {
    pub open: bool,
    pub story: Option<StoryId>,
    pub range: std::ops::Range<usize>,
    pub kind: Kind,
    pub url: String,
    pub page: Option<PageId>,
    /// Whether the selection already carried a link, so Remove is offered.
    pub had_link: bool,
}

impl HyperlinkWindow {
    /// Open on the selection, loading whatever link it already has.
    pub fn open(&mut self, state: &TesseraApp) {
        let Some((id, buffer)) = state.active().editing.as_ref() else {
            return;
        };
        let Some(range) = buffer.selection_range().filter(|r| !r.is_empty()) else {
            return;
        };
        let Some(story) =
            crate::view::viewport::editing_story(state, *id, state.active().editing_cell)
        else {
            return;
        };
        let doc = state.active().document();
        let current = buffer
            .story()
            .run_at(range.start)
            .map(|run| buffer.story().resolve_run(run, doc))
            .and_then(|f| f.link);
        *self = Self::default();
        self.story = Some(story);
        self.range = range;
        match current {
            Some(Hyperlink::Url(url)) => {
                self.kind = Kind::Url;
                self.url = url;
                self.had_link = true;
            }
            Some(Hyperlink::Destination(name)) => {
                self.kind = Kind::Page;
                self.page = doc.destination_page(&name);
                self.had_link = true;
            }
            _ => {}
        }
        if self.page.is_none() {
            self.page = state.current_page();
        }
        self.open = true;
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.hyperlink.open {
        return;
    }
    let mut window = state.hyperlink.clone();
    let mut go = false;
    let mut remove = false;
    let pages: Vec<(PageId, String)> = state
        .active()
        .document()
        .page_numbers()
        .into_iter()
        .map(|(id, n)| (id, n.label))
        .collect();

    let response = egui::Modal::new(egui::Id::new("hyperlink"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 460.0));
            ui.heading("Hyperlink");
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut window.kind, Kind::Url, "Web address");
                ui.selectable_value(&mut window.kind, Kind::Page, "Page in this document");
            });
            ui.add_space(Theme::space_1());
            match window.kind {
                Kind::Url => {
                    crate::view::panels::field(ui, "URL", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut window.url)
                                .hint_text("https://")
                                .desired_width(f32::INFINITY),
                        );
                    });
                }
                Kind::Page => {
                    crate::view::panels::field(ui, "Page", |ui| {
                        let shown = window
                            .page
                            .and_then(|p| pages.iter().find(|(id, _)| *id == p))
                            .map_or("(choose)", |(_, label)| label.as_str());
                        egui::ComboBox::from_id_salt("hyperlink-page")
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                for (id, label) in &pages {
                                    ui.selectable_value(&mut window.page, Some(*id), label);
                                }
                            });
                    });
                }
            }
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                let ready = match window.kind {
                    Kind::Url => !window.url.trim().is_empty(),
                    Kind::Page => window.page.is_some(),
                };
                go = ui.add_enabled(ready, super::primary_button("OK")).clicked();
                if window.had_link && ui.button("Remove link").clicked() {
                    remove = true;
                }
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    if go || remove {
        if let Some(story) = window.story {
            let link = if remove {
                Hyperlink::None
            } else {
                match window.kind {
                    Kind::Url => Hyperlink::Url(window.url.trim().to_owned()),
                    Kind::Page => {
                        let Some(page) = window.page else {
                            state.hyperlink = window;
                            return;
                        };
                        let name = pages
                            .iter()
                            .find(|(id, _)| *id == page)
                            .map(|(_, label)| format!("Page {label}"))
                            .unwrap_or_else(|| "Page".to_owned());
                        apply(
                            state,
                            Command::SetDestination {
                                name: name.clone(),
                                page,
                            },
                        );
                        Hyperlink::Destination(name)
                    }
                }
            };
            apply(
                state,
                Command::SetCharacterFormat {
                    story,
                    range: window.range.clone(),
                    format: CharacterFormat {
                        link: Some(link),
                        ..Default::default()
                    },
                },
            );
        }
        window.open = false;
    }
    state.hyperlink = window;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::FrameKind;
    use tessera_geometry::DocRect;

    fn editing_with_selection(text: &str, range: std::ops::Range<usize>) -> TesseraApp {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 20.0,
                y: 20.0,
                width: 300.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        crate::view::viewport::start_editing(&mut state, id);
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.select(range);
        }
        state
    }

    fn link_at(state: &TesseraApp, at: usize) -> Option<Hyperlink> {
        let (id, _) = state.active().editing.as_ref().unwrap();
        let FrameKind::Text { story, .. } = state.active().document().frame(*id).unwrap().kind
        else {
            panic!()
        };
        let story = state.active().document().story(story).unwrap();
        story.run_at(at).and_then(|r| r.local.link.clone())
    }

    #[test]
    fn the_box_opens_on_a_selection_and_not_on_a_bare_caret() {
        let state = editing_with_selection("see the site", 4..12);
        let mut window = HyperlinkWindow::default();
        window.open(&state);
        assert!(window.open);
        assert_eq!(window.range, 4..12);
        assert!(!window.had_link);

        let mut bare = editing_with_selection("see", 0..3);
        if let Some((_, buffer)) = bare.active_mut().editing.as_mut() {
            buffer.set_cursor(1);
        }
        let mut window = HyperlinkWindow::default();
        window.open(&bare);
        assert!(!window.open, "nothing selected, nothing to link");
    }

    #[test]
    fn a_page_link_makes_a_destination_the_page_can_be_found_by() {
        let mut state = editing_with_selection("see page two", 4..12);
        let second = state.active_mut().document_mut().add_page();
        let story = {
            let (id, _) = state.active().editing.as_ref().unwrap();
            let FrameKind::Text { story, .. } = state.active().document().frame(*id).unwrap().kind
            else {
                panic!()
            };
            story
        };
        apply(
            &mut state,
            Command::SetDestination {
                name: "Page 2".into(),
                page: second,
            },
        );
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 4..12,
                format: CharacterFormat {
                    link: Some(Hyperlink::Destination("Page 2".into())),
                    ..Default::default()
                },
            },
        );
        assert_eq!(
            link_at(&state, 5),
            Some(Hyperlink::Destination("Page 2".into()))
        );
        assert_eq!(
            state.active().document().destination_page("Page 2"),
            Some(second)
        );
        // Reopening reads it back, as a page link.
        let mut window = HyperlinkWindow::default();
        window.open(&state);
        assert!(window.had_link);
        assert_eq!(window.kind, Kind::Page);
        assert_eq!(window.page, Some(second));
    }
}
