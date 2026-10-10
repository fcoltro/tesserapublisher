# Tessera Publisher

An InDesign-class desktop publishing application in native Rust: egui/eframe
for the interface, Vello on wgpu for the document surface, parley for text,
`pdf-writer` for PDF/X export. No webview, no TypeScript. GPL-3.0-or-later.

## Layout

One Cargo workspace. `apps/tessera_app` is the binary; everything else is a
library under `crates/`:

| Crate | What it owns |
|---|---|
| `tessera_geometry` | Document and screen coordinate spaces, kept as distinct types |
| `tessera_color` | RGB, CMYK and spot colour, ICC profiles (Little CMS, vendored) |
| `tessera_text` | Fonts, shaping, the story model, the editable text buffer |
| `tessera_document` | The document model, undo/redo, the `.tsrdf` file format |
| `tessera_io` | Filesystem primitives and image decoding |
| `tessera_layout` | Where things go, without drawing them |
| `tessera_render` | A resolved document to pixels |
| `tessera_pdf` | PDF and PDF/X-1a / X-4 export |
| `tessera_preflight` | What is wrong with a document before it goes to press |
| `tessera_import` | Reading other applications' documents (IDML and others) |
| `tessera_html` | Export as a web page |
| `tessera_bridge` | Lets a model drive Tessera the way a person does |
| `tessera_ui` | Theme, tools, commands, panels, viewport |

Docs worth reading before a large change: `docs/superpowers/specs/` (design
decisions and their rejected alternatives), `docs/INDESIGN-PARITY.md`,
`ROADMAP.md` (acceptance criteria per milestone; `[~]`/`[ ]` entries state
their own shortfall) and `docs/USING.md`.

## Build, lint, test

The toolchain is pinned to an exact version in `rust-toolchain.toml`. These are
the checks CI runs, and they are run locally before every push (CI is manual
only, `workflow_dispatch`):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib --tests
```

- A cold build takes a few minutes; the full test run is roughly 2,600 tests.
- Test one crate with `cargo test -p tessera_pdf` while iterating.
- GPU tests in `crates/tessera_render/tests/gpu_render.rs` are `#[ignore]`d.
  Run them by hand and alone (`-- --ignored`): two GPU test binaries contending
  for one adapter deadlock. Cloud containers have no GPU.
- Linux needs `libgtk-3-dev libxkbcommon-dev libwayland-dev` and a C compiler
  (Little CMS is built from vendored source). The SessionStart hook in
  `.claude/hooks/` installs these in cloud sessions.
- `cargo run -p tessera_app` launches the GUI; it needs a display.

## Conventions

- `unsafe_code` is forbidden workspace-wide.
- `Cargo.lock` is committed; build with `--locked` in automation.
- Bumping the toolchain means bumping `rust-toolchain.toml`, both workflows'
  `dtolnay/rust-toolchain@` tags, and fixing new lints, in one commit.
- ICC profiles in `assets/profiles/` are checked in on purpose; every bundled
  profile needs its licence quoted in `assets/profiles/LICENCES.md`.
- Changes to the `.tsrdf` format bump `FORMAT_VERSION` in
  `crates/tessera_document/src/format/mod.rs`; commit messages cite
  it as "(format N)".
- Commit messages: a short plain title naming the feature ("Mixed inks"), then
  prose and bullets saying what a user gets and what is deliberately not built.
- Tests are named as sentences describing behaviour
  (`a_failed_open_reports_an_error_and_leaves_the_document_alone`).
- Errors are reported, never panicked on, for anything a user's file can cause.
