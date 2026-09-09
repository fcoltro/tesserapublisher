# Tessera Publisher

A professional desktop publishing application — an InDesign-class layout tool,
free, and genuinely cross-platform.

Linux is a first-class target rather than an afterthought. The absence of a
serious DTP application on Linux is the reason this project exists.

> **Status: milestones 0 to 7 are built.** Tessera lays out pages, sets and
> threads type, places artwork, manages colour through real press profiles,
> preflights a job and exports PDF/X. Milestone 8 — installers, signing and
> documentation — is under way. The previous Tauri + Svelte implementation was
> discarded in full on 2026-09-01; this rebuild is clean-room and reuses none
> of it.
>
> What is not yet true is recorded rather than glossed: see the `[~]` and `[ ]`
> entries in the [roadmap](ROADMAP.md), each of which states its own shortfall.

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
- **[Instrument spec](docs/superpowers/specs/2026-09-03-instrument-milestone-design.md)** —
  the interface design: eight numbered decisions, what InDesign gets right,
  and the three of its surfaces Tessera refuses.
- **[InDesign parity](docs/INDESIGN-PARITY.md)** — every element of an
  InDesign window, read against this codebase, priced as a model gap or a
  missing surface, and assigned to a milestone.
- **[Using Tessera](docs/USING.md)** — for somebody who has opened it and wants
  to lay something out: the tools and their keys, threading, colour, preflight
  and what Tessera refuses to claim on your behalf.
- **[Releasing](docs/RELEASING.md)** — how an installer is built, what each
  platform carries, and why signing is not done by CI.
- **[Roadmap](ROADMAP.md)** — milestones 0 through 8. Each states its
  acceptance criteria as sentences a person can perform, not as a list of
  components that exist.
- **[Milestone 0 plan](docs/superpowers/plans/2026-09-01-milestone-0-walking-skeleton.md)** —
  24 tasks taking the project from an empty workspace to an application that
  can save, reopen and export a document.

## Building

```bash
cargo run -p tessera_app
```

Little CMS is built from vendored source, so a C compiler is needed; nothing has
to be installed system-wide.

### Colour profiles, once

```bash
python tools/vendor-profiles.py
```

Optional, and Tessera runs without it — the list of output intents is simply
shorter. What it fetches, and why the list is assembled from three places rather
than one, is worth a paragraph:

- **The RGB working spaces are built, not shipped.** sRGB, Adobe RGB (1998)
  compatible, Display P3, ProPhoto RGB and Rec. 2020 are each defined by three
  primaries, a white point and a transfer curve, all published in standards. They
  are constructed at runtime from those numbers, so they are always available and
  are colorimetrically exact.
- **The CMYK presses cannot be.** A CMYK profile is *measured* — thousands of
  printed and read patches, with no formula behind it. `Coated FOGRA39` and
  `U.S. Web Coated (SWOP) v2` are Adobe’s builds of public characterisation data:
  the data is public, the files are not ours to redistribute. So Tessera **reads
  the profiles already installed on the machine**, which on any machine with a
  creative suite on it is the whole standard set — and means a document proofed
  here is proofed against the same bytes the next application will use.
- **The free CMYK presses are the CGATS.21-2 reference printing conditions**,
  seven of them from cold-set news to premium coated, published through the ICC
  registry and granted for sharing. **These are checked in**, so a normal
  checkout already has them and the script is only needed to refresh them. They
  may not be *sold*, so a build that will be — or a distribution package that
  requires the
  freedom to — runs `--skip idealliance-crpc` and relies on the discovered
  profiles instead, which is what Scribus does.
- **Nothing fetched is committed.** The repository carries URLs and licence terms,
  never profiles, so Tessera’s source can be redistributed and sold by anyone
  without a thought about profile licensing.
- More on what else is worth adding: that is what the script is for.
  See [assets/profiles/CANDIDATES.md](assets/profiles/CANDIDATES.md) for which
  presses are free, which are not, and what has to be confirmed before one can be
  added. Every bundled profile needs its terms quoted in
  [LICENCES.md](assets/profiles/LICENCES.md); the script refuses a profile whose
  licence is not written down.

The script verifies rather than trusts: a download that has rotted into an error
page, been truncated, or turns out to be a different colour space than expected is
reported and discarded rather than bundled. A colour-managed application that
proofs against nonsense will be believed, which is why.

## Licence

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).
