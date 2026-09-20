//! Drawing in the opposite of whatever is already on the screen.
//!
//! The pointer over the canvas is painted, not requested (see
//! [`crate::cursor`]), and it used to be painted black on a page and white on
//! the pasteboard — the two grounds the canvas was assumed to have. A page
//! has more than one colour on it: a black rectangle, a dark photograph, a
//! block of colour, and over each of those the pointer vanished.
//!
//! So the pointer is drawn through a blend that inverts: every pixel it
//! covers becomes one minus what was there, and the edge pixels blend by
//! coverage. Over white it is black, over black white, over red cyan, over
//! a photograph a negative of the photograph — always the other colour,
//! because it *is* the other colour, computed by the GPU from the pixels
//! rather than guessed from the document.
//!
//! egui has no blend modes, so this is a paint callback: the same mesh the
//! tessellator would make for the icon, drawn by a pipeline of its own with
//! the source factor *one minus destination*. Added to the painter after every
//! overlay, so it sits over the handles and the guides rather than under them.

use eframe::egui_wgpu;
use vello::wgpu;

/// The pipeline and the buffers, in egui's `callback_resources`.
pub struct InvertResources {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// How many bytes each buffer can take before it is made again.
    room: (u64, u64),
}

/// One vertex: clip-space position and premultiplied coverage colour.
const VERTEX_BYTES: u64 = 6 * 4;

const SHADER: &str = r#"
struct In {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
};
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};
@vertex
fn vs_main(v: In) -> Out {
    var o: Out;
    o.pos = vec4<f32>(v.pos, 0.0, 1.0);
    o.color = v.color;
    return o;
}
@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    return i.color;
}
"#;

fn buffer(
    device: &wgpu::Device,
    label: &str,
    bytes: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.max(64),
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Called once, from the eframe creation closure, after the surface's format
/// is known: the pipeline is built for it.
pub fn install(state: &egui_wgpu::RenderState) -> Result<(), String> {
    let device = &state.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("tessera invert"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("tessera invert"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    // `out = src * (1 - dst) + dst * (1 - src.a)`, with `src = (a, a, a, a)`:
    // full coverage gives `1 - dst`, no coverage leaves `dst`, and the
    // feathered edge blends between the two.
    let blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::OneMinusDst,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tessera invert"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: state.target_format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        // egui's own meshes are drawn without multisampling, and this pass
        // is theirs.
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    let room = (VERTEX_BYTES * 1024, 4 * 2048);
    let vertices = buffer(
        device,
        "tessera invert vertices",
        room.0,
        wgpu::BufferUsages::VERTEX,
    );
    let indices = buffer(
        device,
        "tessera invert indices",
        room.1,
        wgpu::BufferUsages::INDEX,
    );
    state
        .renderer
        .write()
        .callback_resources
        .insert(InvertResources {
            pipeline,
            vertices,
            indices,
            room,
        });
    Ok(())
}

/// A mesh to draw inverted, in points, and the rectangle the callback was
/// added with — egui sets the viewport to that rectangle, so clip space is
/// measured from it.
pub struct InvertCallback {
    pub mesh: egui::epaint::Mesh,
    pub viewport: egui::Rect,
}

impl InvertCallback {
    /// The vertices in clip space with their colours, as the pipeline reads
    /// them.
    fn vertex_bytes(&self) -> Vec<u8> {
        let vp = self.viewport;
        let mut out = Vec::with_capacity(self.mesh.vertices.len() * VERTEX_BYTES as usize);
        for v in &self.mesh.vertices {
            let x = (v.pos.x - vp.min.x) / vp.width() * 2.0 - 1.0;
            let y = 1.0 - (v.pos.y - vp.min.y) / vp.height() * 2.0;
            let c = v.color;
            for f in [
                x,
                y,
                f32::from(c.r()) / 255.0,
                f32::from(c.g()) / 255.0,
                f32::from(c.b()) / 255.0,
                f32::from(c.a()) / 255.0,
            ] {
                out.extend_from_slice(&f.to_le_bytes());
            }
        }
        out
    }

    fn index_bytes(&self) -> Vec<u8> {
        self.mesh
            .indices
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect()
    }
}

impl egui_wgpu::CallbackTrait for InvertCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(res) = resources.get_mut::<InvertResources>() else {
            return Vec::new();
        };
        if self.mesh.is_empty() || self.viewport.width() <= 0.0 || self.viewport.height() <= 0.0 {
            return Vec::new();
        }
        let vertices = self.vertex_bytes();
        let indices = self.index_bytes();
        if (vertices.len() as u64) > res.room.0 {
            res.room.0 = (vertices.len() as u64).next_power_of_two();
            res.vertices = buffer(
                device,
                "tessera invert vertices",
                res.room.0,
                wgpu::BufferUsages::VERTEX,
            );
        }
        if (indices.len() as u64) > res.room.1 {
            res.room.1 = (indices.len() as u64).next_power_of_two();
            res.indices = buffer(
                device,
                "tessera invert indices",
                res.room.1,
                wgpu::BufferUsages::INDEX,
            );
        }
        queue.write_buffer(&res.vertices, 0, &vertices);
        queue.write_buffer(&res.indices, 0, &indices);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(res) = resources.get::<InvertResources>() else {
            return;
        };
        if self.mesh.is_empty() {
            return;
        }
        let index_count = self.mesh.indices.len() as u32;
        pass.set_pipeline(&res.pipeline);
        pass.set_vertex_buffer(
            0,
            res.vertices
                .slice(..self.mesh.vertices.len() as u64 * VERTEX_BYTES),
        );
        pass.set_index_buffer(
            res.indices.slice(..u64::from(index_count) * 4),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..index_count, 0, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    use egui::{Color32, Rect, pos2};

    #[test]
    fn vertices_are_mapped_into_the_viewports_clip_space() {
        // egui sets the viewport to the callback's rectangle, so its corners
        // are the corners of clip space — not the window's.
        let viewport = Rect::from_min_max(pos2(100.0, 50.0), pos2(300.0, 250.0));
        let mut mesh = Mesh::default();
        for (pos, color) in [
            (pos2(100.0, 50.0), Color32::WHITE),
            (
                pos2(300.0, 250.0),
                Color32::from_rgba_premultiplied(0, 0, 0, 0),
            ),
            (
                pos2(200.0, 150.0),
                Color32::from_rgba_premultiplied(128, 128, 128, 128),
            ),
        ] {
            mesh.vertices.push(Vertex {
                pos,
                uv: WHITE_UV,
                color,
            });
        }
        mesh.indices = vec![0, 1, 2];
        let cb = InvertCallback { mesh, viewport };

        let bytes = cb.vertex_bytes();
        assert_eq!(bytes.len(), 3 * VERTEX_BYTES as usize);
        let floats: Vec<f32> = bytes
            .chunks(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(&floats[0..2], &[-1.0, 1.0], "top left is clip's top left");
        assert_eq!(
            &floats[6..8],
            &[1.0, -1.0],
            "bottom right is clip's bottom right"
        );
        assert_eq!(&floats[12..14], &[0.0, 0.0], "the centre is the origin");
        assert_eq!(
            &floats[2..6],
            &[1.0, 1.0, 1.0, 1.0],
            "white at full coverage"
        );
        assert!(
            (floats[14] - 128.0 / 255.0).abs() < 1e-6,
            "half coverage stays premultiplied"
        );
        assert_eq!(cb.index_bytes().len(), 12);
    }
}
