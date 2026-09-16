# Deep project review — 2026-09-15

Reviewed commit `d1b1a408cb31641f1974fdb80b4a94cf2e6a37db` on Windows. **12 confirmed findings: seven P1 and five P2.** This report records the reproductions and the fixes now applied in the working tree.

P1 means a high-priority credential exposure, crash, unintended document mutation, or serious output corruption. P2 means a narrower functional or fidelity defect. Severity describes the consequence of the stated trigger, not its frequency.

## Findings

### 1. [P1] Preference tools disclose the configured API key

Location: [tools.rs:862](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/tools.rs:862), also the return at [tools.rs:877](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/tools.rs:877).

`get_preferences` serializes the entire preferences object, including `assistant.api_key`. `set_preferences` returns the same object even when changing an unrelated field. These are tools offered to both MCP clients and the built-in assistant. The console places their results in the conversation, so subsequent model requests carry the credential as message content as well as any intended authentication header.

Reproduction: set a headless app's key to a synthetic sentinel and call `get_preferences`; the result contains that sentinel at `assistant.api_key`. No real credentials were read or sent, and no provider request was made.

Return an explicitly filtered preference view from both tools; expose a configured/not-configured flag instead of the key. Keep credential updates separate from general preference reads and responses.

Probe: `preference_tool_must_not_return_api_key`.

### 2. [P1] An in-flight console edit can modify another document

Location: [console.rs:99](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/console.rs:99).

The driver sends context for the active document but does not retain that document's identity in `Running` or `Pending`. When the reply arrives, it executes against whichever document is active then. Frame IDs are local to each document and commonly overlap, so the ordinary frame-existence check does not protect against this.

Reproduction: start a console request for a text frame in document A, hold the transport response, create/switch to document B with its own first text frame, and release an A-targeted `set_text` call. B's `unrelated` text becomes `MODEL EDIT`; A is not edited. This uses the real `Driver::pump` with a deterministic fake transport.

Associate each turn and pending mutation with a document identity. Refuse or pause stale-context edits when the user switches/closes documents, or route them to the explicitly captured document. Deliberate model-requested document switches need to update that association explicitly.

Probe: `pending_console_edit_must_not_land_in_another_tab`.

### 3. [P1] Stop still executes tools returned by an outstanding request

Locations: [assistant.rs:220](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/assistant.rs:220), [console.rs:96](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/console.rs:96).

Cancellation is checked before `step()`, but not after the blocking HTTP call or between tools in its returned batch. The driver's Stop handler sets the flag and then continues draining and executing pending calls without checking it. A user stopping an unwanted edit can therefore still get that edit, including deletion or file-writing tools.

Reproduction: a fake transport sets the cancellation flag while answering the outstanding request with a `delete_frame` call. The tool callback runs once before the turn finally reports `stopped`.

Check cancellation after transport completion, before every tool, and at the UI-thread execution boundary. Supply cancellation results for unexecuted calls so the retained conversation remains valid.

Probe: `stop_during_http_must_prevent_subsequent_tool_execution`.

### 4. [P1] A valid JSON tool call with a mid-character offset panics

Location: [tools.rs:605](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/tools.rs:605); panic in [story.rs:1719](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_text/src/story.rs:1719).

`apply_paragraph_style` clamps byte offsets to the text length but does not verify UTF-8 boundaries. `paragraph_bounds` then directly slices at the supplied offsets. The live bridge and console execute this on the application thread without a panic boundary.

Reproduction: create text `éclair`, define a paragraph style, and apply it with `start: 1, end: 2`. The first slice panics because byte 1 is inside `é`. A model confusing character and byte offsets is enough to trigger this; no malformed JSON is necessary.

Reject invalid boundaries with a tool error before applying the command. Audit the generic command entry point and other text-range operations for the same invariant, since they also deserialize caller-supplied ranges.

Probe: `paragraph_style_tool_must_reject_mid_utf8_offset_without_panicking`.

### 5. [P1] Replacing text leaves the live editing buffer stale

Location: [command.rs:1029](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_ui/src/command.rs:1029); tool caller at [tools.rs:545](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/tools.rs:545).

`SetText` replaces only the document's story. If the frame is being edited, its `EditBuffer` retains the old story. The next canvas keystroke writes that buffer back over the document through `replace_story_from_edit`, undoing the external text replacement. The formatting commands already synchronize both copies, and the related `edit_text`/`set_cell_text` tools explicitly end editing; `set_text` does neither.

Reproduction: open an edit buffer containing `old`, then invoke `set_text` with `replacement`. The document contains `replacement` while the active edit buffer still contains `old`. The stale buffer was verified directly; the subsequent overwrite follows the existing keystroke writeback at [viewport.rs:1083](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_ui/src/view/viewport.rs:1083).

Synchronize or end the matching editing session inside the command so both the named tool and generic command path are protected. Re-clamp cursor/selection and clear composition state as appropriate.

Probe: `set_text_must_synchronise_live_edit_buffer`.

### 6. [P1] Grayscale JPEGs export as corrupt RGB images

Location: [images.rs:80](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_pdf/src/images.rs:80).

The JPEG pass-through recognizes dimensions but unconditionally marks the original compressed stream as `Space::Rgb`. A grayscale JPEG has one component, whereas the resulting PDF image declares three-component `/DeviceRGB`. The same branch does not distinguish four-component JPEGs either; the executed reproduction covers grayscale.

Reproduction: create a uniform 4×4 grayscale JPEG, place it in a graphic frame, and export without CMYK conversion. The PDF embeds the original JPEG with `/DCTDecode` and `/ColorSpace /DeviceRGB`. An independent Poppler render shows missing parts and a cyan patch instead of a uniform gray square.

Inspect the JPEG component/color model before pass-through. Emit a matching color space, including any required CMYK decode behavior, or decode unsupported JPEG modes into the declared sample format.

Probe: `grayscale_jpeg_must_not_be_declared_rgb`. Evidence: [PDF](C:/Users/hailmary/Downloads/tessera-publisher/target/review-2026-09-16/gray.pdf), [Poppler render](C:/Users/hailmary/Downloads/tessera-publisher/target/review-2026-09-16/gray-render.png).

### 7. [P1] Merging into an already merged neighbor breaks table invariants

Location: [table.rs:426](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_document/src/table.rs:426); caller at [command.rs:945](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_ui/src/command.rs:945).

`Table::merge` checks only the requested rectangle's outer bounds. It does not account for an existing cell span crossing that rectangle. Absorbing that cell's origin leaves its remaining covered slots orphaned. The command removes absorbed stories before `finish_table_edit` checks the invariant.

Reproduction: in a one-row, three-column table, merge columns 1–2, then merge columns 0–1. This is the same sequence available through the menu's merge-with-right-neighbor action. The second operation panics at `debug_assert!(table.spans_are_sound())`. With debug assertions disabled, the same code stores a table with an orphan covered slot.

Validate merges against complete existing spans before changing anything. Either expand to include the entire neighboring span or refuse partial overlaps; preserve a sound table on every return path.

Probe: `merging_into_an_existing_span_must_not_corrupt_table`.

### 8. [P2] Cross-document paste substitutes unrelated variable values

Location: [transfer.rs:343](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_document/src/transfer.rs:343).

The transfer remaps styles and artwork but copies variable marker characters unchanged and does not transfer variable definitions. A marker encodes an index into its document's variable table, not a global identity.

Reproduction: source variable 0 expands to `ACME`; destination variable 0 expands to `WRONG`. Copy a frame containing the source marker through `import_frames`, the clipboard transfer path. The copied text expands to `WRONG`. With no destination variable at that index, the text disappears.

Transfer referenced definitions and rewrite marker indices, including running-header style references. Handle exhausted variable capacity explicitly.

Probe: `cross_document_copy_must_preserve_variable_value`.

### 9. [P2] Footnotes retain source-document style IDs after paste

Location: [transfer.rs:343](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_document/src/transfer.rs:343).

The story transfer visits only the main story's runs and paragraph runs. Its nested `footnotes: Vec<Story>` is cloned without remapping named styles or referenced colors. Those IDs may resolve to unrelated destination resources.

Reproduction: a source footnote uses a 42pt character style; the destination has an unrelated 8pt style at the same slot-map ID. After importing the containing frame, the footnote resolves to 8pt. This is independent of the variable-table issue above.

Apply story/resource transfer recursively to footnotes, preserving local formatting and remapping their character/paragraph styles and colors.

Probe: `cross_document_copy_must_remap_footnote_styles`.

### 10. [P2] Word character styles shift after paragraph breaks

Locations: [docx.rs:191](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_import/src/docx.rs:191), [command.rs:1935](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_ui/src/command.rs:1935).

The DOCX builder inserts plain runs to cover synthesized paragraph separators. `run_style_names` contains entries only for source XML runs and is captured before those gap runs are inserted. `PlaceText` zips these differently shaped lists by position, assigning later styles to separators or preceding runs and leaving their intended text unstyled.

Reproduction: import two paragraphs, `one` and `two`, with distinct named character styles. The second style requests 30pt. After placement, `two` resolves to the document's 12pt default; the style was attached to the inserted newline instead.

Carry each style name with its run throughout construction, including a `None` entry for every synthetic gap, or assign styles by source range rather than a parallel positional list.

Probe: `word_character_styles_must_survive_paragraph_boundaries`.

### 11. [P2] Relative artwork paths break after save and reopen

Location: [command.rs:857](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_ui/src/command.rs:857); bridge input at [more.rs:359](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/more.rs:359).

Placement reads a relative path against the process working directory and stores it unchanged. Loading a saved document interprets that same path relative to the document's directory, as required for packaged links. Those two bases differ for normal documents saved in subdirectories.

Reproduction: place `target/review-2026-09-16/relative.png`, then save as `target/review-2026-09-16/relative.tessera`. Reopening resolves the image under `.../target/review-2026-09-16/target/review-2026-09-16/relative.png`, which does not exist.

Normalize placement paths to absolute paths at ingestion, or explicitly serialize links relative to the save destination. Preserve the intentional package-relative loader behavior.

Probe: `relative_placed_image_must_survive_save_and_reopen`.

### 12. [P2] Closing an older window removes the newer window's bridge discovery file

Location: [live.rs:101](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/src/live.rs:101).

Every GUI instance writes the same `bridge.port` file. `Listener::drop` unconditionally removes it, regardless of which listener currently owns the recorded port. Multiple GUI processes are possible through ordinary file-association launches.

Reproduction: start listeners A and B against the same scratch port file, then drop A. B is still listening, but `recorded_port` returns `None`. A newly launched MCP relay consequently falls back to a separate headless document instead of reaching the remaining window.

Remove the discovery record only when it still identifies the closing listener, using an ownership mechanism that also avoids startup/shutdown races. A multi-instance registry is another option if selecting among windows is intended.

Probe: `dropping_older_listener_must_keep_newer_port_record`.

## Fix status and verification

All 12 findings are fixed in the current tree. The fixes redact assistant credentials, bind console turns to their document, honor cancellation, validate UTF-8 ranges, synchronize live editing, decode unsupported JPEG color models, reject partial table-span overlaps, remap copied variables and footnote resources, preserve DOCX style alignment, normalize placed artwork paths, and conditionally remove the bridge record.

- `cargo test --workspace --locked --offline`: **1,942 passed, zero failed, ten ignored**. The ignored cases are nine GPU-dependent tests and one machine-profile reporting test.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passed.
- Twelve permanent regression tests in `crates/tessera_bridge/tests/review_regressions.rs` now pass, covering each original reproduction.
- The grayscale JPEG regression also writes an artifact-aware PDF during review runs; the original Poppler render was used to confirm the pre-fix corruption.
- Reviewed the new bridge and assistant workflow, document/resource ownership, live text buffers, save/open/recovery paths, table operations, import, and PDF image handling. Earlier review reports and regression tests were inspected to avoid repeating repaired defects.
- Native mouse/keyboard interaction, real provider requests, installers, other operating systems, GPU rendering, and comprehensive PDF/X conformance were not exercised. The inherited graph-tool instructions were checked; no code-review-graph tools are available in this session. Findings rely on source inspection and executable local reproductions.

The [regression source](C:/Users/hailmary/Downloads/tessera-publisher/crates/tessera_bridge/tests/review_regressions.rs), [original probe source](C:/Users/hailmary/Downloads/tessera-publisher/target/review-2026-09-16/probes.rs), [suite log](C:/Users/hailmary/Downloads/tessera-publisher/target/review-2026-09-16/fix-tests.log), and [Clippy log](C:/Users/hailmary/Downloads/tessera-publisher/target/review-2026-09-16/fix-clippy.log) provide the evidence. Scratch artifacts remain under ignored `target/`; the pre-existing untracked `.claude/` directory was left alone.
