//! Hosting Vello inside egui.
//!
//! Decision D2, verified by the Task 1 spike: Vello renders into a texture on
//! **the same `wgpu::Device` eframe owns**, and egui composites that texture
//! as an ordinary image. One device, one frame, one input queue — so a panel
//! overlapping the canvas is just two egui widgets overlapping, and the
//! previous architecture's "panels must not be trapped under the canvas"
//! constraint cannot even be expressed.

use std::sync::Mutex;

use eframe::egui_wgpu;
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

/// Lives in egui's `callback_resources`.
///
/// `Renderer` is wrapped in a `Mutex` because it holds a `RefCell<Vec<u8>>`
/// and is therefore `Send` but **not** `Sync`, while `CallbackResources`
/// requires `Send + Sync`. A hard constraint of the two crates, not a
/// stylistic choice.
pub struct VelloResources {
    renderer: Mutex<Renderer>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    pub texture_id: egui::TextureId,
    size: (u32, u32),
}

fn create_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tessera viewport"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        // Vello writes through a compute shader; egui samples the result.
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Called once, from the eframe creation closure.
pub fn install(state: &egui_wgpu::RenderState) -> Result<(), String> {
    const INITIAL: u32 = 16;

    let renderer = Renderer::new(&state.device, RendererOptions::default())
        .map_err(|e| format!("vello could not create a renderer: {e:?}"))?;
    let (texture, view) = create_target(&state.device, INITIAL, INITIAL);

    let mut egui_renderer = state.renderer.write();
    let texture_id =
        egui_renderer.register_native_texture(&state.device, &view, wgpu::FilterMode::Linear);
    egui_renderer.callback_resources.insert(VelloResources {
        renderer: Mutex::new(renderer),
        texture,
        view,
        texture_id,
        size: (INITIAL, INITIAL),
    });

    Ok(())
}

/// Ensure the target matches the widget, and return the id egui should draw.
///
/// Resizing rebinds the **same** `TextureId` rather than issuing a new one, so
/// nothing downstream has to notice that a resize happened.
pub fn prepare_target(
    state: &egui_wgpu::RenderState,
    width: u32,
    height: u32,
) -> Option<egui::TextureId> {
    let mut egui_renderer = state.renderer.write();

    let needs_resize = egui_renderer
        .callback_resources
        .get::<VelloResources>()
        .is_some_and(|r| r.size != (width, height));

    if needs_resize {
        let (texture, view) = create_target(&state.device, width, height);
        let id = egui_renderer
            .callback_resources
            .get::<VelloResources>()?
            .texture_id;
        egui_renderer.update_egui_texture_from_wgpu_texture(
            &state.device,
            &view,
            wgpu::FilterMode::Linear,
            id,
        );
        let res = egui_renderer
            .callback_resources
            .get_mut::<VelloResources>()?;
        res.texture = texture;
        res.view = view;
        res.size = (width, height);
    }

    egui_renderer
        .callback_resources
        .get::<VelloResources>()
        .map(|r| r.texture_id)
}

/// The page thumbnails: small targets of their own, drawn by the canvas's
/// renderer on the canvas's device, each kept until its document moves on.
#[derive(Default)]
pub struct Thumbnails {
    held: std::collections::HashMap<u64, Thumb>,
}

struct Thumb {
    // Held so they stay valid: egui samples them until they are replaced.
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    id: egui::TextureId,
    size: (u32, u32),
    revision: u64,
}

/// What [`thumbnail`] has to show.
pub struct Thumbnail {
    pub texture: egui::TextureId,
    /// Drawn from the document as it is now.
    pub current: bool,
    /// Drawn on this call, which is what the caller's budget counts.
    pub rendered: bool,
}

/// The thumbnail named `key`, `size` device pixels, for document `revision`.
///
/// Rendered from `scene` — built only if it is needed — when what is held is
/// of another revision or size and `may_render` allows; otherwise the one held
/// is handed back as it is, stale or not, so a page never goes blank while it
/// waits its turn. `None` when there is nothing held and no render allowed, or
/// no renderer.
pub fn thumbnail(
    state: &egui_wgpu::RenderState,
    key: u64,
    revision: u64,
    size: (u32, u32),
    may_render: bool,
    scene: impl FnOnce() -> Scene,
) -> Option<Thumbnail> {
    let mut egui_renderer = state.renderer.write();
    let mut thumbnails = egui_renderer
        .callback_resources
        .remove::<Thumbnails>()
        .unwrap_or_default();
    let answer = (|| {
        let held = thumbnails.held.get(&key);
        if let Some(t) = held
            && t.size == size
            && t.revision == revision
        {
            return Some(Thumbnail {
                texture: t.id,
                current: true,
                rendered: false,
            });
        }
        if !may_render {
            return held.map(|t| Thumbnail {
                texture: t.id,
                current: false,
                rendered: false,
            });
        }
        let (texture, view) = create_target(&state.device, size.0, size.1);
        let id = match held {
            Some(t) => {
                egui_renderer.update_egui_texture_from_wgpu_texture(
                    &state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                    t.id,
                );
                t.id
            }
            None => egui_renderer.register_native_texture(
                &state.device,
                &view,
                wgpu::FilterMode::Linear,
            ),
        };
        let scene = scene();
        let vello = egui_renderer.callback_resources.get::<VelloResources>()?;
        let mut renderer = vello.renderer.lock().ok()?;
        if let Err(e) = renderer.render_to_texture(
            &state.device,
            &state.queue,
            &scene,
            &view,
            &RenderParams {
                base_color: vello::peniko::color::AlphaColor::new([1.0, 1.0, 1.0, 1.0]),
                width: size.0,
                height: size.1,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            eprintln!("tessera: thumbnail render failed: {e:?}");
        }
        drop(renderer);
        thumbnails.held.insert(
            key,
            Thumb {
                _texture: texture,
                _view: view,
                id,
                size,
                revision,
            },
        );
        Some(Thumbnail {
            texture: id,
            current: true,
            rendered: true,
        })
    })();
    egui_renderer.callback_resources.insert(thumbnails);
    answer
}

pub struct VelloCallback {
    pub scene: Scene,
    pub width: u32,
    pub height: u32,
    pub background: vello::peniko::color::AlphaColor<vello::peniko::color::Srgb>,
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
        if res.size != (self.width, self.height) {
            // The target was not resized this frame. Skipping is correct and
            // self-correcting: `prepare_target` resizes before the next one.
            return Vec::new();
        }

        let Ok(mut renderer) = res.renderer.lock() else {
            return Vec::new();
        };
        // A render failure must not take the application down: the document is
        // still intact and still saveable, which is exactly why save, export
        // and preflight are independent of the GPU.
        if let Err(e) = renderer.render_to_texture(
            device,
            queue,
            &self.scene,
            &res.view,
            &RenderParams {
                base_color: self.background,
                width: self.width,
                height: self.height,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            eprintln!("tessera: vello render failed: {e:?}");
        }

        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        // Intentionally empty. `prepare` fills the texture; egui draws it as
        // an ordinary image, which is the whole point of decision D2.
    }
}
