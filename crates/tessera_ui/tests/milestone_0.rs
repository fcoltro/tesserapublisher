//! The milestone 0 acceptance sentence, end to end.
//!
//! > Launch Tessera. A new document opens with one spread. Draw a rectangle
//! > and give it a fill colour. Draw a text frame, type into it on the canvas,
//! > and see the text shaped and rendered. Save the file as `.tessera`. Quit
//! > the application. Launch it again, open that file, and find the rectangle
//! > and the text exactly as they were left. Export a PDF, and open that PDF
//! > in Acrobat with the text selectable.
//!
//! Everything but the window is covered here. The window itself is verified by
//! hand, on Windows — see the roadmap.

use tessera_color::Color;
use tessera_geometry::DocRect;
use tessera_ui::app::TesseraApp;
use tessera_ui::command::{Command, apply};
use tessera_ui::file_ops::{export_pdf_to_path, open_from_path, save_to_path};

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("tessera_milestone_0");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

#[test]
fn the_milestone_0_sentence_holds() {
    let path = temp("acceptance.tessera");
    let pdf_path = temp("acceptance.pdf");
    let teal = Color::Cmyk {
        c: 0.8,
        m: 0.2,
        y: 0.0,
        k: 0.0,
        a: 1.0,
    };

    // --- A new document opens with one spread.
    let mut state = TesseraApp::headless();
    assert_eq!(state.document.spread_ids().count(), 1);
    assert_eq!(
        (
            state.first_page_bounds().width,
            state.first_page_bounds().height
        ),
        (612.0, 792.0)
    );

    // --- Draw a rectangle and give it a fill colour.
    apply(
        &mut state,
        Command::AddRectangle(DocRect {
            x: 72.0,
            y: 72.0,
            width: 200.0,
            height: 100.0,
        }),
    );
    let rect_id = state.selection.expect("the new rectangle is selected");
    apply(
        &mut state,
        Command::SetFill {
            id: rect_id,
            color: teal.clone(),
        },
    );

    // --- Draw a text frame and type into it.
    apply(
        &mut state,
        Command::AddTextFrame(DocRect {
            x: 72.0,
            y: 240.0,
            width: 400.0,
            height: 60.0,
        }),
    );
    let text_id = state.selection.expect("the new text frame is selected");
    apply(
        &mut state,
        Command::SetText {
            id: text_id,
            text: "Hello, Tessera.".to_string(),
        },
    );

    // --- The text is really shaped, not merely stored.
    let resolved = tessera_layout::resolve::resolve(&state.document, &mut state.shaper);
    let shaped_glyphs: usize = resolved
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            tessera_layout::resolve::ResolvedKind::Text { shaped, .. } => {
                Some(shaped.glyph_count())
            }
            _ => None,
        })
        .sum();
    assert!(
        shaped_glyphs >= "Hello, Tessera.".len() - 2,
        "the text must shape into glyphs, saw {shaped_glyphs}"
    );

    // --- Save.
    save_to_path(&mut state, &path).expect("save");
    assert!(!state.dirty, "a saved document is not dirty");

    // --- Quit and launch again.
    drop(state);
    let mut reopened = TesseraApp::headless();
    open_from_path(&mut reopened, &path).expect("open");

    // --- Find the rectangle and the text exactly as they were left.
    assert_eq!(reopened.document.frames.len(), 2, "both frames survived");

    let rect = reopened
        .document
        .frame(rect_id)
        .expect("the rectangle survived, under its original id");
    assert_eq!(rect.bounds.width, 200.0);
    assert_eq!(rect.bounds.height, 100.0);
    assert_eq!(rect.fill, teal, "including its CMYK fill");

    assert_eq!(
        reopened
            .document
            .stories
            .values()
            .next()
            .expect("the story survived")
            .text,
        "Hello, Tessera.",
        "the text itself, not just the frame that holds it"
    );

    // --- Export a PDF.
    export_pdf_to_path(&mut reopened, &pdf_path).expect("export");
    let bytes = std::fs::read(&pdf_path).expect("the pdf was written");

    assert!(bytes.starts_with(b"%PDF-1."));
    assert!(bytes.windows(5).any(|w| w == b"%%EOF"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("/FontFile2"),
        "the PDF must embed its font, or a RIP cannot set the text"
    );
    assert!(
        text.contains("Identity-H"),
        "glyph ids are written directly, as the shaper produced them"
    );

    println!(
        "\n  Open this in Acrobat to finish the acceptance check:\n  {}\n",
        pdf_path.display()
    );
}

#[test]
fn undo_reaches_back_through_the_whole_session() {
    let mut state = TesseraApp::headless();
    let bounds = DocRect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    };

    apply(&mut state, Command::AddRectangle(bounds));
    apply(&mut state, Command::AddTextFrame(bounds));
    let id = state.selection.expect("selected");
    apply(
        &mut state,
        Command::SetText {
            id,
            text: "x".to_string(),
        },
    );
    assert_eq!(state.document.frames.len(), 2);

    apply(&mut state, Command::Undo); // the text
    apply(&mut state, Command::Undo); // the text frame
    apply(&mut state, Command::Undo); // the rectangle

    assert_eq!(
        state.document.frames.len(),
        0,
        "every step of the session must be undoable"
    );
}

#[test]
fn a_document_saved_then_exported_twice_produces_the_same_pdf() {
    // Export must be a pure function of the document. If it is not, two
    // exports of the same file differ and nothing downstream can be trusted.
    let path = temp("determinism.tessera");
    let mut state = TesseraApp::headless();
    apply(
        &mut state,
        Command::AddTextFrame(DocRect {
            x: 10.0,
            y: 10.0,
            width: 300.0,
            height: 40.0,
        }),
    );
    let id = state.selection.expect("selected");
    apply(
        &mut state,
        Command::SetText {
            id,
            text: "Determinism".to_string(),
        },
    );
    save_to_path(&mut state, &path).expect("save");

    let first = temp("determinism_a.pdf");
    let second = temp("determinism_b.pdf");
    export_pdf_to_path(&mut state, &first).expect("export");

    let mut reopened = TesseraApp::headless();
    open_from_path(&mut reopened, &path).expect("open");
    export_pdf_to_path(&mut reopened, &second).expect("export");

    assert_eq!(
        std::fs::read(&first).expect("a"),
        std::fs::read(&second).expect("b"),
        "the same document must export to the same bytes"
    );
}
