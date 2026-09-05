# Milestone 2 — Typography

**Status: approved 2026-09-05.** Three of §7's four questions were put to the
author and answered; the fourth is decided below.

Milestone 1.5 closed on 2026-09-05 and this is the next thing in the roadmap.
It is architectural — a new subsystem, a format version bump, and a change to
the type the editable buffer is built on — so it went out as a proposal first
and the shape-changing questions were settled before any code was written.

**One answer went against the recommendation**, and it is the interesting one:
character styles are in, not deferred. See D2.

---

## 1. The problem, in one line of code

```rust
pub struct Story {
    pub text: String,
    pub style: TextStyle,   // ← one style, for all of it
}
```

`TextStyle` carries a family, a size, a line height and a colour, and there is
exactly one per story. **Nothing in the model can express a bold word.** Not a
heading followed by body text, not an italicised title, not a single character
at a different size.

That is the whole of milestone 2's difficulty. Everything else it asks for —
tracking, indents, styles that cascade — is comparatively ordinary once a
story can hold more than one formatting.

## 2. What the milestone asks for

> Set a paragraph in a chosen family, weight, size and leading. Adjust tracking
> and kerning. Set alignment and justification, indents, and space before and
> after. Define a paragraph style, apply it to several paragraphs, change the
> style, and watch every one of them update. Type in a language that needs an
> IME and see the composition preview on the canvas.

## 3. Decisions

### D1 — One string, with range-keyed runs. Not a tree of paragraphs.

```rust
pub struct Story {
    pub text: String,
    /// Character formatting. Non-overlapping, sorted, covering the whole text.
    pub runs: Vec<Run>,
    /// Paragraph formatting. Same invariant.
    pub paragraphs: Vec<ParagraphRun>,
}
```

*Rejected: `Story { paragraphs: Vec<Paragraph> }`,* which is the shape most
document models reach for and the shape InDesign's own object model has.

The reason is `EditBuffer`. Caret movement across grapheme clusters,
selection, word and paragraph selection, and their several hundred headless
tests are all written against **one contiguous `String` and byte offsets into
it**. A tree of paragraphs turns every offset into a (paragraph, offset) pair
and rewrites all of it — for no gain the range lists do not already give.

The cost of the flat form is that runs must be maintained across every edit,
which D4 addresses, and that "the third paragraph" is a search rather than an
index. That is a good trade for keeping a tested editing core intact.

**The invariant is what makes this safe:** runs are sorted, non-overlapping,
and cover `[0, text.len())` exactly. Anything else is a bug, and a property
test over generated edits is the place to catch it.

### D2 — Every format field is optional, and that is what makes a style cascade.

**Approved with both style kinds, against the recommendation.** The proposal
argued for paragraph styles first, on the grounds that the acceptance sentence
names only those and that character styles double the cascade's surface. The
decision was to build both, avoiding a second migration when they inevitably
arrive — the same argument that was *rejected* for `ColorRef` in phase B. It
lands differently here because the consumer is in this milestone rather than
three away.

So the cascade has four levels, not three:

```text
document default  ->  paragraph style  ->  character style  ->  local override
```

and that resolution order is fixed here and nowhere else.


```rust
pub struct CharacterFormat {
    pub family: Option<String>,
    pub size: Option<f32>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub tracking: Option<f32>,
    pub case: Option<Case>,
    pub baseline_shift: Option<f32>,
    pub colour: Option<Color>,
}

pub struct Run {
    pub range: Range<usize>,
    pub style: Option<CharacterStyleId>,
    /// Local overrides, applied over the style.
    pub local: CharacterFormat,
}
```

`None` means *inherit*, and resolution runs document default → paragraph style
→ character style → local override. That is the whole mechanism behind
"change the style and watch every one of them update": the runs hold a
reference and an override, never a resolved copy, so there is nothing to go
stale.

*Rejected: resolved formatting stored per run, with styles as a one-time
apply.* It is simpler to write and it makes the milestone's own acceptance
sentence impossible.

This is the same indirection `ColorRef` would have been in phase B, and it is
being taken here rather than deferred for the opposite reason: phase B had no
consumer for it before milestone 5, and this milestone's acceptance sentence
*is* the consumer.

### D2a — Styles live on the document, in their own arenas.

```rust
pub struct CharacterStyle { pub name: String, pub format: CharacterFormat }
pub struct ParagraphStyle {
    pub name: String,
    pub format: ParagraphFormat,
    /// Character formatting the paragraph imposes before any run speaks.
    pub character: CharacterFormat,
}
```

Held in `SlotMap`s on `Document`, beside `layers` and `pages`, so a style has
an id that survives a rename and a run refers to it by that id. A style
referenced by name would break the moment somebody renamed one.

### D3 — Font enumeration through parley, not a platform layer.

`parley::FontContext` owns a `fontique::Collection`, and
`Collection::family_names()` enumerates what the system has. That is one call
on all three platforms, and `apps/tessera_app/src/platform/` stays empty —
which the roadmap notes as a property worth keeping.

The list is cached and sorted once; a font installed while Tessera is running
is not picked up until asked for, and saying so is better than rescanning on
every frame.

**A named font the system does not have is substituted and marked.** That is
InDesign's behaviour and the only one of the three options that neither lies
about what will print nor refuses to open a file whose text is perfectly
readable. Milestone 6's preflight already has a missing-font rule to report
it; this milestone only has to make the substitution visible.

### D4 — Edits carry the runs with them.

Inserting inside a run extends it. Inserting at a boundary joins the run to
the **left**, which is what every editor does and what makes typing after a
bold word continue bold. Deleting a range removes the runs it wholly covers
and clips the ones it straddles; adjacent runs that resolve identically are
merged, or the list grows without bound over a long editing session.

This is the part most likely to harbour bugs, and it is pure data —
`(text, runs, edit) -> (text, runs)` — so it can be property-tested hard
without a window. **That should be built and tested before any interface
touches it.**

### D5 — Paragraph formatting hangs off the same mechanism.

Alignment, justification, indents, space before and after, and a hyphenation
flag, all `Option`, resolved through the same cascade. Paragraph boundaries
are derived from the text's own line separators rather than stored, so they
cannot disagree with the string.

## 4. Format

**Version 6, one bump.** `Story` changes shape, so every saved document with
text in it needs a migration: the existing single `TextStyle` becomes one run
covering the whole story and one paragraph covering the whole story. That is a
real rewriting migration, the second the project will have written, and
`rotation_to_transform` is the model to follow.

## 5. Shape of the work

Four phases, in dependency order, each producing working software:

1. **The run model** — `Run`, `CharacterFormat`, the invariant, and D4's edit
   arithmetic, all headless. No interface, no format change yet.
2. **The format bump** — version 6 and its migration, with a version-5
   document proven to open.
3. **Shaping runs** — `Shaper` takes runs rather than one style; the renderer
   and the PDF writer follow. This is where a bold word first appears.
4. **The typography inspector** — family, weight, size, leading, tracking,
   alignment, indents; then named styles and their cascade.

Right-to-left is verified in phase 3, where shaping lands — parley handles
bidi, so the work is proving it rather than building it. IME has moved out to
milestone 2.5.

## 6. Risks

**R1 — the edit arithmetic.** Runs that drift out of sync with the text are
corruption, not a glitch, and the symptom appears far from the cause. The
mitigation is that it is pure and property-tested before anything calls it.

**R2 — the migration touches every document with text in it.** Unlike phases
A–C's bumps, this one rewrites. It wants a hand-built version-5 archive in the
test suite, the way version 1 already has one.

**R3 — shaping cost.** `Shaper` currently caches per story. Runs make the
cache key more complicated, and the performance guard from A7 only covers
rectangles. It should be extended to a text-heavy document before phase 3, or
the guard will not be watching the thing that got slower.

## 7. The questions, and their answers

1. **Is IME in this milestone or its own?** -> **Its own.** It is a windowing
   concern rather than a text-model one, and the only item in the milestone
   that cannot be tested headlessly. Moved to **milestone 2.5**, so it stays
   visible rather than being folded into platform work where an unverifiable
   item quietly becomes an unverified one.
2. **A font a document names but the system lacks?** -> **Substitute and
   mark.** See D3.
3. **Character styles too, or paragraph only first?** -> **Both**, against the
   recommendation. See D2.
4. **Is vertical text in scope?** -> Not asked, not answered, not in scope. It
   changes the line-breaking model, so if it is ever wanted it wants its own
   decision rather than an assumption made here.
