//! The per-document container.
//!
//! One document is open at a time in this milestone, so what these tests pin
//! is not multi-document behaviour but the property that makes it possible
//! later: that the state a second document would need is already held per
//! document rather than per application.

use tessera_geometry::DocRect;
use tessera_ui::app::TesseraApp;
use tessera_ui::command::{Command, apply};
use tessera_ui::open_document::OpenDocument;

fn rect(x: f64) -> DocRect {
    DocRect {
        x,
        y: 10.0,
        width: 40.0,
        height: 30.0,
    }
}

#[test]
fn a_fresh_application_has_exactly_one_open_document() {
    let app = TesseraApp::headless();
    assert_eq!(app.open_count(), 1);
    assert!(app.active().document().spread_ids().count() >= 1);
}

#[test]
fn a_fresh_document_is_clean_and_untitled() {
    let app = TesseraApp::headless();
    assert!(!app.active().dirty);
    assert!(app.active().current_path.is_none());
    assert_eq!(app.active().title(), "Untitled");
}

#[test]
fn editing_marks_the_document_dirty_and_records_undo() {
    let mut app = TesseraApp::headless();
    apply(&mut app, Command::AddRectangle(rect(10.0)));

    assert!(app.active().dirty, "an edit dirties its own document");
    assert!(app.active().history.can_undo());
}

#[test]
fn two_documents_keep_separate_frames_history_and_selection() {
    // The whole point of the container. Until this holds, a second open
    // document is a rewrite rather than a data change.
    let mut app = TesseraApp::headless();
    let first = app.active;

    apply(&mut app, Command::AddRectangle(rect(10.0)));
    apply(&mut app, Command::AddRectangle(rect(80.0)));

    let second = app.documents.insert(OpenDocument::new());
    app.active = second;
    apply(&mut app, Command::AddRectangle(rect(200.0)));

    assert_eq!(app.open_count(), 2);
    assert_eq!(
        app.documents[first].document().frames.len(),
        2,
        "the first document kept its own frames"
    );
    assert_eq!(
        app.documents[second].document().frames.len(),
        1,
        "the second document did not inherit them"
    );

    // Undoing in the second must not reach into the first.
    apply(&mut app, Command::Undo);
    assert_eq!(app.documents[second].document().frames.len(), 0);
    assert_eq!(
        app.documents[first].document().frames.len(),
        2,
        "undo in one document left the other alone"
    );

    assert!(
        app.documents[first].selection.as_slice().is_empty()
            || app.documents[second].selection.as_slice().is_empty()
            || app.documents[first].selection.as_slice()
                != app.documents[second].selection.as_slice(),
        "the two documents share a selection"
    );
}

#[test]
fn the_clipboard_and_the_shaper_stay_with_the_application() {
    // Deliberately not per-document: a copy in one document must paste into
    // another, and the font cache is worth sharing.
    let mut app = TesseraApp::headless();
    apply(&mut app, Command::AddRectangle(rect(10.0)));
    app.active_mut().select_all();
    apply(&mut app, Command::CopySelection);

    let copied = app.clipboard.len();
    assert_eq!(copied, 1, "the copy landed on the application's clipboard");

    let second = app.documents.insert(OpenDocument::new());
    app.active = second;
    assert_eq!(
        app.clipboard.len(),
        copied,
        "switching documents did not empty the clipboard"
    );

    apply(&mut app, Command::Paste);
    assert_eq!(
        app.active().document().frames.len(),
        1,
        "a frame copied in one document pasted into another"
    );
}
