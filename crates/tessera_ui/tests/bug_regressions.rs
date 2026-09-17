//! Multi-step reproductions from the deep review, pinned at the public API.
use tessera_document::nodes::{BaselineGrid, Frame, TextWrap};
use tessera_document::{Document, FrameId, FrameKind, StoryId, format};
use tessera_geometry::{DocRect, Transform};
use tessera_layout::ResolvedKind;
use tessera_text::story::{CharacterFormat, NoStyles, ParagraphFormat};
use tessera_text::{Shaper, Story};
use tessera_ui::{Command, TesseraApp, apply};

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("tessera-regression-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn bounds() -> DocRect {
    DocRect {
        x: 20.0,
        y: 20.0,
        width: 200.0,
        height: 100.0,
    }
}
fn selected(app: &TesseraApp) -> FrameId {
    app.active().selection.single().unwrap()
}
fn cell(app: &TesseraApp, id: FrameId, column: usize) -> StoryId {
    let FrameKind::Table(table) = &app.active().document().frame(id).unwrap().kind else {
        panic!()
    };
    table.at(0, column).unwrap().cell().unwrap().story
}
fn frame(bounds: DocRect, kind: FrameKind) -> Frame {
    Frame {
        bounds,
        kind,
        transform: Transform::IDENTITY,
        fill: Default::default(),
        stroke: None,
        wrap: TextWrap::None,
        blend: Default::default(),
        corners: Default::default(),
        shadow: None,
        anchor: None,
        style: None,
    }
}

#[test]
fn deleting_a_duplicated_group_preserves_original_children_and_undo() {
    let mut app = TesseraApp::headless();
    apply(&mut app, Command::AddRectangle(bounds()));
    let a = selected(&app);
    apply(&mut app, Command::AddEllipse(bounds()));
    let b = selected(&app);
    app.active_mut().selection.replace_all(vec![a, b]);
    apply(&mut app, Command::GroupSelection);
    let original = selected(&app);
    apply(&mut app, Command::DuplicateSelection);
    assert_eq!(app.active().document().frames.len(), 6);
    apply(&mut app, Command::DeleteSelection);
    for id in [original, a, b] {
        assert!(app.active().document().frame(id).is_some());
    }
    apply(&mut app, Command::Undo);
    assert_eq!(app.active().document().frames.len(), 6);
}

#[test]
fn duplicated_and_pasted_tables_own_their_cell_stories() {
    let mut app = TesseraApp::headless();
    apply(
        &mut app,
        Command::AddTable {
            bounds: bounds(),
            rows: 1,
            columns: 2,
        },
    );
    let original = selected(&app);
    let a = cell(&app, original, 0);
    let b = cell(&app, original, 1);
    apply(
        &mut app,
        Command::ReplaceMatches {
            edits: vec![(a, 0..0, "first".into()), (b, 0..0, "second".into())],
        },
    );
    apply(&mut app, Command::DuplicateSelection);
    let copied = selected(&app);
    assert_ne!(cell(&app, copied, 0), a);
    apply(
        &mut app,
        Command::MergeCells {
            id: copied,
            row: 0,
            column: 0,
            span: tessera_document::table::Span {
                rows: 1,
                columns: 2,
            },
        },
    );
    assert_eq!(app.active().document().story(a).unwrap().text, "first");
    assert_eq!(app.active().document().story(b).unwrap().text, "second");
    app.active_mut().selection.set(original);
    apply(&mut app, Command::CopySelection);
    tessera_ui::file_ops::new_document(&mut app);
    apply(&mut app, Command::Paste);
    assert_eq!(
        app.active()
            .document()
            .story(cell(&app, selected(&app), 1))
            .unwrap()
            .text,
        "second"
    );
}

#[test]
fn cross_document_import_remaps_conflicting_styles_swatches_and_links() {
    use tessera_color::Color;
    use tessera_document::nodes::Swatch;
    use tessera_text::story::CharacterStyle;
    let mut source = Document::new();
    let layer = source.default_layer().unwrap();
    source.swatches.push(Swatch::new("Brand", Color::BLACK));
    let style = source.add_character_style(CharacterStyle {
        name: "Copy style".into(),
        format: CharacterFormat {
            size: Some(29.0),
            colour: Some(Color::Swatch {
                name: "Brand".into(),
                tint: 1.0,
            }),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut story = Story::new("source");
    story.runs[0].style = Some(style);
    let sid = source.add_story(story);
    let text = source.add_frame(layer, frame(bounds(), FrameKind::text(sid)));
    let link = source.add_link(tessera_document::links::Link::new(
        "source-photo.png",
        (40.0, 20.0),
    ));
    let picture = source.add_frame(
        layer,
        frame(
            bounds(),
            FrameKind::Graphic {
                placed: Some(tessera_document::graphic::Placement {
                    link,
                    inner: Transform::IDENTITY,
                }),
            },
        ),
    );
    let mut target = Document::new();
    let layer = target.default_layer().unwrap();
    target.swatches.push(Swatch::new("Brand", Color::WHITE));
    target.add_character_style(CharacterStyle {
        name: "Unrelated".into(),
        ..Default::default()
    });
    target.add_link(tessera_document::links::Link::new("wrong.png", (1.0, 1.0)));
    let copies = target
        .import_frames(&source, &[text, picture], layer, 12.0, 12.0, false)
        .unwrap();
    let FrameKind::Text { story, .. } = target.frame(copies[0]).unwrap().kind else {
        panic!()
    };
    let story = target.story(story).unwrap();
    let actual = story.resolve_run(&story.runs[0], &target);
    assert_eq!(actual.size, Some(29.0));
    assert_eq!(
        target.resolve_colour(actual.colour.as_ref().unwrap()),
        Color::BLACK
    );
    assert_eq!(target.swatch("Brand").unwrap().colour, Color::WHITE);
    let FrameKind::Graphic {
        placed: Some(placed),
    } = target.frame(copies[1]).unwrap().kind
    else {
        panic!()
    };
    assert_eq!(
        target.links[placed.link].path,
        std::path::Path::new("source-photo.png")
    );
}

fn threaded(grid: bool, height: f64) -> (Document, FrameId, FrameId, StoryId) {
    let mut doc = Document::new();
    doc.setup.facing_pages = false;
    doc.setup.baseline_grid = grid.then_some(BaselineGrid {
        start: 0.0,
        step: 30.0,
    });
    doc.reflow_spreads();
    let sid = doc.add_story(Story::new("one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen"));
    let layer = doc.default_layer().unwrap();
    let mut a = frame(
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 90.0,
            height,
        },
        FrameKind::text(sid),
    );
    if let FrameKind::Text { layout, .. } = &mut a.kind {
        layout.lock_to_grid = grid;
    }
    let a = doc.add_frame(layer, a);
    let spare = doc.add_story(Story::default());
    let b = doc.add_frame(
        layer,
        frame(
            DocRect {
                x: 120.0,
                y: 0.0,
                width: 90.0,
                height: if grid { 400.0 } else { height },
            },
            FrameKind::text(spare),
        ),
    );
    assert!(doc.thread(a, b));
    (doc, a, b, sid)
}

#[test]
fn grid_thread_continues_at_the_last_displayed_byte_and_caret_uses_the_tail() {
    let (doc, a, b, _) = threaded(true, 70.0);
    let resolved = tessera_layout::resolve(&doc, &mut Shaper::new());
    let shaped = |id| {
        resolved
            .items
            .iter()
            .find_map(|i| match &i.kind {
                ResolvedKind::Text { shaped, .. } if i.frame == id => Some(shaped),
                _ => None,
            })
            .unwrap()
    };
    let end = shaped(a).lines.last().unwrap().range.end;
    let tail = shaped(b);
    assert_eq!(end, tail.lines[0].range.start);
    assert!(end > 0);
    assert_eq!(tail.offset_at(-1.0, tail.lines[0].baseline), end);
    let caret = tail
        .caret_geometry(
            tessera_text::edit::TextCursor {
                position: end,
                anchor: end,
            },
            1.0,
        )
        .caret
        .unwrap();
    assert!((caret.y0 - (tail.lines[0].baseline - tail.lines[0].ascent)).abs() < 6.0);
}

#[test]
fn duplicating_a_thread_preserves_sharing_only_inside_the_copy() {
    let (mut doc, a, b, sid) = threaded(false, 80.0);
    let layer = doc.default_layer().unwrap();
    let copies = doc.copy_frames(&[a, b], layer, 10.0, 20.0);
    let FrameKind::Text { story: ca, layout } = doc.frame(copies[0]).unwrap().kind else {
        panic!()
    };
    let FrameKind::Text { story: cb, .. } = doc.frame(copies[1]).unwrap().kind else {
        panic!()
    };
    assert_eq!(ca, cb);
    assert_ne!(ca, sid);
    assert_eq!(layout.next, Some(copies[1]));
}

#[test]
fn preflight_agrees_with_a_story_that_fits_across_two_frames() {
    let (doc, _, _, _) = threaded(false, 80.0);
    assert!(tessera_preflight::rules::overset_text(&doc, &mut Shaper::new()).is_empty());
}

#[test]
fn paragraph_selection_excludes_the_next_paragraph_and_shift_splits_glyph_runs() {
    let mut story = Story::new("first\nsecond");
    story.apply_paragraph_format(
        0..6,
        &ParagraphFormat {
            indent_left: Some(20.0),
            ..Default::default()
        },
    );
    assert_eq!(story.paragraph_bounds(0..6), 0..6);
    assert_eq!(story.paragraph_bounds(6..6), 6..12);
    assert_eq!(story.paragraphs[1].local.indent_left, None);
    let mut shifted = Story::new("ab");
    shifted.apply_character_format(
        1..2,
        &CharacterFormat {
            baseline_shift: Some(6.0),
            ..Default::default()
        },
    );
    let shaped = Shaper::new().shape(&shifted, &NoStyles::default(), 300.0);
    let ys: Vec<_> = shaped.lines[0].glyphs().map(|g| g.y).collect();
    assert!((ys[0] - ys[1] - 6.0).abs() < 0.01);
}

#[test]
fn find_and_change_reaches_table_cells_and_pdf_embeds_their_text() {
    let mut app = TesseraApp::headless();
    let mut b = app.first_page_bounds();
    b.x += 20.0;
    b.y += 20.0;
    b.width = 200.0;
    b.height = 100.0;
    apply(
        &mut app,
        Command::AddTable {
            bounds: b,
            rows: 1,
            columns: 2,
        },
    );
    let table = selected(&app);
    let sid = cell(&app, table, 1);
    apply(
        &mut app,
        Command::ReplaceMatches {
            edits: vec![(sid, 0..0, "needle".into())],
        },
    );
    let hits = tessera_ui::find::search(
        app.active().document(),
        &tessera_ui::find::Query {
            needle: "needle".into(),
            ..Default::default()
        },
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].cell, Some((0, 1)));
    apply(
        &mut app,
        Command::ReplaceMatches {
            edits: tessera_ui::find::edits_for(&hits, "replacement"),
        },
    );
    assert_eq!(
        app.active().document().story(sid).unwrap().text,
        "replacement"
    );
    let resolved = app.resolve_uncached();
    let ResolvedKind::Table { laid, .. } = &resolved.items[0].kind else {
        panic!()
    };
    let text = &laid.cells[1].shaped;
    let caret = text
        .caret_geometry(
            tessera_text::edit::TextCursor {
                position: 0,
                anchor: 0,
            },
            1.0,
        )
        .caret
        .unwrap();
    assert!(caret.x0 >= laid.cells[1].text_area.x);
    assert_eq!(text.offset_at(caret.x0, (caret.y0 + caret.y1) / 2.0), 0);
    let pdf = tessera_pdf::export(&resolved).unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains(" Tf"));
    artifact("table-fixed.pdf", &pdf);
}

#[test]
fn packaging_survives_relocation_and_refreshes_colliding_assets() {
    let temp = Scratch::new();
    let folder = temp.0.join("package");
    let mut doc = Document::new();
    for (dir, bytes) in [("a", b"first".as_slice()), ("b", b"second".as_slice())] {
        std::fs::create_dir_all(temp.0.join(dir)).unwrap();
        let path = temp.0.join(dir).join("photo.png");
        std::fs::write(&path, bytes).unwrap();
        doc.add_link(tessera_document::links::Link::new(path, (10.0, 10.0)));
    }
    let result = tessera_ui::package::collect(&doc, "Job", &folder, &Default::default()).unwrap();
    assert_eq!(result.links.len(), 2);
    assert!(result.missing.is_empty());
    std::fs::write(temp.0.join("a/photo.png"), b"updated").unwrap();
    tessera_ui::package::collect(&doc, "Job", &folder, &Default::default()).unwrap();
    let moved = temp.0.join("moved");
    std::fs::rename(folder, &moved).unwrap();
    for dir in ["a", "b"] {
        std::fs::remove_file(temp.0.join(dir).join("photo.png")).unwrap();
    }
    let reopened = format::load(&moved.join("Job.tessera")).unwrap();
    let mut bytes: Vec<_> = reopened
        .links
        .values()
        .map(|l| {
            assert!(l.path.starts_with(&moved));
            std::fs::read(&l.path).unwrap()
        })
        .collect();
    bytes.sort();
    assert_eq!(bytes, vec![b"second".to_vec(), b"updated".to_vec()]);
}

#[test]
fn equal_revision_documents_each_get_a_copy_and_saving_one_preserves_the_other() {
    use std::time::{Duration, Instant};
    let temp = Scratch::new();
    let mut app = TesseraApp::headless();
    apply(&mut app, Command::AddRectangle(bounds()));
    let first = app.active;
    tessera_ui::file_ops::new_document(&mut app);
    apply(&mut app, Command::AddEllipse(bounds()));
    let second = app.active;
    assert_eq!(
        app.documents[first].document().revision(),
        app.documents[second].document().revision()
    );
    app.autosave_in(&temp.0, Instant::now() + Duration::from_secs(600));
    let a = app.documents[first].recovery.copy_path.clone().unwrap();
    let b = app.documents[second].recovery.copy_path.clone().unwrap();
    assert_ne!(a, b);
    assert!(a.exists() && b.exists());
    tessera_ui::file_ops::save_to_path(&mut app, &temp.0.join("saved.tessera")).unwrap();
    assert!(a.exists());
    assert!(!b.exists());

    // The copies are recovered only once the instance that wrote them is
    // gone. While it runs they are its, and a second instance — the one a
    // double-clicked file starts — must leave them alone.
    let mut running = TesseraApp::headless();
    tessera_ui::recovery::recover_directory(&mut running, &temp.0);
    assert_eq!(running.documents.len(), 1);
    assert!(!running.active().dirty);

    drop(app);
    let mut recovered = TesseraApp::headless();
    tessera_ui::recovery::recover_directory(&mut recovered, &temp.0);
    assert_eq!(recovered.documents.len(), 1);
    assert!(recovered.active().dirty);
}

fn artifact(name: &str, bytes: &[u8]) {
    if let Some(directory) = std::env::var_os("TESSERA_REVIEW_ARTIFACTS") {
        let path = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join(name), bytes).unwrap();
    }
}

#[test]
fn startup_opens_multiple_paths_and_retains_an_error_for_an_unreadable_argument() {
    let temp = Scratch::new();
    let mut source = TesseraApp::headless();
    apply(&mut source, Command::AddRectangle(bounds()));
    let a = temp.0.join("first job.tessera");
    tessera_ui::file_ops::save_to_path(&mut source, &a).unwrap();
    apply(&mut source, Command::AddEllipse(bounds()));
    let b = temp.0.join("second job.tessera");
    tessera_ui::file_ops::save_to_path(&mut source, &b).unwrap();
    let mut app = TesseraApp::headless();
    tessera_ui::file_ops::open_startup_paths(&mut app, &[a, temp.0.join("missing.tessera"), b]);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.active().document().frames.len(), 2);
    assert!(
        app.status
            .as_ref()
            .unwrap()
            .message
            .contains("missing.tessera")
    );
}

#[test]
fn pdf_exports_all_pages_with_page_local_coordinates() {
    let mut app = TesseraApp::headless();
    let page = app.first_page_bounds();
    assert!(page.x > 0.0);
    let b = DocRect {
        x: page.x + 20.0,
        y: page.y + 20.0,
        width: 100.0,
        height: 50.0,
    };
    apply(&mut app, Command::AddRectangle(b));
    apply(&mut app, Command::AddPage);
    let doc = app.resolve_uncached();
    let pdf = tessera_pdf::export(&doc).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 2"));
    for page in &doc.pages {
        assert!(
            text.lines()
                .filter(|line| line.ends_with(" cm"))
                .any(|line| {
                    let values: Vec<f64> = line
                        .split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    values == vec![1.0, 0.0, 0.0, 1.0, -page.bounds.x, page.bounds.y]
                })
        );
    }
    artifact("pages-fixed.pdf", &pdf);
}

#[test]
fn pdf_uses_distinct_fonts_across_independently_shaped_frames() {
    let mut app = TesseraApp::headless();
    for (n, family) in ["sans-serif", "serif"].iter().enumerate() {
        let mut b = app.first_page_bounds();
        b.x += 20.0;
        b.y += 20.0 + n as f64 * 130.0;
        b.width = 200.0;
        b.height = 100.0;
        apply(&mut app, Command::AddTextFrame(b));
        let id = selected(&app);
        apply(
            &mut app,
            Command::SetText {
                id,
                text: "AB".into(),
            },
        );
        let FrameKind::Text { story, .. } = app.active().document().frame(id).unwrap().kind else {
            panic!()
        };
        apply(
            &mut app,
            Command::SetCharacterFormat {
                story,
                range: 0..2,
                format: CharacterFormat {
                    family: Some(family.to_string()),
                    ..Default::default()
                },
            },
        );
    }
    let pdf = tessera_pdf::export(&app.resolve_uncached()).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    let fonts: Vec<_> = text.lines().filter(|l| l.ends_with(" Tf")).collect();
    assert_eq!(fonts.len(), 2);
    assert_ne!(fonts[0], fonts[1]);
    artifact("fonts-fixed.pdf", &pdf);
}

#[test]
fn pdf_respects_image_fitting_and_strokes_and_rejects_alpha_in_x1a() {
    let temp = Scratch::new();
    let path = temp.0.join("alpha.png");
    image::RgbaImage::from_fn(4, 2, |_, y| {
        if y == 0 {
            image::Rgba([255, 0, 0, 128])
        } else {
            image::Rgba([0, 0, 255, 128])
        }
    })
    .save(&path)
    .unwrap();
    let mut app = TesseraApp::headless();
    let mut b = app.first_page_bounds();
    b.x += 20.0;
    b.y += 20.0;
    b.width = 100.0;
    b.height = 100.0;
    apply(&mut app, Command::AddGraphicFrame(b));
    let id = selected(&app);
    apply(
        &mut app,
        Command::PlaceArtwork {
            id,
            path,
            fit: tessera_document::graphic::Fit::Proportionally,
        },
    );
    let mut doc = app.resolve_uncached();
    let fitted = tessera_pdf::export(&doc).unwrap();
    artifact("image-fitted.pdf", &fitted);
    if let ResolvedKind::Graphic { inner, stroke, .. } = &mut doc.items[0].kind {
        *inner = Transform::translate(-40.0, -20.0);
        *stroke = Some(tessera_document::nodes::Stroke::new(
            tessera_color::Color::BLACK,
            9.0,
        ));
    }
    let changed = tessera_pdf::export(&doc).unwrap();
    assert_ne!(fitted, changed);
    assert!(String::from_utf8_lossy(&changed).contains("9 w"));
    let intent = tessera_document::intent::OutputIntent {
        description: "CMYK".into(),
        profile: include_bytes!("../../../assets/profiles/CGATS21_CRPC6.icc").to_vec(),
        rendering: Default::default(),
    };
    let options = tessera_pdf::ExportOptions {
        standard: tessera_pdf::Standard::X1a,
        intent: Some(intent),
        ..Default::default()
    };
    let error = tessera_pdf::export_with(&doc, &options).unwrap_err();
    assert!(error.to_string().contains("transparen"));
}

#[test]
fn pdf_keeps_the_stroke_of_a_filled_path() {
    let mut app = TesseraApp::headless();
    apply(&mut app, Command::AddRectangle(bounds()));
    let mut doc = app.resolve_uncached();
    let mut path = kurbo::BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((50.0, 0.0));
    path.line_to((25.0, 50.0));
    path.close_path();
    doc.items[0].kind = ResolvedKind::Path {
        path,
        fill: Some(Default::default()),
        stroke: Some(tessera_document::nodes::Stroke::new(
            tessera_color::Color::BLACK,
            9.0,
        )),
        text: None,
    };
    let pdf = tessera_pdf::export(&doc).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("9 w"));
    assert!(text.lines().any(|line| line == "S"));
}
