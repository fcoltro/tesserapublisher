//! GPU presentation target for the vello painter.
//!
//! This is the only module in the crate that touches wgpu. It owns the device,
//! the swapchain surface and the vello `Renderer`, and it consumes the
//! `vello::Scene` produced by [`crate::paint`]. Keeping the split here means the
//! painter can be retargeted (offscreen texture, WASM host) without changes.

use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::CurrentSurfaceTexture;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

use crate::paint::{color_from_rgba, Painter, Viewport};
use crate::scene::RenderScene;

/// Failures that can occur while bringing up or driving the GPU pipeline.
#[derive(Debug)]
pub enum RenderError {
    /// No adapter/device could be acquired, or the surface could not be created.
    Initialization(String),
    /// Vello failed to encode or execute the scene.
    Vello(String),
    /// The swapchain could not hand back a frame this tick.
    Surface(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initialization(m) => write!(f, "renderer initialization failed: {m}"),
            Self::Vello(m) => write!(f, "vello render failed: {m}"),
            Self::Surface(m) => write!(f, "surface acquisition failed: {m}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<vello::Error> for RenderError {
    fn from(value: vello::Error) -> Self {
        Self::Vello(value.to_string())
    }
}

/// A vello renderer bound to a live window surface.
pub struct VelloSurface {
    context: RenderContext,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    /// Retained across frames so its encoding buffers are reused, not reallocated.
    scene: Scene,
    /// Owns the font database, so it must outlive individual frames.
    painter: Painter,
    adapter_name: String,
}

impl VelloSurface {
    /// Brings up wgpu against `target` and compiles the vello shader pipelines.
    ///
    /// `width` and `height` are in physical pixels, not CSS pixels.
    pub fn new(
        target: impl Into<vello::wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<Self, RenderError> {
        let mut context = RenderContext::new();
        let (width, height) = (width.max(1), height.max(1));

        let surface = pollster::block_on(context.create_surface(
            target,
            width,
            height,
            vello::wgpu::PresentMode::AutoVsync,
        ))
        .map_err(|e| RenderError::Initialization(e.to_string()))?;

        let device_handle = context
            .devices
            .get(surface.dev_id)
            .ok_or_else(|| RenderError::Initialization("no device for surface".to_string()))?;
        let adapter_name = device_handle.adapter().get_info().name;

        let renderer = Renderer::new(
            &device_handle.device,
            RendererOptions {
                use_cpu: false,
                // Only the area pipeline is compiled: MSAA permutations cost
                // noticeable startup time and nothing here needs them.
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )?;

        Ok(Self {
            context,
            surface,
            renderer,
            scene: Scene::new(),
            painter: Painter::new(),
            adapter_name,
        })
    }

    /// Name of the physical adapter actually in use, for the UI status badge.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Current surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.surface.config.width, self.surface.config.height)
    }

    /// Reconfigures the swapchain after the viewport changes size.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) != self.size() {
            self.context.resize_surface(&mut self.surface, width, height);
        }
    }

    /// Paints `render_scene` and presents it.
    ///
    /// Returns `Ok(false)` when the frame was intentionally skipped because the
    /// swapchain was not ready (window occluded, resized, or timed out). Callers
    /// should treat that as a no-op, not an error.
    pub fn render(
        &mut self,
        render_scene: &RenderScene,
        viewport: Viewport,
    ) -> Result<bool, RenderError> {
        self.painter
            .paint_into(&mut self.scene, render_scene, viewport);

        // Device and Queue are handle types, so cloning them is cheap and frees
        // the borrow on `self.context` needed to reconfigure the surface below.
        let (device, queue) = {
            let device_handle = self
                .context
                .devices
                .get(self.surface.dev_id)
                .ok_or_else(|| RenderError::Vello("device disappeared".to_string()))?;
            (device_handle.device.clone(), device_handle.queue.clone())
        };

        let params = RenderParams {
            base_color: pasteboard_color(render_scene),
            width: self.surface.config.width,
            height: self.surface.config.height,
            antialiasing_method: AaConfig::Area,
        };

        // Vello renders into its own storage texture, which is then blitted onto
        // the swapchain image; it cannot write to a surface texture directly.
        self.renderer.render_to_texture(
            &device,
            &queue,
            &self.scene,
            &self.surface.target_view,
            &params,
        )?;

        let frame = match self.surface.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            // Transient: the compositor cannot give us a frame right now.
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return Ok(false),
            // The swapchain no longer matches the window. Reconfigure and let
            // the next redraw request pick it up.
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                self.context.configure_surface(&self.surface);
                return Ok(false);
            }
            _ => return Err(RenderError::Surface("surface unavailable".to_string())),
        };

        let mut encoder = device.create_command_encoder(&vello::wgpu::CommandEncoderDescriptor {
            label: Some("tessera::blit"),
        });
        let frame_view = frame
            .texture
            .create_view(&vello::wgpu::TextureViewDescriptor::default());
        self.surface.blitter.copy(
            &device,
            &mut encoder,
            &self.surface.target_view,
            &frame_view,
        );
        queue.submit([encoder.finish()]);
        frame.present();

        Ok(true)
    }
}

/// The pasteboard clear color for a scene.
fn pasteboard_color(render_scene: &RenderScene) -> Color {
    color_from_rgba(render_scene.pasteboard_color)
}

/// Bytes per pixel of the `Rgba8Unorm` target vello renders into.
const BYTES_PER_PIXEL: u32 = 4;
/// wgpu requires buffer copy rows to be aligned to 256 bytes.
const COPY_ALIGNMENT: u32 = 256;

/// A vello renderer with no window, rendering into an offscreen texture.
///
/// This exists for two reasons. It is the fallback presentation target if
/// compositing native content under the webview ever proves unworkable on a
/// platform, and — more immediately — it makes the GPU path testable on a
/// machine with no display, which a window surface cannot be.
pub struct VelloHeadless {
    context: RenderContext,
    renderer: Renderer,
    scene: Scene,
    painter: Painter,
    device_id: usize,
    width: u32,
    height: u32,
    target: vello::wgpu::Texture,
    target_view: vello::wgpu::TextureView,
}

impl VelloHeadless {
    /// Acquires an adapter and allocates an offscreen target of the given size.
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        let (width, height) = (width.max(1), height.max(1));
        let mut context = RenderContext::new();

        let device_id = pollster::block_on(context.device(None))
            .ok_or_else(|| RenderError::Initialization("no compatible adapter".to_string()))?;
        let device_handle = context
            .devices
            .get(device_id)
            .ok_or_else(|| RenderError::Initialization("no device".to_string()))?;

        let renderer = Renderer::new(
            &device_handle.device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )?;

        let target = device_handle
            .device
            .create_texture(&vello::wgpu::TextureDescriptor {
                label: Some("tessera::headless_target"),
                size: vello::wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: vello::wgpu::TextureDimension::D2,
                // STORAGE_BINDING is what vello writes through; COPY_SRC is what
                // makes the result readable back on the CPU.
                usage: vello::wgpu::TextureUsages::STORAGE_BINDING
                    | vello::wgpu::TextureUsages::COPY_SRC,
                format: vello::wgpu::TextureFormat::Rgba8Unorm,
                view_formats: &[],
            });
        let target_view = target.create_view(&vello::wgpu::TextureViewDescriptor::default());

        Ok(Self {
            context,
            renderer,
            scene: Scene::new(),
            painter: Painter::new(),
            device_id,
            width,
            height,
            target,
            target_view,
        })
    }

    /// Name of the adapter in use.
    pub fn adapter_name(&self) -> String {
        self.context
            .devices
            .get(self.device_id)
            .map(|d| d.adapter().get_info().name)
            .unwrap_or_default()
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Renders a scene and reads the result back as tightly packed RGBA8 rows.
    pub fn render_to_pixels(
        &mut self,
        render_scene: &RenderScene,
        viewport: Viewport,
    ) -> Result<Vec<u8>, RenderError> {
        self.painter
            .paint_into(&mut self.scene, render_scene, viewport);

        let (device, queue) = {
            let handle = self
                .context
                .devices
                .get(self.device_id)
                .ok_or_else(|| RenderError::Vello("device disappeared".to_string()))?;
            (handle.device.clone(), handle.queue.clone())
        };

        self.renderer.render_to_texture(
            &device,
            &queue,
            &self.scene,
            &self.target_view,
            &RenderParams {
                base_color: pasteboard_color(render_scene),
                width: self.width,
                height: self.height,
                antialiasing_method: AaConfig::Area,
            },
        )?;

        let unpadded_row = self.width * BYTES_PER_PIXEL;
        let padded_row = unpadded_row.div_ceil(COPY_ALIGNMENT) * COPY_ALIGNMENT;
        let readback = device.create_buffer(&vello::wgpu::BufferDescriptor {
            label: Some("tessera::readback"),
            size: (padded_row * self.height) as u64,
            usage: vello::wgpu::BufferUsages::MAP_READ | vello::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&vello::wgpu::CommandEncoderDescriptor {
            label: Some("tessera::readback_copy"),
        });
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            vello::wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: vello::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            vello::wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);

        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(vello::wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        device
            .poll(vello::wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| RenderError::Vello(e.to_string()))?;
        receiver
            .recv()
            .map_err(|e| RenderError::Vello(e.to_string()))?
            .map_err(|e| RenderError::Vello(e.to_string()))?;

        // Strip the row padding so callers get a plain width*height*4 buffer.
        let mapped = readback.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((unpadded_row * self.height) as usize);
        for row in 0..self.height {
            let start = (row * padded_row) as usize;
            pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
        }
        drop(mapped);
        readback.unmap();

        Ok(pixels)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasteboard_color_comes_from_the_scene() {
        let scene = RenderScene {
            pasteboard_color: [0.1, 0.2, 0.3, 1.0],
            ..Default::default()
        };

        assert_eq!(pasteboard_color(&scene).components, [0.1, 0.2, 0.3, 1.0]);
    }

    #[test]
    fn render_errors_describe_their_stage() {
        let err = RenderError::Initialization("no adapter".to_string());
        assert!(err.to_string().contains("initialization"));
        assert!(err.to_string().contains("no adapter"));
    }
}
