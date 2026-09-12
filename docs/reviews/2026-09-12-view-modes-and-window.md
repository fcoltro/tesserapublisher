# View modes, slug guides and window startup

The preview path used `resolved.pages.first()` for its clip, hiding every other page in Preview, Bleed and Slug. The renderer now clips against a compound path containing each page's revealed rectangle. Nonzero winding preserves overlapping bleed areas, and separate subpaths keep the pasteboard between spreads hidden. Paper extends to the selected bleed or slug area.

Slug mode includes both bleed and slug extents; those distances are independent in the document model. Normal mode now draws a blue slug boundary when it differs from trim and bleed. Printing modes suppress non-printing guides. The GPU clear colour now uses the selected mode's surround instead of replacing preview's neutral grey with the theme background.

Menu mode selections are idempotent and highlight the active mode. The Preview shortcut still toggles Preview/Normal.

Startup now creates the restored-size window before requesting native maximization on the first logic frame. Previously egui-winit could apply an inner-size request after creating an already-maximized window. The native wrapper also refits the document once the maximized viewport is reported, so the initial camera does not retain the smaller canvas dimensions. Subsequent user window actions are not overridden.

Interface icons use a shared 18-point grid and a 1.5-unit stroke on the source 24-unit geometry. Button hit areas remain larger than their glyphs. Toolbars, dock tabs, panel controls, status controls, settings and style icons share that scale. Small disclosure arrows and document manipulation cursors retain their contextual geometry. The theme pill has been replaced with an outline sun/moon button, including tooltip, accessible action name and keyboard focus indication.

## Validation

- Workspace tests passed (1,574 tests; GPU tests and one existing colour test are opt-in).
- Eight GPU tests passed in a separate serial run. New pixel tests check all pages, gaps between spreads, trim/bleed/slug extents, expanded paper, blue slug outlines and suppression of guides in printing modes.
- UI regressions cover four pages across spreads, one-sided slug with larger bleed, idempotent mode selection and the W shortcut.
- Formatting, workspace Clippy with warnings denied and the optimized Windows build passed.
- Native startup was observed at 2560 × 1392, compared with the previous incorrectly maximized 1280 × 863 window.
- The final release opens with the page fitted to the maximized canvas. A four-page document was created through the UI; View → Preview was selected, then navigation showed the facing pages 2–3 and the standalone page 4. W returned to Normal.

Logs: `target/view-fixes-workspace-tests.log` and `target/view-fixes-gpu-tests.log`.
