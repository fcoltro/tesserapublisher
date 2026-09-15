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

pub mod catalogue;
pub mod live;
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
