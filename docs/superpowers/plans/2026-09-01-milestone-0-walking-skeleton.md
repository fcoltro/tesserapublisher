# Milestone 0 — Walking Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native Rust application that can draw a rectangle, let the user type text on the canvas, save the result to a `.tessera` file, reopen it unchanged, and export a PDF that opens in Acrobat.

**Architecture:** Nine library crates plus one `eframe` binary, dependencies pointing downward only. The document is an arena of plain serde-serializable structs. Vello renders the document into a texture on the *same* wgpu device egui owns, and egui composites it as an ordinary widget. Text shaping is owned by one crate and consumed by both the renderer and the PDF writer, so the export cannot drift from the screen.

**Tech Stack:** Rust (edition 2024) · egui / eframe 0.35 · wgpu 29.0.4 · vello 0.10 · kurbo 0.13 · parley 0.11 · skrifa 0.44 · slotmap 1.1 · serde + serde_json 1 · zip 8 · pdf-writer 0.15 · subsetter 0.2 · image 0.25 · rfd 0.17 · thiserror 2 · proptest 1

**The committed `Cargo.toml` is authoritative for versions**, not this line — it was written against the registry, and several guesses here were wrong (see Task 2).

**Spec:** [`docs/superpowers/specs/2026-09-01-tessera-rebuild-design.md`](../specs/2026-09-01-tessera-rebuild-design.md)

## Global Constraints

Every task's requirements implicitly include this section.

- **Clean-room. Nothing is reused.** No code from the previous Tessera is read, ported, or consulted — not from the working tree, not from git history, not from the GitHub remote. The only permitted references are upstream crate documentation, the oxiDRAFT project (for egui *conventions* — theme tokens, painter-drawn icons, module layout — never for copied code), and the Task 1 spike note.
- **Toolkit:** egui 0.35 with `eframe`, wgpu backend. No webview, no Tauri, no TypeScript.
- **Rust edition 2024.** Workspace-inherited `version`, `edition`, `license` — set once in `[workspace.package]`.
- **`unsafe_code = "forbid"`** in `[workspace.lints.rust]`. No exceptions.
- **Dependency versions live only in `[workspace.dependencies]`.** A crate writes `serde.workspace = true`.
- **Dependencies point downward only.** Nothing below `tessera_ui` may depend on `egui`. Nothing below `tessera_render` may depend on `wgpu`. `tessera_pdf` must **never** depend on `tessera_render`.
- **Platform-specific code lives only in `apps/tessera_app/src/platform/`.** Nowhere else, in any crate.
- **No silent fallbacks.** Every failure path returns an error that states its cause. Never `unwrap_or_default()` on something a user would want to know about.
- **Tests land in the same commit as the code they cover.**
- **GPU-backed tests run alone, in the foreground, never inside `cargo test --workspace`.** Adapter acquisition hangs intermittently on this hardware and a hang looks exactly like a slow compile. If a GPU test produces no output for two minutes, kill the test binary and any `cargo.exe`, then retry once.
- **Interactive verification is Windows-only.** Where a step says "verified", it means verified on Windows. Linux and macOS are built and headless-tested in CI, and recorded as **known-unverified**.
- **Commit `Cargo.lock`.** This is an application, not a library.

---

# Phase A — Prove the ground, then clear it

---

### Task 1: The Vello-in-egui spike — ✅ COMPLETE (2026-09-01)

**Verdict: PASS.** Vello 0.10 renders on the device eframe creates, and egui composites the result in the same frame. Decision D2 stands; risk R1 is closed.

**Read [`docs/superpowers/notes/2026-09-01-vello-egui-spike.md`](../notes/2026-09-01-vello-egui-spike.md) before Tasks 15 and 16.** It holds the code that actually ran. The speculative code below is kept only as the record of what was assumed; **the note supersedes it wherever they disagree**, and they disagree in four places:

1. **No `WgpuConfiguration` is needed.** Vello worked on eframe's stock device — zero features, wgpu's default `max_storage_buffers_per_shader_stage: 8`. Do not add one.
2. **`vello::Renderer` is `Send` but not `Sync`** (`RefCell<Vec<u8>>` at `wgpu_engine.rs:99`), while `CallbackResources` requires `Send + Sync`. It must be stored as `Mutex<Renderer>`.
3. **`eframe::App` has no `update` method in 0.35.** It requires `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame)`, with an optional `fn logic(&mut self, ctx, frame)` for work that does not paint.
4. **eframe chose Vulkan on Windows** and Vello rendered correctly. Do not force a backend.

Verified on an RTX 4070 Ti / Vulkan / Windows 11. Linux and macOS remain unverified.

**Files:**
- Create: `spike/vello_in_egui/Cargo.toml`
- Create: `spike/vello_in_egui/src/main.rs`
- Create: `docs/superpowers/notes/2026-09-01-vello-egui-spike.md`

**Interfaces:**
- Consumes: nothing.
- Produces: a written note containing the verified `WgpuConfiguration`, the exact `CallbackTrait` implementation, and the Vello render call. Tasks 15 and 16 copy from it verbatim.

- [ ] **Step 1: Create the throwaway spike crate**

This is *outside* the workspace — it is deleted in Task 2.

```toml
# spike/vello_in_egui/Cargo.toml
[package]
name = "vello_in_egui"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]          # standalone, not a member of anything

[dependencies]
eframe = { version = "0.35", default-features = false, features = ["wgpu"] }
egui = "0.35"
egui-wgpu = "0.35"
vello = "0.10"
pollster = "0.4"
```

- [ ] **Step 2: Confirm one wgpu in the dependency graph**

```bash
cd spike/vello_in_egui && cargo tree -i wgpu
```

Expected: exactly one `wgpu v29.0.4`, depended on by both `egui-wgpu` and `vello`. If two versions appear, **stop** — the architecture's fallback (CPU readback) is now in play, and the design must be revisited before continuing.

- [ ] **Step 3: Write the spike**

Draw a red circle with Vello into a texture, on egui's device, and show it under an egui button. The button proves egui's own rendering still works in the same frame.

```rust
// spike/vello_in_egui/src/main.rs
use std::sync::{Arc, Mutex};

use eframe::egui_wgpu::{self, CallbackTrait};
use egui::{Color32, Sense, Vec2};
use vello::kurbo::{Affine, Circle};
use vello::peniko::{color::palette, Fill};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

/// Lives in egui's `callback_resources`, so it is created once and reused.
struct VelloResources {
    renderer: Renderer,
    target: Option<(vello::wgpu::Texture, vello::wgpu::TextureView, u32, u32)>,
}

struct VelloCallback {
    scene: Arc<Mutex<Scene>>,
    width: u32,
    height: u32,
}

impl CallbackTrait for VelloCallback {
    fn prepare(
        &self,
        device: &vello::wgpu::Device,
        queue: &vello::wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut vello::wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<vello::wgpu::CommandBuffer> {
        let res: &mut VelloResources = resources.get_mut().expect("VelloResources missing");

        // (Re)allocate the target texture when the widget size changes.
        let needs_alloc = !matches!(res.target, Some((_, _, w, h)) if w == self.width && h == self.height);
        if needs_alloc {
            let texture = device.create_texture(&vello::wgpu::TextureDescriptor {
                label: Some("vello target"),
                size: vello::wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: vello::wgpu::TextureDimension::D2,
                format: vello::wgpu::TextureFormat::Rgba8Unorm,
                usage: vello::wgpu::TextureUsages::STORAGE_BINDING
                    | vello::wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            res.target = Some((texture, view, self.width, self.height));
        }

        let (_, view, _, _) = res.target.as_ref().expect("target just allocated");
        let scene = self.scene.lock().expect("scene mutex poisoned");
        res.renderer
            .render_to_texture(
                device,
                queue,
                &scene,
                view,
                &RenderParams {
                    base_color: palette::css::WHITE,
                    width: self.width,
                    height: self.height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .expect("vello render failed");

        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _pass: &mut vello::wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        // Intentionally empty: `prepare` fills the texture. The spike's second
        // half proves the texture can then be shown by egui.
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        wgpu_options: egui_wgpu::WgpuConfiguration {
            // Vello needs compute shaders and a raised storage-buffer limit.
            // This is the exact question the spike exists to answer.
            device_descriptor: Arc::new(|adapter| vello::wgpu::DeviceDescriptor {
                label: Some("tessera device"),
                required_features: vello::wgpu::Features::CLEAR_TEXTURE,
                required_limits: vello::wgpu::Limits {
                    max_storage_buffers_per_shader_stage: 16,
                    ..vello::wgpu::Limits::default().using_resolution(adapter.limits())
                },
                memory_hints: Default::default(),
                trace: Default::default(),
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    eframe::run_native(
        "vello in egui spike",
        options,
        Box::new(|cc| {
            let wgpu = cc.wgpu_render_state.as_ref().expect("wgpu backend required");
            let renderer = Renderer::new(&wgpu.device, RendererOptions::default())
                .expect("vello renderer creation failed");
            wgpu.renderer
                .write()
                .callback_resources
                .insert(VelloResources { renderer, target: None });
            Ok(Box::new(SpikeApp))
        }),
    )
}

struct SpikeApp;

impl eframe::App for SpikeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Proves egui's own rendering still works alongside Vello's.
            if ui.button("egui still draws").clicked() {
                println!("clicked");
            }

            let (rect, _) = ui.allocate_exact_size(Vec2::new(400.0, 300.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, Color32::DARK_GRAY);

            let mut scene = Scene::new();
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                palette::css::RED,
                None,
                &Circle::new((200.0, 150.0), 100.0),
            );

            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                VelloCallback {
                    scene: Arc::new(Mutex::new(scene)),
                    width: rect.width() as u32,
                    height: rect.height() as u32,
                },
            ));
        });
    }
}
```

- [ ] **Step 4: Run it**

```bash
cd spike/vello_in_egui && cargo run
```

Expected: a window opens, the button is clickable, and no panic occurs during `prepare`.

**This step is where the real API drift surfaces.** `Renderer::render_to_texture`'s exact signature, `RenderParams`' fields, `DeviceDescriptor`'s fields, and `CallbackTrait`'s method signatures move between releases. Fix the code against `cargo doc --open -p vello -p egui-wgpu` until it compiles and runs. **The corrected code is the deliverable** — do not move on with code that only nearly compiles.

- [ ] **Step 5: Show the texture through egui**

`prepare` fills a texture; egui must display it. Register it with `egui_wgpu::Renderer::register_native_texture` and draw it with `ui.painter().image(...)`. Confirm the red circle appears on screen. Record the working approach in the note.

- [ ] **Step 6: Write the spike note**

Create `docs/superpowers/notes/2026-09-01-vello-egui-spike.md` containing:

1. **Verdict** — shared device works, or it does not.
2. The exact `WgpuConfiguration` that produced a working device, with the real `required_features` and `required_limits`.
3. The exact working `CallbackTrait` implementation.
4. The exact working texture registration and draw call.
5. The adapter name and backend it was verified on.
6. Any API that differed from the code above, so Tasks 15 and 16 are not surprised.

- [ ] **Step 7: Commit the note only**

```bash
git add docs/superpowers/notes/2026-09-01-vello-egui-spike.md
git commit -m "Record the Vello-in-egui spike result"
```

The spike crate is **not** committed. It is deleted in Task 2.

---

### Task 2: Demolition and the workspace skeleton — ✅ COMPLETE (2026-09-01)

17,133 lines removed; ten crates scaffolded; `cargo fmt`, `cargo clippy -D warnings` and 9 tests all green; `tessera_app` runs. **The committed `Cargo.toml` supersedes the manifest below.** Five things differed from what this task assumed:

1. **A dependency cycle.** The spec had `tessera_io` depending on `tessera_document` (for link resolution and packaging) while Task 7's file format calls `tessera_io::atomic::write_atomic` — `document → io → document`, which Cargo rejects. Resolved by making `io` a *lower* crate: filesystem primitives and image decoding only, with no knowledge of the document model. Link staleness and packaging are operations on a document and live above it.
2. **`zip` is at 8.6.0**, not 2. Note that `cargo info zip` reports `9.0.0-pre3`; a prerelease is not something to pin a build to, so the version came from `cargo add`, which selects the latest stable.
3. **`pdf-writer` is at 0.15**, not 0.13.
4. **`tessera_pdf` also needs `tessera_layout`**, since it consumes `ResolvedDocument`. The spec listed only `document, text, color, geometry`.
5. **`.claude/launch.json` and the four Phase 4 design documents were also removed** — the former launched a Vite dev server that no longer exists, the latter describe the discarded architecture.

The three dependency-direction rules were verified with `cargo tree` rather than assumed: `tessera_pdf` does not reach `tessera_render`; no crate below `tessera_ui` reaches `egui`; no crate below `tessera_render` reaches `wgpu`. All three hold.

---

#### Original task, for reference

**Files:**
- Delete: `src/`, `src-tauri/`, `crates/core/`, `crates/renderer/`, `package.json`, `package-lock.json`, `svelte.config.js`, `vite.config.ts`, `vitest.config.ts`, `tsconfig.json`, `static/`, `spike/`
- Modify: `Cargo.toml`, `.gitignore`, `README.md`
- Create: `crates/tessera_{geometry,color,text,document,layout,render,io,pdf,ui}/{Cargo.toml,src/lib.rs}`
- Create: `apps/tessera_app/{Cargo.toml,src/main.rs}`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: nothing.
- Produces: the workspace every later task builds in. Crate names and the dependency direction rule.

- [ ] **Step 1: Delete the old tree**

```bash
git rm -r --quiet src src-tauri crates/core crates/renderer static
git rm --quiet package.json package-lock.json svelte.config.js vite.config.ts vitest.config.ts tsconfig.json
rm -rf spike node_modules .svelte-kit
```

**Nothing removed here is ever read again.** This is a clean-room rebuild by explicit decision: no file in the deleted tree is opened, ported, or consulted, from the working tree or from git history. Every crate is written from upstream documentation and the Task 1 spike note.

- [ ] **Step 2: Write the workspace manifest**

```toml
# Cargo.toml
[workspace]
resolver = "2"
members = [
    "crates/tessera_geometry",
    "crates/tessera_color",
    "crates/tessera_text",
    "crates/tessera_document",
    "crates/tessera_layout",
    "crates/tessera_render",
    "crates/tessera_io",
    "crates/tessera_pdf",
    "crates/tessera_ui",
    "apps/tessera_app",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
# Matches the LICENSE file actually shipping in this repository. The previous
# Cargo.toml declared MIT while shipping GPL-3.0 — a real inconsistency, fixed
# here rather than carried forward.
license = "GPL-3.0-or-later"
authors = ["Hailmary"]
publish = false

[workspace.dependencies]
tessera_geometry = { path = "crates/tessera_geometry" }
tessera_color    = { path = "crates/tessera_color" }
tessera_text     = { path = "crates/tessera_text" }
tessera_document = { path = "crates/tessera_document" }
tessera_layout   = { path = "crates/tessera_layout" }
tessera_render   = { path = "crates/tessera_render" }
tessera_io       = { path = "crates/tessera_io" }
tessera_pdf      = { path = "crates/tessera_pdf" }
tessera_ui       = { path = "crates/tessera_ui" }

egui       = "0.35"
eframe     = { version = "0.35", default-features = false, features = ["wgpu", "persistence"] }
egui-wgpu  = "0.35"
vello      = "0.10"
kurbo      = { version = "0.13", features = ["serde"] }
parley     = "0.11"
skrifa     = "0.44"
slotmap    = { version = "1", features = ["serde"] }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
zip        = "2"
image      = { version = "0.25", default-features = false, features = ["png"] }
pdf-writer = "0.13"
rfd        = "0.17"
thiserror  = "2"
pollster   = "0.4"
proptest   = "1"

# The geometry kernel and everything above it are 100% safe Rust.
[workspace.lints.rust]
unsafe_code = "forbid"

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = true
```

**Before committing, run `cargo add --dry-run` for `zip`, `pdf-writer` and `image` and take the current major release.** The versions above were correct when this plan was written; do not carry a stale pin forward silently.

- [ ] **Step 3: Create every crate with a smoke test**

For each of the nine crates, create `Cargo.toml` and a `src/lib.rs`. Example for the lowest one:

```toml
# crates/tessera_geometry/Cargo.toml
[package]
name = "tessera_geometry"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[lints]
workspace = true

[dependencies]
kurbo.workspace = true
serde.workspace = true
```

```rust
// crates/tessera_geometry/src/lib.rs
//! Document and screen coordinate spaces, kept in distinct types.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(true);
    }
}
```

The other eight follow the same shape, with dependencies exactly as listed in the spec's section 4.1 and nothing more. **`tessera_pdf` must not list `tessera_render`.**

- [ ] **Step 4: Create the application binary**

```toml
# apps/tessera_app/Cargo.toml
[package]
name = "tessera_app"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[lints]
workspace = true

[dependencies]
tessera_ui.workspace = true
eframe.workspace = true
egui.workspace = true
```

```rust
// apps/tessera_app/src/main.rs
fn main() {
    println!("Tessera Publisher");
}
```

- [ ] **Step 5: Un-ignore the lockfile and rewrite the README**

Remove the `Cargo.lock` line from `.gitignore` and remove the Node and Tauri sections. Replace `README.md` — it currently describes a Tauri + Svelte application that no longer exists. State the real stack, the build command (`cargo run -p tessera_app`), and link the design spec and roadmap.

- [ ] **Step 6: Add CI across three platforms**

```yaml
# .github/workflows/ci.yml
name: CI
on: [push, pull_request]

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - name: Install Linux GUI dependencies
        if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt-get update
          sudo apt-get install -y libgtk-3-dev libxkbcommon-dev libwayland-dev
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      # --lib and --test exclude GPU tests, which run manually and alone.
      - run: cargo test --workspace --lib
```

- [ ] **Step 7: Verify and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --lib
```

Expected: clean, ten crates compiled, nine trivial tests pass.

```bash
git add -A
git commit -m "Replace the Tauri workspace with the native Rust crate graph"
```

---

# Phase B — The document

---

### Task 3: `tessera_geometry` — two coordinate spaces that cannot be confused

**Files:**
- Create: `crates/tessera_geometry/src/spaces.rs`
- Create: `crates/tessera_geometry/src/view.rs`
- Modify: `crates/tessera_geometry/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `DocPoint { x: f64, y: f64 }`, `DocRect { x, y, width, height }`, `ScreenPoint { x: f32, y: f32 }`, `ViewTransform { pan: DocPoint, zoom: f64 }` with methods `doc_to_screen(DocPoint) -> ScreenPoint`, `screen_to_doc(ScreenPoint) -> DocPoint`, `doc_rect_to_screen(DocRect) -> (ScreenPoint, ScreenPoint)`, and `to_affine() -> kurbo::Affine`. All are `Serialize`/`Deserialize`, `Copy`, `PartialEq`.

Document units are **points** (1/72 inch) throughout — the unit PDF uses, so export needs no conversion.

- [ ] **Step 1: Write the failing round-trip test**

```rust
// crates/tessera_geometry/src/view.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::spaces::{DocPoint, ScreenPoint};

    #[test]
    fn screen_to_doc_inverts_doc_to_screen() {
        let view = ViewTransform { pan: DocPoint { x: 100.0, y: 50.0 }, zoom: 2.5 };
        let original = DocPoint { x: 42.0, y: -17.5 };

        let round_tripped = view.screen_to_doc(view.doc_to_screen(original));

        assert!((round_tripped.x - original.x).abs() < 1e-6);
        assert!((round_tripped.y - original.y).abs() < 1e-6);
    }

    #[test]
    fn zoom_scales_distance_from_the_pan_origin() {
        let view = ViewTransform { pan: DocPoint::ZERO, zoom: 2.0 };
        let screen = view.doc_to_screen(DocPoint { x: 10.0, y: 10.0 });
        assert_eq!(screen, ScreenPoint { x: 20.0, y: 20.0 });
    }
}
```

- [ ] **Step 2: Run it and confirm it fails**

```bash
cargo test -p tessera_geometry
```

Expected: FAIL — `ViewTransform` not found.

- [ ] **Step 3: Implement**

```rust
// crates/tessera_geometry/src/spaces.rs
use serde::{Deserialize, Serialize};

/// A point in document space. Units are points (1/72 inch), matching PDF.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DocPoint {
    pub x: f64,
    pub y: f64,
}

impl DocPoint {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

/// A point in screen space, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    pub x: f32,
    pub y: f32,
}

/// An axis-aligned rectangle in document space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DocRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DocRect {
    pub fn contains(&self, p: DocPoint) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }

    pub fn to_kurbo(self) -> kurbo::Rect {
        kurbo::Rect::new(self.x, self.y, self.x + self.width, self.y + self.height)
    }
}
```

```rust
// crates/tessera_geometry/src/view.rs
use serde::{Deserialize, Serialize};

use crate::spaces::{DocPoint, DocRect, ScreenPoint};

/// Maps document space to screen space. Owned by the viewport, never by the
/// document — panning is not an edit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewTransform {
    pub pan: DocPoint,
    pub zoom: f64,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self { pan: DocPoint::ZERO, zoom: 1.0 }
    }
}

impl ViewTransform {
    pub fn doc_to_screen(&self, p: DocPoint) -> ScreenPoint {
        ScreenPoint {
            x: ((p.x - self.pan.x) * self.zoom) as f32,
            y: ((p.y - self.pan.y) * self.zoom) as f32,
        }
    }

    pub fn screen_to_doc(&self, p: ScreenPoint) -> DocPoint {
        DocPoint {
            x: f64::from(p.x) / self.zoom + self.pan.x,
            y: f64::from(p.y) / self.zoom + self.pan.y,
        }
    }

    pub fn doc_rect_to_screen(&self, r: DocRect) -> (ScreenPoint, ScreenPoint) {
        (
            self.doc_to_screen(DocPoint { x: r.x, y: r.y }),
            self.doc_to_screen(DocPoint { x: r.x + r.width, y: r.y + r.height }),
        )
    }

    /// The equivalent affine, for handing to Vello.
    pub fn to_affine(&self) -> kurbo::Affine {
        kurbo::Affine::scale(self.zoom)
            * kurbo::Affine::translate((-self.pan.x, -self.pan.y))
    }
}
```

```rust
// crates/tessera_geometry/src/lib.rs
//! Document and screen coordinate spaces, kept in distinct types.
//!
//! Confusing the two is the most common source of defects in a zoomable
//! canvas, so they are different types and the compiler enforces it.

pub mod spaces;
pub mod view;

pub use spaces::{DocPoint, DocRect, ScreenPoint};
pub use view::ViewTransform;
```

- [ ] **Step 4: Run tests and confirm they pass**

```bash
cargo test -p tessera_geometry
```

Expected: PASS, 2 tests.

- [ ] **Step 5: Add a property test for the round trip**

Add `proptest.workspace = true` under `[dev-dependencies]`, then:

```rust
proptest::proptest! {
    #[test]
    fn round_trip_holds_for_any_view(
        px in -10_000.0f64..10_000.0,
        py in -10_000.0f64..10_000.0,
        zoom in 0.01f64..64.0,
        x in -10_000.0f64..10_000.0,
        y in -10_000.0f64..10_000.0,
    ) {
        let view = ViewTransform { pan: DocPoint { x: px, y: py }, zoom };
        let original = DocPoint { x, y };
        let back = view.screen_to_doc(view.doc_to_screen(original));
        // f32 screen coordinates bound the achievable precision.
        proptest::prop_assert!((back.x - original.x).abs() < 0.01);
        proptest::prop_assert!((back.y - original.y).abs() < 0.01);
    }
}
```

- [ ] **Step 6: Run and commit**

```bash
cargo test -p tessera_geometry && git add -A && git commit -m "Add document and screen coordinate spaces"
```

---

### Task 4: `tessera_color` — colour that can already hold CMYK

Built now for its **types**, not its functionality. A `Fill` born RGB-only would force a file-format migration when the colour engine arrives in milestone 5.

**Files:**
- Create: `crates/tessera_color/src/lib.rs`

**Interfaces:**
- Produces: `Color` enum with `Rgb { r, g, b, a }` (f32, 0..=1), `Cmyk { c, m, y, k, a }`, and `Spot { name: String, tint: f32, fallback: Box<Color> }`. Methods: `Color::BLACK`, `Color::WHITE`, `to_rgb_f32(&self) -> [f32; 4]`. All `Serialize`/`Deserialize`, `PartialEq`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmyk_converts_to_rgb_by_the_naive_formula() {
        // Pure cyan. Milestone 5 replaces this with an ICC transform; until
        // then the formula is documented as an approximation, not a fallback.
        let cyan = Color::Cmyk { c: 1.0, m: 0.0, y: 0.0, k: 0.0, a: 1.0 };
        assert_eq!(cyan.to_rgb_f32(), [0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn a_spot_colour_reports_its_fallback() {
        let spot = Color::Spot {
            name: "PANTONE 185 C".to_string(),
            tint: 1.0,
            fallback: Box::new(Color::Rgb { r: 0.9, g: 0.1, b: 0.2, a: 1.0 }),
        };
        assert_eq!(spot.to_rgb_f32(), [0.9, 0.1, 0.2, 1.0]);
    }

    #[test]
    fn colour_survives_a_json_round_trip() {
        let original = Color::Cmyk { c: 0.1, m: 0.2, y: 0.3, k: 0.4, a: 1.0 };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Color = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_color
```

Expected: FAIL — `Color` not found.

- [ ] **Step 3: Implement**

```rust
//! Colour values across RGB, CMYK and spot inks.
//!
//! The CMYK-to-RGB conversion here is the naive formula, and is explicitly a
//! placeholder for the ICC transform that arrives in milestone 5. It is
//! documented as an approximation so it is never mistaken for a silent
//! fallback.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Color {
    Rgb { r: f32, g: f32, b: f32, a: f32 },
    Cmyk { c: f32, m: f32, y: f32, k: f32, a: f32 },
    Spot { name: String, tint: f32, fallback: Box<Color> },
}

impl Color {
    pub const BLACK: Self = Self::Rgb { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Self = Self::Rgb { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    /// Screen approximation. Not colour-managed until milestone 5.
    pub fn to_rgb_f32(&self) -> [f32; 4] {
        match self {
            Self::Rgb { r, g, b, a } => [*r, *g, *b, *a],
            Self::Cmyk { c, m, y, k, a } => [
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
                *a,
            ],
            Self::Spot { fallback, tint, .. } => {
                let [r, g, b, a] = fallback.to_rgb_f32();
                [r, g, b, a * tint]
            }
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p tessera_color
```

Expected: PASS, 3 tests. Add `serde_json.workspace = true` to `[dev-dependencies]` if the round-trip test does not compile.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Add colour values spanning RGB, CMYK and spot inks"
```

---

### Task 5: `tessera_document` — the node arena

**Files:**
- Create: `crates/tessera_document/src/ids.rs`
- Create: `crates/tessera_document/src/nodes.rs`
- Create: `crates/tessera_document/src/document.rs`
- Modify: `crates/tessera_document/src/lib.rs`

**Interfaces:**
- Consumes: `tessera_geometry::{DocPoint, DocRect}`, `tessera_color::Color`.
- Produces: `FrameId`, `LayerId`, `PageId`, `SpreadId`, `StoryId` (slotmap keys); `Frame { transform: DocPoint, size: DocRect, kind: FrameKind, fill: Color, stroke: Option<Stroke> }`; `FrameKind::{Rectangle, Ellipse, Text { story: StoryId }}` (M0 subset); `Document` with `new() -> Document`, `add_frame(LayerId, Frame) -> FrameId`, `frame(FrameId) -> Option<&Frame>`, `frame_mut(FrameId) -> Option<&mut Frame>`, `remove_frame(FrameId)`, `frames_in_layer(LayerId) -> impl Iterator<Item = FrameId>`, `hit_test(DocPoint) -> Option<FrameId>`, and `revision() -> u64`.

M0 implements the subset of the spec's section 6 model that the acceptance sentence needs. Groups, images, paths and masters are deliberately absent — but `FrameKind` is an enum, so they are additive later.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/tessera_document/src/document.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::{DocPoint, DocRect};

    fn rect_frame() -> Frame {
        Frame {
            bounds: DocRect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 },
            kind: FrameKind::Rectangle,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn a_new_document_has_one_spread_with_one_page_and_one_layer() {
        let doc = Document::new();
        assert_eq!(doc.spread_ids().count(), 1);
        assert_eq!(doc.page_ids().count(), 1);
        assert_eq!(doc.layer_ids().count(), 1);
    }

    #[test]
    fn an_added_frame_can_be_read_back() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.frame(id).expect("frame exists").bounds.width, 100.0);
    }

    #[test]
    fn adding_a_frame_advances_the_revision() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("default layer");
        let before = doc.revision();
        doc.add_frame(layer, rect_frame());
        assert!(doc.revision() > before);
    }

    #[test]
    fn hit_test_finds_a_frame_under_the_point_and_nothing_outside_it() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }), Some(id));
        assert_eq!(doc.hit_test(DocPoint { x: 5.0, y: 5.0 }), None);
    }

    #[test]
    fn hit_test_returns_the_topmost_frame() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("default layer");
        let _under = doc.add_frame(layer, rect_frame());
        let over = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }), Some(over));
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_document
```

Expected: FAIL — `Document` not found.

- [ ] **Step 3: Define the ids and nodes**

```rust
// crates/tessera_document/src/ids.rs
use slotmap::new_key_type;

new_key_type! {
    pub struct FrameId;
    pub struct LayerId;
    pub struct PageId;
    pub struct SpreadId;
    pub struct StoryId;
}
```

```rust
// crates/tessera_document/src/nodes.rs
use serde::{Deserialize, Serialize};
use tessera_color::Color;
use tessera_geometry::DocRect;

use crate::ids::{FrameId, LayerId, PageId, StoryId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
}

/// The kinds of frame milestone 0 supports. Additive: groups, images and
/// paths become new variants without disturbing existing documents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FrameKind {
    Rectangle,
    Ellipse,
    Text { story: StoryId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub bounds: DocRect,
    pub kind: FrameKind,
    pub fill: Color,
    pub stroke: Option<Stroke>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// Back to front. The last entry paints on top.
    pub frames: Vec<FrameId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub bounds: DocRect,
    pub layers: Vec<LayerId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spread {
    pub pages: Vec<PageId>,
}
```

- [ ] **Step 4: Implement the document**

```rust
// crates/tessera_document/src/document.rs
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use tessera_geometry::{DocPoint, DocRect};

use crate::ids::{FrameId, LayerId, PageId, SpreadId};
use crate::nodes::{Frame, FrameKind, Layer, Page, Spread};

/// US Letter in points, the default new-document size.
const DEFAULT_PAGE: DocRect = DocRect { x: 0.0, y: 0.0, width: 612.0, height: 792.0 };

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub frames: SlotMap<FrameId, Frame>,
    pub layers: SlotMap<LayerId, Layer>,
    pub pages: SlotMap<PageId, Page>,
    pub spreads: SlotMap<SpreadId, Spread>,
    /// Spread paint and navigation order.
    pub spread_order: Vec<SpreadId>,
    /// Bumped on every mutation. The renderer rebuilds only when it moves.
    /// Skipped in serialization: a reloaded document starts fresh at zero.
    #[serde(skip)]
    revision: u64,
}

impl Document {
    pub fn new() -> Self {
        let mut doc = Self {
            frames: SlotMap::with_key(),
            layers: SlotMap::with_key(),
            pages: SlotMap::with_key(),
            spreads: SlotMap::with_key(),
            spread_order: Vec::new(),
            revision: 0,
        };
        let layer = doc.layers.insert(Layer {
            name: "Layer 1".to_string(),
            visible: true,
            locked: false,
            frames: Vec::new(),
        });
        let page = doc.pages.insert(Page { bounds: DEFAULT_PAGE, layers: vec![layer] });
        let spread = doc.spreads.insert(Spread { pages: vec![page] });
        doc.spread_order.push(spread);
        doc
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn spread_ids(&self) -> impl Iterator<Item = SpreadId> + '_ {
        self.spread_order.iter().copied()
    }

    pub fn page_ids(&self) -> impl Iterator<Item = PageId> + '_ {
        self.spread_ids()
            .filter_map(|s| self.spreads.get(s))
            .flat_map(|s| s.pages.iter().copied())
    }

    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.page_ids()
            .filter_map(|p| self.pages.get(p))
            .flat_map(|p| p.layers.iter().copied())
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub fn add_frame(&mut self, layer: LayerId, frame: Frame) -> FrameId {
        let id = self.frames.insert(frame);
        if let Some(l) = self.layers.get_mut(layer) {
            l.frames.push(id);
        }
        self.revision += 1;
        id
    }

    pub fn remove_frame(&mut self, id: FrameId) {
        self.frames.remove(id);
        for layer in self.layers.values_mut() {
            layer.frames.retain(|f| *f != id);
        }
        self.revision += 1;
    }

    pub fn frame(&self, id: FrameId) -> Option<&Frame> {
        self.frames.get(id)
    }

    /// Bumps the revision on the assumption the caller mutates.
    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        self.revision += 1;
        self.frames.get_mut(id)
    }

    /// Back-to-front paint order across every visible layer.
    pub fn paint_order(&self) -> Vec<FrameId> {
        self.layer_ids()
            .filter_map(|l| self.layers.get(l))
            .filter(|l| l.visible)
            .flat_map(|l| l.frames.iter().copied())
            .collect()
    }

    /// Topmost frame containing the point, or `None`.
    pub fn hit_test(&self, point: DocPoint) -> Option<FrameId> {
        self.paint_order()
            .into_iter()
            .rev()
            .find(|id| self.frames.get(*id).is_some_and(|f| f.bounds.contains(point)))
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}
```

Update `lib.rs` to `pub mod document; pub mod ids; pub mod nodes;` with re-exports.

- [ ] **Step 5: Run tests**

```bash
cargo test -p tessera_document
```

Expected: PASS, 5 tests.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Add the document node arena"
```

---

### Task 6: `tessera_io` — writing a file without losing the old one

Small, and load-bearing. A save that truncates the target and then fails has destroyed the user's work.

**Files:**
- Create: `crates/tessera_io/src/atomic.rs`
- Modify: `crates/tessera_io/src/lib.rs`

**Interfaces:**
- Produces: `write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError>` and `IoError` (a `thiserror` enum with `Write`, `Rename`, `Create` variants each carrying the path and the source error).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_new_file() {
        let dir = std::env::temp_dir().join("tessera_atomic_new");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("a.bin");
        let _ = std::fs::remove_file(&path);

        write_atomic(&path, b"hello").expect("write");

        assert_eq!(std::fs::read(&path).expect("read"), b"hello");
    }

    #[test]
    fn a_failed_write_leaves_the_original_intact() {
        let dir = std::env::temp_dir().join("tessera_atomic_keep");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("b.bin");
        std::fs::write(&path, b"original").expect("seed");

        // A directory where a temp file must go cannot be written to.
        let blocked = dir.join("b.bin.tmp");
        let _ = std::fs::remove_file(&blocked);
        std::fs::create_dir_all(&blocked).expect("block the temp path");

        let result = write_atomic(&path, b"replacement");

        assert!(result.is_err(), "the write should have failed");
        assert_eq!(std::fs::read(&path).expect("read"), b"original");
        std::fs::remove_dir(&blocked).ok();
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_io
```

Expected: FAIL — `write_atomic` not found.

- [ ] **Step 3: Implement**

```rust
// crates/tessera_io/src/atomic.rs
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("could not write {path}: {source}")]
    Write { path: PathBuf, #[source] source: std::io::Error },
    #[error("could not replace {path}: {source}")]
    Rename { path: PathBuf, #[source] source: std::io::Error },
}

/// Write to a sibling temporary file, then rename over the target.
///
/// A rename within a directory is atomic on every platform Tessera targets,
/// so an interrupted save leaves the previous file untouched rather than a
/// half-written one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let temp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));

    std::fs::write(&temp, bytes)
        .map_err(|source| IoError::Write { path: temp.clone(), source })?;

    std::fs::rename(&temp, path).map_err(|source| {
        // Best effort: do not leave litter behind after a failed rename.
        let _ = std::fs::remove_file(&temp);
        IoError::Rename { path: path.to_path_buf(), source }
    })
}
```

- [ ] **Step 4: Run tests and commit**

```bash
cargo test -p tessera_io && git add -A && git commit -m "Add atomic file writing"
```

Expected: PASS, 2 tests.

---

### Task 7: The `.tessera` file format

**N1 lands here.** The round-trip property test in this task is the most valuable single test in the suite.

**Files:**
- Create: `crates/tessera_document/src/format/mod.rs`
- Create: `crates/tessera_document/src/format/meta.rs`
- Create: `crates/tessera_document/tests/round_trip.rs`
- Modify: `crates/tessera_document/Cargo.toml` (add `zip`, `tessera_io`, `serde_json`, `thiserror`; `proptest` under dev)

**Interfaces:**
- Consumes: `Document` (Task 5), `tessera_io::write_atomic` (Task 6).
- Produces: `save(doc: &Document, path: &Path) -> Result<(), FormatError>`, `load(path: &Path) -> Result<Document, FormatError>`, `FORMAT_VERSION: u32 = 1`, `Meta { format_version: u32, app_version: String, created: String, modified: String }`, and `FormatError` with a `NewerFormat { found: u32, supported: u32 }` variant.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/tessera_document/tests/round_trip.rs
use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::format;
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::DocRect;

fn temp_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("tessera_format_tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

#[test]
fn an_empty_document_round_trips() {
    let path = temp_path("empty.tessera");
    let doc = Document::new();

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(loaded.spread_order.len(), doc.spread_order.len());
    assert_eq!(loaded.frames.len(), doc.frames.len());
}

#[test]
fn a_document_with_a_rectangle_round_trips_exactly() {
    let path = temp_path("rect.tessera");
    let mut doc = Document::new();
    let layer = doc.layer_ids().next().expect("default layer");
    let id = doc.add_frame(layer, Frame {
        bounds: DocRect { x: 1.5, y: 2.5, width: 300.0, height: 200.0 },
        kind: FrameKind::Rectangle,
        fill: Color::Cmyk { c: 0.1, m: 0.2, y: 0.3, k: 0.4, a: 1.0 },
        stroke: None,
    });

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(loaded.frame(id).expect("frame survived"), doc.frame(id).expect("original"));
}

#[test]
fn a_newer_format_version_is_refused_rather_than_guessed_at() {
    let path = temp_path("future.tessera");
    let doc = Document::new();
    format::save(&doc, &path).expect("save");
    format::rewrite_version_for_test(&path, format::FORMAT_VERSION + 1).expect("rewrite");

    match format::load(&path) {
        Err(format::FormatError::NewerFormat { found, supported }) => {
            assert_eq!(found, format::FORMAT_VERSION + 1);
            assert_eq!(supported, format::FORMAT_VERSION);
        }
        other => panic!("expected NewerFormat, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_document --test round_trip
```

Expected: FAIL — `format` module not found.

- [ ] **Step 3: Implement the container**

```rust
// crates/tessera_document/src/format/mod.rs
//! The `.tessera` container: a zip archive holding the serialized document,
//! its metadata, and (later) its thumbnail and embedded assets.
//!
//! A container rather than a bare JSON file so that packaging — collecting
//! links and fonts into one deliverable — is the same mechanism as saving,
//! and so a thumbnail can be read without parsing the document.

pub mod meta;

use std::io::{Cursor, Read, Write};
use std::path::Path;

pub use meta::Meta;

use crate::document::Document;

/// Bumped whenever the on-disk shape changes. Loading an older version runs
/// migrations; loading a newer one is refused.
pub const FORMAT_VERSION: u32 = 1;

const DOCUMENT_ENTRY: &str = "document.json";
const META_ENTRY: &str = "meta.json";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("could not read {0}")]
    Read(std::path::PathBuf),
    #[error("the archive is missing {0}")]
    MissingEntry(&'static str),
    #[error("the file is not a valid Tessera document: {0}")]
    Archive(String),
    #[error("could not parse {entry}: {source}")]
    Parse { entry: &'static str, #[source] source: serde_json::Error },
    #[error(
        "this document was saved by a newer version of Tessera \
         (format {found}, this build supports {supported})"
    )]
    NewerFormat { found: u32, supported: u32 },
    #[error(transparent)]
    Io(#[from] tessera_io::atomic::IoError),
    #[error("could not build the archive: {0}")]
    Write(String),
}

pub fn save(doc: &Document, path: &Path) -> Result<(), FormatError> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options: zip::write::FileOptions<()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let meta = serde_json::to_vec_pretty(&Meta::current())
            .map_err(|source| FormatError::Parse { entry: META_ENTRY, source })?;
        zip.start_file(META_ENTRY, options).map_err(|e| FormatError::Write(e.to_string()))?;
        zip.write_all(&meta).map_err(|e| FormatError::Write(e.to_string()))?;

        let body = serde_json::to_vec_pretty(doc)
            .map_err(|source| FormatError::Parse { entry: DOCUMENT_ENTRY, source })?;
        zip.start_file(DOCUMENT_ENTRY, options).map_err(|e| FormatError::Write(e.to_string()))?;
        zip.write_all(&body).map_err(|e| FormatError::Write(e.to_string()))?;

        zip.finish().map_err(|e| FormatError::Write(e.to_string()))?;
    }

    tessera_io::atomic::write_atomic(path, &buffer.into_inner())?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Document, FormatError> {
    let bytes = std::fs::read(path).map_err(|_| FormatError::Read(path.to_path_buf()))?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| FormatError::Archive(e.to_string()))?;

    let meta: Meta = read_json(&mut zip, META_ENTRY)?;
    if meta.format_version > FORMAT_VERSION {
        return Err(FormatError::NewerFormat {
            found: meta.format_version,
            supported: FORMAT_VERSION,
        });
    }
    // Older versions migrate here. At format 1 there is nothing to migrate.

    read_json(&mut zip, DOCUMENT_ENTRY)
}

fn read_json<T: serde::de::DeserializeOwned>(
    zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    entry: &'static str,
) -> Result<T, FormatError> {
    let mut file = zip.by_name(entry).map_err(|_| FormatError::MissingEntry(entry))?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|_| FormatError::MissingEntry(entry))?;
    serde_json::from_str(&text).map_err(|source| FormatError::Parse { entry, source })
}

/// Rewrites only the version field, so the refusal path can be tested without
/// hand-building an archive.
#[doc(hidden)]
pub fn rewrite_version_for_test(path: &Path, version: u32) -> Result<(), FormatError> {
    let doc = load(path)?;
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        let mut meta = Meta::current();
        meta.format_version = version;
        zip.start_file(META_ENTRY, options).map_err(|e| FormatError::Write(e.to_string()))?;
        zip.write_all(&serde_json::to_vec(&meta).expect("meta serializes"))
            .map_err(|e| FormatError::Write(e.to_string()))?;
        zip.start_file(DOCUMENT_ENTRY, options).map_err(|e| FormatError::Write(e.to_string()))?;
        zip.write_all(&serde_json::to_vec(&doc).expect("document serializes"))
            .map_err(|e| FormatError::Write(e.to_string()))?;
        zip.finish().map_err(|e| FormatError::Write(e.to_string()))?;
    }
    tessera_io::atomic::write_atomic(path, &buffer.into_inner())?;
    Ok(())
}
```

```rust
// crates/tessera_document/src/format/meta.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub format_version: u32,
    pub app_version: String,
    pub created: String,
    pub modified: String,
}

impl Meta {
    pub fn current() -> Self {
        // ISO-8601 timestamps arrive with the `time` crate in milestone 3,
        // when document metadata becomes user-visible. Empty is honest here;
        // a fabricated date would not be.
        Self {
            format_version: super::FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created: String::new(),
            modified: String::new(),
        }
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p tessera_document --test round_trip
```

Expected: PASS, 3 tests.

- [ ] **Step 5: Add the round-trip property test**

This is the test that makes N1 structural. It generates arbitrary documents and asserts save-then-load is the identity.

```rust
// append to crates/tessera_document/tests/round_trip.rs
use proptest::prelude::*;

fn any_color() -> impl Strategy<Value = Color> {
    prop_oneof![
        (0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0)
            .prop_map(|(r, g, b)| Color::Rgb { r, g, b, a: 1.0 }),
        (0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0)
            .prop_map(|(c, m, y, k)| Color::Cmyk { c, m, y, k, a: 1.0 }),
    ]
}

fn any_frame() -> impl Strategy<Value = Frame> {
    (-1000.0f64..1000.0, -1000.0f64..1000.0, 1.0f64..1000.0, 1.0f64..1000.0, any_color())
        .prop_map(|(x, y, width, height, fill)| Frame {
            bounds: DocRect { x, y, width, height },
            kind: FrameKind::Rectangle,
            fill,
            stroke: None,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn any_document_survives_a_save_and_load(frames in prop::collection::vec(any_frame(), 0..12)) {
        let path = temp_path("proptest.tessera");
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("default layer");
        let ids: Vec<_> = frames.into_iter().map(|f| doc.add_frame(layer, f)).collect();

        format::save(&doc, &path).expect("save");
        let loaded = format::load(&path).expect("load");

        prop_assert_eq!(loaded.frames.len(), doc.frames.len());
        for id in ids {
            prop_assert_eq!(loaded.frame(id), doc.frame(id));
        }
    }
}
```

- [ ] **Step 6: Run and commit**

```bash
cargo test -p tessera_document && git add -A && git commit -m "Add the .tessera container format with round-trip property tests"
```

---

### Task 8: Snapshot undo

**Files:**
- Create: `crates/tessera_document/src/history.rs`
- Modify: `crates/tessera_document/src/lib.rs`

**Interfaces:**
- Produces: `History::new(limit: usize) -> History`, `History::record(&mut self, doc: &Document)`, `History::undo(&mut self, current: &Document) -> Option<Document>`, `History::redo(&mut self, current: &Document) -> Option<Document>`, `can_undo() -> bool`, `can_redo() -> bool`.

Snapshots, per decision D5. Cheap because D1 made the document a plain clonable struct — and immune to the class of bug where an inverse operation is subtly wrong, which is how the previous codebase ended up with add and remove page never being undoable at all.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::nodes::{Frame, FrameKind};
    use tessera_geometry::DocRect;

    fn frame() -> Frame {
        Frame {
            bounds: DocRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 },
            kind: FrameKind::Rectangle,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn undo_restores_the_state_before_the_change() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("layer");
        let mut history = History::new(50);

        history.record(&doc);
        doc.add_frame(layer, frame());
        assert_eq!(doc.frames.len(), 1);

        let restored = history.undo(&doc).expect("undo available");
        assert_eq!(restored.frames.len(), 0);
    }

    #[test]
    fn redo_reapplies_what_undo_took_away() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("layer");
        let mut history = History::new(50);

        history.record(&doc);
        doc.add_frame(layer, frame());
        let undone = history.undo(&doc).expect("undo");
        let redone = history.redo(&undone).expect("redo");

        assert_eq!(redone.frames.len(), 1);
    }

    #[test]
    fn a_new_change_discards_the_redo_stack() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("layer");
        let mut history = History::new(50);

        history.record(&doc);
        doc.add_frame(layer, frame());
        let mut doc = history.undo(&doc).expect("undo");
        assert!(history.can_redo());

        history.record(&doc);
        doc.add_frame(layer, frame());
        assert!(!history.can_redo());
    }

    #[test]
    fn the_stack_is_bounded() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("layer");
        let mut history = History::new(3);
        for _ in 0..10 {
            history.record(&doc);
            doc.add_frame(layer, frame());
        }
        assert_eq!(history.undo_depth(), 3);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_document --lib
```

Expected: FAIL — `History` not found.

- [ ] **Step 3: Implement**

```rust
// crates/tessera_document/src/history.rs
//! Snapshot-based undo.
//!
//! Decision D5: the document is a plain clonable struct, so snapshots are
//! cheap. Snapshots cannot develop the class of bug where an inverse
//! operation is subtly wrong — or, as happened in the previous codebase,
//! where an operation simply never got an inverse and was silently not
//! undoable.

use std::collections::VecDeque;

use crate::document::Document;

pub struct History {
    past: VecDeque<Document>,
    future: Vec<Document>,
    limit: usize,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { past: VecDeque::new(), future: Vec::new(), limit: limit.max(1) }
    }

    /// Call immediately *before* mutating the document.
    pub fn record(&mut self, doc: &Document) {
        self.past.push_back(doc.clone());
        while self.past.len() > self.limit {
            self.past.pop_front();
        }
        self.future.clear();
    }

    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let previous = self.past.pop_back()?;
        self.future.push(current.clone());
        Some(previous)
    }

    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let next = self.future.pop()?;
        self.past.push_back(current.clone());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }
}
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p tessera_document && git add -A && git commit -m "Add snapshot-based undo"
```

Expected: PASS, 4 new tests.

---

# Phase C — Text

---

### Task 9: `tessera_text` — the story model and shaping

**Files:**
- Create: `crates/tessera_text/src/story.rs`
- Create: `crates/tessera_text/src/shape.rs`
- Modify: `crates/tessera_text/src/lib.rs`, `crates/tessera_text/Cargo.toml`

**Interfaces:**
- Produces: `Story { text: String, style: TextStyle }`; `TextStyle { family: String, size: f32, line_height: f32, color: Color }`; `Shaper::new() -> Shaper`; `Shaper::shape(&mut self, story: &Story, width: f64) -> ShapedText`; `ShapedText { lines: Vec<ShapedLine>, height: f64 }`; `ShapedLine { glyphs: Vec<PositionedGlyph>, baseline: f64 }`; `PositionedGlyph { glyph_id: u16, x: f64, y: f64, font_index: usize }`; `ShapedText::font_data(index) -> &[u8]`.

`PositionedGlyph` is the shared currency of decision D3 — **`tessera_render` and `tessera_pdf` both consume exactly this type**, which is what makes the export match the screen.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn story(text: &str) -> Story {
        Story {
            text: text.to_string(),
            style: TextStyle {
                family: "Arial".to_string(),
                size: 12.0,
                line_height: 1.2,
                color: tessera_color::Color::BLACK,
            },
        }
    }

    #[test]
    fn shaping_empty_text_yields_no_glyphs() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story(""), 200.0);
        assert_eq!(shaped.lines.iter().map(|l| l.glyphs.len()).sum::<usize>(), 0);
    }

    #[test]
    fn shaping_produces_one_glyph_per_character_for_simple_latin() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story("Hello"), 500.0);
        assert_eq!(shaped.lines.iter().map(|l| l.glyphs.len()).sum::<usize>(), 5);
    }

    #[test]
    fn glyphs_advance_left_to_right() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story("AB"), 500.0);
        let glyphs = &shaped.lines[0].glyphs;
        assert!(glyphs[1].x > glyphs[0].x, "second glyph must sit right of the first");
    }

    #[test]
    fn a_narrow_frame_breaks_text_onto_more_than_one_line() {
        let mut shaper = Shaper::new();
        let wide = shaper.shape(&story("the quick brown fox jumps"), 1000.0);
        let narrow = shaper.shape(&story("the quick brown fox jumps"), 60.0);
        assert_eq!(wide.lines.len(), 1);
        assert!(narrow.lines.len() > 1, "narrow frame must wrap");
    }

    #[test]
    fn shaped_height_grows_with_line_count() {
        let mut shaper = Shaper::new();
        let wide = shaper.shape(&story("the quick brown fox jumps"), 1000.0);
        let narrow = shaper.shape(&story("the quick brown fox jumps"), 60.0);
        assert!(narrow.height > wide.height);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_text
```

Expected: FAIL — `Shaper` not found.

- [ ] **Step 3: Implement the story model**

```rust
// crates/tessera_text/src/story.rs
use serde::{Deserialize, Serialize};
use tessera_color::Color;

/// Character formatting. Milestone 2 splits this into runs; milestone 0
/// applies one style to the whole story.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    /// Multiple of the font size.
    pub line_height: f32,
    pub color: Color,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "Arial".to_string(),
            size: 12.0,
            line_height: 1.2,
            color: Color::BLACK,
        }
    }
}

/// A story exists once and is addressed by `StoryId`, independent of the
/// frames that display it. Milestone 4 threads one story through several.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub text: String,
    pub style: TextStyle,
}
```

- [ ] **Step 4: Implement shaping over parley**

```rust
// crates/tessera_text/src/shape.rs
//! Shaping via parley, producing positioned glyphs.
//!
//! `PositionedGlyph` is consumed by BOTH `tessera_render` and `tessera_pdf`.
//! That shared source is what guarantees the export matches the screen; see
//! decision D3.

use crate::story::Story;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedGlyph {
    pub glyph_id: u16,
    /// Baseline-relative, in points, from the frame origin.
    pub x: f64,
    pub y: f64,
    /// Index into `ShapedText::fonts`.
    pub font_index: usize,
}

#[derive(Debug, Clone)]
pub struct ShapedLine {
    pub glyphs: Vec<PositionedGlyph>,
    pub baseline: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ShapedText {
    pub lines: Vec<ShapedLine>,
    pub height: f64,
    /// Font blobs referenced by `PositionedGlyph::font_index`.
    pub fonts: Vec<Vec<u8>>,
    pub font_size: f32,
}

pub struct Shaper {
    font_ctx: parley::FontContext,
    layout_ctx: parley::LayoutContext<[u8; 4]>,
}

impl Shaper {
    pub fn new() -> Self {
        Self {
            font_ctx: parley::FontContext::new(),
            layout_ctx: parley::LayoutContext::new(),
        }
    }

    pub fn shape(&mut self, story: &Story, width: f64) -> ShapedText {
        if story.text.is_empty() {
            return ShapedText { font_size: story.style.size, ..Default::default() };
        }

        let mut builder = self.layout_ctx.ranged_builder(&mut self.font_ctx, &story.text, 1.0, true);
        builder.push_default(parley::StyleProperty::FontStack(
            parley::FontStack::from(story.style.family.as_str()),
        ));
        builder.push_default(parley::StyleProperty::FontSize(story.style.size));
        builder.push_default(parley::StyleProperty::LineHeight(
            parley::LineHeight::FontSizeRelative(story.style.line_height),
        ));

        let mut layout = builder.build(&story.text);
        layout.break_all_lines(Some(width as f32));

        let mut fonts: Vec<Vec<u8>> = Vec::new();
        let mut lines = Vec::new();

        for line in layout.lines() {
            let mut glyphs = Vec::new();
            let baseline = f64::from(line.metrics().baseline);

            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let font = run.run().font();
                let blob = font.data.as_ref().to_vec();
                let font_index = match fonts.iter().position(|f| *f == blob) {
                    Some(i) => i,
                    None => {
                        fonts.push(blob);
                        fonts.len() - 1
                    }
                };

                let mut pen_x = run.offset();
                for glyph in run.glyphs() {
                    glyphs.push(PositionedGlyph {
                        glyph_id: glyph.id,
                        x: f64::from(pen_x + glyph.x),
                        y: baseline + f64::from(glyph.y),
                        font_index,
                    });
                    pen_x += glyph.advance;
                }
            }

            lines.push(ShapedLine { glyphs, baseline });
        }

        ShapedText {
            height: f64::from(layout.height()),
            lines,
            fonts,
            font_size: story.style.size,
        }
    }
}

impl Default for Shaper {
    fn default() -> Self {
        Self::new()
    }
}
```

**Parley's builder API moves between releases.** Run `cargo doc --open -p parley` and correct `ranged_builder`, `StyleProperty`, `break_all_lines`, `lines()`, `items()` and the glyph run fields against 0.11 until it compiles. The tests above pin the *behaviour* and must keep passing regardless of how the calls are spelled.

- [ ] **Step 5: Run tests**

```bash
cargo test -p tessera_text
```

Expected: PASS, 5 tests.

If "Arial" is unavailable on the CI runner, use parley's generic `sans-serif` stack in the test fixture rather than a named family. A missing font must not be silently substituted without the test noticing.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Add the story model and parley-backed shaping"
```

---

### Task 10: `tessera_text` — the editable buffer

**N3 begins here.** Cursor and selection live in persistent application state, not in an egui widget — which is what makes on-canvas text editing possible in an immediate-mode interface, and what the previous architecture could not do at all.

**Files:**
- Create: `crates/tessera_text/src/edit.rs`
- Modify: `crates/tessera_text/src/lib.rs`

**Interfaces:**
- Produces: `TextCursor { position: usize, anchor: usize }` (byte offsets); `EditBuffer::new(story: Story)`, `.story() -> &Story`, `.cursor() -> TextCursor`, `.insert(&mut self, text: &str)`, `.delete_backward(&mut self)`, `.delete_forward(&mut self)`, `.move_left(&mut self, extend: bool)`, `.move_right(&mut self, extend: bool)`, `.select_all(&mut self)`, `.selection_range() -> Option<Range<usize>>`, `.set_ime_preedit(&mut self, text: Option<String>)`, `.ime_preedit() -> Option<&str>`.

Cursor movement is by **grapheme cluster**, not by byte or `char`. Moving over `é` composed of `e` plus a combining accent must move once, not twice.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::Story;

    fn buffer(text: &str) -> EditBuffer {
        let mut story = Story::default();
        story.text = text.to_string();
        let mut b = EditBuffer::new(story);
        b.set_cursor(text.len());
        b
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut b = buffer("Helo");
        b.set_cursor(3);
        b.insert("l");
        assert_eq!(b.story().text, "Hello");
        assert_eq!(b.cursor().position, 4);
    }

    #[test]
    fn backspace_removes_the_character_before_the_cursor() {
        let mut b = buffer("Hello");
        b.delete_backward();
        assert_eq!(b.story().text, "Hell");
    }

    #[test]
    fn backspace_at_the_start_does_nothing() {
        let mut b = buffer("Hello");
        b.set_cursor(0);
        b.delete_backward();
        assert_eq!(b.story().text, "Hello");
        assert_eq!(b.cursor().position, 0);
    }

    #[test]
    fn backspace_removes_a_whole_grapheme_not_a_byte() {
        // "e" plus COMBINING ACUTE ACCENT: four bytes, one visible character.
        let mut b = buffer("cafe\u{0301}");
        b.delete_backward();
        assert_eq!(b.story().text, "caf", "the whole grapheme must go at once");
    }

    #[test]
    fn moving_left_crosses_a_grapheme_in_one_step() {
        let mut b = buffer("cafe\u{0301}");
        let end = b.cursor().position;
        b.move_left(false);
        assert_eq!(b.cursor().position, end - 3, "e + combining accent is one step");
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut b = buffer("Hello");
        b.set_cursor(0);
        b.move_right(true);
        b.move_right(true);
        b.insert("J");
        assert_eq!(b.story().text, "Jllo");
    }

    #[test]
    fn select_all_covers_the_whole_story() {
        let mut b = buffer("Hello");
        b.select_all();
        assert_eq!(b.selection_range(), Some(0..5));
    }

    #[test]
    fn an_ime_preedit_is_visible_without_entering_the_text() {
        let mut b = buffer("");
        b.set_ime_preedit(Some("に".to_string()));
        assert_eq!(b.ime_preedit(), Some("に"));
        assert_eq!(b.story().text, "", "a preedit is not committed text");
    }

    #[test]
    fn committing_an_ime_composition_inserts_it_and_clears_the_preedit() {
        let mut b = buffer("");
        b.set_ime_preedit(Some("に".to_string()));
        b.insert("日本");
        assert_eq!(b.story().text, "日本");
        assert_eq!(b.ime_preedit(), None);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_text
```

Expected: FAIL — `EditBuffer` not found.

- [ ] **Step 3: Add the grapheme dependency**

Add `unicode-segmentation = "1"` to `[workspace.dependencies]` and to `tessera_text`. Grapheme boundaries are not something to hand-roll.

- [ ] **Step 4: Implement**

```rust
// crates/tessera_text/src/edit.rs
//! The editable text buffer.
//!
//! Cursor and selection live here — in persistent application state — rather
//! than inside an egui widget. Immediate-mode widgets are reconstructed every
//! frame, so a cursor owned by the widget cannot survive. The UI layer only
//! reports events into this buffer; egui's own `TextEdit` state is never used
//! for canvas text.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::story::Story;

/// Byte offsets into `Story::text`. `position` is the caret; `anchor` is where
/// the selection started. Equal means no selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCursor {
    pub position: usize,
    pub anchor: usize,
}

pub struct EditBuffer {
    story: Story,
    cursor: TextCursor,
    ime_preedit: Option<String>,
}

impl EditBuffer {
    pub fn new(story: Story) -> Self {
        Self { story, cursor: TextCursor { position: 0, anchor: 0 }, ime_preedit: None }
    }

    pub fn story(&self) -> &Story {
        &self.story
    }

    pub fn into_story(self) -> Story {
        self.story
    }

    pub fn cursor(&self) -> TextCursor {
        self.cursor
    }

    pub fn set_cursor(&mut self, position: usize) {
        let clamped = position.min(self.story.text.len());
        self.cursor = TextCursor { position: clamped, anchor: clamped };
    }

    pub fn selection_range(&self) -> Option<Range<usize>> {
        let (start, end) = if self.cursor.position <= self.cursor.anchor {
            (self.cursor.position, self.cursor.anchor)
        } else {
            (self.cursor.anchor, self.cursor.position)
        };
        (start != end).then_some(start..end)
    }

    pub fn select_all(&mut self) {
        self.cursor = TextCursor { anchor: 0, position: self.story.text.len() };
    }

    pub fn insert(&mut self, text: &str) {
        self.ime_preedit = None;
        if let Some(range) = self.selection_range() {
            self.story.text.replace_range(range.clone(), "");
            self.set_cursor(range.start);
        }
        self.story.text.insert_str(self.cursor.position, text);
        self.set_cursor(self.cursor.position + text.len());
    }

    pub fn delete_backward(&mut self) {
        if let Some(range) = self.selection_range() {
            self.story.text.replace_range(range.clone(), "");
            self.set_cursor(range.start);
            return;
        }
        let Some(previous) = self.previous_grapheme(self.cursor.position) else { return };
        self.story.text.replace_range(previous..self.cursor.position, "");
        self.set_cursor(previous);
    }

    pub fn delete_forward(&mut self) {
        if let Some(range) = self.selection_range() {
            self.story.text.replace_range(range.clone(), "");
            self.set_cursor(range.start);
            return;
        }
        let Some(next) = self.next_grapheme(self.cursor.position) else { return };
        self.story.text.replace_range(self.cursor.position..next, "");
    }

    pub fn move_left(&mut self, extend: bool) {
        let target = self.previous_grapheme(self.cursor.position).unwrap_or(0);
        self.move_to(target, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        let target = self.next_grapheme(self.cursor.position).unwrap_or(self.story.text.len());
        self.move_to(target, extend);
    }

    pub fn set_ime_preedit(&mut self, text: Option<String>) {
        self.ime_preedit = text;
    }

    pub fn ime_preedit(&self) -> Option<&str> {
        self.ime_preedit.as_deref()
    }

    fn move_to(&mut self, position: usize, extend: bool) {
        self.cursor.position = position;
        if !extend {
            self.cursor.anchor = position;
        }
    }

    fn previous_grapheme(&self, from: usize) -> Option<usize> {
        self.story.text[..from].grapheme_indices(true).next_back().map(|(i, _)| i)
    }

    fn next_grapheme(&self, from: usize) -> Option<usize> {
        self.story.text[from..]
            .grapheme_indices(true)
            .next()
            .map(|(_, g)| from + g.len())
            .filter(|next| *next <= self.story.text.len() && *next > from)
    }
}
```

- [ ] **Step 5: Run tests and commit**

```bash
cargo test -p tessera_text && git add -A && git commit -m "Add the editable text buffer with grapheme-aware cursor movement"
```

Expected: PASS, 9 new tests.

---

# Phase D — Rendering

---

### Task 11: `tessera_layout` — resolving what goes where

**Files:**
- Create: `crates/tessera_layout/src/resolve.rs`
- Modify: `crates/tessera_layout/src/lib.rs`

**Interfaces:**
- Consumes: `Document`, `Frame`, `Shaper`, `Story`.
- Produces: `ResolvedDocument { items: Vec<ResolvedItem> }`; `ResolvedItem { frame: FrameId, bounds: DocRect, kind: ResolvedKind }`; `ResolvedKind::{Rectangle { fill, stroke }, Ellipse { fill, stroke }, Text { shaped: ShapedText, color }}`; `resolve(doc: &Document, stories: &StoryMap, shaper: &mut Shaper) -> ResolvedDocument`; `StoryMap = SlotMap<StoryId, Story>`.

**This is the crate that makes the export match the screen.** Both `tessera_render` and `tessera_pdf` consume `ResolvedDocument`, so neither re-derives geometry or re-shapes text.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_resolves_to_a_rectangle_at_its_bounds() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("layer");
        doc.add_frame(layer, Frame {
            bounds: DocRect { x: 5.0, y: 6.0, width: 20.0, height: 30.0 },
            kind: FrameKind::Rectangle,
            fill: Color::BLACK,
            stroke: None,
        });

        let resolved = resolve(&doc, &StoryMap::with_key(), &mut Shaper::new());

        assert_eq!(resolved.items.len(), 1);
        assert_eq!(resolved.items[0].bounds.width, 20.0);
        assert!(matches!(resolved.items[0].kind, ResolvedKind::Rectangle { .. }));
    }

    #[test]
    fn a_text_frame_resolves_with_text_shaped_to_the_frame_width() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("layer");
        let mut stories = StoryMap::with_key();
        let story = stories.insert(Story { text: "Hello".to_string(), ..Default::default() });
        doc.add_frame(layer, Frame {
            bounds: DocRect { x: 0.0, y: 0.0, width: 500.0, height: 100.0 },
            kind: FrameKind::Text { story },
            fill: Color::WHITE,
            stroke: None,
        });

        let resolved = resolve(&doc, &stories, &mut Shaper::new());

        let ResolvedKind::Text { shaped, .. } = &resolved.items[0].kind else {
            panic!("expected text");
        };
        assert_eq!(shaped.lines.iter().map(|l| l.glyphs.len()).sum::<usize>(), 5);
    }

    #[test]
    fn a_hidden_layer_contributes_nothing() {
        let mut doc = Document::new();
        let layer = doc.layer_ids().next().expect("layer");
        doc.add_frame(layer, Frame {
            bounds: DocRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 },
            kind: FrameKind::Rectangle,
            fill: Color::BLACK,
            stroke: None,
        });
        doc.layers.get_mut(layer).expect("layer").visible = false;

        assert_eq!(resolve(&doc, &StoryMap::with_key(), &mut Shaper::new()).items.len(), 0);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_layout
```

Expected: FAIL — `resolve` not found.

- [ ] **Step 3: Implement**

```rust
// crates/tessera_layout/src/resolve.rs
//! Resolves a document into drawable items.
//!
//! Both the screen renderer and the PDF writer consume the output of this
//! module, so neither re-derives geometry nor re-shapes text. That shared
//! source is what keeps an export from drifting away from the screen.

use slotmap::SlotMap;
use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::{FrameId, StoryId};
use tessera_document::nodes::{FrameKind, Stroke};
use tessera_geometry::DocRect;
use tessera_text::shape::{ShapedText, Shaper};
use tessera_text::story::Story;

pub type StoryMap = SlotMap<StoryId, Story>;

#[derive(Debug, Clone)]
pub enum ResolvedKind {
    Rectangle { fill: Color, stroke: Option<Stroke> },
    Ellipse { fill: Color, stroke: Option<Stroke> },
    Text { shaped: ShapedText, color: Color },
}

#[derive(Debug, Clone)]
pub struct ResolvedItem {
    pub frame: FrameId,
    pub bounds: DocRect,
    pub kind: ResolvedKind,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedDocument {
    pub items: Vec<ResolvedItem>,
}

pub fn resolve(doc: &Document, stories: &StoryMap, shaper: &mut Shaper) -> ResolvedDocument {
    let mut items = Vec::new();

    for id in doc.paint_order() {
        let Some(frame) = doc.frame(id) else { continue };

        let kind = match &frame.kind {
            FrameKind::Rectangle => ResolvedKind::Rectangle {
                fill: frame.fill.clone(),
                stroke: frame.stroke.clone(),
            },
            FrameKind::Ellipse => ResolvedKind::Ellipse {
                fill: frame.fill.clone(),
                stroke: frame.stroke.clone(),
            },
            FrameKind::Text { story } => {
                let Some(story) = stories.get(*story) else { continue };
                ResolvedKind::Text {
                    shaped: shaper.shape(story, frame.bounds.width),
                    color: story.style.color.clone(),
                }
            }
        };

        items.push(ResolvedItem { frame: id, bounds: frame.bounds, kind });
    }

    ResolvedDocument { items }
}
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p tessera_layout && git add -A && git commit -m "Add document resolution shared by the renderer and the PDF writer"
```

Expected: PASS, 3 tests.

---

### Task 12: `tessera_render` — building the Vello scene

**Files:**
- Create: `crates/tessera_render/src/scene.rs`
- Modify: `crates/tessera_render/src/lib.rs`

**Interfaces:**
- Consumes: `ResolvedDocument` (Task 11).
- Produces: `build_scene(resolved: &ResolvedDocument, view: ViewTransform, page: DocRect) -> vello::Scene`.

Pure scene construction, no GPU. Testable without a window.

- [ ] **Step 1: Write the failing tests**

Vello's `Scene` does not expose its contents for inspection, so assert on what is observable: that building does not panic and that the encoding is non-empty for non-empty input.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> DocRect {
        DocRect { x: 0.0, y: 0.0, width: 612.0, height: 792.0 }
    }

    #[test]
    fn an_empty_document_still_paints_the_page() {
        let scene = build_scene(&ResolvedDocument::default(), ViewTransform::default(), page());
        assert!(!scene.encoding().is_empty(), "the white page itself must be drawn");
    }

    #[test]
    fn a_rectangle_adds_to_the_encoding() {
        let empty = build_scene(&ResolvedDocument::default(), ViewTransform::default(), page());
        let with_rect = build_scene(
            &ResolvedDocument {
                items: vec![ResolvedItem {
                    frame: Default::default(),
                    bounds: DocRect { x: 10.0, y: 10.0, width: 50.0, height: 50.0 },
                    kind: ResolvedKind::Rectangle { fill: Color::BLACK, stroke: None },
                }],
            },
            ViewTransform::default(),
            page(),
        );
        assert!(with_rect.encoding().n_path_segments() > empty.encoding().n_path_segments());
    }
}
```

If `Scene::encoding()` is not public in vello 0.10, assert instead that `build_scene` returns without panicking for each `ResolvedKind`, and rely on Task 13's reference-image test for real verification. **Do not delete the test — weaken it and say why in a comment.**

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_render
```

Expected: FAIL — `build_scene` not found.

- [ ] **Step 3: Implement**

```rust
// crates/tessera_render/src/scene.rs
use tessera_color::Color;
use tessera_geometry::{DocRect, ViewTransform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedKind};
use vello::kurbo::{Affine, Ellipse, Rect, Stroke as KurboStroke};
use vello::peniko::{color::AlphaColor, Fill};
use vello::Scene;

fn to_peniko(color: &Color) -> AlphaColor<vello::peniko::color::Srgb> {
    let [r, g, b, a] = color.to_rgb_f32();
    AlphaColor::new([r, g, b, a])
}

pub fn build_scene(
    resolved: &ResolvedDocument,
    view: ViewTransform,
    page: DocRect,
) -> Scene {
    let mut scene = Scene::new();
    let transform = view.to_affine();

    // The page itself, so the document reads as paper rather than as objects
    // floating on the pasteboard.
    scene.fill(
        Fill::NonZero,
        transform,
        to_peniko(&Color::WHITE),
        None,
        &page.to_kurbo(),
    );

    for item in &resolved.items {
        let rect: Rect = item.bounds.to_kurbo();

        match &item.kind {
            ResolvedKind::Rectangle { fill, stroke } => {
                scene.fill(Fill::NonZero, transform, to_peniko(fill), None, &rect);
                if let Some(s) = stroke {
                    scene.stroke(
                        &KurboStroke::new(s.width),
                        transform,
                        to_peniko(&s.color),
                        None,
                        &rect,
                    );
                }
            }
            ResolvedKind::Ellipse { fill, stroke } => {
                let ellipse = Ellipse::from_rect(rect);
                scene.fill(Fill::NonZero, transform, to_peniko(fill), None, &ellipse);
                if let Some(s) = stroke {
                    scene.stroke(
                        &KurboStroke::new(s.width),
                        transform,
                        to_peniko(&s.color),
                        None,
                        &ellipse,
                    );
                }
            }
            ResolvedKind::Text { .. } => {
                // Task 13 draws glyphs here.
            }
        }
    }

    scene
}
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p tessera_render && git add -A && git commit -m "Add Vello scene construction for shapes"
```

---

### Task 13: `tessera_render` — glyphs into the scene

**Files:**
- Modify: `crates/tessera_render/src/scene.rs`

**Interfaces:**
- Consumes: `ShapedText`, `PositionedGlyph` (Task 9).
- Produces: no new public names; fills in the `ResolvedKind::Text` arm.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn text_adds_glyphs_to_the_encoding() {
    let mut shaper = tessera_text::shape::Shaper::new();
    let shaped = shaper.shape(
        &tessera_text::story::Story { text: "Hi".to_string(), ..Default::default() },
        200.0,
    );
    let empty = build_scene(&ResolvedDocument::default(), ViewTransform::default(), page());
    let with_text = build_scene(
        &ResolvedDocument {
            items: vec![ResolvedItem {
                frame: Default::default(),
                bounds: DocRect { x: 0.0, y: 0.0, width: 200.0, height: 50.0 },
                kind: ResolvedKind::Text { shaped, color: Color::BLACK },
            }],
        },
        ViewTransform::default(),
        page(),
    );
    assert!(with_text.encoding().n_path_segments() > empty.encoding().n_path_segments());
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_render
```

Expected: FAIL — the text arm draws nothing, so the counts are equal.

- [ ] **Step 3: Implement the text arm**

```rust
ResolvedKind::Text { shaped, color } => {
    for (index, blob) in shaped.fonts.iter().enumerate() {
        let font = vello::peniko::Font::new(
            vello::peniko::Blob::new(std::sync::Arc::new(blob.clone())),
            0,
        );

        let glyphs: Vec<vello::Glyph> = shaped
            .lines
            .iter()
            .flat_map(|line| line.glyphs.iter())
            .filter(|g| g.font_index == index)
            .map(|g| vello::Glyph {
                id: u32::from(g.glyph_id),
                x: (item.bounds.x + g.x) as f32,
                y: (item.bounds.y + g.y) as f32,
            })
            .collect();

        if glyphs.is_empty() {
            continue;
        }

        scene
            .draw_glyphs(&font)
            .font_size(shaped.font_size)
            .transform(transform)
            .brush(to_peniko(color))
            .draw(Fill::NonZero, glyphs.into_iter());
    }
}
```

Cloning each font blob per frame is wasteful and is fixed by a font cache in milestone 2. It is correct now, and correctness comes first.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p tessera_render && git add -A && git commit -m "Draw shaped glyphs into the Vello scene"
```

---

### Task 14: `tessera_render` — headless rasterization

The path that makes rendering regression-testable without a window, and that later produces page thumbnails.

**Files:**
- Create: `crates/tessera_render/src/headless.rs`
- Create: `crates/tessera_render/tests/gpu_render.rs`

**Interfaces:**
- Produces: `HeadlessRenderer::new(width: u32, height: u32) -> Result<Self, RenderError>`, `.render(&mut self, scene: &Scene) -> Result<Vec<u8>, RenderError>` returning RGBA8.

**This task creates GPU tests. Per the global constraints they run alone, in the foreground, never in `cargo test --workspace`.**

- [ ] **Step 1: Write the GPU test**

```rust
// crates/tessera_render/tests/gpu_render.rs
//! GPU-backed tests. RUN ALONE:
//!     cargo test -p tessera_render --test gpu_render
//! Never inside `cargo test --workspace` — adapter acquisition hangs
//! intermittently on this hardware, and two GPU test binaries contending for
//! the adapter deadlock. A hang looks exactly like a slow compile: if there is
//! no output for two minutes, kill the test binary and any cargo.exe.

use tessera_color::Color;
use tessera_geometry::{DocRect, ViewTransform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};
use tessera_render::headless::HeadlessRenderer;
use tessera_render::scene::build_scene;

fn page() -> DocRect {
    DocRect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 }
}

#[test]
fn an_empty_page_renders_white() {
    let mut renderer = HeadlessRenderer::new(100, 100).expect("adapter");
    let scene = build_scene(&ResolvedDocument::default(), ViewTransform::default(), page());
    let pixels = renderer.render(&scene).expect("render");

    assert_eq!(&pixels[0..4], &[255, 255, 255, 255], "the page must be white");
}

#[test]
fn a_black_rectangle_renders_black_pixels_where_it_sits() {
    let mut renderer = HeadlessRenderer::new(100, 100).expect("adapter");
    let scene = build_scene(
        &ResolvedDocument {
            items: vec![ResolvedItem {
                frame: Default::default(),
                bounds: DocRect { x: 10.0, y: 10.0, width: 50.0, height: 50.0 },
                kind: ResolvedKind::Rectangle { fill: Color::BLACK, stroke: None },
            }],
        },
        ViewTransform::default(),
        page(),
    );
    let pixels = renderer.render(&scene).expect("render");

    let at = |x: usize, y: usize| {
        let i = (y * 100 + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };

    assert_eq!(at(30, 30), [0, 0, 0], "inside the rectangle");
    assert_eq!(at(90, 90), [255, 255, 255], "outside the rectangle");
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_render --test gpu_render
```

Expected: FAIL — `HeadlessRenderer` not found.

- [ ] **Step 3: Implement**

Written from scratch against the wgpu and Vello documentation. **Do not read the deleted `crates/renderer` from git history** — this build is clean-room, and the only permitted references are the upstream docs and the Task 1 spike note.

Write `headless.rs` to: request an adapter with `pollster::block_on`, create a device with Vello's required features and limits (**use the values the Task 1 spike note recorded**), create an `Rgba8Unorm` storage texture, call `Renderer::render_to_texture`, copy the texture into a buffer with a row stride padded up to a 256-byte multiple, map it, and un-pad the rows into a tight RGBA8 `Vec<u8>`.

The row-stride padding is the detail most easily got wrong, and it fails in a recognisable way: `wgpu` requires `bytes_per_row` in `TexelCopyBufferLayout` to be a multiple of `COPY_BYTES_PER_ROW_ALIGNMENT` (256). At 4 bytes per pixel a 100px-wide texture needs 400 bytes of data in a 512-byte row, so the copy-back must skip 112 bytes per row. Getting this wrong produces a progressively sheared image rather than an error — which is exactly what the two tests in step 1 catch, since they sample specific pixel coordinates.

`RenderError` is a `thiserror` enum with `NoAdapter`, `Device(String)`, `Render(String)` and `Map(String)` variants. **No variant may be silently swallowed.**

- [ ] **Step 4: Run the GPU tests, alone**

```bash
cargo test -p tessera_render --test gpu_render
```

Expected: PASS, 2 tests, in about two seconds. If there is no output after two minutes, treat it as the known intermittent hang: kill the `gpu_render-*` binary and any `cargo.exe`, then retry once.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Add headless rasterization with GPU-backed tests"
```

---

# Phase E — The application

---

### Task 15: `tessera_app` — a window, a theme, and a Vello-capable device

**Files:**
- Create: `apps/tessera_app/src/platform/mod.rs`
- Modify: `apps/tessera_app/src/main.rs`
- Create: `crates/tessera_ui/src/theme.rs`
- Create: `crates/tessera_ui/src/app.rs`
- Modify: `crates/tessera_ui/src/lib.rs`

**Interfaces:**
- Consumes: the Task 1 spike note.
- Produces: `tessera_ui::app::TesseraApp` implementing `eframe::App`, with `TesseraApp::new(cc: &eframe::CreationContext) -> Self`; `tessera_ui::theme::{Theme, apply}` exposing named tokens — `Theme::PANEL_BG`, `CANVAS_BG`, `TEXT_PRIMARY`, `TEXT_MUTED`, `ACCENT`, `SPACING_SM/MD/LG`, `RADIUS`.

**No literal colour or magic number may appear anywhere else in `tessera_ui`.**

- [ ] **Step 1: Write the theme test**

```rust
// crates/tessera_ui/src/theme.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_the_theme_sets_the_panel_background() {
        let ctx = egui::Context::default();
        apply(&ctx);
        assert_eq!(ctx.style().visuals.panel_fill, Theme::PANEL_BG);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_ui
```

Expected: FAIL — `apply` not found.

- [ ] **Step 3: Implement the theme**

```rust
// crates/tessera_ui/src/theme.rs
//! Design tokens. Every colour and every spacing value in the interface
//! comes from here — nowhere else in this crate may write a literal.

use egui::{Color32, Context};

pub struct Theme;

impl Theme {
    pub const PANEL_BG: Color32 = Color32::from_rgb(0x24, 0x25, 0x28);
    pub const CANVAS_BG: Color32 = Color32::from_rgb(0x18, 0x19, 0x1B);
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE8);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8A, 0x8C, 0x92);
    pub const ACCENT: Color32 = Color32::from_rgb(0x4C, 0x8E, 0xFF);
    pub const SELECTION: Color32 = Color32::from_rgb(0x4C, 0x8E, 0xFF);

    pub const SPACING_SM: f32 = 4.0;
    pub const SPACING_MD: f32 = 8.0;
    pub const SPACING_LG: f32 = 16.0;
    pub const RADIUS: f32 = 4.0;
}

pub fn apply(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.panel_fill = Theme::PANEL_BG;
    style.visuals.window_fill = Theme::PANEL_BG;
    style.visuals.override_text_color = Some(Theme::TEXT_PRIMARY);
    style.visuals.selection.bg_fill = Theme::SELECTION;
    style.spacing.item_spacing = egui::vec2(Theme::SPACING_MD, Theme::SPACING_MD);
    ctx.set_style(style);
}
```

- [ ] **Step 4: Write the application shell**

`crates/tessera_ui/src/app.rs` defines `TesseraApp` holding: `document: Document`, `stories: StoryMap`, `history: History`, `shaper: Shaper`, `view: ViewTransform`, `selection: Option<FrameId>`, `active_tool: Tool`, `editing: Option<(FrameId, EditBuffer)>`, `current_path: Option<PathBuf>`, `dirty: bool`, `status: Option<Status>`.

**`eframe::App` in 0.35 has no `update`** (spike finding 3). Implement the two real methods, and keep the split honest:

```rust
impl eframe::App for TesseraApp {
    /// Non-drawing work only: autosave ticks, deferred commands, preflight.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // egui forbids painting here.
    }

    /// The root Ui — no margin, no background. Drawing only.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // menu bar (Task 21), left tool strip (Task 18),
        // right inspector (Task 20), then the viewport (Task 16).
    }
}
```

`apps/tessera_app/src/main.rs` calls `eframe::run_native` with **plain `NativeOptions::default()`** — no `WgpuConfiguration`, per spike finding 1 — and creates the Vello `Renderer` into `callback_resources` in the creation closure, exactly as the note shows.

`apps/tessera_app/src/platform/mod.rs` exists as the *designated home* for platform-specific code and is **empty in M0**. eframe selected Vulkan on Windows unprompted and Vello rendered correctly (spike finding 4), so there is no backend to force and no evidence justifying one. **This file remains the only place in the workspace permitted to contain `#[cfg(target_os)]`** — the rule is what matters now; the contents arrive when a real platform difference does.

- [ ] **Step 5: Run it and see a window**

```bash
cargo run -p tessera_app
```

Expected: a dark window, a tool strip on the left, an inspector on the right, an empty centre. No panic.

- [ ] **Step 6: Commit**

```bash
cargo test --workspace --lib && git add -A && git commit -m "Add the application shell, theme tokens and per-platform backend selection"
```

---

### Task 16: The viewport widget

Where decision D2 becomes real.

**Files:**
- Create: `crates/tessera_ui/src/view/viewport.rs`
- Create: `crates/tessera_ui/src/view/callback.rs`
- Modify: `crates/tessera_ui/src/app.rs`

**Interfaces:**
- Consumes: the spike note, `build_scene` (Task 12), `resolve` (Task 11).
- Produces: `viewport::show(ui: &mut egui::Ui, state: &mut TesseraApp) -> egui::Response`.

- [ ] **Step 1: Implement the callback**

Copy `VelloCallback` and `VelloResources` **verbatim from the Task 1 spike note**, which contains the version-correct, already-running code. Adapt only the scene source: instead of a hardcoded circle, take the scene the viewport built.

- [ ] **Step 2: Implement the viewport**

```rust
// crates/tessera_ui/src/view/viewport.rs
use egui::{Sense, Ui};

use crate::app::TesseraApp;
use crate::theme::Theme;

pub fn show(ui: &mut Ui, state: &mut TesseraApp) -> egui::Response {
    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, Sense::click_and_drag());
    ui.painter().rect_filled(rect, 0.0, Theme::CANVAS_BG);

    let resolved = tessera_layout::resolve::resolve(
        &state.document,
        &state.stories,
        &mut state.shaper,
    );
    let page = state.first_page_bounds();
    let scene = tessera_render::scene::build_scene(&resolved, state.view, page);

    let pixels_per_point = ui.ctx().pixels_per_point();
    let width = (rect.width() * pixels_per_point) as u32;
    let height = (rect.height() * pixels_per_point) as u32;

    if width > 0 && height > 0 {
        crate::view::callback::paint(ui, rect, scene, width, height);
    }

    response
}
```

- [ ] **Step 3: Run and verify by eye**

```bash
cargo run -p tessera_app
```

Expected: a **white page** on the dark canvas. This is the first moment Vello content appears inside egui. If the page is missing, read `preview_logs`-equivalent output and the wgpu validation messages before changing anything — do not start guessing at the transform.

- [ ] **Step 4: Add a rectangle at startup and confirm it draws**

Temporarily seed `TesseraApp::new` with one black rectangle. Confirm it appears on the page, then remove the seed.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Render the document into egui through a Vello paint callback"
```

---

### Task 17: Camera — pan and zoom

**Files:**
- Modify: `crates/tessera_ui/src/view/viewport.rs`
- Create: `crates/tessera_ui/src/camera.rs`

**Interfaces:**
- Produces: `camera::handle_input(response: &egui::Response, ui: &egui::Ui, view: &mut ViewTransform)`; `camera::zoom_to_fit(view: &mut ViewTransform, page: DocRect, viewport: egui::Vec2)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::{DocPoint, DocRect, ViewTransform};

    #[test]
    fn zoom_to_fit_makes_the_page_fill_the_shorter_axis() {
        let mut view = ViewTransform::default();
        let page = DocRect { x: 0.0, y: 0.0, width: 612.0, height: 792.0 };
        zoom_to_fit(&mut view, page, egui::vec2(1000.0, 800.0));
        // Height binds: 800 / 792, minus the margin factor.
        assert!(view.zoom < 800.0 / 792.0);
        assert!(view.zoom > 0.5);
    }

    #[test]
    fn zooming_about_a_point_keeps_that_document_point_under_the_cursor() {
        let mut view = ViewTransform { pan: DocPoint::ZERO, zoom: 1.0 };
        let cursor = tessera_geometry::ScreenPoint { x: 300.0, y: 200.0 };
        let before = view.screen_to_doc(cursor);
        zoom_about(&mut view, cursor, 1.5);
        let after = view.screen_to_doc(cursor);
        assert!((before.x - after.x).abs() < 0.001);
        assert!((before.y - after.y).abs() < 0.001);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_ui
```

Expected: FAIL — `zoom_to_fit` not found.

- [ ] **Step 3: Implement**

```rust
// crates/tessera_ui/src/camera.rs
use tessera_geometry::{DocRect, ScreenPoint, ViewTransform};

const MIN_ZOOM: f64 = 0.02;
const MAX_ZOOM: f64 = 64.0;
/// Leaves a visible pasteboard margin around a fitted page.
const FIT_MARGIN: f64 = 0.9;

/// Scale about a screen point, keeping the document point under it fixed.
pub fn zoom_about(view: &mut ViewTransform, cursor: ScreenPoint, factor: f64) {
    let anchor = view.screen_to_doc(cursor);
    view.zoom = (view.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    let after = view.screen_to_doc(cursor);
    view.pan.x += anchor.x - after.x;
    view.pan.y += anchor.y - after.y;
}

pub fn zoom_to_fit(view: &mut ViewTransform, page: DocRect, viewport: egui::Vec2) {
    let sx = f64::from(viewport.x) / page.width;
    let sy = f64::from(viewport.y) / page.height;
    view.zoom = (sx.min(sy) * FIT_MARGIN).clamp(MIN_ZOOM, MAX_ZOOM);
    view.pan.x = page.x - (f64::from(viewport.x) / view.zoom - page.width) / 2.0;
    view.pan.y = page.y - (f64::from(viewport.y) / view.zoom - page.height) / 2.0;
}

/// Middle-drag or space-drag pans; the wheel zooms about the cursor.
pub fn handle_input(response: &egui::Response, ui: &egui::Ui, view: &mut ViewTransform) {
    let space_held = ui.input(|i| i.key_down(egui::Key::Space));

    if response.dragged_by(egui::PointerButton::Middle)
        || (space_held && response.dragged_by(egui::PointerButton::Primary))
    {
        let delta = response.drag_delta();
        view.pan.x -= f64::from(delta.x) / view.zoom;
        view.pan.y -= f64::from(delta.y) / view.zoom;
    }

    if response.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            if let Some(pos) = response.hover_pos() {
                let local = pos - response.rect.min;
                zoom_about(
                    view,
                    ScreenPoint { x: local.x, y: local.y },
                    (1.0 + f64::from(scroll) * 0.001).clamp(0.5, 2.0),
                );
            }
        }
    }
}
```

- [ ] **Step 4: Wire it in and verify by hand**

Call `camera::handle_input` from `viewport::show`, and `zoom_to_fit` once when the viewport is first sized. Run the app: the wheel zooms toward the cursor, space-drag pans, and the page stays put under the pointer while zooming.

- [ ] **Step 5: Commit**

```bash
cargo test -p tessera_ui && git add -A && git commit -m "Add camera pan and zoom"
```

---

### Task 18: Tools and commands

**Files:**
- Create: `crates/tessera_ui/src/command.rs`
- Create: `crates/tessera_ui/src/tools.rs`
- Create: `crates/tessera_ui/src/view/tool_strip.rs`
- Modify: `crates/tessera_ui/src/view/viewport.rs`

**Interfaces:**
- Produces: `Tool::{Select, Rectangle, Text}`; `Command::{AddRectangle(DocRect), AddTextFrame(DocRect), MoveFrame { id, delta }, SetFill { id, color }, SetText { id, text }, Undo, Redo}`; `command::apply(state: &mut TesseraApp, command: Command)`.

**Every mutation goes through `Command`.** One place records undo, so no operation can quietly become non-undoable — the failure that left add-page and remove-page without inverses in the previous codebase.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_a_rectangle_puts_a_frame_in_the_document() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(DocRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }));
        assert_eq!(state.document.frames.len(), 1);
    }

    #[test]
    fn every_mutating_command_can_be_undone() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(DocRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }));
        apply(&mut state, Command::Undo);
        assert_eq!(state.document.frames.len(), 0);
    }

    #[test]
    fn undo_then_redo_returns_the_frame() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(DocRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }));
        apply(&mut state, Command::Undo);
        apply(&mut state, Command::Redo);
        assert_eq!(state.document.frames.len(), 1);
    }

    #[test]
    fn a_mutating_command_marks_the_document_dirty() {
        let mut state = TesseraApp::headless();
        assert!(!state.dirty);
        apply(&mut state, Command::AddRectangle(DocRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }));
        assert!(state.dirty);
    }

    #[test]
    fn a_text_frame_gets_its_own_story() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(DocRect { x: 0.0, y: 0.0, width: 100.0, height: 40.0 }));
        assert_eq!(state.stories.len(), 1);
    }
}
```

`TesseraApp::headless()` constructs the state without an `eframe::CreationContext`, so the command layer is testable without a window. Add it in this task.

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_ui
```

Expected: FAIL — `Command` not found.

- [ ] **Step 3: Implement the command layer**

```rust
// crates/tessera_ui/src/command.rs
//! Every user action, in one place.
//!
//! `apply` is the ONLY function that mutates the document. It records undo
//! before every mutating variant, which is why no command can quietly become
//! non-undoable — the exact failure that left add-page and remove-page
//! without inverses in the previous codebase.

use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::DocRect;
use tessera_text::story::Story;

use crate::app::TesseraApp;

#[derive(Debug, Clone)]
pub enum Command {
    AddRectangle(DocRect),
    AddTextFrame(DocRect),
    MoveFrame { id: FrameId, dx: f64, dy: f64 },
    SetFill { id: FrameId, color: Color },
    SetText { id: FrameId, text: String },
    Undo,
    Redo,
}

impl Command {
    fn mutates(&self) -> bool {
        !matches!(self, Self::Undo | Self::Redo)
    }
}

pub fn apply(state: &mut TesseraApp, command: Command) {
    if command.mutates() {
        state.history.record(&state.document);
        state.dirty = true;
    }

    match command {
        Command::AddRectangle(bounds) => {
            let layer = state.default_layer();
            let id = state.document.add_frame(layer, Frame {
                bounds,
                kind: FrameKind::Rectangle,
                fill: Color::BLACK,
                stroke: None,
            });
            state.selection = Some(id);
        }
        Command::AddTextFrame(bounds) => {
            let story = state.stories.insert(Story::default());
            let layer = state.default_layer();
            let id = state.document.add_frame(layer, Frame {
                bounds,
                kind: FrameKind::Text { story },
                fill: Color::WHITE,
                stroke: None,
            });
            state.selection = Some(id);
        }
        Command::MoveFrame { id, dx, dy } => {
            if let Some(frame) = state.document.frame_mut(id) {
                frame.bounds.x += dx;
                frame.bounds.y += dy;
            }
        }
        Command::SetFill { id, color } => {
            if let Some(frame) = state.document.frame_mut(id) {
                frame.fill = color;
            }
        }
        Command::SetText { id, text } => {
            if let Some(FrameKind::Text { story }) =
                state.document.frame(id).map(|f| f.kind.clone())
            {
                if let Some(s) = state.stories.get_mut(story) {
                    s.text = text;
                }
            }
        }
        Command::Undo => {
            if let Some(previous) = state.history.undo(&state.document) {
                state.document = previous;
                state.selection = None;
                state.dirty = true;
            }
        }
        Command::Redo => {
            if let Some(next) = state.history.redo(&state.document) {
                state.document = next;
                state.selection = None;
                state.dirty = true;
            }
        }
    }
}
```

- [ ] **Step 4: Implement tools and the tool strip**

`tools.rs` holds `Tool` and a `Drag { start: DocPoint, current: DocPoint }` in-progress state. In `viewport::show`:

- **Select** — click hit-tests via `Document::hit_test` and sets `selection`; drag issues `MoveFrame`. **Record undo once, on drag release, not per frame** — otherwise a single drag fills the undo stack.
- **Rectangle** — drag draws a preview with egui's painter, and on release issues `AddRectangle` with the normalized `DocRect`.
- **Text** — same, issuing `AddTextFrame`, then enters edit mode (Task 19).

`tool_strip.rs` draws the three tools as a left side panel, icons painted with `egui::Painter` (a square outline, a filled square, a letter "T"), using only `Theme` tokens. Keyboard shortcuts: `V`, `M`, `T`.

Draw the selection outline in `overlays` with `Theme::SELECTION` — **with egui's painter, not Vello**, since selection is interface and must never reach an export.

- [ ] **Step 5: Verify by hand**

```bash
cargo run -p tessera_app
```

Draw rectangles, select them, drag them, press Ctrl+Z and Ctrl+Shift+Z. Confirm one drag is one undo step.

- [ ] **Step 6: Commit**

```bash
cargo test -p tessera_ui && git add -A && git commit -m "Add the command layer, tools and selection"
```

---

### Task 19: Typing on the canvas

**N3 completes here.** The capability the previous architecture made structurally impossible.

**Files:**
- Create: `crates/tessera_ui/src/view/text_edit.rs`
- Modify: `crates/tessera_ui/src/view/viewport.rs`, `crates/tessera_ui/src/app.rs`

**Interfaces:**
- Consumes: `EditBuffer` (Task 10).
- Produces: `text_edit::handle_events(ui: &egui::Ui, buffer: &mut EditBuffer) -> bool` (returns whether the text changed); `text_edit::draw_caret(painter, ...)`.

- [ ] **Step 1: Write the failing test**

egui's `Context` can be driven headlessly by feeding `RawInput`, so this is a real test, not a manual check.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tessera_text::edit::EditBuffer;
    use tessera_text::story::Story;

    fn run_with_events(events: Vec<egui::Event>, buffer: &mut EditBuffer) {
        let ctx = egui::Context::default();
        let input = egui::RawInput { events, ..Default::default() };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                handle_events(ui, buffer);
            });
        });
    }

    #[test]
    fn a_text_event_reaches_the_buffer() {
        let mut buffer = EditBuffer::new(Story::default());
        run_with_events(vec![egui::Event::Text("Hi".to_string())], &mut buffer);
        assert_eq!(buffer.story().text, "Hi");
    }

    #[test]
    fn backspace_reaches_the_buffer() {
        let mut story = Story::default();
        story.text = "Hi".to_string();
        let mut buffer = EditBuffer::new(story);
        buffer.set_cursor(2);
        run_with_events(
            vec![egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut buffer,
        );
        assert_eq!(buffer.story().text, "H");
    }

    #[test]
    fn an_ime_preedit_reaches_the_buffer_without_committing() {
        let mut buffer = EditBuffer::new(Story::default());
        run_with_events(
            vec![egui::Event::Ime(egui::ImeEvent::Preedit("に".to_string()))],
            &mut buffer,
        );
        assert_eq!(buffer.ime_preedit(), Some("に"));
        assert_eq!(buffer.story().text, "");
    }

    #[test]
    fn an_ime_commit_enters_the_text() {
        let mut buffer = EditBuffer::new(Story::default());
        run_with_events(
            vec![egui::Event::Ime(egui::ImeEvent::Commit("日本".to_string()))],
            &mut buffer,
        );
        assert_eq!(buffer.story().text, "日本");
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_ui
```

Expected: FAIL — `handle_events` not found.

- [ ] **Step 3: Implement**

```rust
// crates/tessera_ui/src/view/text_edit.rs
//! Canvas text entry.
//!
//! egui's own `TextEdit` state is deliberately NOT used. The cursor lives in
//! `EditBuffer`, which is persistent application state, because an
//! immediate-mode widget is reconstructed every frame and a cursor it owned
//! could not survive.

use egui::{Event, ImeEvent, Key, Ui};
use tessera_text::edit::EditBuffer;

pub fn handle_events(ui: &Ui, buffer: &mut EditBuffer) -> bool {
    let mut changed = false;

    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            Event::Text(text) => {
                buffer.insert(&text);
                changed = true;
            }
            Event::Key { key, pressed: true, modifiers, .. } => match key {
                Key::Backspace => {
                    buffer.delete_backward();
                    changed = true;
                }
                Key::Delete => {
                    buffer.delete_forward();
                    changed = true;
                }
                Key::ArrowLeft => buffer.move_left(modifiers.shift),
                Key::ArrowRight => buffer.move_right(modifiers.shift),
                Key::A if modifiers.command => buffer.select_all(),
                Key::Enter => {
                    buffer.insert("\n");
                    changed = true;
                }
                _ => {}
            },
            Event::Ime(ImeEvent::Preedit(text)) => {
                buffer.set_ime_preedit(if text.is_empty() { None } else { Some(text) });
            }
            Event::Ime(ImeEvent::Commit(text)) => {
                buffer.insert(&text);
                changed = true;
            }
            _ => {}
        }
    }

    changed
}
```

**Check `ImeEvent`'s variants against egui 0.35's docs** — the enum has changed shape across releases. The tests pin the behaviour.

- [ ] **Step 4: Wire it into the viewport**

Double-clicking a text frame, or creating one with the Text tool, moves its `Story` into `state.editing = Some((frame_id, EditBuffer::new(story)))`. While editing:

- call `ui.ctx().request_focus(...)` on the viewport and set an IME rect via `ctx.output_mut(|o| o.ime = Some(...))` so the platform's IME candidate window appears in the right place;
- call `handle_events`, and on `changed` write the buffer's story back into `state.stories` and bump the revision;
- draw a caret with egui's painter at the shaped position of the cursor's byte offset, blinking on `ui.input(|i| i.time)`;
- draw the IME preedit underlined, if present;
- Escape commits and leaves edit mode, recording **one** undo entry for the whole editing session.

- [ ] **Step 5: Verify by hand — this is the milestone's centrepiece**

```bash
cargo run -p tessera_app
```

Press `T`, drag a text frame, type. **The text must appear on the canvas as you type.** Arrow keys move the caret, Backspace deletes, Escape commits.

- [ ] **Step 6: Commit**

```bash
cargo test -p tessera_ui && git add -A && git commit -m "Add on-canvas text entry with IME support"
```

---

### Task 20: The inspector

**Files:**
- Create: `crates/tessera_ui/src/view/inspector.rs`

**Interfaces:**
- Produces: `inspector::show(ui: &mut egui::Ui, state: &mut TesseraApp)`.

- [ ] **Step 1: Implement**

A right side panel keyed on `state.selection`:

- **Nothing selected** — "No selection", in `Theme::TEXT_MUTED`.
- **Any frame** — X, Y, W, H as `egui::DragValue`s in points, each issuing a command on change.
- **Rectangle or ellipse** — a fill swatch opening `egui::color_picker`, issuing `SetFill`. The picker yields sRGB; wrap it as `Color::Rgb`.
- **Text frame** — the story's text in a multi-line `TextEdit`, issuing `SetText`. This is the panel-side path; canvas editing (Task 19) is the primary one.

Use only `Theme` tokens. No literal colours.

- [ ] **Step 2: Verify by hand**

Select a rectangle, change its fill and its size, and watch the canvas update. Press Ctrl+Z and confirm the change reverts.

- [ ] **Step 3: Commit**

```bash
cargo test --workspace --lib && git add -A && git commit -m "Add the property inspector"
```

---

### Task 21: File operations

**N1 becomes user-visible here.**

**Files:**
- Create: `crates/tessera_ui/src/view/menu.rs`
- Create: `crates/tessera_ui/src/file_ops.rs`
- Modify: `crates/tessera_ui/src/app.rs`

**Interfaces:**
- Consumes: `format::{save, load}` (Task 7).
- Produces: `file_ops::{new_document, open, save, save_as, export_pdf}`, each taking `&mut TesseraApp`; `app::Status { message: String, is_error: bool }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_then_loading_a_path_restores_the_frames() {
        let dir = std::env::temp_dir().join("tessera_file_ops");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("t.tessera");

        let mut state = TesseraApp::headless();
        crate::command::apply(&mut state, crate::command::Command::AddRectangle(
            tessera_geometry::DocRect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 },
        ));
        save_to_path(&mut state, &path).expect("save");

        let mut reopened = TesseraApp::headless();
        open_from_path(&mut reopened, &path).expect("open");

        assert_eq!(reopened.document.frames.len(), 1);
    }

    #[test]
    fn a_successful_save_clears_the_dirty_flag() {
        let dir = std::env::temp_dir().join("tessera_file_ops");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("dirty.tessera");

        let mut state = TesseraApp::headless();
        crate::command::apply(&mut state, crate::command::Command::AddRectangle(
            tessera_geometry::DocRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 },
        ));
        assert!(state.dirty);
        save_to_path(&mut state, &path).expect("save");
        assert!(!state.dirty);
    }

    #[test]
    fn a_failed_open_reports_an_error_and_leaves_the_document_alone() {
        let mut state = TesseraApp::headless();
        crate::command::apply(&mut state, crate::command::Command::AddRectangle(
            tessera_geometry::DocRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 },
        ));
        let before = state.document.frames.len();

        let result = open_from_path(&mut state, std::path::Path::new("does_not_exist.tessera"));

        assert!(result.is_err());
        assert_eq!(state.document.frames.len(), before, "a failed open must not clear the document");
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_ui
```

Expected: FAIL — `save_to_path` not found.

- [ ] **Step 3: Implement**

`file_ops.rs` splits each operation into a **testable core** taking a `&Path` (`save_to_path`, `open_from_path`) and a **dialog wrapper** using `rfd::FileDialog` with a `Tessera Document (*.tessera)` filter. Only the wrapper touches `rfd`, so the core is unit-testable.

Every failure sets `state.status = Some(Status { message: error.to_string(), is_error: true })`, which the status bar shows in the accent colour. **No failure may be swallowed** — including the `NewerFormat` refusal, whose message is precisely what a user needs to see.

`menu.rs` draws a top `egui::TopBottomPanel` with **File** (New, Open…, Save, Save As…, Export PDF…, Quit) and **Edit** (Undo, Redo), with accelerators `Ctrl+N`, `Ctrl+O`, `Ctrl+S`, `Ctrl+Shift+S`, `Ctrl+Z`, `Ctrl+Shift+Z`. Save with no `current_path` falls through to Save As.

Show `dirty` in the window title as a leading `*`.

- [ ] **Step 4: Verify the acceptance sentence's middle**

```bash
cargo run -p tessera_app
```

Draw a rectangle and a text frame, type into it, Ctrl+S, quit, relaunch, Ctrl+O, open the file. **Everything must be exactly as it was left.**

- [ ] **Step 5: Commit**

```bash
cargo test -p tessera_ui && git add -A && git commit -m "Add new, open, save and save-as"
```

---

# Phase F — PDF

---

### Task 22: `tessera_pdf` — shapes

**Files:**
- Create: `crates/tessera_pdf/src/lib.rs`
- Create: `crates/tessera_pdf/src/writer.rs`
- Create: `crates/tessera_pdf/tests/export.rs`

**Interfaces:**
- Consumes: `ResolvedDocument` (Task 11) — **never `tessera_render`.**
- Produces: `export(resolved: &ResolvedDocument, page: DocRect) -> Result<Vec<u8>, PdfError>`.

PDF's origin is bottom-left; the document's is top-left. That flip is the one systematic transformation, and it belongs in exactly one function.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/tessera_pdf/tests/export.rs
use tessera_color::Color;
use tessera_geometry::DocRect;
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};

fn page() -> DocRect {
    DocRect { x: 0.0, y: 0.0, width: 612.0, height: 792.0 }
}

#[test]
fn an_empty_document_produces_a_valid_pdf_header_and_trailer() {
    let bytes = tessera_pdf::export(&ResolvedDocument::default(), page()).expect("export");
    assert!(bytes.starts_with(b"%PDF-1."), "must carry a PDF header");
    assert!(
        bytes.windows(5).any(|w| w == b"%%EOF"),
        "must be terminated with %%EOF"
    );
}

#[test]
fn the_media_box_matches_the_page_size() {
    let bytes = tessera_pdf::export(&ResolvedDocument::default(), page()).expect("export");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("612"), "the media box must carry the page width");
    assert!(text.contains("792"), "the media box must carry the page height");
}

#[test]
fn a_rectangle_emits_a_fill_operator() {
    let bytes = tessera_pdf::export(
        &ResolvedDocument {
            items: vec![ResolvedItem {
                frame: Default::default(),
                bounds: DocRect { x: 10.0, y: 10.0, width: 50.0, height: 50.0 },
                kind: ResolvedKind::Rectangle { fill: Color::BLACK, stroke: None },
            }],
        },
        page(),
    )
    .expect("export");
    // Content streams are compressed by default; export uncompressed in M0 so
    // the operators are assertable, and note the follow-up.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains(" re"), "a rectangle path operator must be present");
    assert!(text.contains(" f"), "a fill operator must be present");
}

#[test]
fn a_rectangle_is_flipped_into_pdf_coordinates() {
    // A rectangle 10pt from the document top must sit 792 - 10 - 50 = 732
    // from the PDF bottom.
    let bytes = tessera_pdf::export(
        &ResolvedDocument {
            items: vec![ResolvedItem {
                frame: Default::default(),
                bounds: DocRect { x: 10.0, y: 10.0, width: 50.0, height: 50.0 },
                kind: ResolvedKind::Rectangle { fill: Color::BLACK, stroke: None },
            }],
        },
        page(),
    )
    .expect("export");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("732"), "the y coordinate must be flipped, not copied");
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_pdf
```

Expected: FAIL — `export` not found.

- [ ] **Step 3: Implement**

Use `pdf-writer` to build: catalog, page tree, one page with `MediaBox [0 0 612 792]`, and one content stream. Write the stream **uncompressed in M0** so the tests can assert on operators; add a `# TODO(m6): compress content streams` note, since milestone 6 owns export quality.

The single coordinate flip:

```rust
/// PDF's origin is bottom-left; the document's is top-left. This is the only
/// place that conversion happens.
fn to_pdf_y(page: DocRect, doc_y: f64, height: f64) -> f64 {
    page.height - doc_y - height
}
```

For each item, emit `rg` (fill colour from `Color::to_rgb_f32`), `re` (the flipped rectangle) and `f`. Ellipses approximate with four bezier segments via `c`. `PdfError` is a `thiserror` enum; nothing is swallowed.

- [ ] **Step 4: Run tests and open a real PDF**

```bash
cargo test -p tessera_pdf
```

Expected: PASS, 4 tests. Then write one to disk and **open it in Acrobat** — a passing byte-assertion is not proof a reader accepts the file.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Add PDF export for shapes"
```

---

### Task 23: `tessera_pdf` — text

**N2 completes here.**

**Files:**
- Modify: `crates/tessera_pdf/src/writer.rs`
- Modify: `crates/tessera_pdf/tests/export.rs`

**Interfaces:**
- Consumes: `ShapedText` and `PositionedGlyph` (Task 9) — **the same values the renderer drew**, which is what makes the PDF match the screen.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_text_frame_embeds_a_font() {
    let mut shaper = tessera_text::shape::Shaper::new();
    let shaped = shaper.shape(
        &tessera_text::story::Story { text: "Hello".to_string(), ..Default::default() },
        400.0,
    );
    let bytes = tessera_pdf::export(
        &ResolvedDocument {
            items: vec![ResolvedItem {
                frame: Default::default(),
                bounds: DocRect { x: 20.0, y: 20.0, width: 400.0, height: 40.0 },
                kind: ResolvedKind::Text { shaped, color: Color::BLACK },
            }],
        },
        page(),
    )
    .expect("export");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/FontFile"), "the font must be embedded, not referenced");
    assert!(text.contains("Tj") || text.contains("TJ"), "a show-text operator must be present");
}

#[test]
fn text_is_positioned_by_the_same_glyphs_the_renderer_drew() {
    let mut shaper = tessera_text::shape::Shaper::new();
    let shaped = shaper.shape(
        &tessera_text::story::Story { text: "Hi".to_string(), ..Default::default() },
        400.0,
    );
    let first_x = shaped.lines[0].glyphs[0].x;
    let bytes = tessera_pdf::export(
        &ResolvedDocument {
            items: vec![ResolvedItem {
                frame: Default::default(),
                bounds: DocRect { x: 20.0, y: 20.0, width: 400.0, height: 40.0 },
                kind: ResolvedKind::Text { shaped, color: Color::BLACK },
            }],
        },
        page(),
    )
    .expect("export");
    let text = String::from_utf8_lossy(&bytes);
    let expected_x = format!("{:.2}", 20.0 + first_x);
    assert!(
        text.contains(&expected_x),
        "the glyph x must come from the shaper, not be recomputed"
    );
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test -p tessera_pdf
```

Expected: FAIL — no `/FontFile` in the output.

- [ ] **Step 3: Add the subsetting dependency**

`skrifa` reads glyph data but does not subset. Add the current release of `subsetter` (the crate typst uses) to `[workspace.dependencies]` and to `tessera_pdf`. Verify with `cargo add --dry-run subsetter`.

- [ ] **Step 4: Implement**

For each font in `ShapedText::fonts`: collect the glyph ids actually used, subset the blob to those, embed it as a `FontFile2` stream, and write a `Type0`/`CIDFontType2` font with an `Identity-H` encoding — the encoding that lets glyph ids be written directly, which is exactly what a shaper produces.

Emit per glyph run: `BT`, `/F0 <size> Tf`, `<x> <y> Td`, `<hex glyph ids> Tj`, `ET`, with `y` through `to_pdf_y`.

**Positions come from `PositionedGlyph` unchanged.** Never recompute them from the string — recomputation is precisely how an export drifts from the screen.

- [ ] **Step 5: Run tests, then verify in Acrobat**

```bash
cargo test -p tessera_pdf
```

Then export a real document and open it in Acrobat. **Select the text with the text tool.** Selectable text proves the encoding and the embedded font are correct in a way no byte assertion can.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Add PDF text export with embedded subsetted fonts"
```

---

### Task 24: The acceptance test

The milestone's sentence, driven end to end without a window.

**Files:**
- Create: `crates/tessera_ui/tests/milestone_0.rs`
- Modify: `ROADMAP.md`, `README.md`

**Interfaces:**
- Consumes: everything.

- [ ] **Step 1: Write the acceptance test**

```rust
// crates/tessera_ui/tests/milestone_0.rs
//! The milestone 0 acceptance sentence, end to end.
//!
//! "Draw a rectangle and give it a fill colour. Draw a text frame, type into
//! it, save as .tessera, quit, reopen, and export a PDF."
//!
//! Headless: the window is the only part not covered, and that is verified by
//! hand on Windows.

use tessera_color::Color;
use tessera_geometry::DocRect;
use tessera_ui::app::TesseraApp;
use tessera_ui::command::{apply, Command};
use tessera_ui::file_ops::{open_from_path, save_to_path};

#[test]
fn the_milestone_0_sentence_holds() {
    let dir = std::env::temp_dir().join("tessera_m0");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("acceptance.tessera");
    let pdf_path = dir.join("acceptance.pdf");

    // --- Draw a rectangle and give it a fill colour.
    let mut state = TesseraApp::headless();
    apply(&mut state, Command::AddRectangle(DocRect { x: 72.0, y: 72.0, width: 200.0, height: 100.0 }));
    let rect_id = state.selection.expect("the new rectangle is selected");
    apply(&mut state, Command::SetFill {
        id: rect_id,
        color: Color::Cmyk { c: 0.8, m: 0.2, y: 0.0, k: 0.0, a: 1.0 },
    });

    // --- Draw a text frame and type into it.
    apply(&mut state, Command::AddTextFrame(DocRect { x: 72.0, y: 240.0, width: 400.0, height: 60.0 }));
    let text_id = state.selection.expect("the new text frame is selected");
    apply(&mut state, Command::SetText { id: text_id, text: "Hello, Tessera.".to_string() });

    // --- Save.
    save_to_path(&mut state, &path).expect("save");
    assert!(!state.dirty, "a saved document is not dirty");

    // --- Quit and relaunch.
    drop(state);
    let mut reopened = TesseraApp::headless();
    open_from_path(&mut reopened, &path).expect("open");

    // --- Everything is exactly as it was left.
    assert_eq!(reopened.document.frames.len(), 2, "both frames survived");
    let rect = reopened.document.frame(rect_id).expect("the rectangle survived");
    assert_eq!(rect.bounds.width, 200.0);
    assert_eq!(rect.fill, Color::Cmyk { c: 0.8, m: 0.2, y: 0.0, k: 0.0, a: 1.0 });
    assert_eq!(
        reopened.stories.values().next().expect("the story survived").text,
        "Hello, Tessera."
    );

    // --- Export a PDF.
    let resolved = tessera_layout::resolve::resolve(
        &reopened.document,
        &reopened.stories,
        &mut reopened.shaper,
    );
    let bytes = tessera_pdf::export(&resolved, reopened.first_page_bounds()).expect("export");
    std::fs::write(&pdf_path, &bytes).expect("write pdf");

    assert!(bytes.starts_with(b"%PDF-1."));
    assert!(bytes.windows(5).any(|w| w == b"%%EOF"));
    assert!(
        String::from_utf8_lossy(&bytes).contains("/FontFile"),
        "the PDF must embed its font"
    );

    println!("Open this in Acrobat to complete the acceptance check: {}", pdf_path.display());
}
```

- [ ] **Step 2: Run it**

```bash
cargo test -p tessera_ui --test milestone_0 -- --nocapture
```

Expected: PASS. Note the printed PDF path.

- [ ] **Step 3: Perform the sentence by hand**

The headless test covers everything but the window. Now do it as a user, on Windows, against a release build:

```bash
cargo build --release -p tessera_app && ./target/release/tessera_app
```

Draw a rectangle, set its fill. Press `T`, drag a frame, **type into it and watch the text appear**. Ctrl+S. Close the window. Relaunch. Ctrl+O, open the file — everything as it was. File ▸ Export PDF. **Open the PDF in Acrobat and select the text.**

- [ ] **Step 4: Run the full suite once**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib --tests
```

Then, **separately and alone**:

```bash
cargo test -p tessera_render --test gpu_render
```

- [ ] **Step 5: Mark the milestone**

Tick milestone 0 in `ROADMAP.md` — **only after step 3 was actually performed**, not because step 2 passed. Record in the milestone note that verification was Windows-only, and that Linux and macOS remain built-and-headless-tested but interactively unverified.

Update `README.md` with a screenshot and the real build instructions.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Add the milestone 0 acceptance test and mark the milestone complete"
```

---

## Self-review

**Spec coverage.** Every section of the design has at least one task: crate graph (2), geometry (3), colour (4), document model (5, §6), file format (7, D4), undo (8, D5), text model and IME (9, 10, D3), layout resolution (11), Vello scene (12, 13), headless rasterization and reference testing (14, §10), Vello-in-egui (1, 16, D2), theme (15, §11), platform module (15, §12), PDF from the document rather than the scene (22, 23, §8), error tiers (6, 21, §9), acceptance (24, §15).

**Known gaps, deliberate.** Autosave and crash recovery (§9) are specified but not built in M0 — they need a document-open lifecycle that only exists after Task 21, and they are scheduled into milestone 7 where the roadmap already lists them. The `thumbnail.png` and `links.json` container entries (D4) are defined in the format but not written until milestones 3 and 5 need them; the loader tolerates their absence, and the version field exists so adding them is a migration rather than a break.

**Placeholder scan.** No "TBD", no "add error handling", no "similar to Task N". Three tasks (1, 9, 19) instruct the implementer to correct code against version-current docs — that is a verification step against real, complete code, not a placeholder, and each states exactly which API to check and which tests pin the behaviour.

**Type consistency.** `Frame.bounds` is `DocRect` in Tasks 5, 7, 11, 18 and 24. `PositionedGlyph` fields `{glyph_id, x, y, font_index}` match across Tasks 9, 13 and 23. `Document::paint_order` is used by Task 11 and defined in Task 5. `TesseraApp::headless()` is introduced in Task 18 and used in 21 and 24. `first_page_bounds()` is used in Tasks 16 and 24 and must be defined on `TesseraApp` in Task 15.
