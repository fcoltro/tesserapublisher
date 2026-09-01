//! Rasterizing a scene to a CPU pixel buffer, with no window.
//!
//! This is what makes rendering regression-testable, and it is what will
//! produce page thumbnails for the `.tessera` container. Written from the wgpu
//! and Vello documentation: this build is clean-room and does not consult the
//! previous implementation.

use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

/// wgpu requires `bytes_per_row` in a texture-to-buffer copy to be a multiple
/// of this. At 4 bytes per pixel a 100px-wide image needs 400 bytes of data in
/// a 512-byte row, so the copy-back must skip the 112-byte tail of each row.
/// Getting this wrong produces a progressively sheared image rather than an
/// error, which is why the tests sample specific pixel coordinates.
const COPY_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
const BYTES_PER_PIXEL: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no GPU adapter is available: {0}")]
    NoAdapter(String),
    #[error("could not create a device: {0}")]
    Device(String),
    #[error("vello failed to render: {0}")]
    Render(String),
    #[error("could not read the rendered pixels back: {0}")]
    Readback(String),
}

pub struct HeadlessRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    adapter_name: String,
}

impl HeadlessRenderer {
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::default();

        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let adapter_name = adapter.get_info().name;

        // The Task 1 spike established that Vello needs no extra features and
        // no raised limits: it ran on eframe's stock device. The same holds
        // here, so nothing is requested beyond the defaults.
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tessera headless"),
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;

        let renderer = Renderer::new(&device, RendererOptions::default())
            .map_err(|e| RenderError::Device(format!("{e:?}")))?;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tessera headless target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            device,
            queue,
            renderer,
            texture,
            view,
            width,
            height,
            adapter_name,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Render `scene` and read the result back as tightly packed RGBA8.
    pub fn render(&mut self, scene: &Scene) -> Result<Vec<u8>, RenderError> {
        self.renderer
            .render_to_texture(
                &self.device,
                &self.queue,
                scene,
                &self.view,
                &RenderParams {
                    base_color: vello::peniko::color::palette::css::WHITE,
                    width: self.width,
                    height: self.height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| RenderError::Render(format!("{e:?}")))?;

        self.read_back()
    }

    fn read_back(&self) -> Result<Vec<u8>, RenderError> {
        let unpadded_row = self.width * BYTES_PER_PIXEL;
        let padded_row = unpadded_row.div_ceil(COPY_ALIGNMENT) * COPY_ALIGNMENT;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tessera readback"),
            size: u64::from(padded_row) * u64::from(self.height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("tessera readback"),
            });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            // A send failure means the receiver is gone, which cannot happen
            // while this function is on the stack.
            let _ = tx.send(r);
        });
        // A bounded wait, not an indefinite one. Adapter and submission
        // waits are known to hang intermittently on this hardware, and a hang
        // in a test looks exactly like a slow compile. A timeout turns that
        // into a reported error instead.
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;

        // Drop the padding: each row carries `unpadded_row` real bytes inside
        // a `padded_row` stride.
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((unpadded_row * self.height) as usize);
        for row in 0..self.height as usize {
            let start = row * padded_row as usize;
            pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
        }
        drop(mapped);
        buffer.unmap();

        Ok(pixels)
    }
}
