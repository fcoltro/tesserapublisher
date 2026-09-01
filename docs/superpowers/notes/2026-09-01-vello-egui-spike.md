# Spike: Vello inside egui — result

**Date:** 2026-09-01
**Task:** Milestone 0, Task 1
**Spec decision under test:** D2 — Vello renders into egui through a wgpu paint callback

---

## Verdict: PASS

Vello 0.10 renders into a texture on **the same `wgpu::Device` eframe creates**,
and egui composites that texture as an ordinary image widget in the same frame
it draws its own buttons.

Decision D2 stands. Risk R1 is closed.

**Verified on:**

| | |
|---|---|
| Adapter | NVIDIA GeForce RTX 4070 Ti |
| Backend | **Vulkan** (eframe's own choice on Windows) |
| OS | Windows 11 Pro 26200 |
| wgpu | 29.0.4 — single version, shared by `egui-wgpu 0.35` and `vello 0.10` |
| Result | 30 UI frames, 29 successful Vello renders, no errors |

Linux and macOS remain **unverified**.

---

## Finding 1: no `WgpuConfiguration` is needed

The plan budgeted for a custom `device_descriptor` requesting Vello's features
and raised limits. **It is not required.** Vello built its renderer and drew
successfully on eframe's stock device:

```
device features: Features { features_wgpu: FeaturesWGPU(0x0), features_webgpu: FeaturesWebGPU(0x0) }
max storage buffers per shader stage: 8
```

No features at all, and wgpu's default storage-buffer limit. `eframe::NativeOptions::default()`
is sufficient.

**Consequence for Task 15:** do not add a `WgpuConfiguration`. If a later
milestone needs one (Vello's `RendererOptions` gain a feature-dependent path,
or a weaker adapter appears), add it then, with the failure that motivated it
recorded.

## Finding 2: `vello::Renderer` is `Send` but **not** `Sync`

`Renderer` holds a `RefCell<Vec<u8>>` (`vello-0.10.0/src/wgpu_engine.rs:99`,
the `MaterializedBuffer::Cpu` variant). `egui_wgpu::CallbackResources` is a
`TypeMap` whose `insert` requires `T: Send + Sync`, and `CallbackTrait` itself
is declared `Send + Sync`.

So the renderer **must** be wrapped:

```rust
struct VelloResources {
    renderer: Mutex<Renderer>,
    // ...
}
```

This is a hard constraint of the two crates, not a stylistic choice. It costs
one uncontended lock per frame.

## Finding 3: `eframe::App` has no `update` method in 0.35

This is the largest deviation from what the plan assumed, and it changes the
shape of `TesseraApp`.

```rust
pub trait App {
    /// Optional. Called once before each `ui`. May NOT paint.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut Frame) { }

    /// Required. The root Ui, with no margin or background.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame);
}
```

The familiar `fn update(&mut self, ctx: &Context, frame: &mut Frame)` does not
exist. Consequences:

- The app is handed a **root `egui::Ui`**, not a `Context`. Reach the context
  with `ui.ctx()`.
- `CentralPanel::show` takes `&mut Ui`, not `&Context`. Drawing straight into
  the provided `ui` is simplest; wrap in `egui::Frame::central_panel` if a
  background is wanted.
- **`logic()` is a good fit for Tessera**: document mutation, autosave ticks
  and preflight belong there, with `ui()` restricted to drawing. That
  separation is worth adopting deliberately rather than putting everything in
  `ui()`.

## Finding 4: eframe picks Vulkan on Windows, and it works

The plan's platform module said "DX12 on Windows". eframe selected **Vulkan**
on this NVIDIA adapter with no prompting, and Vello rendered correctly.

**Do not force a backend.** Let wgpu choose, and record what it chose. A
forced DX12 here would have been a change with no evidence behind it.

---

## The verified code

Copy from here rather than from memory. This exact code ran.

### Resources, created once

```rust
use std::sync::Mutex;
use vello::wgpu;
use vello::{Renderer, RendererOptions};

const SIZE: u32 = 512; // in the real app: the viewport's pixel size

struct VelloResources {
    renderer: Mutex<Renderer>,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

// inside the eframe creation closure, given `cc: &eframe::CreationContext`:
let state = cc.wgpu_render_state.as_ref().expect("wgpu backend required");
let device = &state.device;

let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("vello target"),
    size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
});
let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

let renderer = Renderer::new(device, RendererOptions::default())
    .expect("vello renderer");

let texture_id = state.renderer.write().register_native_texture(
    device,
    &view,
    wgpu::FilterMode::Linear,
);

state.renderer.write().callback_resources.insert(VelloResources {
    renderer: Mutex::new(renderer),
    _texture: texture,
    view,
});
```

`STORAGE_BINDING | TEXTURE_BINDING` is the required usage pair: Vello writes
through a compute shader, egui samples the result.

### The callback

```rust
use eframe::egui_wgpu;
use vello::{AaConfig, RenderParams, Scene};
use vello::peniko::color::palette;

struct VelloCallback {
    scene: Scene,      // built by the viewport each frame
    width: u32,
    height: u32,
}

impl egui_wgpu::CallbackTrait for VelloCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(res) = callback_resources.get_mut::<VelloResources>() else {
            return Vec::new();
        };

        let params = RenderParams {
            base_color: palette::css::WHITE,
            width: self.width,
            height: self.height,
            antialiasing_method: AaConfig::Area,
        };

        let mut renderer = res.renderer.lock().expect("renderer mutex poisoned");
        renderer
            .render_to_texture(device, queue, &self.scene, &res.view, &params)
            .expect("vello render");

        Vec::new()
    }

    // Required — has no default implementation.
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        // Empty on purpose. `prepare` fills the texture; egui draws it.
    }
}
```

### Showing it

```rust
let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());

ui.painter().add(egui_wgpu::Callback::new_paint_callback(
    rect,
    VelloCallback { scene, width, height },
));

ui.painter().image(
    texture_id,
    rect,
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
    egui::Color32::WHITE,
);
```

Order matters: add the callback, then the image. `prepare` runs before the
render pass, so the texture is filled by the time egui samples it.

---

## Still open for Task 16

The spike used a fixed 512×512 texture. The real viewport must reallocate the
texture when the widget resizes, inside `prepare`, keyed on `(width, height)`,
**and re-register it** — `register_native_texture` returns a new `TextureId`
for a new view, so the id cannot be cached across a resize. Use
`update_egui_texture_from_wgpu_texture` if it avoids the churn; otherwise
re-register and store the fresh id.

One frame of lag was observed (30 UI frames, 29 Vello renders) because the
first `ui()` runs before the first `prepare()`. Harmless — but the viewport
should paint its background so frame one is not a hole.
