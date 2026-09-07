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

    // --- the blurred backdrop, for glass panels
    //
    // **This is the whole blur, and there is no blur shader.** The document is
    // rendered a second time into a much smaller texture, and egui stretches it
    // back up with a linear filter when a panel paints it. A bilinear
    // magnification of an n-times reduction *is* a box blur of radius n, and
    // Vello has already antialiased the small render properly, so the result is
    // smoother than a box blur of the full-size image would be.
    //
    // What that buys: no WGSL to get right, no extra pipeline, no second device
    // queue, and the blur radius is one integer that a preferences slider can
    // hold. What it costs: at a divisor of six, one thirty-sixth of the pixels
    // of the main render. A stronger blur is *cheaper*, which is a pleasant
    // inversion of the usual arrangement.
    backdrop: wgpu::Texture,
    backdrop_view: wgpu::TextureView,
    pub backdrop_id: egui::TextureId,
    backdrop_size: (u32, u32),
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
    let (backdrop, backdrop_view) = create_target(&state.device, INITIAL, INITIAL);

    let mut egui_renderer = state.renderer.write();
    let texture_id =
        egui_renderer.register_native_texture(&state.device, &view, wgpu::FilterMode::Linear);
    // `Linear`, and that filter is not a detail here: it is what turns a small
    // render into a blur when it is stretched back up. `Nearest` would give
    // visible squares.
    let backdrop_id = egui_renderer.register_native_texture(
        &state.device,
        &backdrop_view,
        wgpu::FilterMode::Linear,
    );
    egui_renderer.callback_resources.insert(VelloResources {
        renderer: Mutex::new(renderer),
        texture,
        view,
        texture_id,
        size: (INITIAL, INITIAL),
        backdrop,
        backdrop_view,
        backdrop_id,
        backdrop_size: (INITIAL, INITIAL),
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

/// The smallest a backdrop may be rendered.
///
/// Below about this the page stops being recognisable as a page and the glass
/// looks like a coloured smear, which reads as a bug rather than as a blur.
const SMALLEST_BACKDROP: u32 = 8;

/// Make sure the backdrop target matches the canvas reduced by `divisor`, and
/// return the id a panel should paint.
///
/// `None` when there is nothing to show — which is not a fault: a glass panel
/// falls back to painting itself solid, and a person who cannot get a backdrop
/// gets a working interface rather than a transparent one.
pub fn prepare_backdrop(
    state: &egui_wgpu::RenderState,
    width: u32,
    height: u32,
    divisor: u32,
) -> Option<egui::TextureId> {
    // The floor is applied to the *divisor*, not to each axis on its own.
    // Clamping the two independently would keep one axis at its reduced size
    // while the other hit the floor, so the backdrop would no longer be the same
    // shape as the canvas — and the scene, scaled by one factor, would be
    // letterboxed or cropped inside it. A tall thin window is exactly where that
    // shows.
    let divisor = divisor.max(1);
    let smallest = width.min(height).max(1);
    let divisor = divisor.min((smallest / SMALLEST_BACKDROP).max(1));
    let small = (
        (width / divisor).max(SMALLEST_BACKDROP),
        (height / divisor).max(SMALLEST_BACKDROP),
    );

    let mut egui_renderer = state.renderer.write();
    let needs_resize = egui_renderer
        .callback_resources
        .get::<VelloResources>()
        .is_some_and(|r| r.backdrop_size != small);

    if needs_resize {
        let (texture, view) = create_target(&state.device, small.0, small.1);
        let id = egui_renderer
            .callback_resources
            .get::<VelloResources>()?
            .backdrop_id;
        // Rebound rather than re-registered, for the same reason the main
        // target is: nothing downstream should have to notice a resize.
        egui_renderer.update_egui_texture_from_wgpu_texture(
            &state.device,
            &view,
            wgpu::FilterMode::Linear,
            id,
        );
        let res = egui_renderer
            .callback_resources
            .get_mut::<VelloResources>()?;
        res.backdrop = texture;
        res.backdrop_view = view;
        res.backdrop_size = small;
    }

    egui_renderer
        .callback_resources
        .get::<VelloResources>()
        .map(|r| r.backdrop_id)
}

pub struct VelloCallback {
    pub scene: Scene,
    pub width: u32,
    pub height: u32,
    /// How much smaller the backdrop is rendered, or `None` to render none.
    pub backdrop_divisor: Option<u32>,
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

        // The backdrop: the same scene again, scaled down to fit a much smaller
        // texture. `Scene::append` with a transform costs an encoding copy
        // rather than a rebuild, so the layout work is done once however many
        // times it is drawn.
        if self.backdrop_divisor.is_some() {
            let small = res.backdrop_size;
            // The scale is worked out from the *actual* texture size, not from
            // the divisor, because the target has a floor: at a small window and
            // a strong blur the two stop agreeing, and scaling by the divisor
            // then would render a corner of the page rather than the page.
            let scale = f64::from(small.0) / f64::from(self.width.max(1));

            let mut reduced = Scene::new();
            reduced.append(&self.scene, Some(vello::kurbo::Affine::scale(scale)));

            if let Err(e) = renderer.render_to_texture(
                device,
                queue,
                &reduced,
                &res.backdrop_view,
                &RenderParams {
                    base_color: self.background,
                    width: small.0,
                    height: small.1,
                    antialiasing_method: AaConfig::Area,
                },
            ) {
                eprintln!("tessera: vello backdrop render failed: {e:?}");
            }
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
