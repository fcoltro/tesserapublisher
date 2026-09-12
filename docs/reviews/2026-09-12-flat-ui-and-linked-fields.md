# Flat UI, dock tabs and linked fields

The dark canvas, panel, toolbar and separator colors were sampled from the supplied prototype: `#111113`, `#161618`, `#1C1C1F` and `#2A2A2F`. Controls use recessed fields and neutral selection fills; popup/window shadows and hover expansion are disabled. Property groups have separator lines. Noto Sans, its 300 body weight and the 13-point body size are retained.

Dock tabs no longer use a panel's persistent open flag to color inactive icons. A shared tab widget paints icon and title together, with a consistent 18-point icon size, source stroke weight of 1.5, hover/focus treatment and accessible selected state. Properties, pages, layers, styles, swatches and preflight have distinct publishing-related symbols. Styles sub-tabs use the same widget and scroll if necessary. Clicking a tab shows its content immediately; the Preflight status link also activates its dock tab.

Document tabs and their dirty dots now appear beside the status message at the bottom. The document strip scrolls within its allocated space and reveals the active document when it changes. Close actions retain the existing unsaved-change guard and use a vector icon with an accessible name.

Margins, bleed, slug, text-frame insets and text-wrap standoff have independent lock controls. Locks are scoped to the current document/object and group. Linking alone does not change stored values; editing any linked edge applies that value to all four edges through one document command. Corner linking is also remembered instead of being recomputed from equal radii every frame, which previously prevented unlinking uniform corners reliably.

Validation for this pass is limited to source/diff review and formatting. Regression tests were added for linked-edge propagation, independent values, unchanged values when linking alone, and undo of a linked margin change. They have not been run on the final changes. The user explicitly requested no further builds or app launches; the release executable has not been updated for this pass.
