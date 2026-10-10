# Fuzzing

Every reader that takes somebody else's file has a target here. Each one
feeds arbitrary bytes to the reader and checks that it answers with an error
and does not panic, hang or run out of memory.

| Target | Reads |
| --- | --- |
| `document` | a `.tsrdf`: the archive, its JSON, every migration |
| `idml` | an InDesign package |
| `docx` | a Word document placed as a story |
| `psd` | a placed Photoshop file's composite |
| `eps` | an EPS file's bounding box |
| `swatches` | an Adobe swatch exchange (`.ase`) file |
| `delimited` | a CSV or tab-separated data source |

These targets need a nightly compiler and `cargo-fuzz`. The
`rust-toolchain.toml` in this folder picks nightly.

```bash
cargo install cargo-fuzz
cd fuzz
cargo fuzz run idml                            # until stopped
cargo fuzz run idml -- -max_total_time=60      # for a minute
cargo fuzz run idml artifacts/idml/crash-…     # replay a crash
```

To give the fuzzer a start from real files, put some in
`corpus/<target>/` before the first run. For example, put a `.tsrdf` from
`crates/tessera_import/tests/frozen/` in `corpus/document/`.

CI runs each target for a minute every night when `main` has moved (see
`.github/workflows/ci.yml`). It keeps any crashing input as an artifact.
