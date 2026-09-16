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

## Reach

Everything the application can do is reachable, in three layers:

**Every command.** `list_commands` names all 114 variants of the command
layer — the whole mutation surface — each with the documentation written on
it in the source, how it takes its arguments, and its fields' types.
`command` runs any of them by name. `describe_shapes` shows the JSON of
every object a field can take (`TextLayout`, `Paint`, `ParagraphFormat`, …)
as examples that read back exactly. `select` sets the selection the
`*Selection` commands act on. A command added to the enum is reachable, and
described, the moment it is written: the bridge parses `command.rs`
embedded in the binary.

**Every menu action.** `list_actions` names all 134 — File through Help and
the tools — with shortcut, menu, submenu and whether it can run now;
`run_action` runs one as the menu would, or says why not.

**Every dialog, as its choices.** A dialog opens on screen where a model
cannot click, so each is a tool that does what OK does: `new_document`,
`export_pdf` (standard, marks, or a saved preset), `find_text` (with
replace), `step_and_repeat`, `check_spelling` and `add_to_dictionary`,
`package`; `get_preferences` and `set_preferences` for the settings window,
field by field.

And the named tools for the common work:

| Tool | Does |
| --- | --- |
| `describe_document` | Pages, every frame (number, kind, page, seen bounds, rotation, text, overset lines), paragraph style names |
| `document_json`, `frame_layout`, `preflight`, `list_fonts` | The whole document; a frame's lines; the preflight report; the fonts |
| `add_text_frame`, `add_rectangle`, `place_image` | Make a frame; put a picture on the page |
| `set_text`, `get_text`, `edit_text` | A text frame's text, its overset count, a byte range of it |
| `set_bounds`, `delete_frame` | Move, resize, remove |
| `define_paragraph_style`, `apply_paragraph_style` | Make a style by name; apply it to a range |
| `describe_table`, `set_cell_text` | A table cell by cell |
| `list_documents`, `switch_document`, `close_document`, `show_page` | The open documents and the page in view |
| `add_page`, `undo`, `redo`, `open`, `save` | |

Measurements are points from the document's top-left. A frame's number is
stable for the life of the document in this process; it is not saved.
Inside the objects `describe_shapes` shows, an id appears as the document
keeps it, `{"idx", "version"}`, which `describe_document` gives beside each
number as `key`.

## Trust

The port is loopback only and carries no secret: any process on the same
machine can reach a running window's document — the trust a local script
has always had over an application's files. Nothing listens on other
interfaces.

## Not yet

A rendering of a page as an image, for a model to look at — the PDF is the
artifact to inspect. Tool calls through a real client, once one is logged
in. The crate is `crates/tessera_bridge`; a tool is a name, a
sentence, a schema and a function, and adding one is adding one entry to
`tools::ALL`.
