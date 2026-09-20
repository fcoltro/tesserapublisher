use tessera_document::{Document, Frame, FrameId, FrameKind, StoryId};
use tessera_geometry::{Anchor, DocRect, Transform};
use tessera_text::{shape::Shaper, story::Story};
use tessera_ui::{Command, TesseraApp, apply};

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("tessera-second-review-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn artifact(name: &str, bytes: &[u8]) {
    if let Some(directory) = std::env::var_os("TESSERA_REVIEW_ARTIFACTS") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}

fn rect(x: f64, y: f64) -> DocRect {
    DocRect {
        x,
        y,
        width: 100.0,
        height: 60.0,
    }
}
fn frame(bounds: DocRect, kind: FrameKind) -> Frame {
    Frame {
        bounds,
        kind,
        transform: Transform::IDENTITY,
        fill: Default::default(),
        stroke: None,
        wrap: Default::default(),
        blend: Default::default(),
        corners: Default::default(),
        shadow: None,
        anchor: None,
        style: None,
    }
}
fn add(doc: &mut Document, bounds: DocRect, kind: FrameKind) -> FrameId {
    doc.add_frame(doc.default_layer().unwrap(), frame(bounds, kind))
}
fn text(doc: &mut Document, bounds: DocRect, words: &str) -> FrameId {
    let mut s = Story::default();
    s.set_text(words);
    let id = doc.add_story(s);
    add(doc, bounds, FrameKind::text(id))
}
fn sid(doc: &Document, id: FrameId) -> StoryId {
    match doc.frame(id).unwrap().kind {
        FrameKind::Text { story, .. } => story,
        _ => panic!(),
    }
}
fn master_doc() -> (Document, FrameId, tessera_document::PageId) {
    let mut d = Document::new();
    let m = d.add_master("Master");
    let p = d.page_ids().next().unwrap();
    let mp = d.pages_of_master(m)[1];
    d.pages[p].master = Some(mp);
    let b = d.pages[mp].bounds;
    let id = add(&mut d, rect(b.x + 50.0, b.y + 50.0), FrameKind::Rectangle);
    (d, id, p)
}

#[test]
fn pdf_clear_fill_must_change_output() {
    let mut a = TesseraApp::headless();
    let b = a.first_page_bounds();
    apply(&mut a, Command::AddRectangle(rect(b.x + 50.0, b.y + 50.0)));
    let id = a.active().selection.single().unwrap();
    let opaque = tessera_pdf::export(&a.resolve_uncached()).unwrap();
    apply(&mut a, Command::ClearFill(id));
    let clear = tessera_pdf::export(&a.resolve_uncached()).unwrap();
    artifact("clear-fill-fixed.pdf", &clear);
    assert!(
        opaque != clear,
        "ClearFill produced byte-identical PDF to opaque black fill"
    );
}

#[test]
fn duplicate_page_must_remap_thread_and_share_copied_story() {
    let mut d = Document::new();
    let p = d.page_ids().next().unwrap();
    let b = d.pages[p].bounds;
    let a = text(
        &mut d,
        rect(b.x + 20.0, b.y + 20.0),
        "one two three four five six seven eight nine ten ",
    );
    let b = text(&mut d, rect(b.x + 20.0, b.y + 120.0), "");
    assert!(d.thread(a, b));
    let copy = d.duplicate_page(p).unwrap();
    let copied = d.frames_on_page(copy);
    assert_eq!(copied.len(), 2);
    assert_eq!(
        d.next_in_thread(copied[0]),
        Some(copied[1]),
        "copy still points into original thread"
    );
    assert_eq!(sid(&d, copied[0]), sid(&d, copied[1]));
}

#[test]
fn prepending_to_a_thread_must_update_every_story() {
    let mut d = Document::new();
    let p = d.first_page_bounds();
    let a = text(&mut d, rect(p.x + 20.0, 20.0), &"SOURCE ".repeat(80));
    let b = text(&mut d, rect(p.x + 20.0, 120.0), &"OLD ".repeat(80));
    let c = text(&mut d, rect(p.x + 20.0, 220.0), "");
    assert!(d.thread(b, c));
    assert!(d.thread(a, b));
    assert_eq!(
        sid(&d, a),
        sid(&d, c),
        "third frame still shows the old target story after joining A -> B -> C"
    );
}

#[test]
fn deleting_middle_frame_must_reconnect_thread() {
    let mut d = Document::new();
    let p = d.first_page_bounds();
    let a = text(&mut d, rect(p.x + 20.0, 20.0), &"SOURCE ".repeat(80));
    let b = text(&mut d, rect(p.x + 20.0, 120.0), "");
    let c = text(&mut d, rect(p.x + 20.0, 220.0), "");
    assert!(d.thread(a, b));
    assert!(d.thread(b, c));
    d.remove_frame(b);
    assert_eq!(
        d.thread_of(c),
        vec![a, c],
        "deleting middle frame leaves dangling predecessor and restarts tail at byte zero"
    );
}

#[test]
fn moving_page_must_preserve_rotated_objects_local_position() {
    let mut d = Document::new();
    let p = d.page_ids().next().unwrap();
    d.add_page();
    let old = d.pages[p].bounds;
    let id = add(
        &mut d,
        rect(old.x + 70.0, old.y + 70.0),
        FrameKind::Rectangle,
    );
    let center = d.frame(id).unwrap().bounds.center();
    d.frame_mut(id).unwrap().transform = Transform::rotate_about(90.0, center);
    let before = d.frame(id).unwrap().centre();
    d.move_page(p, 1);
    let new = d.pages[p].bounds;
    let actual = d.frame(id).unwrap().centre();
    let expected = (before.x + new.x - old.x, before.y + new.y - old.y);
    assert!(
        (actual.x - expected.0).abs() < 1e-6 && (actual.y - expected.1).abs() < 1e-6,
        "actual {actual:?}, expected {expected:?}"
    );
}

#[test]
fn group_inspector_transform_must_move_children() {
    let mut a = TesseraApp::headless();
    let p = a.first_page_bounds();
    apply(&mut a, Command::AddRectangle(rect(p.x + 40.0, 40.0)));
    let one = a.active().selection.single().unwrap();
    apply(&mut a, Command::AddRectangle(rect(p.x + 200.0, 40.0)));
    let two = a.active().selection.single().unwrap();
    a.active_mut().selection.replace_all(vec![one, two]);
    apply(&mut a, Command::GroupSelection);
    let g = a.active().selection.single().unwrap();
    let before = a.active().document().visual_bounds(g).unwrap();
    apply(
        &mut a,
        Command::TransformAbout {
            id: g,
            anchor: Anchor::Centre,
            scale: (2.0, 2.0),
            rotate: 0.0,
            shear: 0.0,
        },
    );
    let after = a.active().document().visual_bounds(g).unwrap();
    assert!(
        (after.width - before.width * 2.0).abs() < 1e-6,
        "group visible bounds unchanged: before {before:?}, after {after:?}"
    );
}

#[test]
fn align_page_must_use_current_page() {
    let mut a = TesseraApp::headless();
    apply(&mut a, Command::AddPage);
    let p = a.active().document().page_ids().nth(1).unwrap();
    let b = a.active().document().pages[p].bounds;
    a.active_mut().current_spread = 1;
    apply(&mut a, Command::AddRectangle(rect(b.x + 40.0, b.y + 40.0)));
    let id = a.active().selection.single().unwrap();
    apply(
        &mut a,
        Command::Align {
            edge: tessera_ui::align::Edge::Top,
            to: tessera_ui::align::AlignTo::Page,
        },
    );
    assert_eq!(
        a.active().document().visual_bounds(id).unwrap().y,
        b.y,
        "aligned to first page instead"
    );
}

#[test]
fn rotated_master_must_translate_after_rotation() {
    let (mut d, id, p) = master_doc();
    let b = d.frame(id).unwrap().bounds;
    d.frame_mut(id).unwrap().transform = Transform::rotate_about(90.0, b.center());
    let (_, dx, dy) = d.inherited_by(p)[0];
    let before = d.frame(id).unwrap().centre();
    let r = tessera_layout::resolve(&d, &mut Shaper::new());
    let item = r.items.iter().find(|i| i.frame == id).unwrap();
    let actual = item.transform.apply(item.bounds.center());
    let expected = (before.x + dx, before.y + dy);
    assert!(
        (actual.x - expected.0).abs() < 1e-6 && (actual.y - expected.1).abs() < 1e-6,
        "actual {actual:?}, expected {expected:?}"
    );
}

#[test]
fn hidden_master_layer_must_not_render() {
    let (mut d, id, _) = master_doc();
    let layer = d.layer_of_frame(id).unwrap();
    d.layers[layer].visible = false;
    let r = tessera_layout::resolve(&d, &mut Shaper::new());
    assert!(
        r.items.is_empty(),
        "hidden master contributes {} items",
        r.items.len()
    );
}

#[test]
fn duplicate_page_must_remap_anchored_objects() {
    let mut d = Document::new();
    let p = d.page_ids().next().unwrap();
    let b = d.pages[p].bounds;
    let t = text(&mut d, rect(b.x + 20.0, 20.0), "before \u{FFFC} after");
    let story = sid(&d, t);
    let picture = add(&mut d, rect(b.x + 200.0, 20.0), FrameKind::Rectangle);
    d.frame_mut(picture).unwrap().anchor =
        Some(tessera_document::anchored::Anchored::new(story, 0));
    let page = d.duplicate_page(p).unwrap();
    let copy = d
        .frames_on_page(page)
        .into_iter()
        .find(|id| matches!(d.frame(*id).unwrap().kind, FrameKind::Text { .. }))
        .unwrap();
    assert_eq!(
        d.anchors_in(sid(&d, copy)).frames.len(),
        1,
        "copied story has marker but no anchored object; old story owns two objects at index zero"
    );
}

#[test]
fn deleting_derived_style_must_preserve_inherited_formatting() {
    use tessera_text::story::{CharacterFormat, CharacterStyle};
    let mut d = Document::new();
    let b = d.first_page_bounds();
    let t = text(&mut d, rect(b.x + 20.0, 20.0), "styled");
    let story = sid(&d, t);
    let parent = d.add_character_style(CharacterStyle {
        name: "Parent".into(),
        based_on: None,
        format: CharacterFormat {
            size: Some(36.0),
            ..Default::default()
        },
    });
    let child = d.add_character_style(CharacterStyle {
        name: "Child".into(),
        based_on: Some(parent),
        format: Default::default(),
    });
    d.story_mut(story)
        .unwrap()
        .set_character_style(0..6, Some(child));
    let before = d
        .story(story)
        .unwrap()
        .resolve_run(&d.story(story).unwrap().runs[0], &d);
    let mut a = TesseraApp::headless();
    a.active_mut().replace_document(d);
    apply(&mut a, Command::DeleteCharacterStyle { id: child });
    let d = a.active().document();
    let after = d
        .story(story)
        .unwrap()
        .resolve_run(&d.story(story).unwrap().runs[0], d);
    assert_eq!(
        before.size, after.size,
        "delete flattens only local format, losing inherited 36pt"
    );
}

#[test]
fn same_file_alias_must_not_open_second_independent_tab() {
    let temp = Scratch::new();
    let root = &temp.0;
    std::fs::create_dir_all(root.join("alias")).unwrap();
    let path = root.join("alias-test.tsrdf");
    tessera_document::format::save(&Document::new(), &path).unwrap();
    let mut a = TesseraApp::headless();
    tessera_ui::file_ops::open_from_path(&mut a, &path).unwrap();
    tessera_ui::file_ops::open_from_path(&mut a, &root.join("alias/../alias-test.tsrdf")).unwrap();
    assert_eq!(
        a.documents.len(),
        1,
        "same file opened twice with independent histories and save paths"
    );
}

#[test]
fn save_as_refuses_another_open_files_alias_without_overwriting_it() {
    let temp = Scratch::new();
    std::fs::create_dir_all(temp.0.join("alias")).unwrap();
    let path = temp.0.join("saved.tsrdf");
    let mut a = TesseraApp::headless();
    apply(&mut a, Command::AddRectangle(rect(650.0, 20.0)));
    tessera_ui::file_ops::save_to_path(&mut a, &path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    tessera_ui::file_ops::new_document(&mut a);
    apply(&mut a, Command::AddEllipse(rect(650.0, 20.0)));
    let result = tessera_ui::file_ops::save_to_path(&mut a, &temp.0.join("alias/../saved.tsrdf"));
    assert!(matches!(
        result,
        Err(tessera_document::format::FormatError::AlreadyOpen(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert!(a.active().current_path.is_none() && a.active().dirty);
}

#[cfg(windows)]
#[test]
fn different_case_does_not_open_the_file_twice() {
    let temp = Scratch::new();
    let path = temp.0.join("MixedCase.tsrdf");
    tessera_document::format::save(&Document::new(), &path).unwrap();
    let mut a = TesseraApp::headless();
    tessera_ui::file_ops::open_from_path(&mut a, &path).unwrap();
    tessera_ui::file_ops::open_from_path(&mut a, &temp.0.join("mixedcase.TSRDF")).unwrap();
    assert_eq!(a.documents.len(), 1);
}

#[test]
fn deleting_paragraph_styles_preserves_character_precedence_and_descendants() {
    use tessera_text::story::{
        CharacterFormat, CharacterStyle, ParagraphFormat, ParagraphStyle, Styles,
    };
    let mut d = Document::new();
    let t = text(&mut d, rect(650.0, 20.0), "styled");
    let story = sid(&d, t);
    let parent = d.add_paragraph_style(ParagraphStyle {
        name: "Parent".into(),
        based_on: None,
        format: ParagraphFormat {
            character: CharacterFormat {
                size: Some(36.0),
                ..Default::default()
            },
            ..Default::default()
        },
    });
    let child = d.add_paragraph_style(ParagraphStyle {
        name: "Child".into(),
        based_on: Some(parent),
        format: Default::default(),
    });
    let grandchild = d.add_paragraph_style(ParagraphStyle {
        name: "Grandchild".into(),
        based_on: Some(child),
        format: Default::default(),
    });
    let character = d.add_character_style(CharacterStyle {
        name: "Override".into(),
        based_on: None,
        format: CharacterFormat {
            size: Some(18.0),
            ..Default::default()
        },
    });
    d.story_mut(story)
        .unwrap()
        .set_paragraph_style(0..6, Some(child));
    d.story_mut(story)
        .unwrap()
        .set_character_style(0..3, Some(character));
    let before: Vec<_> = d
        .story(story)
        .unwrap()
        .runs
        .iter()
        .map(|r| d.story(story).unwrap().resolve_run(r, &d))
        .collect();
    let mut a = TesseraApp::headless();
    a.active_mut().replace_document(d);
    a.active_mut().editing = Some((
        t,
        tessera_text::edit::EditBuffer::new(a.active().document().story(story).unwrap().clone()),
    ));
    apply(&mut a, Command::DeleteParagraphStyle { id: child });
    let d = a.active().document();
    let after: Vec<_> = d
        .story(story)
        .unwrap()
        .runs
        .iter()
        .map(|r| d.story(story).unwrap().resolve_run(r, d))
        .collect();
    assert_eq!(before, after);
    assert_eq!(d.paragraph_chain(grandchild).character.size, Some(36.0));
    assert_eq!(d.paragraph_styles[grandchild].based_on, Some(parent));
    assert_eq!(
        a.active().editing.as_ref().unwrap().1.story(),
        d.story(story).unwrap()
    );
    apply(&mut a, Command::Undo);
    assert!(a.active().document().paragraph_styles.contains_key(child));
}

#[test]
fn deleting_a_parent_character_style_keeps_derived_styles_and_ancestor_updates() {
    use tessera_text::story::{CharacterFormat, CharacterStyle, Styles};
    let mut d = Document::new();
    let grandparent = d.add_character_style(CharacterStyle {
        name: "Grandparent".into(),
        based_on: None,
        format: CharacterFormat {
            size: Some(36.0),
            ..Default::default()
        },
    });
    let parent = d.add_character_style(CharacterStyle {
        name: "Parent".into(),
        based_on: Some(grandparent),
        format: CharacterFormat {
            colour: Some(tessera_color::Color::WHITE),
            ..Default::default()
        },
    });
    let child = d.add_character_style(CharacterStyle {
        name: "Child".into(),
        based_on: Some(parent),
        format: Default::default(),
    });
    let before = d.character_chain(child);
    let mut a = TesseraApp::headless();
    a.active_mut().replace_document(d);
    apply(&mut a, Command::DeleteCharacterStyle { id: parent });
    assert_eq!(a.active().document().character_chain(child), before);
    assert_eq!(
        a.active().document().character_styles[child].based_on,
        Some(grandparent)
    );
    let mut changed = a.active().document().character_styles[grandparent].clone();
    changed.format.size = Some(42.0);
    apply(
        &mut a,
        Command::EditCharacterStyle {
            id: grandparent,
            style: changed,
        },
    );
    assert_eq!(
        a.active().document().character_chain(child).size,
        Some(42.0)
    );
}

#[test]
fn copied_page_preserves_layers_shared_stories_and_master_overrides() {
    let (mut d, master_item, p) = master_doc();
    let local = d.override_master_item(p, master_item).unwrap();
    let layer = d.layer_of_frame(local).unwrap();
    let first = text(&mut d, rect(650.0, 200.0), "shared story");
    let second = text(&mut d, rect(650.0, 300.0), "");
    let other_layer = d.add_layer("Other");
    d.move_frames_to_layer(&[second], other_layer);
    d.thread(first, second);
    let copy = d.duplicate_page(p).unwrap();
    let copies = d.frames_on_page(copy);
    let overridden = copies
        .iter()
        .copied()
        .find(|id| d.overrides.get(*id) == Some(&master_item))
        .unwrap();
    assert_eq!(d.layer_of_frame(overridden), Some(layer));
    assert!(d.inherited_by(copy).is_empty());
    let copied_first = copies
        .iter()
        .copied()
        .find(|id| d.next_in_thread(*id).is_some())
        .unwrap();
    let copied_second = d.next_in_thread(copied_first).unwrap();
    assert!(copies.contains(&copied_second));
    assert_eq!(sid(&d, copied_first), sid(&d, copied_second));
    assert_eq!(d.layer_of_frame(copied_second), Some(other_layer));
}

#[test]
fn deleting_a_group_of_threaded_frames_reconnects_the_surviving_ends() {
    let mut d = Document::new();
    let ids: Vec<_> = (0..4)
        .map(|i| text(&mut d, rect(650.0, 20.0 + i as f64 * 100.0), "story"))
        .collect();
    for pair in ids.windows(2) {
        d.thread(pair[0], pair[1]);
    }
    let group = d.group(&ids[1..3]).unwrap();
    d.remove_frame(group);
    assert_eq!(d.thread_of(ids[0]), vec![ids[0], ids[3]]);
    assert_eq!(d.next_in_thread(ids[0]), Some(ids[3]));
}

#[test]
fn pdf_multiplies_fill_and_stroke_alpha_by_object_opacity() {
    use tessera_color::Color;
    let mut d = Document::new();
    let id = add(&mut d, rect(662.0, 50.0), FrameKind::Rectangle);
    let f = d.frame_mut(id).unwrap();
    f.fill = tessera_document::paint::Paint::Solid(Color::Rgb {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 0.5,
    });
    f.stroke = Some(tessera_document::nodes::Stroke::new(
        Color::Rgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 0.8,
        },
        10.0,
    ));
    f.blend.opacity = 0.5;
    let resolved = tessera_layout::resolve(&d, &mut Shaper::new());
    let pdf = tessera_pdf::export(&resolved).unwrap();
    let output = String::from_utf8_lossy(&pdf);
    assert!(output.contains("/ca 0.25") && output.contains("/CA 0.4"));
    artifact("paint-alpha-fixed.pdf", &pdf);
    let options = tessera_pdf::ExportOptions {
        standard: tessera_pdf::Standard::X1a,
        intent: Some(tessera_document::intent::OutputIntent {
            description: "CMYK".into(),
            profile: include_bytes!("../../../assets/profiles/CGATS21_CRPC6.icc").to_vec(),
            rendering: Default::default(),
        }),
        ..Default::default()
    };
    assert!(tessera_pdf::export_with(&resolved, &options).is_err());
    // Even on an opaque object, fractional paint alpha prevents an X-1a claim.
    d.frame_mut(id).unwrap().blend.opacity = 1.0;
    assert!(
        tessera_pdf::export_with(&tessera_layout::resolve(&d, &mut Shaper::new()), &options)
            .is_err()
    );
}

#[test]
fn gradient_alpha_has_a_luminosity_mask_and_does_not_fade_the_stroke() {
    use tessera_color::Color;
    use tessera_document::paint::{Gradient, Paint, Ramp, Stop};
    let mut d = Document::new();
    let id = add(&mut d, rect(662.0, 50.0), FrameKind::Rectangle);
    let f = d.frame_mut(id).unwrap();
    f.fill = Paint::Gradient(Gradient::new(
        Ramp::Linear { angle: 0.0 },
        vec![
            Stop {
                at: 0.0,
                colour: Color::Rgb {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                },
            },
            Stop {
                at: 1.0,
                colour: Color::BLACK,
            },
        ],
    ));
    f.stroke = Some(tessera_document::nodes::Stroke::new(Color::BLACK, 6.0));
    let pdf = tessera_pdf::export(&tessera_layout::resolve(&d, &mut Shaper::new())).unwrap();
    let output = String::from_utf8_lossy(&pdf);
    assert!(output.contains("/S /Luminosity") && output.contains("/SMask"));
    artifact("gradient-alpha-fixed.pdf", &pdf);
}
