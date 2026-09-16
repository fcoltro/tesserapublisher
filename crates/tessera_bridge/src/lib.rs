//! The bridge: a way for a model to sit at Tessera the way a person does.
//!
//! An MCP server — JSON-RPC 2.0, one message per line, over stdio — whose
//! tools are the things a person does at the canvas: make a frame, set its
//! text, apply a style, ask what is overset, undo. Every change goes through
//! [`tessera_ui::command::apply`], so it is one undo entry and the same
//! code path the menus take. Nothing here knows how to change a document on
//! its own, which is the point.
//!
//! **Hand-rolled, on purpose.** The protocol a client needs to call tools is
//! five methods (`initialize`, `notifications/initialized`, `ping`,
//! `tools/list`, `tools/call`), and writing them over `serde_json` costs
//! less than an async runtime and a framework would — and leaves the whole
//! exchange testable as strings in, strings out, with no socket and no
//! screen. See [`Bridge::handle`].
//!
//! **Curated, not generated.** `Command` has over a hundred variants; a tool
//! per variant would be a menu nobody reads to the end of, and a model is a
//! reader like any other. The tools here are the dozen a layout is made
//! with; the rest arrive as somebody needs them.

use serde_json::{Value, json};
use tessera_document::ids::FrameId;
use tessera_ui::TesseraApp;
use tessera_ui::command::{Command, apply};

pub mod assistant;
pub mod catalogue;
pub mod console;
pub mod dialogs;
pub mod live;
pub mod more;
pub mod shapes;
pub mod tools;

/// The protocol revision this speaks. A client offering another is answered
/// with this one, which the protocol allows.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// One connection: the application it drives, headless.
pub struct Bridge {
    pub state: TesseraApp,
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

impl Bridge {
    /// A bridge over a fresh, headless application with an empty document.
    pub fn new() -> Self {
        Self {
            state: TesseraApp::headless(),
        }
    }

    /// Answer one JSON-RPC message. `None` for a notification, which has no
    /// answer, and for nothing but whitespace.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        handle(&mut self.state, line)
    }
}

/// Answer one JSON-RPC message against `state`: the same exchange whether
/// the application is this process's own headless one or the window on
/// screen, which is what lets [`live`] serve the latter.
pub fn handle(state: &mut TesseraApp, line: &str) -> Option<String> {
    {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(error(Value::Null, -32700, &format!("parse error: {e}")).to_string());
            }
        };
        // A notification carries no id and gets no reply.
        let id = request.get("id").cloned()?;
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let response = match method {
            "initialize" => result(
                id,
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "tessera",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": tools::INSTRUCTIONS,
                }),
            ),
            "ping" => result(id, json!({})),
            "tools/list" => result(id, json!({ "tools": tools::list() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                match tools::call(state, name, &arguments) {
                    Ok(value) => result(
                        id,
                        json!({ "content": [{ "type": "text", "text": value.to_string() }] }),
                    ),
                    Err(Failure::NoSuchTool) => {
                        error(id, -32602, &format!("no tool named {name:?}"))
                    }
                    Err(Failure::Refused(message)) => result(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": message }],
                            "isError": true,
                        }),
                    ),
                }
            }
            _ => error(id, -32601, &format!("no method named {method:?}")),
        };
        Some(response.to_string())
    }
}

/// Why a tool call gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Not a tool this server has: a protocol error, not a tool result.
    NoSuchTool,
    /// The tool could not do what was asked, in words for the model.
    Refused(String),
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A frame's id as the number a model is handed and hands back.
pub fn frame_key(id: FrameId) -> u64 {
    slotmap::Key::data(&id).as_ffi()
}

/// The frame behind a number a model handed back, if the document has it.
pub fn frame_from_key(state: &TesseraApp, key: u64) -> Option<FrameId> {
    let id = FrameId::from(slotmap::KeyData::from_ffi(key));
    state.active().document().frame(id).map(|_| id)
}

/// Run one command and say what came of it: the status line the canvas
/// would show, and the document's revision.
///
/// The status is read by clearing it first and seeing what the command
/// left — and put back afterwards when the command said nothing, so a
/// model acting on the window on screen does not wipe what the person
/// there was being told.
pub fn run(state: &mut TesseraApp, command: Command) -> Value {
    let before = state.status.take();
    apply(state, command);
    let status = state.status.as_ref().map(|s| s.message.clone());
    if state.status.is_none() {
        state.status = before;
    }
    json!({
        "revision": state.active().document().revision(),
        "status": status,
    })
}

/// Serve a client on this process's stdin and stdout until stdin closes.
///
/// Stdout is the protocol channel and nothing else may write to it; anything
/// worth saying about the server itself goes to stderr.
pub fn serve_stdio() {
    use std::io::{BufRead, Write};
    let mut bridge = Bridge::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if let Some(reply) = bridge.handle(&line) {
            if writeln!(stdout, "{reply}").is_err() {
                break;
            }
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(bridge: &mut Bridge, id: u64, method: &str, params: Value) -> Value {
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let reply = bridge.handle(&request.to_string()).expect("a reply");
        serde_json::from_str(&reply).expect("json")
    }

    /// A tool's result, parsed out of its text content.
    fn tool(bridge: &mut Bridge, name: &str, arguments: Value) -> Value {
        let reply = call(
            bridge,
            1,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("text content in {reply}"));
        assert!(
            reply["result"]["isError"].is_null(),
            "the tool refused: {text}"
        );
        serde_json::from_str(text).unwrap_or_else(|_| json!(text))
    }

    #[test]
    fn initialize_names_the_server_and_its_protocol() {
        let mut bridge = Bridge::new();
        let reply = call(
            &mut bridge,
            0,
            "initialize",
            json!({ "protocolVersion": "2025-03-26", "capabilities": {} }),
        );
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(reply["result"]["serverInfo"]["name"], "tessera");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        // The notification that follows gets no reply.
        assert_eq!(
            bridge.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            None
        );
    }

    #[test]
    fn bad_json_and_unknown_methods_are_errors_not_panics() {
        let mut bridge = Bridge::new();
        let reply: Value = serde_json::from_str(&bridge.handle("{not json").unwrap()).unwrap();
        assert_eq!(reply["error"]["code"], -32700);
        let reply = call(&mut bridge, 1, "resources/list", json!({}));
        assert_eq!(reply["error"]["code"], -32601);
        let reply = call(
            &mut bridge,
            2,
            "tools/call",
            json!({ "name": "make_coffee", "arguments": {} }),
        );
        assert_eq!(reply["error"]["code"], -32602);
        assert_eq!(bridge.handle("   "), None);
    }

    #[test]
    fn every_tool_is_listed_with_a_schema() {
        let mut bridge = Bridge::new();
        let reply = call(&mut bridge, 1, "tools/list", json!({}));
        let tools = reply["result"]["tools"].as_array().expect("tools");
        assert!(tools.len() >= 12, "{} tools", tools.len());
        for t in tools {
            assert!(t["name"].is_string());
            assert!(!t["description"].as_str().unwrap_or("").is_empty());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn a_frame_is_made_filled_read_back_and_undone() {
        let mut bridge = Bridge::new();
        let made = tool(
            &mut bridge,
            "add_text_frame",
            json!({ "x": 20, "y": 20, "width": 200, "height": 100, "text": "Hello, page" }),
        );
        let id = made["frame"].as_u64().expect("an id");

        let doc = tool(&mut bridge, "describe_document", json!({}));
        let frames = doc["frames"].as_array().expect("frames");
        assert_eq!(frames.len(), 1);
        // A page carries the id the page commands take, beside its index.
        let page_id = doc["pages"][0]["page"].as_u64().expect("a page id");
        tool(
            &mut bridge,
            "command",
            json!({ "name": "DuplicatePage", "arguments": { "id": page_id } }),
        );
        assert_eq!(
            tool(&mut bridge, "describe_document", json!({}))["pages"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        tool(&mut bridge, "undo", json!({}));
        assert_eq!(frames[0]["frame"], id);
        assert_eq!(frames[0]["kind"], "text");
        assert_eq!(frames[0]["text"], "Hello, page");
        assert_eq!(frames[0]["overset_lines"], 0);

        tool(
            &mut bridge,
            "set_text",
            json!({ "frame": id, "text": "Changed" }),
        );
        let text = tool(&mut bridge, "get_text", json!({ "frame": id }));
        assert_eq!(text["text"], "Changed");

        tool(&mut bridge, "undo", json!({}));
        let text = tool(&mut bridge, "get_text", json!({ "frame": id }));
        assert_eq!(text["text"], "Hello, page", "one command, one undo entry");
    }

    #[test]
    fn overset_is_reported_so_a_model_can_fix_it() {
        let mut bridge = Bridge::new();
        let made = tool(
            &mut bridge,
            "add_text_frame",
            json!({ "x": 20, "y": 20, "width": 120, "height": 30,
                    "text": "word ".repeat(200) }),
        );
        let id = made["frame"].as_u64().unwrap();
        let text = tool(&mut bridge, "get_text", json!({ "frame": id }));
        assert!(text["overset_lines"].as_u64().unwrap() > 0, "{text}");

        tool(
            &mut bridge,
            "set_bounds",
            json!({ "frame": id, "x": 20, "y": 20, "width": 400, "height": 700 }),
        );
        let text = tool(&mut bridge, "get_text", json!({ "frame": id }));
        assert_eq!(text["overset_lines"], 0, "and now it fits");
    }

    #[test]
    fn a_style_is_applied_by_name_and_a_wrong_name_is_refused() {
        let mut bridge = Bridge::new();
        let made = tool(
            &mut bridge,
            "add_text_frame",
            json!({ "x": 0, "y": 0, "width": 200, "height": 100, "text": "Head\nBody" }),
        );
        let id = made["frame"].as_u64().unwrap();
        // A fresh document has no styles: a model has to make them.
        tool(
            &mut bridge,
            "define_paragraph_style",
            json!({ "name": "Heading", "size": 24, "weight": 700, "alignment": "centre" }),
        );
        let styles = tool(&mut bridge, "describe_document", json!({}));
        assert_eq!(styles["paragraph_styles"], json!(["Heading"]));
        tool(
            &mut bridge,
            "apply_paragraph_style",
            json!({ "frame": id, "style": "Heading", "start": 0, "end": 4 }),
        );
        // Defining it again edits it rather than doubling it.
        tool(
            &mut bridge,
            "define_paragraph_style",
            json!({ "name": "Heading", "size": 30 }),
        );
        let styles = tool(&mut bridge, "describe_document", json!({}));
        assert_eq!(styles["paragraph_styles"], json!(["Heading"]));

        let reply = call(
            &mut bridge,
            9,
            "tools/call",
            json!({ "name": "apply_paragraph_style",
                    "arguments": { "frame": id, "style": "No Such Style" } }),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("No Such Style")
        );
    }

    #[test]
    fn frames_can_be_deleted_and_pages_added() {
        let mut bridge = Bridge::new();
        let before = tool(&mut bridge, "describe_document", json!({}));
        let pages = before["pages"].as_array().unwrap().len();
        tool(&mut bridge, "add_page", json!({}));
        let after = tool(&mut bridge, "describe_document", json!({}));
        assert_eq!(after["pages"].as_array().unwrap().len(), pages + 1);

        let made = tool(
            &mut bridge,
            "add_rectangle",
            json!({ "x": 0, "y": 0, "width": 50, "height": 50 }),
        );
        let id = made["frame"].as_u64().unwrap();
        tool(&mut bridge, "delete_frame", json!({ "frame": id }));
        let doc = tool(&mut bridge, "describe_document", json!({}));
        assert!(doc["frames"].as_array().unwrap().is_empty());
        // Gone means gone: the number no longer names anything.
        let reply = call(
            &mut bridge,
            3,
            "tools/call",
            json!({ "name": "get_text", "arguments": { "frame": id } }),
        );
        assert_eq!(reply["result"]["isError"], true);
    }

    #[test]
    fn any_command_is_reachable_by_name_with_its_arguments() {
        let mut bridge = Bridge::new();
        let listed = tool(&mut bridge, "list_commands", json!({ "filter": "ellipse" }));
        assert!(listed["count"].as_u64().unwrap() >= 1, "{listed}");
        assert!(
            listed["commands"][0]["name"]
                .as_str()
                .unwrap()
                .contains("Ellipse")
        );

        // A newtype: AddEllipse(DocRect).
        let made = tool(
            &mut bridge,
            "command",
            json!({ "name": "AddEllipse", "arguments": { "x": 10, "y": 10, "width": 80, "height": 40 } }),
        );
        assert_eq!(made["command"], "AddEllipse");
        let id = made["selection"][0]
            .as_u64()
            .expect("the new frame is selected");

        // A struct with an id and a shape from describe_shapes.
        let shapes = tool(&mut bridge, "describe_shapes", json!({ "type": "Paint" }));
        let red = shapes["Paint"]["examples"][1].clone();
        tool(
            &mut bridge,
            "command",
            json!({ "name": "SetFill", "arguments": { "id": id, "paint": red } }),
        );
        // A unit.
        tool(&mut bridge, "command", json!({ "name": "Undo" }));
        // The selection, then a selection command.
        tool(&mut bridge, "select", json!({ "frames": [id] }));
        tool(
            &mut bridge,
            "command",
            json!({ "name": "TranslateSelection", "arguments": { "dx": 5, "dy": 0 } }),
        );
        let doc = tool(&mut bridge, "describe_document", json!({}));
        assert_eq!(doc["frames"][0]["kind"], "ellipse");
        assert_eq!(doc["frames"][0]["x"], 15.0);

        // Wrong name, wrong fields: told, not crashed.
        let reply = call(
            &mut bridge,
            5,
            "tools/call",
            json!({ "name": "command", "arguments": { "name": "AddElipse" } }),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("AddEllipse")
        );
        let reply = call(
            &mut bridge,
            6,
            "tools/call",
            json!({ "name": "command", "arguments": { "name": "SetText", "arguments": { "id": id } } }),
        );
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("text")
        );
    }

    #[test]
    fn menu_actions_are_listed_and_run_with_their_guards() {
        let mut bridge = Bridge::new();
        let listed = tool(&mut bridge, "list_actions", json!({ "filter": "page" }));
        assert!(listed["count"].as_u64().unwrap() >= 1, "{listed}");
        let all = tool(&mut bridge, "list_actions", json!({}));
        assert!(all["count"].as_u64().unwrap() > 80, "{}", all["count"]);

        // One that needs a selection is refused with the reason...
        let reply = call(
            &mut bridge,
            1,
            "tools/call",
            json!({ "name": "run_action", "arguments": { "name": "Duplicate" } }),
        );
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("selection")
        );

        // ...and runs once there is one.
        let made = tool(
            &mut bridge,
            "add_rectangle",
            json!({ "x": 0, "y": 0, "width": 10, "height": 10 }),
        );
        tool(
            &mut bridge,
            "select",
            json!({ "frames": [made["frame"].as_u64().unwrap()] }),
        );
        let ran = tool(&mut bridge, "run_action", json!({ "name": "Duplicate" }));
        assert_eq!(ran["ran"], "Duplicate");
        let doc = tool(&mut bridge, "describe_document", json!({}));
        assert_eq!(doc["frames"].as_array().unwrap().len(), 2);

        // The ellipsis a menu shows is not part of the name a model types.
        let ran = tool(
            &mut bridge,
            "run_action",
            json!({ "name": "Check spelling" }),
        );
        assert!(ran["ran"].as_str().unwrap().starts_with("Check spelling"));
    }

    #[test]
    fn preferences_are_read_and_changed_field_by_field() {
        let mut bridge = Bridge::new();
        let prefs = tool(&mut bridge, "get_preferences", json!({}));
        assert_eq!(prefs["snapping"], true);
        assert_eq!(prefs["dynamic_spelling"], true);
        let after = tool(
            &mut bridge,
            "set_preferences",
            json!({ "changes": { "snapping": false, "updates": { "enabled": false } } }),
        );
        assert_eq!(after["snapping"], false);
        assert_eq!(after["updates"]["enabled"], false);
        assert_eq!(
            after["dynamic_spelling"], true,
            "untouched fields keep their values"
        );
        assert!(!bridge.state.prefs.snapping);
        // A field that is not a preference is refused, and nothing changes.
        let reply = call(
            &mut bridge,
            2,
            "tools/call",
            json!({ "name": "set_preferences", "arguments": { "changes": { "snapping": "sometimes" } } }),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert!(!bridge.state.prefs.snapping);
    }

    #[test]
    fn the_dialogs_are_tools_new_document_find_step_spelling_preflight() {
        let mut bridge = Bridge::new();
        let made = tool(
            &mut bridge,
            "new_document",
            json!({ "width": 500, "height": 700, "facing_pages": false, "pages": 2, "margin": 36, "intent": "screen" }),
        );
        assert_eq!(made["pages"], 2);
        assert_eq!(made["page"]["width"], 500.0);
        let page_x = made["page"]["x"].as_f64().unwrap();

        let frame = tool(
            &mut bridge,
            "add_text_frame",
            json!({ "x": page_x + 36.0, "y": 36, "width": 300, "height": 200,
                    "text": "The cat sat on the mat. The cat purred." }),
        );
        let id = frame["frame"].as_u64().unwrap();

        // Find, then change.
        let found = tool(&mut bridge, "find_text", json!({ "query": "cat" }));
        assert_eq!(found["count"], 2);
        assert_eq!(found["matches"][0]["frame"], id);
        let changed = tool(
            &mut bridge,
            "find_text",
            json!({ "query": "cat", "replace": "dog" }),
        );
        assert_eq!(changed["replaced"], 2);
        assert!(
            tool(&mut bridge, "get_text", json!({ "frame": id }))["text"]
                .as_str()
                .unwrap()
                .starts_with("The dog sat")
        );

        // Edit a range: insert at the start.
        tool(
            &mut bridge,
            "edit_text",
            json!({ "frame": id, "start": 0, "end": 0, "text": "Once, " }),
        );
        assert!(
            tool(&mut bridge, "get_text", json!({ "frame": id }))["text"]
                .as_str()
                .unwrap()
                .starts_with("Once, The dog")
        );

        // The layout, line by line.
        let layout = tool(&mut bridge, "frame_layout", json!({ "frame": id }));
        let lines = layout["lines"].as_array().unwrap();
        assert!(!lines.is_empty());
        assert_eq!(lines[0]["start"], 0);
        assert!(lines[0]["text"].as_str().unwrap().starts_with("Once"));

        // Step and repeat needs a selection, then makes copies.
        tool(&mut bridge, "select", json!({ "frames": [] }));
        let reply = call(
            &mut bridge,
            1,
            "tools/call",
            json!({ "name": "step_and_repeat", "arguments": { "copies": 2, "dx": 0, "dy": 220 } }),
        );
        assert_eq!(reply["result"]["isError"], true);
        tool(&mut bridge, "select", json!({ "frames": [id] }));
        tool(
            &mut bridge,
            "step_and_repeat",
            json!({ "copies": 2, "dx": 0, "dy": 220 }),
        );
        let doc = tool(&mut bridge, "describe_document", json!({}));
        assert_eq!(doc["frames"].as_array().unwrap().len(), 3);

        // Spelling, with a small dictionary; then a word vouched for.
        bridge.state.dictionaries.insert(
            "en",
            tessera_text::spell::Dictionary::parse("", "6\nonce\nthe\nsat\non\nmat\npurred\n"),
        );
        let spelt = tool(&mut bridge, "check_spelling", json!({ "frame": id }));
        assert!(spelt["count"].as_u64().unwrap() >= 1, "{spelt}");
        assert_eq!(spelt["findings"][0]["word"], "dog");
        tool(&mut bridge, "add_to_dictionary", json!({ "word": "dog" }));
        let spelt = tool(&mut bridge, "check_spelling", json!({ "frame": id }));
        assert!(
            spelt["findings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["word"] != "dog")
        );

        // Preflight reads.
        let report = tool(&mut bridge, "preflight", json!({}));
        assert!(report["problems"].is_array());

        // Fonts and the whole document.
        assert!(tool(&mut bridge, "list_fonts", json!({}))["families"].is_array());
        let whole = tool(&mut bridge, "document_json", json!({}));
        assert!(
            whole["frames"].is_object() || whole["frames"].is_array(),
            "{}",
            whole.to_string().len()
        );
    }

    #[test]
    fn export_takes_the_dialog_s_choices() {
        let dir = std::env::temp_dir().join(format!("tessera-bridge-x-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut bridge = Bridge::new();
        tool(
            &mut bridge,
            "add_rectangle",
            json!({ "x": 10, "y": 10, "width": 50, "height": 50 }),
        );
        let pdf = dir.join("marks.pdf");
        let out = tool(
            &mut bridge,
            "export_pdf",
            json!({ "path": pdf, "standard": "plain", "crop": true, "registration": true, "offset": 12 }),
        );
        assert_eq!(out["marks"]["crop"], true);
        assert_eq!(out["marks"]["offset"], 12.0);
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        let reply = call(
            &mut bridge,
            1,
            "tools/call",
            json!({ "name": "export_pdf", "arguments": { "path": pdf, "standard": "x9" } }),
        );
        assert_eq!(reply["result"]["isError"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn documents_tables_pages_and_pictures_are_reachable() {
        let mut bridge = Bridge::new();
        // Two documents, switched and closed. The first gets a frame first:
        // a new document replaces an untouched blank one rather than
        // leaving a stray "Untitled" tab, which is the application's rule.
        let one = tool(&mut bridge, "list_documents", json!({}));
        assert_eq!(one["count"], 1);
        tool(
            &mut bridge,
            "add_rectangle",
            json!({ "x": 0, "y": 0, "width": 5, "height": 5 }),
        );
        tool(&mut bridge, "new_document", json!({ "pages": 3 }));
        let two = tool(&mut bridge, "list_documents", json!({}));
        assert_eq!(two["count"], 2);
        let first = two["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["active"] == false)
            .unwrap()["document"]
            .as_u64()
            .unwrap();
        tool(&mut bridge, "switch_document", json!({ "document": first }));
        assert_eq!(
            tool(&mut bridge, "describe_document", json!({}))["pages"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let second = two["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["active"] == true)
            .unwrap()["document"]
            .as_u64()
            .unwrap();
        tool(
            &mut bridge,
            "close_document",
            json!({ "document": second, "discard": true }),
        );
        assert_eq!(tool(&mut bridge, "list_documents", json!({}))["count"], 1);
        let reply = call(
            &mut bridge,
            1,
            "tools/call",
            json!({ "name": "close_document", "arguments": { "document": first } }),
        );
        assert_eq!(reply["result"]["isError"], true, "the last one stays");

        // A page shown.
        tool(&mut bridge, "add_page", json!({}));
        let shown = tool(&mut bridge, "show_page", json!({ "page": 1 }));
        assert_eq!(shown["page"], 1);
        assert!(!bridge.state.active().fitted);

        // A table, cell by cell.
        let made = tool(
            &mut bridge,
            "command",
            json!({ "name": "AddTable", "arguments": { "bounds": { "x": 0, "y": 0, "width": 200, "height": 100 }, "rows": 2, "columns": 3 } }),
        );
        let table = made["selection"][0]
            .as_u64()
            .expect("the table is selected");
        let described = tool(&mut bridge, "describe_table", json!({ "frame": table }));
        assert_eq!(
            (described["rows"].as_u64(), described["columns"].as_u64()),
            (Some(2), Some(3))
        );
        tool(
            &mut bridge,
            "set_cell_text",
            json!({ "frame": table, "row": 1, "column": 2, "text": "Total" }),
        );
        let described = tool(&mut bridge, "describe_table", json!({ "frame": table }));
        assert_eq!(described["cells"][1][2]["text"], "Total");
        assert_eq!(described["cells"][0][0]["text"], "");

        // A picture: a missing file is refused; a real one lands in a new frame.
        let reply = call(
            &mut bridge,
            2,
            "tools/call",
            json!({ "name": "place_image", "arguments": { "path": "C:/nowhere/none.png", "x": 0, "y": 0, "width": 50, "height": 50 } }),
        );
        assert_eq!(reply["result"]["isError"], true);
        let dir = std::env::temp_dir().join(format!("tessera-bridge-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("dot.png");
        // The smallest valid PNG: one white pixel.
        std::fs::write(
            &png,
            [
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
                0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
                0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
                0xD7, 0x63, 0xF8, 0xFF, 0xFF, 0x3F, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x35, 0x81,
                0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
            ],
        )
        .unwrap();
        let placed = tool(
            &mut bridge,
            "place_image",
            json!({ "path": png, "x": 10, "y": 10, "width": 100, "height": 100 }),
        );
        let frame = placed["frame"].as_u64().unwrap();
        let doc = tool(&mut bridge, "describe_document", json!({}));
        let kind = doc["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["frame"] == frame)
            .unwrap()["kind"]
            .clone();
        assert_eq!(kind, "graphic");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "needs a GPU adapter; run alone with -- --ignored"]
    fn a_page_renders_to_a_png_a_model_can_look_at() {
        let dir = std::env::temp_dir().join(format!("tessera-bridge-png-{}", std::process::id()));
        let mut bridge = Bridge::new();
        // On the page — which, facing pages, does not start at x 0 — and
        // filled black, so there is something to see.
        let page = tool(&mut bridge, "describe_document", json!({}))["pages"][0].clone();
        let (px, py) = (page["x"].as_f64().unwrap(), page["y"].as_f64().unwrap());
        let made = tool(
            &mut bridge,
            "add_rectangle",
            json!({ "x": px + 20.0, "y": py + 20.0, "width": 100, "height": 100 }),
        );
        tool(
            &mut bridge,
            "command",
            json!({ "name": "SetFill", "arguments": { "id": made["frame"], "paint": { "Solid": { "Rgb": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 } } } } }),
        );
        let png = dir.join("page.png");
        let out = tool(
            &mut bridge,
            "render_page",
            json!({ "page": 0, "path": png, "ppi": 36 }),
        );
        assert_eq!(out["ppi"], 36.0);
        let image = image::open(&png).expect("a PNG").to_rgba8();
        assert_eq!(
            (image.width(), image.height()),
            (
                out["width"].as_u64().unwrap() as u32,
                out["height"].as_u64().unwrap() as u32
            )
        );
        // The page is white where nothing is, and dark inside the rectangle
        // (the default fill), at half scale.
        let white = image.get_pixel(image.width() - 5, image.height() - 5);
        assert_eq!(&white.0[..3], &[255, 255, 255], "{white:?}");
        let inside = image.get_pixel(35, 35);
        assert_ne!(&inside.0[..3], &[255, 255, 255], "{inside:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_document_is_saved_and_opened_again() {
        let dir = std::env::temp_dir().join(format!("tessera-bridge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bridged.tessera");

        let mut bridge = Bridge::new();
        tool(
            &mut bridge,
            "add_text_frame",
            json!({ "x": 0, "y": 0, "width": 200, "height": 100, "text": "kept" }),
        );
        tool(&mut bridge, "save", json!({ "path": path }));

        let mut again = Bridge::new();
        tool(&mut again, "open", json!({ "path": path }));
        let doc = tool(&mut again, "describe_document", json!({}));
        assert_eq!(doc["frames"][0]["text"], "kept");

        let pdf = dir.join("bridged.pdf");
        tool(&mut again, "export_pdf", json!({ "path": pdf }));
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
