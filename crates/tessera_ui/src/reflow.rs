//! Body text takes pages with it as it is typed.
//!
//! InDesign's smart text reflow, for the frames it is meant for: a thread
//! whose last frame fills its page's margins is body text, and typing past
//! its end adds a page after it with a frame threaded on. A caption, a
//! sidebar or a box does not fill the margins and is left overset, as it
//! should be — a pull quote that grew a page would be a surprise.
//!
//! Asked once for each change the typing makes, of the layout the canvas has
//! already made, so a keystroke that changes nothing about the pages costs
//! nothing here.

use tessera_document::Document;
use tessera_document::ids::{FrameId, PageId};
use tessera_layout::ResolvedKind;

use crate::app::TesseraApp;

/// Whether `frame` is body text's: a frame filling its page's margins, as a
/// book's text frames do and as the frames flowed onto new pages are made.
pub(crate) fn fills_margins(doc: &Document, frame: FrameId) -> bool {
    let (Some(frame), Some(page)) = (doc.frame(frame), doc.page_of_frame(frame)) else {
        return false;
    };
    let Some(margins) = doc.margin_rect(page) else {
        return false;
    };
    let near = |a: f64, b: f64| (a - b).abs() <= 0.5;
    frame.transform.is_identity()
        && near(frame.bounds.x, margins.x)
        && near(frame.bounds.y, margins.y)
        && near(frame.bounds.width, margins.width)
        && near(frame.bounds.height, margins.height)
}

/// While text is being typed: carry body text onto a new page when it runs
/// past its last frame, and — when asked for — take away the pages at its end
/// it no longer reaches.
pub(crate) fn while_typing(state: &mut TesseraApp) {
    let prefs = (
        state.prefs.reflow_adds_pages,
        state.prefs.reflow_removes_pages,
    );
    if prefs == (false, false) {
        return;
    }
    let Some(editing) = state.active().editing.as_ref().map(|(frame, _)| *frame) else {
        return;
    };
    let revision = state.active().document().revision();
    if state.active().reflowed_at == Some(revision) {
        return;
    }
    state.active_mut().reflowed_at = Some(revision);

    let chain = state.active().document().thread_of(editing);
    let Some(&last) = chain.last() else {
        return;
    };
    if !fills_margins(state.active().document(), last) {
        return;
    }
    // The canvas's own layout, made already this frame or about to be: the
    // question costs nothing it would not have cost anyway.
    let laid = state.resolve_active();
    // Each frame of the thread: the lines it holds, and those that fit
    // nowhere.
    let counts: Vec<(FrameId, usize, usize)> = laid
        .items
        .iter()
        .filter(|item| chain.contains(&item.frame))
        .filter_map(|item| match &item.kind {
            ResolvedKind::Text {
                shaped,
                overset_lines,
                ..
            } => Some((item.frame, shaped.lines.len(), *overset_lines)),
            _ => None,
        })
        .collect();
    let lines = |frame: FrameId| {
        counts
            .iter()
            .find(|(f, ..)| *f == frame)
            .map(|(_, placed, overset)| (*placed, *overset))
    };
    let Some((_, overset)) = lines(last) else {
        return;
    };

    if overset > 0 && prefs.0 {
        let key = state.active;
        // undo-bracketed: inside the typing's own entry, so undoing the word
        // that ran past the page takes the page back with it.
        let flow = tessera_layout::autoflow::flow_onto_new_pages(
            state.documents[key].document_mut(),
            &mut state.shaper,
            last,
            // A keystroke's worth: a paste of a chapter flows the rest from
            // the menu, where a page count is not a surprise.
            PAGES_PER_KEYSTROKE,
        );
        if !flow.pages.is_empty() {
            state.active_mut().dirty = true;
        }
        return;
    }

    if overset == 0 && prefs.1 {
        // From the end back: frames that fill their margins, hold nothing,
        // and stand on pages holding nothing else. Never the frame with the
        // caret, and never the first of the thread.
        let doc = state.active().document();
        let mut going: Vec<PageId> = Vec::new();
        for frame in chain.iter().skip(1).rev() {
            let empty = lines(*frame).is_some_and(|(placed, _)| placed == 0);
            let Some(page) = doc.page_of_frame(*frame) else {
                break;
            };
            if *frame == editing
                || !empty
                || !fills_margins(doc, *frame)
                || doc.frames_on_page(page) != [*frame]
            {
                break;
            }
            going.push(page);
        }
        if going.is_empty() {
            return;
        }
        let key = state.active;
        // undo-bracketed: inside the typing's own entry, as the pages added
        // are.
        state.documents[key].document_mut().remove_pages(&going);
        state.active_mut().dirty = true;
    }
}

/// The most pages one keystroke adds.
const PAGES_PER_KEYSTROKE: usize = 20;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};

    const PARAGRAPH: &str = "The harbour wakes before the town does and the boats leave in the \
        grey light, one by one, past the breakwater and out to where the water changes colour. ";

    /// One page, a text frame filling its margins, and the caret in it.
    fn typing_body_text() -> (TesseraApp, FrameId) {
        let mut state = TesseraApp::headless();
        let page = state.current_page().expect("a page");
        let margins = state
            .active()
            .document()
            .margin_rect(page)
            .expect("margins");
        apply(&mut state, Command::AddTextFrame(margins));
        let id = state.active().selection.single().expect("the frame");
        crate::view::viewport::start_editing(&mut state, id);
        (state, id)
    }

    fn type_pages(state: &mut TesseraApp, paragraphs: usize) {
        let text = format!("{PARAGRAPH}\n").repeat(paragraphs);
        crate::view::viewport::type_text(state, &text);
        while_typing(state);
    }

    fn pages(state: &TesseraApp) -> usize {
        state.active().document().page_ids().count()
    }

    #[test]
    fn a_frame_filling_its_margins_is_body_text_and_a_box_is_not() {
        let (state, id) = typing_body_text();
        assert!(fills_margins(state.active().document(), id));
        let mut state = state;
        apply(
            &mut state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 100.0,
                y: 100.0,
                width: 120.0,
                height: 40.0,
            }),
        );
        let box_ = state.active().selection.single().expect("a box");
        assert!(!fills_margins(state.active().document(), box_));
    }

    #[test]
    fn typing_past_the_end_of_body_text_adds_pages_threaded_on() {
        let (mut state, id) = typing_body_text();
        type_pages(&mut state, 30);
        assert!(pages(&state) > 1, "pages were added");
        let chain = state.active().document().thread_of(id);
        assert_eq!(chain.len(), pages(&state));
        let laid = state.resolve_active();
        let last = laid
            .items
            .iter()
            .find(|i| Some(&i.frame) == chain.last())
            .expect("the last frame");
        assert!(matches!(
            &last.kind,
            ResolvedKind::Text {
                overset_lines: 0,
                ..
            }
        ));
        assert!(state.active().dirty);
    }

    #[test]
    fn a_box_typed_past_its_end_is_left_overset() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 100.0,
                y: 100.0,
                width: 120.0,
                height: 40.0,
            }),
        );
        let id = state.active().selection.single().expect("a box");
        crate::view::viewport::start_editing(&mut state, id);
        type_pages(&mut state, 5);
        assert_eq!(pages(&state), 1);
    }

    #[test]
    fn nothing_is_added_when_it_is_not_asked_for() {
        let (mut state, _) = typing_body_text();
        state.prefs.reflow_adds_pages = false;
        type_pages(&mut state, 30);
        assert_eq!(pages(&state), 1);
    }

    #[test]
    fn pages_the_text_no_longer_reaches_are_taken_away_only_when_asked() {
        let (mut state, id) = typing_body_text();
        type_pages(&mut state, 60);
        let grown = pages(&state);
        assert!(grown > 2);
        // Most of it cut: the text now fills a page or so.
        let cut = |state: &mut TesseraApp| {
            if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
                let end = buffer.story().text.len();
                buffer.select(400..end);
            }
            crate::view::viewport::type_text(state, "x");
            while_typing(state);
        };
        cut(&mut state);
        assert_eq!(pages(&state), grown, "off unless asked for");

        let (mut state, id2) = typing_body_text();
        state.prefs.reflow_removes_pages = true;
        type_pages(&mut state, 60);
        cut(&mut state);
        assert_eq!(pages(&state), 1, "the pages it no longer reaches went");
        assert_eq!(state.active().document().thread_of(id2), [id2]);
        let _ = id;
    }

    /// Body text over several pages, most of it then cut with the caret in
    /// `caret_in` (an index into the thread), and a picture on the second
    /// page when asked.
    fn cut_short(
        caret_in: impl Fn(&[FrameId]) -> FrameId,
        picture: bool,
    ) -> (TesseraApp, Vec<FrameId>) {
        let (mut state, id) = typing_body_text();
        state.prefs.reflow_removes_pages = true;
        type_pages(&mut state, 100);
        let chain = state.active().document().thread_of(id);
        assert!(chain.len() > 3, "{} frames", chain.len());
        if picture {
            let second = state
                .active()
                .document()
                .page_of_frame(chain[1])
                .expect("a page");
            let mut spot = state
                .active()
                .document()
                .margin_rect(second)
                .expect("margins");
            spot.width = 40.0;
            spot.height = 40.0;
            apply(&mut state, Command::AddRectangle(spot));
        }
        crate::view::viewport::start_editing(&mut state, caret_in(&chain));
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            let end = buffer.story().text.len();
            buffer.select(200..end);
        }
        crate::view::viewport::type_text(&mut state, "x");
        while_typing(&mut state);
        let now = state.active().document().thread_of(id);
        (state, now)
    }

    #[test]
    fn the_frame_with_the_caret_is_kept() {
        let (state, now) = cut_short(|chain| *chain.last().expect("last"), false);
        assert!(now.len() > 3, "nothing past the caret, so nothing went");
        let _ = state;
    }

    #[test]
    fn a_page_with_more_on_it_than_the_text_is_kept() {
        let (state, now) = cut_short(|chain| chain[0], true);
        assert_eq!(now.len(), 2, "the empty pages after the picture's went");
        assert_eq!(state.active().document().page_ids().count(), 2);
    }

    #[test]
    fn an_unchanged_document_is_not_looked_at_again() {
        // Overset body text looked at with adding pages off...
        let (mut state, _) = typing_body_text();
        state.prefs.reflow_adds_pages = false;
        state.prefs.reflow_removes_pages = true;
        type_pages(&mut state, 60);
        assert_eq!(pages(&state), 1);
        // ...is not looked at again, adding switched on, until it changes: the
        // evidence that a frame with nothing new typed costs nothing here.
        state.prefs.reflow_adds_pages = true;
        while_typing(&mut state);
        assert_eq!(pages(&state), 1);
        crate::view::viewport::type_text(&mut state, " more");
        while_typing(&mut state);
        assert!(pages(&state) > 1, "the next change is");
    }

    #[test]
    fn a_change_is_looked_at_once() {
        let (mut state, _) = typing_body_text();
        crate::view::viewport::type_text(&mut state, "Word");
        while_typing(&mut state);
        let at = state.active().reflowed_at;
        assert_eq!(at, Some(state.active().document().revision()));
        // Nothing changed since: not looked at again.
        state.prefs.reflow_adds_pages = true;
        while_typing(&mut state);
        assert_eq!(state.active().reflowed_at, at);
    }

    /// Body text on one page with more of it than the page holds, not
    /// being typed in.
    fn overset_body_text() -> (TesseraApp, FrameId) {
        let mut state = TesseraApp::headless();
        let page = state.current_page().expect("a page");
        let margins = state
            .active()
            .document()
            .margin_rect(page)
            .expect("margins");
        apply(&mut state, Command::AddTextFrame(margins));
        let id = state.active().selection.single().expect("the frame");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: format!("{PARAGRAPH}\n").repeat(100),
            },
        );
        (state, id)
    }

    #[test]
    fn flowing_a_thread_is_one_step_to_undo_and_says_how_many_pages() {
        let (mut state, id) = overset_body_text();
        let depth = state.active().history.undo_depth();
        apply(&mut state, Command::FlowText { id });
        let added = pages(&state) - 1;
        assert!(added >= 2, "{added}");
        assert_eq!(state.active().history.undo_depth(), depth + 1);
        let said = state
            .status
            .as_ref()
            .map(|s| s.message.clone())
            .unwrap_or_default();
        assert_eq!(said, format!("The text flowed onto {added} new pages."));
        apply(&mut state, Command::Undo);
        assert_eq!(pages(&state), 1);
        assert_eq!(state.active().document().thread_of(id), [id]);
    }

    #[test]
    fn the_menu_flows_the_selected_frame_and_leaves_one_that_fits_alone() {
        use crate::actions::{Cmd, Run, run};
        let (mut state, id) = overset_body_text();
        state.active_mut().selection.set(id);
        run(&mut state, Run::Command(Cmd::FlowText));
        assert!(pages(&state) > 2);
        let depth = state.active().history.undo_depth();
        run(&mut state, Run::Command(Cmd::FlowText));
        assert_eq!(state.active().history.undo_depth(), depth, "no empty step");
        let said = state
            .status
            .as_ref()
            .map(|s| s.message.clone())
            .unwrap_or_default();
        assert_eq!(said, "Its text fits already.");
        state.active_mut().selection.clear();
        run(&mut state, Run::Command(Cmd::FlowText));
        let said = state
            .status
            .as_ref()
            .map(|s| s.message.clone())
            .unwrap_or_default();
        assert!(said.starts_with("Select the text frame"), "{said}");
    }

    fn manuscript() -> crate::command::PlacedText {
        let story = tessera_text::story::Story::new(format!("{PARAGRAPH}\n").repeat(100));
        let paragraphs = story.paragraphs.len();
        let runs = story.runs.len();
        crate::command::PlacedText {
            story,
            paragraph_styles: Vec::new(),
            character_styles: Vec::new(),
            paragraph_style_names: vec![None; paragraphs],
            run_style_names: vec![None; runs],
        }
    }

    #[test]
    fn a_placed_manuscript_flows_onto_new_pages_in_the_same_step() {
        let mut state = TesseraApp::headless();
        let depth = state.active().history.undo_depth();
        apply(
            &mut state,
            Command::PlaceText {
                id: None,
                text: manuscript(),
            },
        );
        assert!(pages(&state) > 2, "{} pages", pages(&state));
        assert_eq!(state.active().history.undo_depth(), depth + 1);
        apply(&mut state, Command::Undo);
        assert_eq!(pages(&state), 1);

        let mut state = TesseraApp::headless();
        state.prefs.flow_placed_text = false;
        apply(
            &mut state,
            Command::PlaceText {
                id: None,
                text: manuscript(),
            },
        );
        assert_eq!(pages(&state), 1, "left overset when not asked");
    }
}
