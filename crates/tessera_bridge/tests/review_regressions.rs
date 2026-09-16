use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tessera_bridge::assistant::{Provider, Session, Transport};
use tessera_document::{Document, Frame, FrameId, FrameKind, StoryId};
use tessera_geometry::{DocRect, Transform};
use tessera_text::story::{CharacterFormat, CharacterStyle, Story};
use tessera_text::variables::{Marker, Variables};
use tessera_ui::{Command, TesseraApp, apply};

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn in_directory(root: std::path::PathBuf) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(
            "tessera-review-regression-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn new() -> Self {
        Self::in_directory(std::env::temp_dir())
    }
    fn relative() -> Self {
        Self::in_directory(std::path::PathBuf::from("target"))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rect() -> DocRect {
    DocRect {
        x: 630.,
        y: 20.,
        width: 200.,
        height: 100.,
    }
}
fn add_text(d: &mut Document, s: Story) -> FrameId {
    let story = d.add_story(s);
    d.add_frame(
        d.default_layer().unwrap(),
        Frame {
            bounds: rect(),
            kind: FrameKind::text(story),
            transform: Transform::IDENTITY,
            fill: Default::default(),
            stroke: None,
            wrap: Default::default(),
            blend: Default::default(),
            corners: Default::default(),
            shadow: None,
            anchor: None,
            style: None,
        },
    )
}
fn sid(d: &Document, id: FrameId) -> StoryId {
    match d.frames[id].kind {
        FrameKind::Text { story, .. } => story,
        _ => panic!(),
    }
}
fn tool(a: &mut TesseraApp, name: &str, args: Value) -> Value {
    tessera_bridge::tools::call(a, name, &args).unwrap()
}
fn provider() -> Provider {
    Provider::OpenAiCompatible {
        api_key: String::new(),
        model: "synthetic".into(),
        base_url: "http://unused.invalid".into(),
    }
}
fn tool_reply(name: &str, arguments: Value) -> String {
    json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"call1","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]}}]}).to_string()
}

#[test]
fn preference_tool_must_not_return_api_key() {
    let mut a = TesseraApp::headless();
    a.prefs.assistant.api_key = "SYNTHETIC-SECRET-FOR-REVIEW".into();
    let v = tool(&mut a, "get_preferences", json!({}));
    assert!(
        v["assistant"]["api_key"].as_str().is_none_or(str::is_empty),
        "get_preferences returned the synthetic credential"
    );
}

struct CancelOnReply(Arc<AtomicBool>);
impl Transport for CancelOnReply {
    fn post(&self, _: &str, _: &[(String, String)], _: &str) -> Result<String, String> {
        self.0.store(true, Ordering::SeqCst);
        Ok(tool_reply("delete_frame", json!({"frame":1})))
    }
}
#[test]
fn stop_during_http_must_prevent_subsequent_tool_execution() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut session = Session::new(
        provider(),
        String::new(),
        vec![],
        Box::new(CancelOnReply(cancel.clone())),
    );
    session.cancel = Some(cancel);
    let mut ran = 0;
    let result = session.turn(
        "stop before applying",
        |_| {
            ran += 1;
            ("ok".into(), false)
        },
        |_| {},
    );
    assert_eq!(result.unwrap_err(), "stopped");
    assert_eq!(ran, 0, "a tool still ran after cancellation during HTTP");
}

struct Gate {
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
    count: Mutex<usize>,
    reply: String,
}
impl Transport for Gate {
    fn post(&self, _: &str, _: &[(String, String)], _: &str) -> Result<String, String> {
        let mut count = self.count.lock().unwrap();
        *count += 1;
        if *count == 1 {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(self.reply.clone())
        } else {
            Ok(json!({"choices":[{"message":{"content":"done"}}]}).to_string())
        }
    }
}
#[test]
fn pending_console_edit_must_not_land_in_another_tab() {
    let mut a = TesseraApp::headless();
    a.prefs.assistant.provider = "openai".into();
    a.prefs.assistant.model = "synthetic".into();
    let source_key = a.active;
    let first = tool(
        &mut a,
        "add_text_frame",
        json!({"x":630,"y":20,"width":100,"height":100,"text":"source"}),
    )["frame"]
        .as_u64()
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let transport = Arc::new(Mutex::new(Some(Gate {
        entered: entered_tx,
        release: Mutex::new(release_rx),
        count: Mutex::new(0),
        reply: tool_reply("set_text", json!({"frame":first,"text":"MODEL EDIT"})),
    })));
    let mut driver = tessera_bridge::console::Driver::new(
        Arc::new(move || Box::new(transport.lock().unwrap().take().unwrap())),
        Arc::new(|| {}),
    );
    a.console
        .heard(tessera_ui::view::console::Line::You("edit source".into()));
    a.console.outbox.push("edit source".into());
    driver.pump(&mut a);
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    tessera_ui::file_ops::new_document(&mut a);
    let second = tool(
        &mut a,
        "add_text_frame",
        json!({"x":630,"y":20,"width":100,"height":100,"text":"unrelated"}),
    )["frame"]
        .as_u64()
        .unwrap();
    assert_eq!(first, second, "slot-map keys overlap between documents");
    release_tx.send(()).unwrap();
    let start = std::time::Instant::now();
    while driver.busy() {
        driver.pump(&mut a);
        assert!(start.elapsed().as_secs() < 5);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_ne!(a.active, source_key);
    let actual = tool(&mut a, "get_text", json!({"frame":second}));
    assert_eq!(
        actual["text"], "unrelated",
        "the source document's edit modified the other tab"
    );
}

#[test]
fn set_text_must_synchronise_live_edit_buffer() {
    let mut a = TesseraApp::headless();
    apply(&mut a, Command::AddTextFrame(rect()));
    let id = a.active().selection.single().unwrap();
    apply(
        &mut a,
        Command::SetText {
            id,
            text: "old".into(),
        },
    );
    let story = a
        .active()
        .document()
        .story(sid(a.active().document(), id))
        .unwrap()
        .clone();
    a.active_mut().editing = Some((id, tessera_text::edit::EditBuffer::new(story)));
    tool(
        &mut a,
        "set_text",
        json!({"frame":tessera_bridge::frame_key(id),"text":"replacement"}),
    );
    assert!(
        a.active()
            .editing
            .as_ref()
            .is_none_or(|(_, b)| b.story().text == "replacement"),
        "the next keystroke will write the old story back"
    );
}

#[test]
fn paragraph_style_tool_must_reject_mid_utf8_offset_without_panicking() {
    let mut a = TesseraApp::headless();
    let frame = tool(
        &mut a,
        "add_text_frame",
        json!({"x":630,"y":20,"width":100,"height":100,"text":"éclair"}),
    )["frame"]
        .clone();
    tool(&mut a, "define_paragraph_style", json!({"name":"Body"}));
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tessera_bridge::tools::call(
            &mut a,
            "apply_paragraph_style",
            &json!({"frame":frame,"style":"Body","start":1,"end":2}),
        )
    }));
    assert!(
        attempt.is_ok(),
        "invalid tool byte offsets panicked on the UI thread"
    );
}

#[test]
fn cross_document_copy_must_preserve_variable_value() {
    let mut source = Document::new();
    source.set_variables(vec![tessera_document::variables::TextVariable::custom(
        "Brand", "ACME",
    )]);
    let id = add_text(
        &mut source,
        Story::new(Marker::Variable(0).character().to_string()),
    );
    let mut dest = Document::new();
    dest.set_variables(vec![tessera_document::variables::TextVariable::custom(
        "Other", "WRONG",
    )]);
    let copied = dest
        .import_frames(&source, &[id], dest.default_layer().unwrap(), 0., 0., false)
        .unwrap()[0];
    let vars = Variables {
        variables: dest
            .variables
            .iter()
            .map(|v| match &v.kind {
                tessera_document::variables::VariableKind::Custom(s) => s.clone(),
                _ => String::new(),
            })
            .collect(),
        ..Default::default()
    };
    let actual =
        tessera_text::variables::expand(&dest.story(sid(&dest, copied)).unwrap().text, Some(&vars));
    assert_eq!(actual, "ACME");
}

#[test]
fn cross_document_copy_must_remap_footnote_styles() {
    let mut source = Document::new();
    let style = source.add_character_style(CharacterStyle {
        name: "Note".into(),
        based_on: None,
        format: CharacterFormat {
            size: Some(42.),
            ..Default::default()
        },
    });
    let mut body = Story::new(format!("body{}", Marker::FootnoteReference.character()));
    body.footnotes[0].set_text("note");
    body.footnotes[0].runs[0].style = Some(style);
    let id = add_text(&mut source, body);
    let mut dest = Document::new();
    let other = dest.add_character_style(CharacterStyle {
        name: "Other".into(),
        based_on: None,
        format: CharacterFormat {
            size: Some(8.),
            ..Default::default()
        },
    });
    assert_eq!(style, other);
    let copy = dest
        .import_frames(&source, &[id], dest.default_layer().unwrap(), 0., 0., false)
        .unwrap()[0];
    let note = &dest.story(sid(&dest, copy)).unwrap().footnotes[0];
    assert_eq!(note.resolve_run(&note.runs[0], &dest).size, Some(42.));
}

#[test]
fn dropping_older_listener_must_keep_newer_port_record() {
    let scratch = Scratch::new();
    let path = scratch.0.join("bridge-test.port");
    let old = tessera_bridge::live::Listener::start(path.clone(), || {}).unwrap();
    let latest = tessera_bridge::live::Listener::start(path.clone(), || {}).unwrap();
    let port = latest.port();
    drop(old);
    assert_eq!(tessera_bridge::live::recorded_port(&path), Some(port));
}

#[test]
fn grayscale_jpeg_must_not_be_declared_rgb() {
    let scratch = Scratch::new();
    let path = &scratch.0.join("gray.jpg");
    image::GrayImage::from_pixel(4, 4, image::Luma([100]))
        .save(path)
        .unwrap();
    let mut a = TesseraApp::headless();
    apply(&mut a, Command::AddGraphicFrame(rect()));
    let id = a.active().selection.single().unwrap();
    apply(
        &mut a,
        Command::PlaceArtwork {
            id,
            path: path.to_path_buf(),
            fit: tessera_document::graphic::Fit::Proportionally,
        },
    );
    let bytes = tessera_pdf::export(&a.resolve_uncached()).unwrap();
    if let Some(out) = std::env::var_os("TESSERA_REVIEW_ARTIFACTS") {
        let out = std::path::PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("gray.pdf"), &bytes).unwrap();
    }
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("/DCTDecode") || !s.contains("/ColorSpace /DeviceRGB"),
        "a single-component JPEG was passed through as three-component DeviceRGB"
    );
}

#[test]
fn merging_into_an_existing_span_must_not_corrupt_table() {
    let mut a = TesseraApp::headless();
    apply(
        &mut a,
        Command::AddTable {
            bounds: rect(),
            rows: 1,
            columns: 3,
        },
    );
    let id = a.active().selection.single().unwrap();
    apply(
        &mut a,
        Command::MergeCells {
            id,
            row: 0,
            column: 1,
            span: tessera_document::table::Span {
                rows: 1,
                columns: 2,
            },
        },
    );
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        apply(
            &mut a,
            Command::MergeCells {
                id,
                row: 0,
                column: 0,
                span: tessera_document::table::Span {
                    rows: 1,
                    columns: 2,
                },
            },
        )
    }));
    assert!(
        attempt.is_ok(),
        "merging the first cell with its already-merged right neighbour panics"
    );
    let FrameKind::Table(table) = &a.active().document().frames[id].kind else {
        panic!()
    };
    assert!(table.spans_are_sound());
}

fn docx(entries: &[(&str, &str)]) -> tessera_import::docx::Imported {
    use std::io::Write;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(body.as_bytes()).unwrap();
    }
    tessera_import::docx::import_bytes(
        archive.finish().unwrap().into_inner(),
        std::path::Path::new("synthetic.docx"),
    )
    .unwrap()
}
#[test]
fn word_character_styles_must_survive_paragraph_boundaries() {
    let imported = docx(&[
        (
            "word/document.xml",
            r#"<w:document xmlns:w="urn:word"><w:body><w:p><w:r><w:rPr><w:rStyle w:val="A"/></w:rPr><w:t>one</w:t></w:r></w:p><w:p><w:r><w:rPr><w:rStyle w:val="B"/></w:rPr><w:t>two</w:t></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="urn:word"><w:style w:type="character" w:styleId="A"><w:name w:val="First"/><w:rPr><w:sz w:val="40"/></w:rPr></w:style><w:style w:type="character" w:styleId="B"><w:name w:val="Second"/><w:rPr><w:sz w:val="60"/></w:rPr></w:style></w:styles>"#,
        ),
    ]);
    let mut a = TesseraApp::headless();
    apply(&mut a, Command::AddTextFrame(rect()));
    let id = a.active().selection.single().unwrap();
    apply(
        &mut a,
        Command::PlaceText {
            id: Some(id),
            text: tessera_ui::command::PlacedText {
                story: imported.story,
                paragraph_styles: imported.paragraph_styles,
                character_styles: imported.character_styles,
                paragraph_style_names: imported.paragraph_style_names,
                run_style_names: imported.run_style_names,
            },
        },
    );
    let d = a.active().document();
    let s = d.story(sid(d, id)).unwrap();
    assert_eq!(s.text, "one\ntwo");
    let run = s.runs.iter().find(|r| r.range.contains(&4)).unwrap();
    assert_eq!(
        s.resolve_run(run, d).size,
        Some(30.),
        "the second named style was assigned to the synthetic newline instead of its text"
    );
}

#[test]
fn relative_placed_image_must_survive_save_and_reopen() {
    let scratch = Scratch::relative();
    let path = &scratch.0.join("relative.png");
    image::RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]))
        .save(path)
        .unwrap();
    let mut a = TesseraApp::headless();
    tool(
        &mut a,
        "place_image",
        json!({"path":path,"x":630,"y":20,"width":100,"height":100}),
    );
    let file = &scratch.0.join("relative.tessera");
    tessera_ui::file_ops::save_to_path(&mut a, file).unwrap();
    let reopened = tessera_document::format::load(file).unwrap();
    let link = reopened.links.values().next().unwrap();
    assert!(
        link.path.exists(),
        "reopening resolved a CWD-relative placement against the document directory: {}",
        link.path.display()
    );
}
