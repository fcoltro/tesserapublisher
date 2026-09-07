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
