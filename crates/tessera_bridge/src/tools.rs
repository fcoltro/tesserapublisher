//! The tools a model is handed, and what each does.
//!
//! Each tool is a name, a sentence for the model, a JSON schema for its
//! arguments, and a function from those arguments to a result. A tool that
//! changes the document says so in its result — the revision it left, and
//! the status line the canvas would have shown — because a change nobody is
//! told about is a change nobody can act on. Reads cost nothing and say
//! everything; writes are loud.

use serde_json::{Value, json};
use tessera_document::nodes::FrameKind;
use tessera_geometry::DocRect;
use tessera_ui::TesseraApp;
use tessera_ui::command::Command;

use crate::{Failure, frame_from_key, frame_key, run};

/// What the client is told at `initialize`, before it has called anything.
pub const INSTRUCTIONS: &str = "Tessera is a page-layout application. Measurements are in \
    points (72 to the inch) from the top-left of the document; a page's frames are placed \
    inside its bounds, which describe_document reports. Text is set in text frames; a frame \
    whose text does not fit reports overset_lines, and the fix is a bigger frame or shorter \
    copy. Every change is one undo entry. The named tools cover the common work; everything \
    the application can do is reachable through `command`: `list_commands` names every \
    command with its documentation and fields, `describe_shapes` shows the JSON of the \
    objects those fields take, and `select` chooses the frames the selection commands act \
    on. Menu actions and dialogs are `list_actions` and `run_action`; preferences are \
    `get_preferences` and `set_preferences`.";

/// The tools, in the order a model reads them.
pub fn list() -> Vec<Value> {
    every()
        .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.schema() }))
        .collect()
}

/// Every tool: the canvas work here, the dialogs and the read side in
/// [`crate::dialogs`].
fn every() -> impl Iterator<Item = &'static Tool> {
    ALL.iter()
        .chain(crate::dialogs::ALL.iter())
        .chain(crate::more::ALL.iter())
}

/// Run the tool called `name` with `arguments`.
pub fn call(state: &mut TesseraApp, name: &str, arguments: &Value) -> Result<Value, Failure> {
    let tool = every()
        .find(|t| t.name == name)
        .ok_or(Failure::NoSuchTool)?;
    (tool.run)(state, arguments).map_err(Failure::Refused)
}

pub(crate) struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// `(name, type, description, required)`.
    pub arguments: &'static [(&'static str, &'static str, &'static str, bool)],
    pub run: fn(&mut TesseraApp, &Value) -> Result<Value, String>,
}

impl Tool {
    fn schema(&self) -> Value {
        let properties: serde_json::Map<String, Value> = self
            .arguments
            .iter()
            .map(|(name, kind, description, _)| {
                (
                    (*name).to_owned(),
                    json!({ "type": kind, "description": description }),
                )
            })
            .collect();
        let required: Vec<&str> = self
            .arguments
            .iter()
            .filter(|(_, _, _, required)| *required)
            .map(|(name, ..)| *name)
            .collect();
        json!({ "type": "object", "properties": properties, "required": required })
    }
}

const BOX: [(&str, &str, &str, bool); 4] = [
    (
        "x",
        "number",
        "Left edge, in points from the document's left.",
        true,
    ),
    (
        "y",
        "number",
        "Top edge, in points from the document's top.",
        true,
    ),
    ("width", "number", "Width in points.", true),
    ("height", "number", "Height in points.", true),
];

static ALL: [Tool; 22] = [
    Tool {
        name: "describe_document",
        description: "Everything on the page: each page's index and size, every frame with its \
            number, kind, page, bounds, text and overset line count, and the names of the \
            document's paragraph styles. Call it first, and after any change you want to see.",
        arguments: &[],
        run: describe_document,
    },
    Tool {
        name: "add_text_frame",
        description: "Make a text frame at the given box, optionally with its text. Returns the \
            frame's number, which the other tools take.",
        arguments: &[
            BOX[0],
            BOX[1],
            BOX[2],
            BOX[3],
            (
                "text",
                "string",
                "The text to set in it; empty if absent.",
                false,
            ),
        ],
        run: add_text_frame,
    },
    Tool {
        name: "add_rectangle",
        description: "Make a rectangle at the given box. Returns the frame's number.",
        arguments: &BOX,
        run: add_rectangle,
    },
    Tool {
        name: "set_text",
        description: "Replace all of a text frame's text.",
        arguments: &[
            ("frame", "integer", "The frame's number.", true),
            (
                "text",
                "string",
                "The whole text; paragraphs separated by newlines.",
                true,
            ),
        ],
        run: set_text,
    },
    Tool {
        name: "get_text",
        description: "A text frame's text and how many lines of it do not fit (overset_lines).",
        arguments: &[("frame", "integer", "The frame's number.", true)],
        run: get_text,
    },
    Tool {
        name: "set_bounds",
        description: "Move and resize a frame to the given box.",
        arguments: &[
            ("frame", "integer", "The frame's number.", true),
            BOX[0],
            BOX[1],
            BOX[2],
            BOX[3],
        ],
        run: set_bounds,
    },
    Tool {
        name: "delete_frame",
        description: "Remove a frame from the document.",
        arguments: &[("frame", "integer", "The frame's number.", true)],
        run: delete_frame,
    },
    Tool {
        name: "apply_paragraph_style",
        description: "Apply one of the document's paragraph styles, by name, to the paragraphs \
            a range of a text frame's text touches — or to all of it when no range is given. \
            describe_document lists the names.",
        arguments: &[
            ("frame", "integer", "The frame's number.", true),
            ("style", "string", "The paragraph style's name.", true),
            (
                "start",
                "integer",
                "Byte offset the range starts at; 0 if absent.",
                false,
            ),
            (
                "end",
                "integer",
                "Byte offset the range ends at; the text's end if absent.",
                false,
            ),
        ],
        run: apply_paragraph_style,
    },
    Tool {
        name: "define_paragraph_style",
        description: "Define a paragraph style on the document, or redefine one with the same \
            name. Only the properties given are set; the rest stay as the document's defaults. \
            Sizes are in points; leading is a multiple of the size (1.2 is usual); alignment is \
            left, centre, right or justify.",
        arguments: &[
            ("name", "string", "The style's name.", true),
            (
                "family",
                "string",
                "Font family, as the system names it.",
                false,
            ),
            ("size", "number", "Type size in points.", false),
            (
                "weight",
                "integer",
                "Weight, 100 to 900; 400 regular, 700 bold.",
                false,
            ),
            ("italic", "boolean", "Whether the face is italic.", false),
            (
                "leading",
                "number",
                "Line height as a multiple of the size.",
                false,
            ),
            (
                "alignment",
                "string",
                "left, centre, right or justify.",
                false,
            ),
            (
                "space_before",
                "number",
                "Space above each paragraph, in points.",
                false,
            ),
            (
                "space_after",
                "number",
                "Space below each paragraph, in points.",
                false,
            ),
            (
                "indent_first",
                "number",
                "First-line indent, in points.",
                false,
            ),
        ],
        run: define_paragraph_style,
    },
    Tool {
        name: "list_commands",
        description: "Every command the application has — the whole of what it can do — each \
            with its documentation, how it takes its arguments (none, one value, an array, or \
            an object of named fields) and the fields' types. Give `filter` to keep only names \
            containing a word.",
        arguments: &[(
            "filter",
            "string",
            "A word the command's name or doc must contain.",
            false,
        )],
        run: list_commands,
    },
    Tool {
        name: "command",
        description: "Run any command from list_commands by name, with its arguments as that \
            listing describes: nothing for a command that takes none, the value itself for one \
            that takes one value, an array for several, an object of the named fields \
            otherwise. Ids are the numbers describe_document reports. One undo entry, like \
            every change.",
        arguments: &[
            (
                "name",
                "string",
                "The command's name, as list_commands gives it.",
                true,
            ),
            (
                "arguments",
                "object",
                "The arguments; shape per list_commands. Omit for a command that takes none.",
                false,
            ),
        ],
        run: command,
    },
    Tool {
        name: "describe_shapes",
        description: "The JSON of the objects a command's fields take — TextLayout, Paint, \
            ParagraphFormat and the rest — as examples that read back exactly, and every value \
            of each enum. Give `type` for one of them; omit it for all.",
        arguments: &[(
            "type",
            "string",
            "One type name, as list_commands spells it.",
            false,
        )],
        run: describe_shapes,
    },
    Tool {
        name: "select",
        description: "Set the selection to these frames — what the *Selection commands act on \
            (DeleteSelection, DuplicateSelection, GroupSelection, TranslateSelection, \
            MoveSelectionInZ, Align…). An empty list clears it. Returns the selection.",
        arguments: &[("frames", "array", "Frame numbers.", true)],
        run: select,
    },
    Tool {
        name: "list_actions",
        description: "Every menu action — File, Edit, Object, Type, Layout, Table, View, Window, \
            Help and the tools — by name, with its shortcut, its menu and submenu, and whether \
            it can run now (some need a selection, some cannot run while text is being edited). \
            An action that opens a dialog opens it on screen; the dialog's own settings are \
            reachable as tools of their own.",
        arguments: &[(
            "filter",
            "string",
            "A word the name or menu must contain.",
            false,
        )],
        run: list_actions,
    },
    Tool {
        name: "run_action",
        description: "Run a menu action by its name from list_actions, exactly as choosing it \
            from the menu would. Refused, with the reason, when it cannot run now.",
        arguments: &[(
            "name",
            "string",
            "The action's name, as the menu shows it.",
            true,
        )],
        run: run_action,
    },
    Tool {
        name: "get_preferences",
        description: "Every preference — units, theme, density, snapping, typographer's quotes, \
            dynamic spelling, recovery copy, export presets, update checking, shortcuts, \
            workspaces — as JSON. The assistant key is omitted; api_key_configured says \
            whether one is set.",
        arguments: &[],
        run: get_preferences,
    },
    Tool {
        name: "set_preferences",
        description: "Change preferences: an object of the fields to change, in the shape \
            get_preferences shows; the rest keep their values. Saved to disk when the \
            application is one that saves. Returns the preferences after the change, with \
            credentials omitted. Set API keys in the Preferences window.",
        arguments: &[(
            "changes",
            "object",
            "Fields to change, nested as get_preferences shows them.",
            true,
        )],
        run: set_preferences,
    },
    Tool {
        name: "add_page",
        description: "Add a page at the end of the document.",
        arguments: &[],
        run: |state, _| Ok(run(state, Command::AddPage)),
    },
    Tool {
        name: "undo",
        description: "Undo the last change.",
        arguments: &[],
        run: |state, _| Ok(run(state, Command::Undo)),
    },
    Tool {
        name: "redo",
        description: "Redo the last undone change.",
        arguments: &[],
        run: |state, _| Ok(run(state, Command::Redo)),
    },
    Tool {
        name: "open",
        description: "Open a .tsrdf document from a path, replacing the current one.",
        arguments: &[("path", "string", "The file's path.", true)],
        run: open,
    },
    Tool {
        name: "save",
        description: "Save the document to a path, as .tsrdf.",
        arguments: &[("path", "string", "The file's path.", true)],
        run: save,
    },
];

// --- reading the arguments ---------------------------------------------------

pub(crate) fn number(arguments: &Value, name: &str) -> Result<f64, String> {
    arguments
        .get(name)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{name} must be a number"))
}

pub(crate) fn text(arguments: &Value, name: &str) -> Result<String, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{name} must be a string"))
}

pub(crate) fn rect_arg(arguments: &Value) -> Result<DocRect, String> {
    Ok(DocRect {
        x: number(arguments, "x")?,
        y: number(arguments, "y")?,
        width: number(arguments, "width")?,
        height: number(arguments, "height")?,
    })
}

pub(crate) fn frame_arg(
    state: &TesseraApp,
    arguments: &Value,
) -> Result<tessera_document::ids::FrameId, String> {
    let key = arguments
        .get("frame")
        .and_then(Value::as_u64)
        .ok_or("frame must be a frame number from describe_document")?;
    frame_from_key(state, key).ok_or_else(|| format!("no frame numbered {key}"))
}

pub(crate) fn text_frame_arg(
    state: &TesseraApp,
    arguments: &Value,
) -> Result<
    (
        tessera_document::ids::FrameId,
        tessera_document::ids::StoryId,
    ),
    String,
> {
    let id = frame_arg(state, arguments)?;
    match state.active().document().frame(id).map(|f| &f.kind) {
        Some(FrameKind::Text { story, .. }) => Ok((id, *story)),
        _ => Err(format!("frame {} is not a text frame", frame_key(id))),
    }
}

// --- the tools ---------------------------------------------------------------

fn describe_document(state: &mut TesseraApp, _: &Value) -> Result<Value, String> {
    use tessera_layout::resolve::ResolvedKind;

    // Overset is the layout's to say, and only the layout's.
    let overset: Vec<(tessera_document::ids::FrameId, usize)> = state
        .resolve_active()
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ResolvedKind::Text { overset_lines, .. } => Some((item.frame, *overset_lines)),
            _ => None,
        })
        .collect();

    let doc = state.active().document();
    let pages: Vec<PageId> = doc.page_ids().collect();
    let page_index = |page: Option<tessera_document::ids::PageId>| {
        page.and_then(|p| pages.iter().position(|q| *q == p))
    };

    let frames: Vec<Value> = doc
        .frames
        .iter()
        .map(|(id, frame)| {
            // A path carrying text reports that story, as a text frame
            // does: the words are what a model asks after.
            let (kind, story) = match &frame.kind {
                FrameKind::Text { story, .. } => ("text", Some(*story)),
                FrameKind::Rectangle => ("rectangle", None),
                FrameKind::Ellipse => ("ellipse", None),
                FrameKind::Graphic { .. } => ("graphic", None),
                FrameKind::Path(_) => ("path", doc.path_text(id).map(|t| t.story)),
                FrameKind::Table(_) => ("table", None),
                FrameKind::Group(_) => ("group", None),
            };
            // Where it is seen, transform included: a frame moved by a
            // translation keeps its bounds and gains a transform, and a
            // model asked to move something wants to see it moved.
            let seen = doc.visual_bounds(id).unwrap_or(frame.bounds);
            json!({
                "frame": frame_key(id),
                "key": serde_json::to_value(slotmap::Key::data(&id)).unwrap_or(Value::Null),
                "kind": kind,
                "page": page_index(doc.page_of_frame(id)),
                "x": seen.x,
                "y": seen.y,
                "width": seen.width,
                "height": seen.height,
                "rotation_degrees": frame.transform.rotation_degrees(),
                // The story's id as the commands take it — SetCharacterFormat,
                // SetParagraphFormat, ApplyParagraphStyle — which was missing:
                // a model could read the words and not address them.
                "story": story.map(|s| slotmap::Key::data(&s).as_ffi()),
                "text": story.and_then(|s| doc.story(s)).map(|s| s.text.clone()),
                "overset_lines": overset.iter().find(|(f, _)| *f == id).map(|(_, n)| *n),
            })
        })
        .collect();

    Ok(json!({
        "revision": doc.revision(),
        "pages": pages.iter().enumerate().map(|(index, page)| json!({
            "index": index,
            // The id the page commands take — MovePage, RemovePage,
            // ApplyMaster — beside the index the named tools take.
            "page": slotmap::Key::data(page).as_ffi(),
            "x": doc.pages[*page].bounds.x,
            "y": doc.pages[*page].bounds.y,
            "width": doc.pages[*page].bounds.width,
            "height": doc.pages[*page].bounds.height,
        })).collect::<Vec<_>>(),
        "frames": frames,
        "paragraph_styles": doc.paragraph_styles.values().map(|s| s.name.clone()).collect::<Vec<_>>(),
    }))
}

type PageId = tessera_document::ids::PageId;

fn add_text_frame(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let bounds = rect_arg(arguments)?;
    let mut outcome = run(state, Command::AddTextFrame(bounds));
    let id = state
        .active()
        .selection
        .single()
        .ok_or("the frame was not made")?;
    if let Some(text) = arguments.get("text").and_then(Value::as_str)
        && !text.is_empty()
    {
        outcome = run(
            state,
            Command::SetText {
                id,
                text: text.to_owned(),
            },
        );
    }
    outcome["frame"] = json!(frame_key(id));
    Ok(outcome)
}

fn add_rectangle(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let bounds = rect_arg(arguments)?;
    let mut outcome = run(state, Command::AddRectangle(bounds));
    let id = state
        .active()
        .selection
        .single()
        .ok_or("the rectangle was not made")?;
    outcome["frame"] = json!(frame_key(id));
    Ok(outcome)
}

fn set_text(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let (id, _) = text_frame_arg(state, arguments)?;
    let text = text(arguments, "text")?;
    Ok(run(state, Command::SetText { id, text }))
}

fn get_text(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    use tessera_layout::resolve::ResolvedKind;
    let (id, story) = text_frame_arg(state, arguments)?;
    let overset = state
        .resolve_active()
        .items
        .iter()
        .find(|item| item.frame == id)
        .and_then(|item| match &item.kind {
            ResolvedKind::Text { overset_lines, .. } => Some(*overset_lines),
            _ => None,
        })
        .unwrap_or(0);
    let text = state
        .active()
        .document()
        .story(story)
        .map(|s| s.text.clone())
        .unwrap_or_default();
    Ok(json!({ "frame": frame_key(id), "text": text, "overset_lines": overset }))
}

fn set_bounds(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let id = frame_arg(state, arguments)?;
    let bounds = rect_arg(arguments)?;
    Ok(run(state, Command::SetBounds { id, bounds }))
}

fn delete_frame(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let id = frame_arg(state, arguments)?;
    state.active_mut().selection.set(id);
    Ok(run(state, Command::DeleteSelection))
}

fn apply_paragraph_style(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let (_, story) = text_frame_arg(state, arguments)?;
    let name = text(arguments, "style")?;
    let doc = state.active().document();
    let style = doc
        .paragraph_styles
        .iter()
        .find(|(_, s)| s.name == name)
        .map(|(id, _)| id)
        .ok_or_else(|| {
            format!(
                "no paragraph style named {name:?}; the document has: {}",
                doc.paragraph_styles
                    .values()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let length = doc.story(story).map_or(0, |s| s.text.len());
    let start = arguments
        .get("start")
        .and_then(Value::as_u64)
        .map_or(0, |n| n as usize)
        .min(length);
    let end = arguments
        .get("end")
        .and_then(Value::as_u64)
        .map_or(length, |n| n as usize)
        .clamp(start, length);
    if let Some(text) = doc.story(story).map(|s| &s.text)
        && (!text.is_char_boundary(start) || !text.is_char_boundary(end))
    {
        return Err("start and end must be UTF-8 character boundaries".into());
    }
    Ok(run(
        state,
        Command::SetParagraphStyleOf {
            story,
            range: start..end,
            style: Some(style),
        },
    ))
}

fn define_paragraph_style(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    use tessera_text::story::{Alignment, CharacterFormat, ParagraphFormat, ParagraphStyle};

    let name = text(arguments, "name")?;
    let f32_of = |key: &str| arguments.get(key).and_then(Value::as_f64).map(|n| n as f32);
    let alignment = match arguments.get("alignment").and_then(Value::as_str) {
        None => None,
        Some("left") => Some(Alignment::Left),
        Some("centre") | Some("center") => Some(Alignment::Centre),
        Some("right") => Some(Alignment::Right),
        Some("justify") => Some(Alignment::Justify),
        Some(other) => {
            return Err(format!(
                "alignment {other:?} is not left, centre, right or justify"
            ));
        }
    };
    let format = ParagraphFormat {
        alignment,
        space_before: f32_of("space_before"),
        space_after: f32_of("space_after"),
        indent_first: f32_of("indent_first"),
        character: CharacterFormat {
            family: arguments
                .get("family")
                .and_then(Value::as_str)
                .map(str::to_owned),
            size: f32_of("size"),
            weight: arguments
                .get("weight")
                .and_then(Value::as_u64)
                .map(|w| w as u16),
            italic: arguments.get("italic").and_then(Value::as_bool),
            line_height: f32_of("leading"),
            ..CharacterFormat::default()
        },
        ..ParagraphFormat::default()
    };

    // A second definition with the same name edits the first: a model that
    // sets a style up in two steps should not end with two styles.
    let existing = state
        .active()
        .document()
        .paragraph_styles
        .iter()
        .find(|(_, s)| s.name == name)
        .map(|(id, s)| (id, s.based_on));
    let command = match existing {
        Some((id, based_on)) => Command::EditParagraphStyle {
            id,
            style: ParagraphStyle {
                name,
                based_on,
                format,
            },
        },
        None => Command::DefineParagraphStyle(ParagraphStyle {
            name,
            based_on: None,
            format,
        }),
    };
    Ok(run(state, command))
}

fn list_commands(_: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let filter = arguments
        .get("filter")
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let all = crate::catalogue::listing();
    let kept: Vec<Value> = match filter {
        None => all,
        Some(word) => all
            .into_iter()
            .filter(|v| {
                v["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&word)
                    || v["doc"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&word)
            })
            .collect(),
    };
    Ok(json!({ "count": kept.len(), "commands": kept }))
}

fn command(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let name = text(arguments, "name")?;
    let variant = crate::catalogue::variant(&name).ok_or_else(|| {
        // Names near a misspelt one: sharing a word, or the first letters.
        let asked = name.to_lowercase();
        let near: Vec<&str> = crate::catalogue::variants()
            .iter()
            .map(|v| v.name.as_str())
            .filter(|n| {
                let n = n.to_lowercase();
                n.contains(&asked)
                    || asked.contains(&n)
                    || n.chars()
                        .zip(asked.chars())
                        .take_while(|(a, b)| a == b)
                        .count()
                        >= 4
            })
            .take(8)
            .collect();
        format!("no command named {name:?}; near it: {near:?}. list_commands has them all.")
    })?;
    let given = arguments.get("arguments").cloned().unwrap_or(Value::Null);
    let value = crate::catalogue::command_json(variant, given)?;
    let command: Command = serde_json::from_value(value)
        .map_err(|e| format!("{name}: the arguments did not read as the command: {e}. describe_shapes shows what each object looks like."))?;
    match &command {
        Command::SetCharacterFormat { story, range, .. }
        | Command::SetParagraphFormat { story, range, .. }
        | Command::ClearCharacterOverrides { story, range }
        | Command::ClearParagraphOverrides { story, range }
        | Command::RedefineCharacterStyle { story, range, .. }
        | Command::RedefineParagraphStyle { story, range, .. }
        | Command::BreakCharacterStyleLink { story, range }
        | Command::BreakParagraphStyleLink { story, range }
        | Command::SetCharacterStyleOf { story, range, .. }
        | Command::SetParagraphStyleOf { story, range, .. } => {
            validate_text_range(state, *story, range)?
        }
        Command::ReplaceMatches { edits } => {
            for (story, range, _) in edits {
                validate_text_range(state, *story, range)?;
            }
        }
        _ => {}
    }
    let mut outcome = run(state, command);
    outcome["command"] = json!(name);
    outcome["selection"] = json!(selection_keys(state));
    Ok(outcome)
}

pub(crate) fn validate_text_range(
    state: &TesseraApp,
    story: tessera_document::StoryId,
    range: &std::ops::Range<usize>,
) -> Result<(), String> {
    let text = &state
        .active()
        .document()
        .story(story)
        .ok_or("the story does not exist")?
        .text;
    if text.get(range.clone()).is_none() {
        return Err(
            "text range must be ordered, within the story, and on UTF-8 character boundaries"
                .into(),
        );
    }
    Ok(())
}

fn describe_shapes(_: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let all = crate::shapes::all();
    match arguments.get("type").and_then(Value::as_str) {
        None => Ok(all),
        Some(name) => all
            .get(name)
            .cloned()
            .map(|shape| json!({ name: shape }))
            .ok_or_else(|| {
                format!(
                    "no shape named {name:?}; there are: {}",
                    all.as_object()
                        .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                        .unwrap_or_default()
                )
            }),
    }
}

fn selection_keys(state: &TesseraApp) -> Vec<u64> {
    state.active().selection.iter().map(frame_key).collect()
}

fn select(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let wanted = arguments
        .get("frames")
        .and_then(Value::as_array)
        .ok_or("frames must be an array of frame numbers")?;
    let mut ids = Vec::new();
    for v in wanted {
        let key = v
            .as_u64()
            .ok_or_else(|| format!("{v} is not a frame number"))?;
        ids.push(frame_from_key(state, key).ok_or_else(|| format!("no frame numbered {key}"))?);
    }
    state.active_mut().selection.replace_all(ids);
    Ok(json!({ "selection": selection_keys(state) }))
}

fn action_json(state: &TesseraApp, action: &tessera_ui::actions::Action) -> Value {
    json!({
        "name": action.name,
        "shortcut": action.shortcut,
        "menu": action.group.menu(),
        "submenu": action.group.submenu(),
        "enabled": tessera_ui::actions::enabled(state, action.run),
    })
}

fn list_actions(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let filter = arguments
        .get("filter")
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let actions: Vec<Value> = tessera_ui::actions::all()
        .iter()
        .filter(|a| match &filter {
            None => true,
            Some(word) => {
                a.name.to_lowercase().contains(word)
                    || a.group
                        .menu()
                        .is_some_and(|m| m.to_lowercase().contains(word))
                    || a.group
                        .submenu()
                        .is_some_and(|m| m.to_lowercase().contains(word))
            }
        })
        .map(|a| action_json(state, a))
        .collect();
    Ok(json!({ "count": actions.len(), "actions": actions }))
}

fn run_action(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let name = text(arguments, "name")?;
    let wanted = name.trim().trim_end_matches('\u{2026}').to_lowercase();
    let action = tessera_ui::actions::all()
        .iter()
        .find(|a| a.name.trim_end_matches('\u{2026}').to_lowercase() == wanted)
        .ok_or_else(|| {
            let near: Vec<&str> = tessera_ui::actions::all()
                .iter()
                .map(|a| a.name)
                .filter(|n| n.to_lowercase().contains(&wanted))
                .take(8)
                .collect();
            format!("no action named {name:?}; near it: {near:?}. list_actions has them all.")
        })?;
    if !tessera_ui::actions::enabled(state, action.run) {
        let why = match tessera_ui::actions::guard(action.run) {
            tessera_ui::actions::Guard::NeedsSelection => "it needs a selection (see select)",
            tessera_ui::actions::Guard::NotWhileTyping => {
                "it cannot run while text is being edited"
            }
            tessera_ui::actions::Guard::Always => "nothing for it to act on now",
        };
        return Err(format!("{} cannot run now: {why}.", action.name));
    }
    let before = state.status.take();
    tessera_ui::actions::run(state, action.run);
    let status = state.status.as_ref().map(|s| s.message.clone());
    if state.status.is_none() {
        state.status = before;
    }
    Ok(json!({
        "ran": action.name,
        "revision": state.active().document().revision(),
        "status": status,
        "selection": selection_keys(state),
    }))
}

fn get_preferences(state: &mut TesseraApp, _: &Value) -> Result<Value, String> {
    public_preferences(&state.prefs)
}

/// Tool results become model conversation content. Credentials never belong there.
fn public_preferences(prefs: &tessera_ui::prefs::Preferences) -> Result<Value, String> {
    let mut value = serde_json::to_value(prefs).map_err(|e| e.to_string())?;
    if let Some(assistant) = value.get_mut("assistant").and_then(Value::as_object_mut) {
        assistant.remove("api_key");
        assistant.insert(
            "api_key_configured".into(),
            json!(!prefs.assistant.api_key.trim().is_empty()),
        );
    }
    Ok(value)
}

fn set_preferences(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let changes = arguments
        .get("changes")
        .and_then(Value::as_object)
        .ok_or("changes must be an object of preference fields")?;
    if changes
        .get("assistant")
        .is_some_and(|a| a.get("api_key").is_some())
    {
        return Err("Set the assistant API key in Preferences, not through a tool call".into());
    }
    let mut current = serde_json::to_value(&state.prefs).map_err(|e| e.to_string())?;
    merge(&mut current, changes);
    let prefs: tessera_ui::prefs::Preferences = serde_json::from_value(current)
        .map_err(|e| format!("the changes did not read as preferences: {e}"))?;
    state.prefs = prefs;
    tessera_ui::prefs::remember(state);
    public_preferences(&state.prefs)
}

/// Lay `changes` over `into`, object by object, so a nested field can be
/// changed without restating its neighbours.
fn merge(into: &mut Value, changes: &serde_json::Map<String, Value>) {
    let Value::Object(target) = into else {
        return;
    };
    for (key, value) in changes {
        match (target.get_mut(key), value) {
            (Some(existing @ Value::Object(_)), Value::Object(inner)) => merge(existing, inner),
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn path(arguments: &Value) -> Result<std::path::PathBuf, String> {
    text(arguments, "path").map(std::path::PathBuf::from)
}

fn open(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let path = path(arguments)?;
    tessera_ui::file_ops::open_from_path(state, &path).map_err(|e| e.to_string())?;
    Ok(json!({ "opened": path, "revision": state.active().document().revision() }))
}

fn save(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let path = path(arguments)?;
    tessera_ui::file_ops::save_to_path(state, &path).map_err(|e| e.to_string())?;
    Ok(json!({ "saved": path }))
}
