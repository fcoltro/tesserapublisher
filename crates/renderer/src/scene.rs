use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use tessera_core::{
    geometry, BelongsTo, BoundingBox, Document, Frame, FrameType, Layer, Page, PathData, Size,
    Style, TextContent, Transform, ZIndex,
};

/// Primitive renderable elements compiled from the ECS state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RenderElement {
    PageSurface {
        page_number: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        bleed: f32,
        shadow_blur: f32,
    },
    RectShape {
        id: u32,
        name: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rotation: f32,
        fill_color: [f32; 4],
        stroke_color: Option<[f32; 4]>,
        stroke_width: f32,
        corner_radius: f32,
        is_selected: bool,
    },
    EllipseShape {
        id: u32,
        name: String,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        rotation: f32,
        fill_color: [f32; 4],
        stroke_color: Option<[f32; 4]>,
        stroke_width: f32,
        is_selected: bool,
    },
    TextBlock {
        id: u32,
        name: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text: String,
        font_size: f32,
        line_height: f32,
        align: tessera_core::TextAlignment,
        font_family: Option<String>,
        font_weight: f32,
        fill_color: [f32; 4],
        is_selected: bool,
    },
    /// An arbitrary bezier outline placed by an affine.
    ///
    /// Lines and paths share this variant: both are outlines in the frame's
    /// local space, so the only difference is whether they enclose an area.
    PathShape {
        id: u32,
        name: String,
        /// The outline in the frame's local space, as an SVG path string.
        svg: String,
        /// Affine mapping local space to document space, as kurbo coefficients.
        transform: [f32; 6],
        fill_color: [f32; 4],
        stroke_color: Option<[f32; 4]>,
        stroke_width: f32,
        /// Open outlines are stroked only; closed ones are filled first.
        is_closed: bool,
        is_selected: bool,
    },
    SelectionOverlay {
        entity_id: u32,
        min_x: f32,
        min_y: f32,
        max_x: f32,
        max_y: f32,
        corner_nodes: Vec<[f32; 2]>,
    },
}

/// Compiled render scene ready for GPU/canvas execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderScene {
    pub revision: u64,
    pub pasteboard_color: [f32; 4],
    pub page_width: f32,
    pub page_height: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    pub elements: Vec<RenderElement>,
    pub total_frames: usize,
}

impl Default for RenderScene {
    fn default() -> Self {
        Self {
            revision: 0,
            pasteboard_color: [0.02, 0.031, 0.067, 1.0], // Dark pasteboard
            page_width: 800.0,
            page_height: 600.0,
            pan_x: 60.0,
            pan_y: 60.0,
            zoom: 1.0,
            elements: Vec::new(),
            total_frames: 0,
        }
    }
}

/// Compiles ECS entities into a structured RenderScene
pub struct SceneCompiler;

impl SceneCompiler {
    pub fn compile(
        world: &World,
        selected_id: Option<u32>,
        revision: u64,
        camera: &tessera_core::Camera,
    ) -> RenderScene {
        let mut elements = Vec::new();

        // 1. Get Document & Page configurations
        let mut doc_width = 800.0;
        let mut doc_height = 600.0;
        let mut doc_bleed = 3.0;

        for e in world.iter_entities() {
            if let Some(doc) = e.get::<Document>() {
                doc_width = doc.width;
                doc_height = doc.height;
                doc_bleed = doc.bleed;
                break;
            }
        }

        // 2. Render Page Surface(s) (white paper canvas on top of dark pasteboard)
        let page_entities: Vec<_> = world
            .iter_entities()
            .filter_map(|e| e.get::<Page>().map(|p| (e.id(), p.clone())))
            .collect();

        if page_entities.is_empty() {
            // Default fallback page
            elements.push(RenderElement::PageSurface {
                page_number: 1,
                x: 40.0,
                y: 40.0,
                width: doc_width,
                height: doc_height,
                bleed: doc_bleed,
                shadow_blur: 15.0,
            });
        } else {
            for (_, page) in &page_entities {
                elements.push(RenderElement::PageSurface {
                    page_number: page.page_number,
                    x: 40.0,
                    y: 40.0,
                    width: page.width,
                    height: page.height,
                    bleed: doc_bleed,
                    shadow_blur: 15.0,
                });
            }
        }

        // 3. Discover active visible layers
        let visible_layer_ids: Vec<_> = world
            .iter_entities()
            .filter_map(|e| {
                let layer = e.get::<Layer>()?;
                if layer.is_visible {
                    Some(e.id())
                } else {
                    None
                }
            })
            .collect();

        // 4. Iterate frames belonging to visible layers, sorted by ZIndex
        struct FrameData {
            id: u32,
            frame: Frame,
            transform: Transform,
            size: Size,
            z_index: i32,
            bounding_box: BoundingBox,
            style: Style,
            text_content: Option<TextContent>,
            path_data: Option<PathData>,
        }

        let mut frame_list: Vec<FrameData> = world
            .iter_entities()
            .filter_map(|e| {
                let frame = e.get::<Frame>()?;
                let transform = e.get::<Transform>()?;
                let size = e.get::<Size>()?;
                let z_index = e.get::<ZIndex>().copied().unwrap_or(ZIndex(0));
                let bounding_box = e.get::<BoundingBox>()?;
                let style = e.get::<Style>()?;
                let text_content = e.get::<TextContent>().cloned();
                let path_data = e.get::<PathData>().cloned();

                // Check layer visibility
                if let Some(parent_link) = e.get::<BelongsTo>() {
                    if !visible_layer_ids.contains(&parent_link.0) {
                        return None;
                    }
                }

                Some(FrameData {
                    id: e.id().index(),
                    frame: frame.clone(),
                    transform: *transform,
                    size: *size,
                    z_index: z_index.0,
                    bounding_box: *bounding_box,
                    style: style.clone(),
                    text_content,
                    path_data,
                })
            })
            .collect();

        frame_list.sort_by(|a, b| a.z_index.cmp(&b.z_index));
        let total_frames = frame_list.len();

        // 5. Compile ECS components into RenderElements
        for f in &frame_list {
            let is_sel = selected_id == Some(f.id);

            match f.frame.frame_type {
                FrameType::Rectangle => {
                    elements.push(RenderElement::RectShape {
                        id: f.id,
                        name: f.frame.name.clone(),
                        x: f.transform.position.x,
                        y: f.transform.position.y,
                        width: f.size.width * f.transform.scale_x,
                        height: f.size.height * f.transform.scale_y,
                        rotation: f.transform.rotation,
                        fill_color: f.style.fill_color,
                        stroke_color: f.style.stroke_color,
                        stroke_width: f.style.stroke_width,
                        corner_radius: 6.0,
                        is_selected: is_sel,
                    });
                }
                FrameType::Ellipse => {
                    let rx = (f.size.width * f.transform.scale_x) / 2.0;
                    let ry = (f.size.height * f.transform.scale_y) / 2.0;
                    elements.push(RenderElement::EllipseShape {
                        id: f.id,
                        name: f.frame.name.clone(),
                        cx: f.transform.position.x + rx,
                        cy: f.transform.position.y + ry,
                        rx,
                        ry,
                        rotation: f.transform.rotation,
                        fill_color: f.style.fill_color,
                        stroke_color: f.style.stroke_color,
                        stroke_width: f.style.stroke_width,
                        is_selected: is_sel,
                    });
                }
                FrameType::Text => {
                    let text = f
                        .text_content
                        .as_ref()
                        .map(|t| t.text.clone())
                        .unwrap_or_else(|| f.frame.name.clone());
                    let font_size = f
                        .text_content
                        .as_ref()
                        .map(|t| t.font_size)
                        .unwrap_or(16.0);
                    let line_height = f
                        .text_content
                        .as_ref()
                        .map(|t| t.line_height)
                        .unwrap_or(1.4);
                    let align = f
                        .text_content
                        .as_ref()
                        .map(|t| t.align)
                        .unwrap_or_default();
                    let font_family =
                        f.text_content.as_ref().and_then(|t| t.font_family.clone());
                    let font_weight = f
                        .text_content
                        .as_ref()
                        .map(|t| t.font_weight)
                        .unwrap_or(400.0);

                    elements.push(RenderElement::TextBlock {
                        id: f.id,
                        name: f.frame.name.clone(),
                        x: f.transform.position.x,
                        y: f.transform.position.y,
                        width: f.size.width * f.transform.scale_x,
                        height: f.size.height * f.transform.scale_y,
                        text,
                        font_size,
                        line_height,
                        align,
                        font_family,
                        font_weight,
                        fill_color: f.style.fill_color,
                        is_selected: is_sel,
                    });
                }
                FrameType::Line | FrameType::Path => {
                    // Both are outlines in local space; the affine carries the
                    // position, scale and rotation across to the painter.
                    let outline =
                        geometry::local_outline(f.frame.frame_type, &f.size, f.path_data.as_ref());
                    let coeffs = geometry::frame_affine(&f.transform, &f.size).as_coeffs();

                    elements.push(RenderElement::PathShape {
                        id: f.id,
                        name: f.frame.name.clone(),
                        svg: outline.to_svg(),
                        transform: [
                            coeffs[0] as f32,
                            coeffs[1] as f32,
                            coeffs[2] as f32,
                            coeffs[3] as f32,
                            coeffs[4] as f32,
                            coeffs[5] as f32,
                        ],
                        fill_color: f.style.fill_color,
                        stroke_color: f.style.stroke_color,
                        stroke_width: f.style.stroke_width.max(1.0),
                        is_closed: f.frame.frame_type == FrameType::Path,
                        is_selected: is_sel,
                    });
                }
                FrameType::Image => {
                    elements.push(RenderElement::RectShape {
                        id: f.id,
                        name: f.frame.name.clone(),
                        x: f.transform.position.x,
                        y: f.transform.position.y,
                        width: f.size.width * f.transform.scale_x,
                        height: f.size.height * f.transform.scale_y,
                        rotation: f.transform.rotation,
                        fill_color: [0.15, 0.2, 0.3, 1.0],
                        stroke_color: Some([0.4, 0.6, 0.9, 1.0]),
                        stroke_width: 1.0,
                        corner_radius: 4.0,
                        is_selected: is_sel,
                    });
                }
            }

            // If selected, append SelectionOverlay with corner anchor handles
            if is_sel {
                let bb = f.bounding_box;
                let corner_nodes = vec![
                    [bb.min_x, bb.min_y], // Top-Left
                    [bb.max_x, bb.min_y], // Top-Right
                    [bb.max_x, bb.max_y], // Bottom-Right
                    [bb.min_x, bb.max_y], // Bottom-Left
                ];

                elements.push(RenderElement::SelectionOverlay {
                    entity_id: f.id,
                    min_x: bb.min_x,
                    min_y: bb.min_y,
                    max_x: bb.max_x,
                    max_y: bb.max_y,
                    corner_nodes,
                });
            }
        }

        RenderScene {
            revision,
            pasteboard_color: [0.02, 0.031, 0.067, 1.0],
            page_width: doc_width,
            page_height: doc_height,
            pan_x: camera.pan_x,
            pan_y: camera.pan_y,
            zoom: camera.zoom,
            elements,
            total_frames,
        }
    }
}
