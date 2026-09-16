//! The dialogs, as tools — and the read side.
//!
//! A menu action that opens a dialog opens it on screen, where a model
//! cannot click. So each dialog's *choices* are a tool here, taking what
//! the dialog's fields take and doing what its OK button does, through the
//! same function. New document, export with its marks and standard, find
//! and change, step and repeat, spelling, package. Beside them, what a
//! model needs to read that no command gives back: the whole document as
//! JSON, a frame's laid-out lines, the preflight report, the fonts.

use serde_json::{Value, json};
use tessera_document::nodes::FrameKind;
use tessera_ui::TesseraApp;
use tessera_ui::command::Command;

use crate::tools::{Tool, frame_arg, number, text, text_frame_arg};
use crate::{frame_key, run};

pub(crate) static ALL: [Tool; 12] = [
    Tool {
        name: "new_document",
        description: "Make a new document and open it, as File ▸ New does: page size in points \
            (A4 is 595.28 × 841.89, US Letter 612 × 792), facing pages, page count, margin and \
            bleed, and whether it is for print (CMYK, an output intent) or screen (RGB). Only \
            the fields given change from the dialog's defaults.",
        arguments: &[
            ("width", "number", "Page width in points.", false),
            ("height", "number", "Page height in points.", false),
            ("landscape", "boolean", "Turn the page on its side.", false),
            (
                "facing_pages",
                "boolean",
                "Spreads of two, as a book; default true.",
                false,
            ),
            ("pages", "integer", "How many pages; default 1.", false),
            (
                "margin",
                "number",
                "Margin on every side, in points.",
                false,
            ),
            ("bleed", "number", "Bleed on every side, in points.", false),
            ("intent", "string", "print or screen.", false),
            (
                "minimum_ppi",
                "number",
                "Below which artwork is reported as too low.",
                false,
            ),
        ],
        run: new_document,
    },
    Tool {
        name: "export_pdf",
        description: "Export the document as a PDF, as File ▸ Export does: a standard (plain, \
            x1a or x4), and printer's marks — crop, bleed, registration, colour bar — offset \
            from the trim. Or name one of the saved presets. The output intent comes from the \
            document.",
        arguments: &[
            ("path", "string", "Where to write the PDF.", true),
            (
                "preset",
                "string",
                "A saved preset's name; see get_preferences export_presets.",
                false,
            ),
            ("standard", "string", "plain, x1a or x4.", false),
            ("crop", "boolean", "Crop marks.", false),
            ("bleed", "boolean", "Bleed marks.", false),
            ("registration", "boolean", "Registration marks.", false),
            ("colour_bar", "boolean", "A colour bar.", false),
            (
                "offset",
                "number",
                "Marks' distance from the trim, in points.",
                false,
            ),
        ],
        run: export_pdf,
    },
    Tool {
        name: "find_text",
        description: "Find every occurrence of a text in the document's stories — as Edit ▸ \
            Find and Change does — with the frame, story and byte range of each. Give \
            `replace` to change them all in one undo entry.",
        arguments: &[
            ("query", "string", "The text to find.", true),
            (
                "replace",
                "string",
                "Text to put in place of every match.",
                false,
            ),
            (
                "match_case",
                "boolean",
                "Whether case must match; default false.",
                false,
            ),
            (
                "whole_word",
                "boolean",
                "Whether only whole words match; default false.",
                false,
            ),
        ],
        run: find_text,
    },
    Tool {
        name: "step_and_repeat",
        description: "Copy the selection `copies` times, each moved `dx`, `dy` from the last — \
            Object ▸ Step and repeat. Select the frames first.",
        arguments: &[
            ("copies", "integer", "How many copies.", true),
            ("dx", "number", "Horizontal step in points.", true),
            ("dy", "number", "Vertical step in points.", true),
        ],
        run: step_and_repeat,
    },
    Tool {
        name: "check_spelling",
        description: "The words no dictionary knows, with where each is, its context and its \
            suggestions — the walk Edit ▸ Spelling ▸ Check spelling makes, all at once. A \
            language with no dictionary in the dictionaries folder is not checked. Fix a word \
            with edit_text or find_text; vouch for one with add_to_dictionary.",
        arguments: &[
            (
                "frame",
                "integer",
                "Check only this text frame's story; every story if absent.",
                false,
            ),
            ("limit", "integer", "At most this many; default 50.", false),
        ],
        run: check_spelling,
    },
    Tool {
        name: "add_to_dictionary",
        description: "Vouch for a word from now on, in every language, and remember it.",
        arguments: &[("word", "string", "The word.", true)],
        run: |state, arguments| {
            let word = text(arguments, "word")?;
            state.dictionaries.add(&word);
            Ok(json!({ "added": word }))
        },
    },
    Tool {
        name: "preflight",
        description: "The preflight report: every problem the document has for print — missing \
            links, low-resolution artwork, overset text, text outside the page, RGB in a CMYK \
            job — with its severity and where it is.",
        arguments: &[],
        run: preflight,
    },
    Tool {
        name: "package",
        description: "Collect the job into a folder of its own inside `folder` — the document, \
            its links and a summary with the preflight report — as File ▸ Package does.",
        arguments: &[("folder", "string", "The folder to package into.", true)],
        run: |state, arguments| {
            let folder = std::path::PathBuf::from(text(arguments, "folder")?);
            let message = tessera_ui::file_ops::package_into(state, &folder)?;
            Ok(json!({ "packaged": message }))
        },
    },
    Tool {
        name: "edit_text",
        description: "Replace a byte range of a text frame's text with new text — insert when \
            start equals end, delete when the new text is empty. Offsets are bytes into the \
            story as get_text shows it; paragraphs are separated by newlines.",
        arguments: &[
            ("frame", "integer", "The frame's number.", true),
            ("start", "integer", "Byte offset the range starts at.", true),
            ("end", "integer", "Byte offset the range ends at.", true),
            ("text", "string", "What to put there.", true),
        ],
        run: edit_text,
    },
    Tool {
        name: "document_json",
        description: "The whole document as the JSON it is saved in: pages, spreads, layers, \
            frames, stories with their runs and formats, styles, swatches, masters, sections, \
            variables, links. Large; ask for describe_document first.",
        arguments: &[],
        run: |state, _| serde_json::to_value(state.active().document()).map_err(|e| e.to_string()),
    },
    Tool {
        name: "frame_layout",
        description: "How a text frame's text was laid out: each line's baseline, the text on \
            it, the byte range it holds, and its left and right extents — plus the overset \
            count. What to read before deciding whether a line break is where it should be.",
        arguments: &[("frame", "integer", "The frame's number.", true)],
        run: frame_layout,
    },
    Tool {
        name: "list_fonts",
        description: "The font families installed on this machine, by the names \
            CharacterFormat.family takes.",
        arguments: &[],
        run: |state, _| Ok(json!({ "families": state.shaper.families() })),
    },
];

fn new_document(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    use tessera_ui::view::new_document::{Intent, NewDocument};
    let mut settings = NewDocument::default();
    let f = |key: &str| arguments.get(key).and_then(Value::as_f64);
    let b = |key: &str| arguments.get(key).and_then(Value::as_bool);
    if let Some(w) = f("width") {
        settings.width = w;
        settings.preset = None;
    }
    if let Some(h) = f("height") {
        settings.height = h;
        settings.preset = None;
    }
    if b("landscape") == Some(true) {
        settings.orientation = tessera_document::nodes::Orientation::Landscape;
    }
    if let Some(v) = b("facing_pages") {
        settings.facing_pages = v;
    }
    if let Some(n) = arguments.get("pages").and_then(Value::as_u64) {
        settings.pages = n.max(1) as u32;
    }
    if let Some(m) = f("margin") {
        settings.margin = m;
    }
    if let Some(m) = f("bleed") {
        settings.bleed = m;
    }
    if let Some(ppi) = f("minimum_ppi") {
        settings.minimum_ppi = ppi;
    }
    match arguments.get("intent").and_then(Value::as_str) {
        None => {}
        Some("print") => settings.intent = Intent::Print,
        Some("screen") => settings.intent = Intent::Screen,
        Some(other) => return Err(format!("intent {other:?} is not print or screen")),
    }
    settings.open = false;
    state.new_document = settings;
    tessera_ui::view::new_document::create(state);
    let doc = state.active().document();
    Ok(json!({
        "pages": doc.page_ids().count(),
        "page": doc.page_ids().next().map(|p| {
            let b = doc.pages[p].bounds;
            json!({ "x": b.x, "y": b.y, "width": b.width, "height": b.height })
        }),
        "revision": doc.revision(),
    }))
}

fn export_pdf(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    use tessera_pdf::Standard;
    let path = std::path::PathBuf::from(text(arguments, "path")?);
    if let Some(name) = arguments.get("preset").and_then(Value::as_str) {
        let preset = state
            .prefs
            .export_presets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "no export preset named {name:?}; there are: {}",
                    state
                        .prefs
                        .export_presets
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        state.export.adopt(&preset);
    }
    match arguments.get("standard").and_then(Value::as_str) {
        None => {}
        Some("plain") => state.export.standard = Standard::Plain,
        Some("x1a") | Some("x-1a") => state.export.standard = Standard::X1a,
        Some("x4") | Some("x-4") => state.export.standard = Standard::X4,
        Some(other) => return Err(format!("standard {other:?} is not plain, x1a or x4")),
    }
    let b = |key: &str| arguments.get(key).and_then(Value::as_bool);
    if let Some(v) = b("crop") {
        state.export.marks.crop = v;
    }
    if let Some(v) = b("bleed") {
        state.export.marks.bleed = v;
    }
    if let Some(v) = b("registration") {
        state.export.marks.registration = v;
    }
    if let Some(v) = b("colour_bar") {
        state.export.marks.colour_bar = v;
    }
    if let Some(v) = arguments.get("offset").and_then(Value::as_f64) {
        state.export.marks.offset = v;
    }
    tessera_ui::file_ops::export_pdf_to_path(state, &path).map_err(|e| e.to_string())?;
    Ok(json!({
        "exported": path,
        "standard": format!("{:?}", state.export.standard),
        "marks": {
            "crop": state.export.marks.crop,
            "bleed": state.export.marks.bleed,
            "registration": state.export.marks.registration,
            "colour_bar": state.export.marks.colour_bar,
            "offset": state.export.marks.offset,
        },
    }))
}

fn find_text(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    use tessera_ui::find::{Query, edits_for, search};
    let query = Query {
        needle: text(arguments, "query")?,
        match_case: arguments
            .get("match_case")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        whole_word: arguments
            .get("whole_word")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    if query.needle.is_empty() {
        return Err("query must not be empty".into());
    }
    let hits = search(state.active().document(), &query);
    let listed: Vec<Value> = hits
        .iter()
        .map(|h| {
            json!({
                "frame": frame_key(h.frame),
                "story": slotmap::Key::data(&h.story).as_ffi(),
                "cell": h.cell,
                "start": h.range.start,
                "end": h.range.end,
            })
        })
        .collect();
    let mut out = json!({ "count": hits.len(), "matches": listed });
    if let Some(replacement) = arguments.get("replace").and_then(Value::as_str) {
        if hits.is_empty() {
            out["replaced"] = json!(0);
        } else {
            let edits = edits_for(&hits, replacement);
            let n = edits.len();
            state.active_mut().editing = None;
            let outcome = run(state, Command::ReplaceMatches { edits });
            out["replaced"] = json!(n);
            out["revision"] = outcome["revision"].clone();
        }
    }
    Ok(out)
}

fn step_and_repeat(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let copies = arguments
        .get("copies")
        .and_then(Value::as_u64)
        .ok_or("copies must be a whole number")? as usize;
    let (dx, dy) = (number(arguments, "dx")?, number(arguments, "dy")?);
    if state.active().selection.is_empty() {
        return Err("nothing is selected; select the frames to repeat first".into());
    }
    Ok(run(state, Command::StepAndRepeat { copies, dx, dy }))
}

fn check_spelling(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let story = match arguments.get("frame") {
        Some(_) => Some(text_frame_arg(state, arguments)?.1),
        None => None,
    };
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(50, |n| n as usize);
    let findings = tessera_ui::view::spelling::findings(state, story, limit);
    let listed: Vec<Value> = findings
        .iter()
        .map(|f| {
            json!({
                "word": f.word,
                "story": slotmap::Key::data(&f.story).as_ffi(),
                "start": f.range.start,
                "end": f.range.end,
                "context": f.context,
                "language": f.language,
                "suggestions": f.suggestions,
            })
        })
        .collect();
    Ok(json!({ "count": listed.len(), "findings": listed }))
}

fn preflight(state: &mut TesseraApp, _: &Value) -> Result<Value, String> {
    let report = tessera_ui::preflight::Preflight::report(state).clone();
    let problems: Vec<Value> = report
        .problems
        .iter()
        .map(|p| {
            let at = match &p.at {
                tessera_preflight::Where::Frame(id) => json!({ "frame": frame_key(*id) }),
                tessera_preflight::Where::Page(page) => {
                    let index = state
                        .active()
                        .document()
                        .page_ids()
                        .position(|q| q == *page);
                    json!({ "page": index })
                }
                other => json!(format!("{other:?}")),
            };
            json!({
                "rule": format!("{:?}", p.rule),
                "severity": format!("{:?}", p.severity()),
                "message": p.message,
                "at": at,
            })
        })
        .collect();
    Ok(json!({
        "errors": report.errors(),
        "warnings": report.warnings(),
        "problems": problems,
    }))
}

fn edit_text(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let (_, story) = text_frame_arg(state, arguments)?;
    let length = state
        .active()
        .document()
        .story(story)
        .map_or(0, |s| s.text.len());
    let at = |key: &str| -> Result<usize, String> {
        let n = arguments
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{key} must be a byte offset"))? as usize;
        if n > length {
            return Err(format!(
                "{key} {n} is past the end of the text ({length} bytes)"
            ));
        }
        Ok(n)
    };
    let (start, end) = (at("start")?, at("end")?);
    if end < start {
        return Err("end is before start".into());
    }
    let replacement = text(arguments, "text")?;
    state.active_mut().editing = None;
    Ok(run(
        state,
        Command::ReplaceMatches {
            edits: vec![(story, start..end, replacement)],
        },
    ))
}

fn frame_layout(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    use tessera_layout::resolve::ResolvedKind;
    let id = frame_arg(state, arguments)?;
    let (shaped, overset) = state
        .resolve_active()
        .items
        .iter()
        .find(|item| item.frame == id)
        .and_then(|item| match &item.kind {
            ResolvedKind::Text {
                shaped,
                overset_lines,
                ..
            } => Some((shaped.clone(), *overset_lines)),
            _ => None,
        })
        .ok_or_else(|| format!("frame {} is not laid out as text", frame_key(id)))?;
    let story = match state.active().document().frame(id).map(|f| &f.kind) {
        Some(FrameKind::Text { story, .. }) => state
            .active()
            .document()
            .story(*story)
            .map(|s| s.text.clone())
            .unwrap_or_default(),
        _ => String::new(),
    };
    let lines: Vec<Value> = shaped
        .lines
        .iter()
        .map(|l| {
            let xs: Vec<f64> = l.glyphs().map(|g| g.x).collect();
            json!({
                "baseline": l.baseline,
                "start": l.range.start,
                "end": l.range.end,
                "text": story.get(l.range.clone()).unwrap_or(""),
                "left": xs.iter().copied().fold(f64::INFINITY, f64::min),
                "right": xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            })
        })
        .collect();
    Ok(json!({
        "frame": frame_key(id),
        "lines": lines,
        "overset_lines": overset,
        "height": shaped.height,
    }))
}
