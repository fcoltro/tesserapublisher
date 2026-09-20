# Resource panels and icon clarity

Extended the Properties-panel redesign to Pages, Layers, Styles, Swatches, Glyphs, Book, Preflight and AI Console.

## Design decisions

[NN/g's icon-usability research](https://www.nngroup.com/articles/icon-usability/) supports visible labels for actions whose meaning is not universally recognizable. Primary creation actions now pair a vector icon with text. Secondary commands sit in named action menus; layer visibility and lock icons remain directly accessible because they communicate state.

- Pages and Layers expose creation actions before their lists. Layer rows have larger hit areas, an accent selection state and clipped long names.
- Styles uses text-only category tabs, clearer empty states and named edit/duplicate/delete actions.
- Swatches separates the palette list from the selected colour's settings. Names commit after editing, and duplicate or empty names are rejected.
- Glyphs uses full-width font and search controls, with insertion guidance and a no-results state.
- Book groups creation, chapters and publishing; chapter operations no longer appear as repeated arrow/delete glyphs.
- Preflight separates the summary from its refresh action and explains how to locate an issue.
- AI Console provides setup guidance, a Preferences shortcut and an explicit Send prompt button.
- The floating multi-selection toolbar groups twelve actions into Align, Distribute and Transform menus.
- Vector icons use their native two-unit stroke, with existing round caps and DPI-aware flattening, for more legible rendering at 18 points. No egui fork or icon-font dependency was added.

## Bug fix

Swatch renaming previously removed the original definition, leaving its references unresolved. `Document::edit_swatch` now updates references in object fills, gradient stops, strokes, shadows, tables, text, styles and alias swatches. The command is a single undoable edit and preserves palette order.

## Validation

- Current combined repository: `cargo test -p tessera_ui -p tessera_document --offline` passed 1,243 tests; one platform-specific test ignored.
- `cargo clippy -p tessera_ui -p tessera_document --all-targets --offline -- -D warnings` passed.
- `cargo build -p tessera_app --offline` and `cargo fmt --all -- --check` passed.
- Added regression coverage for populated and empty resource panels at 208- and 288-point content widths, swatch references after renaming, collision rejection and single-step undo.
- Native visual inspection was interrupted by the user's Escape key. Live appearance and pointer interactions were not verified in this pass.

Concurrent repository changes were preserved. Test totals include that work; this report does not attribute it to this panel cleanup.
