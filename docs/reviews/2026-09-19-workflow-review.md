# Workflow bug review and UI improvements — 2026-09-19

Reviewed the native Rust application on Windows, concentrating on document creation, modal input routing, search and replacement, story editing, and interactions between document tabs and live edit buffers. Also inspected save/open, recovery, export configuration, and the existing review regressions. The findings below are fixed in the working tree.

## Findings and fixes

1. **[P1] Document shortcuts escaped several modal dialogs.**
   `view/mod.rs::modal_open` omitted Print, Story editor, Spelling, and the long-document dialogs. With no text field focused, Delete could reach the selected document object behind a modal; other global shortcuts could also run. The shared guard now includes every `egui::Modal` represented by application state. A regression sends Delete through the actual accelerator handler for 16 dialogs and checks that the selected object survives.

2. **[P1] Find and Change could replace a result the user had never confirmed.**
   The window remembered only a result-list index. Find in document A, change the query or switch to B, then Change: the old index could select unrelated text in the new result list. The result position is now tied to the document key, revision, and complete query. Context changes clear it; Change is disabled until a new result is found. Tests cover query changes, tab switches, and edits after finding.

3. **[P1] The generic replacement command left a stale canvas buffer.**
   `Command::ReplaceMatches` changed the stored story without ending a matching live edit. Callers that did not explicitly close the editor could have their replacement overwritten by the next canvas keystroke. The command itself now ends only the affected editing session and clears its cell identity. Regression coverage checks the stored text, buffer teardown, and Undo.

4. **[P1] Story editor drafts could overwrite another document or newer copy.**
   The modal retained a story ID without its document identity, then applied against the current document. Story IDs can coincide across documents, and an external edit could also change the story while the modal was open. The editor now remembers the source document and original text. It refuses conflicting application, keeps the draft available to copy, and displays the reason beside the disabled Apply button. Tests exercise both a document switch and an external text update.

5. **[P2] A configured empty document was mistaken for the disposable startup page.**
   Creating a document with custom pages and print setup left it clean and untitled. Before drawing a frame, another New operation could preview into or replace it; quitting could discard its setup without a save prompt. Explicit creation now marks that setup as unsaved work. A regression creates a five-page document and verifies that previewing and creating a second document preserve the first.

6. **[P2] Replacement text containing the search term trapped Change on the same occurrence.**
   Replacing `cat` with `catfish` and pressing Change again could produce `catfishfish`. Navigation now resumes beyond the inserted bytes before wrapping, so successive changes reach the remaining original occurrences. A regression verifies `cat cat cat` becomes `catfish catfish cat` after two changes.

7. **[P2] Search could select an invisible result.**
   Finding text selected its frame and range, but did not turn to that spread or request camera movement. Search now exits parent-page editing, selects the owning spread, and sends a reveal request through the viewport's existing camera path. A regression finds a frame on the second spread while the first is active.

8. **[P2] Reversed replacement ranges were accepted.**
   The shared command checked bounds and UTF-8 boundaries but not `start <= end`. A reversed range could still insert replacement text despite representing no valid replacement. Such ranges are now ignored, preserving text and the live editor. Tests also cover out-of-bounds and mid-character offsets.

9. **[P2] New-document settings were not validated at the creation and preview boundaries.**
   Invalid dimensions or margins could create unusable page geometry. A programmatic zero-page preview request could loop forever because `remove_page` refuses to remove the last page while the preview loop kept retrying. The dialog, preview, creation function, and bridge now share validation. The removal loop also stops when removal is refused. Tests verify invalid requests leave document data untouched. The zero-page case concerns the public programmatic preview boundary; the normal page-count widget already clamps its value.

10. **[P2] Landscape dimensions disagreed with the resulting page.**
    Width and Height edited the unrotated preset values even when Landscape was selected. They now display the effective page dimensions; manual dimension changes update orientation and preset recognition. Native UI inspection verified A4 switching from 210 × 297 mm to 297 × 210 mm in the fields, summary, and preview.

## UI/UX changes

- Find and Change has aligned, labelled fields, visible document scope, Previous navigation, Enter/Shift+Enter shortcuts that retain search focus, and separate navigation/replacement rows.
- Change is unavailable until an occurrence has been found in the current context. Tooltips explain unavailable actions and Change all's undo behavior.
- Replacing the final occurrence reports successful completion instead of replacing that feedback with “Not found.”
- New document shows effective dimensions, page count, and facing-page mode before creation, with inline validation and a disabled Create button for invalid settings.
- Story editor shows word/character counts, explicit Apply changes/Cancel actions, and conflict feedback while retaining a draft that cannot safely be applied.

## Validation

- `cargo test --workspace --lib --tests --locked --offline`: 2,020 passed, 11 ignored, no failures.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Native app build succeeded. The rebuilt Windows app was launched and inspected: New document layout, landscape dimensions and preview, unsaved document indication, and Find and Change layout/focus/disabled actions.
- Added 14 regression tests, including a headless egui interaction test for repeated Enter and Shift+Enter navigation.

Logs are in `target/review-final-tests.log`, `target/review-clippy.log`, `target/review-build.log`, and `target/review-keyboard.log`.

The ignored tests include the repository's GPU tests; they were not enabled in this pass. macOS/Linux UI behavior and every publishing workflow were not manually exercised. The configured code-review-graph tools were not available in this session, so the review used source inspection and executable regression tests.
