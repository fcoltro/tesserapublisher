# The bridge

`tessera_app --mcp` is an MCP server on stdin and stdout, so a model can
open, make and edit a document the way a person does at the canvas. Every
change goes through the same command layer the menus use, so each is one
undo entry.

**If a Tessera window is open, the model works on it.** The window listens
on a loopback port and records it in `bridge.port` beside the preferences;
`--mcp` relays to it when it answers, and each change lands on the canvas
as it is made, undoable from the Edit menu like any other. With no window
open, `--mcp` is a headless Tessera of its own. The client is configured
the same way in both cases.

## Connecting

Claude Code:

```bash
claude mcp add tessera -- "C:\path\to\tessera_app.exe" --mcp
```

Claude Desktop, in `claude_desktop_config.json`:

```json
{ "mcpServers": { "tessera": { "command": "C:\path\to\tessera_app.exe", "args": ["--mcp"] } } }
```

Any other MCP client: launch the binary with `--mcp` and speak JSON-RPC 2.0,
one message per line. `initialize`, `ping`, `tools/list` and `tools/call`
are answered; notifications are not.

## Tools

| Tool | Does |
| --- | --- |
| `describe_document` | Pages, every frame (number, kind, page, bounds, text, overset lines), paragraph style names |
| `add_text_frame`, `add_rectangle` | Make a frame at a box; returns its number |
| `set_text`, `get_text` | A text frame's text, and how many lines do not fit |
| `set_bounds`, `delete_frame` | Move, resize, remove |
| `define_paragraph_style`, `apply_paragraph_style` | Make a style by name; apply it to a range |
| `add_page`, `undo`, `redo` | |
| `open`, `save`, `export_pdf` | By path |

Measurements are points from the document's top-left. A frame's number is
stable for the life of the document in this process; it is not saved.

## Trust

The port is loopback only and carries no secret: any process on the same
machine can reach a running window's document — the trust a local script
has always had over an application's files. Nothing listens on other
interfaces.

## Not yet

Images; character styles and local formatting; tables; threading. The crate is `crates/tessera_bridge`; a tool is a name, a
sentence, a schema and a function, and adding one is adding one entry to
`tools::ALL`.
