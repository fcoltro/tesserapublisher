# UI/UX assessment and improvements — 2026-09-11

The interface is suitable for continued development and controlled layout experiments. It is not yet ready to replace a production publishing application: the document-integrity, text-composition and export issues in the [code review](2026-09-11-code-review.md) remain relevant. This pass improves actual interactions and the existing visual system without changing the document format or rendering architecture.

## Bugs addressed

| Problem | Result |
| --- | --- |
| Closing the native window could abandon unsaved documents. | Native close requests check every open document. The modal offers Save all and quit, Cancel, and Discard and quit. A failed or cancelled save keeps the application open and restores the active tab. Recovery is not discarded on an unconfirmed dirty shutdown. |
| Document shortcuts could act while an inspector field owned the keyboard. | Application accelerators respect egui keyboard focus. Delete in a field no longer deletes the selected object. |
| An open story could also receive text typed in an inspector field. | Story input respects field focus; Enter/Escape in fields cannot finish an in-progress pen path. |
| The viewport retained a second set of fixed tool, preview and paint shortcuts. | Those actions use the central remappable shortcut dispatcher. Menu entries and tooltips display configured shortcuts. Leaving the Pen tool through the toolbar also commits the path. Backspace remains an explicit conventional delete alias. |
| Commands and raw canvas handlers could act behind dialogs. | New document, command search, document close, application quit, PDF export and step/repeat use modal handling. Canvas and ruler input respect modal state; onboarding stays behind dialogs. |
| New document could not be cancelled with only one document open. | Cancel and Escape dismiss it. Existing document contents stay visible when preview is off. |
| Command search could use stale results/highlight positions. | Results are filtered after text input, highlights are clamped/reset, Enter uses current matches, and keyboard navigation scrolls the selected row into view. |
| Preflight could reuse results from another document at the same revision. | Cache identity includes the active document, revision, resolution threshold and exact bleed value; replacing a document invalidates it. |

## Visual and interaction improvements

- Larger base text, section headings, control heights and button padding.
- Selection fills that retain readable foreground text; visible keyboard focus on custom tool buttons.
- Consistent dialog surfaces, margins, rounded corners, backdrop dimming and prominent primary actions.
- A persistent document tab with a fixed-width unsaved indicator, including when only one document is open.
- Command search with full-width rows, aligned shortcut hints, an empty state and explicit keyboard instructions. Its top edge remains stable while the result count changes.
- Wider paired-field label columns to avoid collisions with labels such as Bottom and Outside.
- Horizontal scrolling in the contextual control bar to keep controls reachable in narrower windows.
- Menu availability for common actions such as Undo, Redo, Paste, Group, Ungroup, Place and Delete page reflects the current document/selection.

These choices follow the principles of visible focus and usable target sizing described by [W3C focus-visible guidance](https://www.w3.org/WAI/WCAG22/Understanding/focus-visible.html) and [WCAG 2.2 changes](https://www.w3.org/WAI/standards-guidelines/wcag/new-in-22/). This is not an accessibility certification; native screen-reader navigation and all custom controls still need a dedicated audit.

## Verification

- `cargo test --workspace --locked --offline`: 1,570 passed, 7 intentionally ignored.
- After final interaction adjustments, `cargo test -p tessera_ui --lib --tests --locked --offline`: 603 passed, including eight added regressions.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `cargo build -p tessera_app --locked --offline`: passed.
- Native Windows inspection: dark/light themes, final inspector label spacing, new-document Escape, command-search filtering and Enter activation, unsaved title/tab feedback, Alt+F4 confirmation, Escape cancelling quit, and saving a temporary document. The native test file is under ignored `target/ui-native-check.tessera`.

Regression coverage includes focused-field Delete, modal shortcut suppression, inspector typing with an open story, same-frame query/Enter handling, single-document dialog cancellation, dirty background tabs, cancelled/successful save-all callbacks, and preflight document identity.

## Remaining work

1. Fix the earlier grouped-object/clipboard ownership, text hit-testing/threading and export findings. Visual polish does not resolve these correctness problems.
2. Replace the global recovery slot with recovery for every dirty document; native close protection only addresses normal closing.
3. Test macOS and Linux installations, fractional display scaling, IME composition, screen readers and keyboard-only operation. Native interaction testing in this pass was Windows only; ignored GPU tests were not run.
4. Exercise long documents and dense text selections at the minimum window size. Scrollable controls improve access but do not establish complete responsive-layout coverage.
5. Improve editorial workflows: find/change, tables, manuscript import, richer paragraph controls and print/export validation. See the earlier review for the wider missing-tools assessment.

Build and verification logs are in ignored `target/ui-build.log`, `target/ui-tests.log`, `target/ui-workspace-tests.log`, `target/ui-clippy.log` and `target/ui-fmt.log`.

## Font and icon follow-up

Updated September 12: the UI embeds Noto Sans variable, with a 300-weight body instance and a 600-weight heading instance at normal width. Standard labels and captions use 13 logical points, scaled by the display DPI; ruler annotations currently use 9 points. Noto Sans is distributed under the SIL Open Font License; see `assets/fonts/OFL-NotoSans.txt` and [Google Fonts](https://github.com/google/fonts/tree/main/ofl/notosans).

The earlier claim that text-origin pixel alignment undoes subpixel rasterization was incorrect. egui 0.35 already stores fractional glyph offsets in its atlas. The galley origin must align to physical pixels so the GPU does not interpolate the rasterized coverage again. Text-origin alignment is now restored, with light outline hinting, subpixel binning and grayscale coverage antialiasing. Noto uses its supported `wght` and `wdth` axes rather than Inter's `opsz` axis. Geometry feathering is separate from glyph antialiasing.

Regression checks cover accented text, partially covered glyph edge pixels and texture alignment at 100%, 125%, 150% and 200% display scaling.

September 12 validation: 597 UI unit tests and 10 integration tests passed; workspace Clippy with warnings denied, formatting and the optimized Windows build passed. The running release was inspected at 1280 × 863: the new-document dialog, Properties controls, document tab and File menu fit their Noto labels; both light and dark themes were checked. Physical multi-monitor DPI transitions and minimum-window-size coverage remain unverified. The 9-point ruler annotations are still smaller than standard UI captions; enlarging them needs a corresponding ruler layout adjustment.

The toolbar and dock no longer reuse ambiguous shapes: Direct Select has an anchor pointer, Picture Box has a crossed image frame, Polygon has a polygon outline, Scissors has a scissor glyph, and the Properties, Pages, Swatches and Preflight docks each have dedicated symbols. Closed icon paths now use closed-line tessellation and open strokes receive round end caps, improving small-size antialiasing.
