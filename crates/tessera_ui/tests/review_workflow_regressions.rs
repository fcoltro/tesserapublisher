use tessera_document::FrameKind;
use tessera_text::edit::EditBuffer;
use tessera_ui::{Command, TesseraApp, apply};

fn editing_document() -> (TesseraApp, tessera_document::StoryId) {
    let mut state = TesseraApp::headless();
    let bounds = state.first_page_bounds();
    apply(&mut state, Command::AddTextFrame(bounds));
    let id = state.active().selection.single().unwrap();
    apply(
        &mut state,
        Command::SetText {
            id,
            text: "cat café".into(),
        },
    );
    let FrameKind::Text { story, .. } = state.active().document().frame(id).unwrap().kind else {
        panic!()
    };
    let buffer = EditBuffer::new(state.active().document().story(story).unwrap().clone());
    state.active_mut().editing = Some((id, buffer));
    (state, story)
}

#[test]
fn replacing_matches_cannot_leave_a_stale_canvas_buffer() {
    let (mut state, story) = editing_document();
    apply(
        &mut state,
        Command::ReplaceMatches {
            edits: vec![(story, 0..3, "dog".into())],
        },
    );
    assert_eq!(
        state.active().document().story(story).unwrap().text,
        "dog café"
    );
    assert!(state.active().editing.is_none());
    assert!(state.active().editing_cell.is_none());
    apply(&mut state, Command::Undo);
    assert_eq!(
        state.active().document().story(story).unwrap().text,
        "cat café"
    );
}

#[test]
fn malformed_replacement_ranges_do_not_mutate_text_or_close_the_editor() {
    let (mut state, story) = editing_document();
    for range in [std::ops::Range { start: 3, end: 1 }, 0..99, 7..8] {
        apply(
            &mut state,
            Command::ReplaceMatches {
                edits: vec![(story, range, "wrong".into())],
            },
        );
        assert_eq!(
            state.active().document().story(story).unwrap().text,
            "cat café"
        );
        assert!(state.active().editing.is_some());
    }
}
