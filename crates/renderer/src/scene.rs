use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use tessera_core::{
    geometry, AppliedMaster, BelongsTo, BoundingBox, Document, Frame, FrameType, GuideAxis, Layer,
    MasterOverride, Page,
    PageGuides, PathData, RulerGuide, Size, SnapResult, Style, TextContent, TextThread, Transform,
    ZIndex,
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
        /// When threaded, the story's id and this frame's index in the chain.
        story: Option<[u32; 2]>,
        fill_color: [f32; 4],
        is_selected: bool,
    },
    /// Non-printing guides for one page: bleed, margins and columns.
    ///
    /// These are drawn under the page's content and never appear in output;
    /// they exist so a designer can see the live area while working.
    PageChrome {
        page_number: u32,
        /// Bleed box `[x0, y0, x1, y1]`, outside the trim.
        bleed: [f32; 4],
        /// Slug box, outside the bleed. Equal to `bleed` when no slug is set.
        slug: [f32; 4],
        /// Margin box, inside the trim.
        margins: [f32; 4],
        /// Horizontal extent `[x0, x1]` of each column within the margins.
        columns: Vec<[f32; 2]>,
        /// Vertical extent of the columns, matching the margin box.
        column_top: f32,
        column_bottom: f32,
    },
    /// A user-placed ruler guide, spanning the visible pasteboard.
    GuideLine {
        entity_id: u32,
        /// True for a vertical guide at a fixed x.
        is_vertical: bool,
        position: f32,
        /// Highlighted while a dragged object is snapped to it.
        is_active: bool,
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

/// A story: text flowing through a chain of threaded frames.
///
/// The story is carried once, alongside the geometry of every frame in its
/// chain, so the painter can flow it in one pass rather than re-deriving the
/// chain from each frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Story {
    /// Entity id of the frame that starts the chain, used as the story's id.
    pub id: u32,
    pub text: String,
    /// Width and height of each frame in the chain, in order.
    pub frames: Vec<[f32; 2]>,
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
    /// Threaded stories referenced by `TextBlock` elements.
    pub stories: Vec<Story>,
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
            stories: Vec::new(),
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
        Self::compile_with_snap(world, selected_id, revision, camera, None)
    }

    /// Compiles a scene, highlighting any guides the active gesture snapped to.
    pub fn compile_with_snap(
        world: &World,
        selected_id: Option<u32>,
        revision: u64,
        camera: &tessera_core::Camera,
        active_snap: Option<SnapResult>,
    ) -> RenderScene {
        let mut elements = Vec::new();

        // 1. Document settings drive every page's placement.
        let document = world
            .iter_entities()
            .find_map(|e| e.get::<Document>().cloned())
            .unwrap_or_default();
        let doc_width = document.width;
        let doc_height = document.height;
        let doc_bleed = document.bleed;
        let spread_layout = document.spread_layout();

        // 2. Pages sit on the pasteboard where the spread layout puts them.
        let mut page_entities: Vec<_> = world
            .iter_entities()
            .filter_map(|e| {
                e.get::<Page>().map(|p| {
                    (
                        p.page_number,
                        e.get::<PageGuides>().copied(),
                        e.get::<AppliedMaster>().map(|m| m.0),
                    )
                })
            })
            .collect();
        page_entities.sort_by_key(|(number, _, _)| *number);

        if page_entities.is_empty() {
            page_entities.push((1, None, None));
        }

        for (page_number, guides, _) in &page_entities {
            let placement = spread_layout.place(*page_number);
            let guides = guides.unwrap_or(document.guides);

            elements.push(RenderElement::PageSurface {
                page_number: placement.page_number,
                x: placement.x,
                y: placement.y,
                width: placement.width,
                height: placement.height,
                bleed: doc_bleed,
                shadow_blur: 15.0,
            });

            let (bx0, by0, bx1, by1) = placement.bleed_rect(doc_bleed);
            let (sx0, sy0, sx1, sy1) = placement.bleed_rect(doc_bleed + document.slug);
            let (mx0, my0, mx1, my1) = guides.content_rect(&placement);

            elements.push(RenderElement::PageChrome {
                page_number: placement.page_number,
                bleed: [bx0, by0, bx1, by1],
                slug: [sx0, sy0, sx1, sy1],
                margins: [mx0, my0, mx1, my1],
                columns: guides
                    .column_ranges(&placement)
                    .into_iter()
                    .map(|(x0, x1)| [x0, x1])
                    .collect(),
                column_top: my0,
                column_bottom: my1,
            });
        }

        // Ruler guides sit above the page chrome but below content.
        for entity in world.iter_entities() {
            let Some(guide) = entity.get::<RulerGuide>() else {
                continue;
            };
            let is_vertical = guide.axis == GuideAxis::Vertical;
            let is_active = active_snap
                .and_then(|snap| {
                    if is_vertical {
                        snap.snapped_vertical
                    } else {
                        snap.snapped_horizontal
                    }
                })
                .is_some_and(|line| (line.position - guide.position).abs() < 1e-3);

            elements.push(RenderElement::GuideLine {
                entity_id: entity.id().index(),
                is_vertical,
                position: guide.position,
                is_active,
            });
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

        // Master items are inherited onto each page that applies a master.
        // They are stored page-local, so each is offset to the page it appears
        // on, and any item overridden locally is skipped so it is not drawn twice.
        for (page_number, _, applied_master) in &page_entities {
            let Some(master) = applied_master else {
                continue;
            };
            let placement = spread_layout.place(*page_number);

            let page_entity = world
                .iter_entities()
                .find(|e| e.get::<Page>().is_some_and(|p| p.page_number == *page_number))
                .map(|e| e.id());

            let overridden: Vec<_> = world
                .iter_entities()
                .filter_map(|e| e.get::<MasterOverride>())
                .filter(|o| Some(o.page) == page_entity)
                .map(|o| o.source)
                .collect();

            for entity in world.iter_entities() {
                if entity.get::<BelongsTo>().map(|b| b.0) != Some(*master) {
                    continue;
                }
                if overridden.contains(&entity.id()) {
                    continue;
                }
                let (Some(frame), Some(transform), Some(size), Some(style)) = (
                    entity.get::<Frame>(),
                    entity.get::<Transform>(),
                    entity.get::<Size>(),
                    entity.get::<Style>(),
                ) else {
                    continue;
                };

                let placed = Transform {
                    position: tessera_core::Position {
                        x: transform.position.x + placement.x,
                        y: transform.position.y + placement.y,
                    },
                    ..*transform
                };
                let path_data = entity.get::<PathData>().cloned();
                let bounding_box = geometry::frame_bounds(
                    frame.frame_type,
                    &placed,
                    size,
                    path_data.as_ref(),
                );

                frame_list.push(FrameData {
                    // Inherited items are not directly selectable, so they
                    // carry the master item's id purely for identification.
                    id: entity.id().index(),
                    frame: frame.clone(),
                    transform: placed,
                    size: *size,
                    // Masters sit behind page content, as in any layout tool.
                    z_index: i32::MIN + entity.get::<ZIndex>().copied().unwrap_or_default().0,
                    bounding_box,
                    style: style.clone(),
                    text_content: entity.get::<TextContent>().cloned(),
                    path_data,
                });
            }
        }

        // Build the threaded stories before emitting elements, so each text
        // frame can be tagged with its place in a chain.
        let mut stories: Vec<Story> = Vec::new();
        let mut story_refs: HashMap<u32, [u32; 2]> = HashMap::new();

        for entity in world.iter_entities() {
            let Some(thread) = entity.get::<TextThread>() else {
                continue;
            };
            if !thread.is_head() {
                continue;
            }

            let head_id = entity.id().index();
            let text = entity
                .get::<TextContent>()
                .map(|t| t.text.clone())
                .unwrap_or_default();

            let mut frames = Vec::new();
            let mut cursor = Some(entity.id());
            let mut index = 0u32;
            while let Some(current) = cursor {
                let Ok(current_entity) = world.get_entity(current) else {
                    break;
                };
                let size = current_entity.get::<Size>().copied().unwrap_or_default();
                let transform = current_entity.get::<Transform>().copied().unwrap_or_default();
                frames.push([
                    size.width * transform.scale_x,
                    size.height * transform.scale_y,
                ]);
                story_refs.insert(current.index(), [head_id, index]);

                index += 1;
                if index > 10_000 {
                    break;
                }
                cursor = current_entity.get::<TextThread>().and_then(|t| t.next);
            }

            stories.push(Story {
                id: head_id,
                text,
                frames,
            });
        }

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
                        story: story_refs.get(&f.id).copied(),
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
            stories,
        }
    }
}
