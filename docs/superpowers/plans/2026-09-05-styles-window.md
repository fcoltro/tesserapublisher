# The Styles Window — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** A window where paragraph and character styles are authored from
scratch, edited in full, duplicated and deleted — rather than only derived from
whatever happens to be selected.

**Architecture:** A floating, non-modal `egui::Window` in a new
`view/styles.rs`, opened by an action so the menu and the command palette both
get it for free. It reads and writes the document only through `Command`, so
every style edit is undoable. View state (open, which tab, which style is being
edited) lives on `TesseraApp`, not on the document.

**Tech Stack:** egui 0.35, existing `Command`/`apply` path, `tessera_text`
`CharacterFormat` / `ParagraphFormat`.

**Spec:** none written. This is one panel over a model that milestone 2 phase 4
already built and tested; the two decisions worth arguing are recorded below
rather than in a separate document.

## Global Constraints

- `unsafe_code = "forbid"` workspace-wide.
- Every document mutation goes through `Command`; a live gesture may bypass it
  only with an adjacent `undo-bracketed:` comment. Enforced by
  `tests/command_invariant.rs`.
- Clippy clean at `-D warnings`.
- British spelling in identifiers and copy (`Centre`, `colour`).
- A control that sets a value nothing draws must say so. Properties the shaper
  cannot honour yet go under a heading that names the shortfall.

## The two decisions

**D1 — Every row can say "inherit".** All nine `CharacterFormat` fields are
`Option`, and `None` means inherit. A window that always wrote a value would
make every style pin all nine properties, and a character style meaning only
"bold" would also fix family, size and colour — the cascade would collapse into
a flat list. Each row therefore carries an explicit set/inherit toggle, and an
unset row's control is disabled rather than hidden, so the reader can see that
the property exists and is deliberately not specified.

**D2 — Deleting a style keeps the look.** A style in use is deleted by folding
its resolved format into the local overrides of every span that referenced it,
across every story in the document and any live edit buffer, and then removing
the style. The alternatives are reverting the text to the document default,
which destroys work, and InDesign's "replace with…" dialog, which asks a
question the fold answers. It is one undo entry.

## File Structure

- **Create** `crates/tessera_ui/src/view/styles.rs` — the window: the list, the
  buttons, the property editor, and the `optional_row` helper D1 needs.
  `panels.rs` is already long and this is a separate responsibility.
- **Modify** `crates/tessera_ui/src/app.rs` — `StylesWindow` view state.
- **Modify** `crates/tessera_ui/src/view/mod.rs` — show the window.
- **Modify** `crates/tessera_ui/src/actions.rs` — one action, in a new `Type`
  group, which lights up the Type menu that milestone 1.5 left empty.
- **Modify** `crates/tessera_ui/src/command.rs` — two delete commands.
- **Modify** `crates/tessera_text/src/story.rs` — the fold D2 needs.
- **Modify** `crates/tessera_ui/src/view/panels.rs` — drop the inline
  field-editing block, now that the window owns editing. The two pickers stay:
  attaching a style to a selection is inspector work.

---

### Task 1: Folding a style back into the text

**Files:** `crates/tessera_text/src/story.rs`

**Produces:**
- `Story::flatten_character_style(&mut self, id, format: &CharacterFormat)` —
  every run referencing `id` keeps its appearance with the style gone: the
  style's format is merged *under* the run's own overrides, and `run.style` is
  cleared.
- `Story::flatten_paragraph_style(&mut self, id, style: &ParagraphFormat)` —
  the same for paragraphs, including the character half.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn flattening_a_character_style_keeps_the_appearance() {
    let mut story = Story::new("abcd");
    let id = CharacterStyleId::default();
    story.runs[0].local.size = Some(30.0);
    story.set_character_style(0..4, Some(id));

    let style = CharacterFormat {
        weight: Some(700),
        size: Some(9.0),
        ..CharacterFormat::default()
    };
    story.flatten_character_style(id, &style);

    assert_eq!(story.runs[0].style, None, "the reference is gone");
    assert_eq!(story.runs[0].local.weight, Some(700), "the style's own survives");
    assert_eq!(
        story.runs[0].local.size,
        Some(30.0),
        "and the run's override still beats it"
    );
}

#[test]
fn flattening_leaves_runs_using_another_style_alone() {
    // Two styles, one deleted. The other must not be touched, and slotmap
    // gives distinct ids so this is testable.
    // (Build with two ids from a real SlotMap in the document tests.)
}
```

- [ ] **Step 2: Run to verify they fail** — `cargo test -p tessera_text`,
  expected: `no method named flatten_character_style`.
- [ ] **Step 3: Implement.** `run.local = run.local.over(style)` — the run's own
  wins, which is the same precedence the version 6→7 migration used. Then
  `run.style = None`, then `merge_equal_neighbours`.
- [ ] **Step 4: Run tests** — expect pass.
- [ ] **Step 5: Commit** — `Fold a style back into the text it styled`

---

### Task 2: Deleting a style

**Files:** `crates/tessera_ui/src/command.rs`

**Interfaces:**
- Consumes: `Story::flatten_character_style`, `flatten_paragraph_style`.
- Produces: `Command::DeleteCharacterStyle { id }`,
  `Command::DeleteParagraphStyle { id }`.

The handler resolves the style's format **before** removing it, folds it into
every story in the document and into the live edit buffer if there is one, then
removes it from the table.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn deleting_a_style_in_use_keeps_the_text_looking_the_same() {
    let (mut state, _, story) = a_text_frame("abcd");
    apply(&mut state, Command::DefineCharacterStyle(CharacterStyle {
        name: "Lead".to_string(),
        format: CharacterFormat { size: Some(30.0), ..CharacterFormat::default() },
    }));
    let id = state.active().document().character_styles.keys().next().unwrap();
    apply(&mut state, Command::SetCharacterStyleOf { story, range: 0..4, style: Some(id) });

    apply(&mut state, Command::DeleteCharacterStyle { id });

    let doc = state.active().document();
    assert!(doc.character_styles.is_empty());
    let s = doc.story(story).unwrap();
    assert_eq!(s.runs[0].style, None);
    assert_eq!(s.resolve_run(&s.runs[0], doc).size, Some(30.0), "same size, no style");
}
```

- [ ] **Step 2: Run to verify it fails.**
- [ ] **Step 3: Implement**, plus a `Document::remove_character_style` /
  `remove_paragraph_style` that bump the revision.
- [ ] **Step 4: Run tests.**
- [ ] **Step 5: Commit** — `Delete a style without changing how anything looks`

---

### Task 3: The window

**Files:** `crates/tessera_ui/src/view/styles.rs` (create),
`crates/tessera_ui/src/app.rs`, `crates/tessera_ui/src/view/mod.rs`,
`crates/tessera_ui/src/actions.rs`

**Produces:**
- `pub struct StylesWindow { pub open: bool, pub tab: StyleKind, pub character: Option<CharacterStyleId>, pub paragraph: Option<ParagraphStyleId> }` on `TesseraApp`
- `styles::show(ui, state)`
- `Run::ToggleStyles` in a new `Group::Type`, named "Paragraph and character
  styles", shortcut `F11`.

Left of the window: a Paragraph/Character switch, the list of names, then New,
Duplicate, Delete. Right: the selected style's properties, every row with the
set/inherit toggle D1 requires. Properties the shaper cannot honour sit under a
heading saying so.

- [ ] **Step 1: `StylesWindow` on `TesseraApp`, defaulting closed.** Test that
  a fresh app has it closed, so the window cannot appear unasked.
- [ ] **Step 2: The action and the Type menu.** Test that `actions::all()`
  contains exactly one entry in `Group::Type` and that `Group::Type.menu()` is
  `"Type"` — the menu bar builds itself from this, so the test is what proves
  the menu appears.
- [ ] **Step 3: `optional_row`, and a test of it as a pure function** — the
  set/inherit decision is logic, not drawing: given `None` and a toggle, the
  result is `Some(default)`; given `Some(v)` and a toggle, `None`.
- [ ] **Step 4: The window body.** New creates a style with every field unset
  and a fresh name; Duplicate copies the selected one; Delete runs task 2's
  command and clears the view selection.
- [ ] **Step 5: Show it from `view::mod`, run the suite, clippy.**
- [ ] **Step 6: Commit** — `A window for authoring styles`

---

### Task 4: Retire the inline editor

**Files:** `crates/tessera_ui/src/view/panels.rs`

The inspector's indented Name/Style size/Style align block goes: the window owns
editing, and two places to edit one thing is how they drift apart. The two
pickers and their New buttons stay — attaching a style to a selection is what an
inspector is for.

- [ ] **Step 1: Remove the two indented blocks.**
- [ ] **Step 2: Run the suite and clippy.**
- [ ] **Step 3: Commit** — `Edit a style in one place`

---

## Closing

- [ ] Full non-GPU suite; GPU alone; clippy at `-D warnings`.
- [ ] **Perform the sentence, by hand:**

> Open the styles window with F11. Make a paragraph style from scratch, setting
> only its size and alignment and leaving the rest inherited. Apply it to two
> paragraphs. Duplicate it, change the copy's size, and apply the copy to one of
> them. Delete the original and watch the paragraph still using it keep its
> appearance.

- [ ] Record in `ROADMAP.md` under milestone 2, including which properties the
  window can set but the shaper cannot yet draw.
