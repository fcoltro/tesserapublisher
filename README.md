# Tessera Publisher

A professional desktop publishing application — an InDesign-class layout tool,
free, and genuinely cross-platform.

Linux is a first-class target rather than an afterthought. The absence of a
serious DTP application on Linux is the reason this project exists.

> **Status: rebuilding from zero.** This repository currently holds the design
> and the implementation plan, and no application code. The previous
> Tauri + Svelte implementation was discarded in full on 2026-09-01; the
> rebuild is clean-room and reuses none of it.

## Architecture

Native Rust throughout. One process, one renderer, one input queue.

| | |
|---|---|
| Interface | [egui](https://github.com/emilk/egui) 0.35 with `eframe` |
| Document surface | [Vello](https://github.com/linebender/vello) on wgpu, composited by egui |
| Text | [parley](https://github.com/linebender/parley) for shaping, with an editable buffer shared by the screen and the PDF writer |
| Geometry | [kurbo](https://github.com/linebender/kurbo) |
| Export | `pdf-writer`, targeting PDF/X-1a and PDF/X-4 |

There is no webview and no TypeScript.

## Documents

- **[Design](docs/superpowers/specs/2026-09-01-tessera-rebuild-design.md)** —
  architecture, the crate graph, five numbered decisions with their rejected
  alternatives, risks, and what is deliberately out of scope.
- **[Roadmap](ROADMAP.md)** — milestones 0 through 8. Each states its
  acceptance criteria as sentences a person can perform, not as a list of
  components that exist.
- **[Milestone 0 plan](docs/superpowers/plans/2026-09-01-milestone-0-walking-skeleton.md)** —
  24 tasks taking the project from an empty workspace to an application that
  can save, reopen and export a document.

## Building

Nothing to build yet. Once milestone 0 lands:

```bash
cargo run -p tessera_app
```

## Licence

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).
