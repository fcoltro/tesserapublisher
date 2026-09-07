//! The tool strip, the inspector and the status bar.

use egui::{Sense, Ui, Vec2};
use tessera_color::Color;
use tessera_document::ids::StoryId;
use tessera_document::nodes::{Orientation, PagePreset};
use tessera_document::paint::Paint;
use tessera_geometry::{Anchor, Unit};
use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyle, CharacterStyleId, ParagraphFormat,
    ParagraphStyle, ParagraphStyleId,
};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;
use crate::tools::Tool;

// --- tool strip --------------------------------------------------------

pub fn tool_strip(ui: &mut Ui, state: &mut TesseraApp) {
    ui.vertical(|ui| {
        ui.add_space(Theme::SPACING_SM);
        for tool in Tool::ALL {
            if tool_button(ui, tool, state.active_tool == tool).clicked() {
                state.active_tool = tool;
            }
        }
    });
}

/// Icons come from Lucide, painted through `egui::Painter` from path data
/// rather than loaded as assets — so they stay crisp at any DPI and re-tint
/// with the theme. See [`crate::icons`].
fn tool_button(ui: &mut Ui, tool: Tool, active: bool) -> egui::Response {
    let size = Vec2::splat(Theme::TOOL_SIZE);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let bg = if active {
        Theme::ACCENT
    } else if response.hovered() {
        Theme::BORDER
    } else {
        Theme::PANEL_BG_ALT
    };
    let fg = if active {
        Theme::PANEL_BG
    } else {
        Theme::TEXT_PRIMARY
    };

    ui.painter().rect_filled(rect, Theme::RADIUS, bg);
    // Inset so the 24-unit icon grid does not touch the button edge.
    let inset = Theme::TOOL_SIZE * 0.22;
    crate::icons::paint(ui.painter(), rect.shrink(inset), tool.icon(), fg);

    response.on_hover_text(format!("{} ({:?})", tool.label(), tool.shortcut()))
}

// --- inspector ---------------------------------------------------------

/// The inspector's sections, in the order they are drawn.
///
/// The order is the whole design. Hiding a section moves everything below it,
/// so the sections that apply to every frame come first and the ones that can
/// be absent come last — hiding one then never moves a control the user
/// reaches for often. A control that relocates by context is one the hand
/// cannot find without the eye.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Transform,
    Fill,
    Stroke,
    Effects,
    Text,
    Frame,
    Graphic,
    Wrap,
}

impl Section {
    /// Display order. Universal sections first; see the type's note.
    pub const ALL: [Section; 8] = [
        Section::Transform,
        Section::Fill,
        Section::Stroke,
        // Every object composites, so this belongs with the always-present
        // sections — and it reads under Fill and Stroke because it is about
        // what happens to them once they are painted.
        Section::Effects,
        // Wrap applies to every object, so it belongs with the sections that
        // are always there. The ones that can be absent come last, or hiding
        // one would move a section above it.
        Section::Wrap,
        Section::Graphic,
        Section::Text,
        Section::Frame,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Transform => "Transform",
            Section::Fill => "Fill",
            Section::Stroke => "Stroke",
            Section::Text => "Text",
            Section::Frame => "Frame",
            Section::Wrap => "Text wrap",
            Section::Graphic => "Artwork",
            Section::Effects => "Effects",
        }
    }

    /// The glyph that names the section, so a column of them can be found
    /// by shape rather than read.
    pub fn icon(self) -> crate::icons::Icon {
        use crate::icons::Icon;
        match self {
            Section::Transform => Icon::Scale,
            Section::Fill => Icon::Palette,
            Section::Stroke => Icon::Line,
            Section::Text => Icon::CaseSensitive,
            Section::Frame => Icon::TextFrame,
            Section::Wrap => Icon::AlignJustify,
            Section::Graphic => Icon::Rectangle,
            Section::Effects => Icon::Blend,
        }
    }

    /// Whether this section says anything about `frame`.
    pub fn applies_to(self, frame: &tessera_document::nodes::Frame) -> bool {
        use tessera_document::nodes::FrameKind;
        match self {
            // Every frame has a place, a fill and a stroke — even when the
            // stroke is None, which is a value the section can set.
            Section::Transform | Section::Fill | Section::Stroke | Section::Effects => true,
            Section::Text => matches!(frame.kind, FrameKind::Text { .. }),
            Section::Frame => matches!(frame.kind, FrameKind::Group(_)),
            // Every kind of object. A picture is the thing most often
            // wrapped, and it is the obstacle that carries the setting.
            Section::Wrap => true,
            Section::Graphic => matches!(frame.kind, FrameKind::Graphic { .. }),
        }
    }
}

pub fn inspector(ui: &mut Ui, state: &mut TesseraApp) {
    // No heading: the rail draws one, and printing a second underneath it was
    // the word "Properties" twice in a column 292 points wide.
    if state.active().selection.is_empty() {
        document_setup(ui, state);
        return;
    }

    // Geometry fields edit one frame. With several selected there is no single
    // value to show, and silently editing only the first would be worse than
    // saying so.
    let Some(id) = state.active().selection.single() else {
        ui.colored_label(
            Theme::TEXT_MUTED,
            format!("{} objects selected", state.active().selection.len()),
        );
        return;
    };
    let Some(frame) = state.active().document().frame(id).cloned() else {
        ui.colored_label(Theme::TEXT_MUTED, "No selection");
        return;
    };

    fill_stroke_proxy(ui, state, id, &frame);

    for section in Section::ALL {
        if !section.applies_to(&frame) {
            continue;
        }
        if !section_heading(ui, state, section.icon(), section.title()) {
            continue;
        }
        // Indented under its heading, which is what says the fields belong to
        // it rather than merely follow it.
        ui.scope(|ui| {
            ui.add_space(Theme::SPACE_1);
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: Theme::SPACE_3 as i8,
                    right: 0,
                    top: 0,
                    bottom: Theme::SPACE_2 as i8,
                })
                .show(ui, |ui| match section {
                    Section::Transform => transform_section(ui, state, id, &frame),
                    Section::Fill => fill_section(ui, state, id, &frame),
                    Section::Stroke => stroke_section(ui, state, id, &frame),
                    Section::Text => text_section(ui, state, id, &frame),
                    Section::Frame => frame_section(ui, &frame),
                    Section::Wrap => wrap_controls(ui, state, id, &frame),
                    Section::Graphic => graphic_section(ui, state, id, &frame),
                    Section::Effects => effects_section(ui, state, id, &frame),
                });
        });
    }
}

/// The fill and stroke proxy: two overlapping swatches with their three keys.
///
/// The arrangement every drawing tool since MacDraw has used, and the one
/// place InDesign's design is worth copying exactly — a shape so familiar that
/// it needs no label.
fn fill_stroke_proxy(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    const SWATCH: f32 = 26.0;
    const OFFSET: f32 = 10.0;

    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(SWATCH + OFFSET + 60.0, SWATCH + OFFSET),
        Sense::hover(),
    );
    let painter = ui.painter();

    let to_colour = |c: &Color| {
        let [r, g, b, a] = c.to_rgb_f32();
        egui::Color32::from_rgba_unmultiplied(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        )
    };

    // Stroke behind, fill in front — the stroke's swatch is a ring, so the
    // fill sitting over it still shows both.
    let stroke_rect =
        egui::Rect::from_min_size(rect.min + Vec2::splat(OFFSET), Vec2::splat(SWATCH));
    let fill_rect = egui::Rect::from_min_size(rect.min, Vec2::splat(SWATCH));

    let stroke_colour = frame
        .stroke
        .as_ref()
        .map_or(Theme::PANEL_BG, |s| to_colour(&s.color));
    painter.rect_filled(stroke_rect, 2.0, stroke_colour);
    painter.rect_filled(stroke_rect.shrink(6.0), 1.0, Theme::PANEL_BG);
    painter.rect_stroke(
        stroke_rect,
        2.0,
        egui::Stroke::new(1.0, Theme::BORDER),
        egui::StrokeKind::Inside,
    );

    // One colour on a 26-point button. The section below draws the real
    // ramp, where there is room for it.
    painter.rect_filled(fill_rect, 2.0, to_colour(&frame.fill.representative()));
    painter.rect_stroke(
        fill_rect,
        2.0,
        egui::Stroke::new(1.0, Theme::BORDER),
        egui::StrokeKind::Inside,
    );

    ui.horizontal(|ui| {
        if glyph_button(ui, crate::icons::Icon::Swap, "Swap fill and stroke (X)").clicked() {
            apply(state, Command::SwapFillAndStroke(id));
        }
        if glyph_button(ui, crate::icons::Icon::Rectangle, "Defaults (D)").clicked() {
            apply(state, Command::DefaultFillAndStroke(id));
        }
        if glyph_button(ui, crate::icons::Icon::NoFill, "No fill (/)").clicked() {
            apply(state, Command::ClearFill(id));
        }
    });
    ui.add_space(Theme::SPACING_SM);
}

/// A small icon button, for the places a word would be worse than a picture.
fn glyph_button(ui: &mut Ui, icon: crate::icons::Icon, tip: &str) -> egui::Response {
    const SIZE: f32 = 20.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, Theme::RADIUS, Theme::HOVER_BG);
    }
    crate::icons::paint(ui.painter(), rect.shrink(3.0), icon, Theme::TEXT_PRIMARY);
    response.on_hover_text(tip)
}

/// The nine-point reference proxy.
///
/// Bigger than InDesign's, which is a grid of targets a few pixels across —
/// small enough that hitting the wrong one is easy and noticing that you did
/// is not. Returns whether the anchor changed.
pub fn reference_proxy(ui: &mut Ui, anchor: &mut Anchor) -> bool {
    const CELL: f32 = 15.0;
    let side = CELL * 3.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let mut changed = false;

    for (i, candidate) in Anchor::ALL.iter().enumerate() {
        let (col, row) = ((i % 3) as f32, (i / 3) as f32);
        let cell = egui::Rect::from_min_size(
            rect.min + Vec2::new(col * CELL, row * CELL),
            Vec2::splat(CELL),
        );
        let response = ui.interact(cell, ui.id().with(("anchor", i)), Sense::click());
        if response.clicked() {
            *anchor = *candidate;
            changed = true;
        }

        let selected = *candidate == *anchor;
        let colour = if selected {
            Theme::ACCENT
        } else if response.hovered() {
            Theme::TEXT_PRIMARY
        } else {
            Theme::TEXT_MUTED
        };
        ui.painter()
            .circle_filled(cell.center(), if selected { 4.0 } else { 2.0 }, colour);
    }

    ui.painter().rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, Theme::BORDER),
        egui::StrokeKind::Inside,
    );
    changed
}

/// Position, size and angle, laid out as one row for the control bar.
///
/// **The one place geometry lives.** It was eight rows in a 240-point column
/// that could not fit its own labels; it is a row now, in the same place
/// whatever is selected. Scale and shear stay in Properties — they are asked
/// for rarely and read badly in a row.
pub fn transform_row(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    let mut anchor = state.anchor;
    if reference_proxy(ui, &mut anchor) {
        state.anchor = anchor;
    }
    crate::view::control::separator(ui);

    let unit = state.prefs.unit;
    // X and Y are the **reference point's** position, not the top-left
    // corner's. That is what the nine-point proxy is for, and what it means in
    // InDesign: choose the centre and the fields read the centre.
    //
    // Resolved where the frame really is, through its own transform, for the
    // same reason `Command::TransformAbout` does: `bounds` says where a frame
    // is in its own space and not where it sits on the page.
    let origin = frame.transform.apply(state.anchor.in_rect(frame.bounds));
    let (mut x, mut y) = (origin.x, origin.y);
    let mut bounds = frame.bounds;
    let (was_w, was_h) = (bounds.width, bounds.height);

    let mut moved = measure_inline(ui, "X", &mut x, unit);
    moved |= measure_inline(ui, "Y", &mut y, unit);
    crate::view::control::separator(ui);

    let w_changed = measure_inline(ui, "W", &mut bounds.width, unit);
    let h_changed = measure_inline(ui, "H", &mut bounds.height, unit);

    let mut chain = state.constrain_proportions;
    // A padlock rather than InDesign's chain link, because there is no chain
    // in the icon set and a locked ratio is what the control means.
    if icon_button(
        ui,
        if chain {
            crate::icons::Icon::Lock
        } else {
            crate::icons::Icon::Unlock
        },
        "Constrain proportions",
        chain,
    ) {
        chain = !chain;
        state.constrain_proportions = chain;
    }
    crate::view::control::separator(ui);

    let d = frame.transform.decompose();
    let mut rotation = d.rotation_degrees;
    let turned = angle_inline(ui, &mut rotation);

    if moved {
        // Translated in document space, so a turned frame goes where the
        // number says rather than off along its own axes.
        apply(
            state,
            Command::TranslateSelection {
                dx: x - origin.x,
                dy: y - origin.y,
            },
        );
    }
    if w_changed || h_changed {
        if chain {
            let (w, h) = constrained((was_w, was_h), (bounds.width, bounds.height), w_changed);
            bounds.width = w;
            bounds.height = h;
        }
        apply(state, Command::SetBounds { id, bounds });
    }
    if turned {
        apply(
            state,
            Command::TransformAbout {
                id,
                anchor: state.anchor,
                scale: (1.0, 1.0),
                rotate: rotation - d.rotation_degrees,
                shear: 0.0,
            },
        );
    }
}

/// Scale and shear: what is left of the old transform section.
fn transform_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    // Read from one decomposition and written back as deltas about the
    // reference point, so the fields, the handles and the proxy mean one
    // thing rather than three.
    let d = frame.transform.decompose();
    let anchor = state.anchor;

    let (mut sx, mut sy) = (d.scale_x * 100.0, d.scale_y * 100.0);
    let (a, b) = pair(
        ui,
        ("Scale", |ui: &mut Ui| percent_bare(ui, &mut sx)),
        ("by", |ui: &mut Ui| percent_bare(ui, &mut sy)),
    );
    let scaled = a || b;
    if scaled && d.scale_x != 0.0 && d.scale_y != 0.0 {
        apply(
            state,
            Command::TransformAbout {
                id,
                anchor,
                // A ratio, because the command takes a delta: the anchor is
                // what the operation is about, and an absolute would have to
                // rebuild the translation itself.
                scale: (sx / 100.0 / d.scale_x, sy / 100.0 / d.scale_y),
                rotate: 0.0,
                shear: 0.0,
            },
        );
        return;
    }

    let mut shear = d.shear_degrees;
    if angle(ui, "Shear", &mut shear) {
        apply(
            state,
            Command::TransformAbout {
                id,
                anchor,
                scale: (1.0, 1.0),
                rotate: 0.0,
                shear: shear - d.shear_degrees,
            },
        );
    }
}

/// A measurement in a row: its label, then a narrow field.
///
/// The control bar's counterpart to [`measure`], which lays a label and a
/// field out as a grid row. Same parser, same formatter, same unit rule.
fn measure_inline(ui: &mut Ui, label: &str, points: &mut f64, unit: Unit) -> bool {
    crate::view::control::label(ui, label);
    let mut shown = unit.from_points(*points);
    let changed = ui
        .add(
            egui::DragValue::new(&mut shown)
                .speed(0.25)
                .custom_formatter(move |v, _| format!("{v:.2} {}", unit.suffix()))
                .custom_parser(move |text| {
                    Unit::parse_to_points(text, unit).map(|p| unit.from_points(p))
                }),
        )
        .changed();
    if changed {
        *points = unit.to_points(shown);
    }
    changed
}

/// An angle in a row.
fn angle_inline(ui: &mut Ui, degrees: &mut f64) -> bool {
    crate::view::control::label(ui, "\u{2220}");
    ui.add(egui::DragValue::new(degrees).speed(0.5).suffix("\u{00B0}"))
        .changed()
}

/// Family, size and leading for the text being edited.
///
/// The three a person changes while typing. Everything else about type —
/// tracking, case, baseline shift, the style tables — stays in Properties and
/// in the Styles section, where there is room to read it.
pub fn type_row(ui: &mut Ui, state: &mut TesseraApp) {
    let Some((id, _)) = &state.active().editing else {
        return;
    };
    let id = *id;
    let Some(frame) = state.active().document().frame(id).cloned() else {
        return;
    };
    let tessera_document::nodes::FrameKind::Text { story, .. } = frame.kind else {
        return;
    };

    let target = format_target(state, id, story);
    let common = state
        .active()
        .document()
        .story(story)
        .map(|s| s.common_format(target.clone(), state.active().document()))
        .unwrap_or_default();

    if let Some(family) = family_picker(ui, state, common.family.as_deref(), &[]) {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                family: Some(family),
                ..CharacterFormat::default()
            },
        );
        return;
    }

    crate::view::control::separator(ui);

    let mut size = common.size.unwrap_or(12.0) as f64;
    crate::view::control::label(ui, "Size");
    if ui
        .add(egui::DragValue::new(&mut size).speed(0.25).suffix(" pt"))
        .changed()
    {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                size: Some(size as f32),
                ..CharacterFormat::default()
            },
        );
        return;
    }

    let mut leading = common.line_height.unwrap_or(1.2) as f64;
    crate::view::control::label(ui, "Leading");
    if ui
        .add(
            egui::DragValue::new(&mut leading)
                .speed(0.02)
                .range(0.5..=4.0),
        )
        .changed()
    {
        set_character(
            state,
            story,
            target,
            CharacterFormat {
                line_height: Some(leading as f32),
                ..CharacterFormat::default()
            },
        );
    }
}

/// The page's own controls, for when nothing is selected.
///
/// The essentials only. Everything else about the page stays in Properties:
/// the bar is a row, and a row that scrolls is a column that lies about it.
pub fn page_row(ui: &mut Ui, state: &mut TesseraApp) {
    let setup = state.active().document().setup;
    let bounds = state.active().document().first_page_bounds();
    let unit = state.prefs.unit;

    let mut size = (bounds.width, bounds.height);
    let mut changed = measure_inline(ui, "W", &mut size.0, unit);
    changed |= measure_inline(ui, "H", &mut size.1, unit);
    if changed {
        apply(
            state,
            Command::SetPageSize {
                width: size.0,
                height: size.1,
            },
        );
    }

    crate::view::control::separator(ui);

    let mut facing = setup.facing_pages;
    if ui.checkbox(&mut facing, "Facing pages").changed() {
        apply(
            state,
            Command::SetDocumentSetup(tessera_document::nodes::DocumentSetup {
                facing_pages: facing,
                ..setup
            }),
        );
    }

    crate::view::control::separator(ui);
    crate::view::control::label(ui, "Measurements in");
    ui.label(unit_name(unit));
}

/// Carry a size change across to the other side, keeping the ratio.
///
/// `w_changed` says which field the user touched; that one drives. A zero
/// side has no ratio to carry, so it is left alone rather than collapsing its
/// partner to nothing.
fn constrained(was: (f64, f64), now: (f64, f64), w_changed: bool) -> (f64, f64) {
    let (was_w, was_h) = was;
    let (w, h) = now;
    if w_changed {
        if was_w == 0.0 {
            return (w, h);
        }
        (w, was_h * (w / was_w))
    } else {
        if was_h == 0.0 {
            return (w, h);
        }
        (was_w * (h / was_h), h)
    }
}

/// A percentage control with no label of its own.
fn percent_bare(ui: &mut Ui, value: &mut f64) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .speed(0.5)
            .fixed_decimals(1)
            .suffix("%"),
    )
    .changed()
}

/// A percentage field.
#[allow(dead_code)]
fn percent(ui: &mut Ui, label: &str, value: &mut f64) -> bool {
    field(ui, label, |ui| {
        ui.add(
            egui::DragValue::new(value)
                .speed(0.5)
                .fixed_decimals(1)
                .suffix("%"),
        )
        .changed()
    })
}

/// An angle field, in degrees.
fn angle(ui: &mut Ui, label: &str, value: &mut f64) -> bool {
    field(ui, label, |ui| {
        ui.add(egui::DragValue::new(value).speed(0.5).suffix("°"))
            .changed()
    })
}

fn fill_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::paint::Ramp;

    // Every frame has a fill, a text frame included — its background. The
    // previous arrangement showed this only for non-text frames, so a text
    // frame's own fill was unreachable.
    //
    // What *kind* of fill comes first, because it decides which controls below
    // it mean anything. The two gradients are named separately rather than
    // offering "gradient" and then a second control for its shape: there are
    // only two, and a person picking a fill is choosing between three things,
    // not between two and then a sub-question.
    let chosen = match &frame.fill {
        Paint::Solid(_) => 0,
        Paint::Gradient(g) => match g.ramp {
            Ramp::Linear { .. } => 1,
            Ramp::Radial => 2,
        },
    };
    let mut want = chosen;
    segmented(
        ui,
        "Kind",
        &mut want,
        &[("Solid", 0), ("Linear", 1), ("Radial", 2)],
    );
    if want != chosen {
        // Switching keeps whatever the other kind can carry: a gradient turned
        // solid takes a colour from its ramp, and a solid turned gradient ramps
        // from the colour it already was rather than from an unrelated black.
        let paint = match want {
            0 => Paint::Solid(frame.fill.representative()),
            other => {
                let ramp = if other == 1 {
                    Ramp::Linear { angle: 90.0 }
                } else {
                    Ramp::Radial
                };
                match frame.fill.gradient() {
                    // Only the shape changed, so the stops are kept.
                    Some(g) => Paint::Gradient(tessera_document::paint::Gradient::new(
                        ramp,
                        g.stops().to_vec(),
                    )),
                    None => Paint::Gradient(tessera_document::paint::Gradient::new(
                        ramp,
                        vec![
                            tessera_document::paint::Stop {
                                at: 0.0,
                                colour: frame.fill.representative(),
                            },
                            tessera_document::paint::Stop {
                                at: 1.0,
                                colour: Color::WHITE,
                            },
                        ],
                    )),
                }
            }
        };
        apply(state, Command::SetFill { id, paint });
        return;
    }

    match &frame.fill {
        Paint::Solid(colour) => {
            let [r, g, b, a] = colour.to_rgb_f32();
            let mut rgba = [r, g, b, a];
            if fill_picker(ui, &mut rgba) {
                apply(
                    state,
                    Command::SetFill {
                        id,
                        paint: Paint::Solid(Color::Rgb {
                            r: rgba[0],
                            g: rgba[1],
                            b: rgba[2],
                            a: rgba[3],
                        }),
                    },
                );
            }
        }
        Paint::Gradient(gradient) => gradient_controls(ui, state, id, gradient),
    }
}

/// The ramp itself: which way it runs, and the colours along it.
fn gradient_controls(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    gradient: &tessera_document::paint::Gradient,
) {
    use tessera_document::paint::{Gradient, Ramp, Stop};

    let mut ramp = gradient.ramp;
    let mut stops = gradient.stops().to_vec();
    let mut changed = false;

    // The angle, for a ramp that has one. It runs either way round rather than
    // stopping at zero, because 350 and -10 are the same direction and a
    // control that stopped would refuse the shorter way there.
    if let Ramp::Linear { angle } = &mut ramp {
        changed |= field(ui, "Angle", |ui| {
            ui.add(
                egui::DragValue::new(angle)
                    .speed(1.0)
                    .range(-360.0..=360.0)
                    .suffix("\u{b0}"),
            )
            .changed()
        });
    }

    // The ramp, drawn. A list of colours and numbers is not a gradient a person
    // can judge, and this is the control they actually read.
    ramp_preview(ui, &stops);

    group_label(ui, "Stops");
    let mut remove: Option<usize> = None;
    for (index, stop) in stops.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let [r, g, b, a] = stop.colour.to_rgb_f32();
            let mut rgba = [r, g, b, a];
            if fill_picker(ui, &mut rgba) {
                stop.colour = Color::Rgb {
                    r: rgba[0],
                    g: rgba[1],
                    b: rgba[2],
                    a: rgba[3],
                };
                changed = true;
            }
            // The position as a percentage along the ramp, which is how a
            // person reads it. The model holds the fraction.
            let mut percent = stop.at * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut percent)
                        .speed(0.5)
                        .range(0.0..=100.0)
                        .suffix("%"),
                )
                .changed()
            {
                stop.at = percent / 100.0;
                changed = true;
            }
            // A ramp needs two ends, so the first two carry no remove button.
            // Offering one and then refusing it would be worse than not
            // offering it.
            if index >= 2 && ui.small_button("\u{2715}").clicked() {
                remove = Some(index);
            }
        });
    }

    if let Some(at) = remove {
        stops.remove(at);
        changed = true;
    }

    if ui.button("Add a stop").clicked() {
        // Halfway between the last two, taking the colour already there, so
        // adding a stop changes nothing until it is moved or recoloured.
        let (a, b) = (stops[stops.len() - 2].at, stops[stops.len() - 1].at);
        let colour = stops[stops.len() - 1].colour.clone();
        stops.push(Stop {
            at: (a + b) / 2.0,
            colour,
        });
        changed = true;
    }

    if changed {
        let paint = Paint::Gradient(Gradient::new(ramp, stops));
        apply(state, Command::SetFill { id, paint });
    }
}

/// The ramp, painted as it will appear.
///
/// A list of colours and numbers is not something a person can judge a gradient
/// from. Drawn as a strip of bands rather than through a real gradient shader,
/// because egui paints solid rectangles and fifty bands across a panel is
/// already smooth to the eye.
fn ramp_preview(ui: &mut Ui, stops: &[tessera_document::paint::Stop]) {
    const BANDS: usize = 48;
    const HEIGHT: f32 = 18.0;

    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(60.0), HEIGHT),
        Sense::hover(),
    );
    let painter = ui.painter();
    let width = rect.width() / BANDS as f32;

    for band in 0..BANDS {
        let at = (band as f32 + 0.5) / BANDS as f32;
        let [r, g, b, a] = sample(stops, at).to_rgb_f32();
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.left() + band as f32 * width, rect.top()),
                Vec2::new(width.ceil(), HEIGHT),
            ),
            0.0,
            egui::Color32::from_rgba_unmultiplied(
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
                (a * 255.0) as u8,
            ),
        );
    }
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, Theme::BORDER),
        egui::StrokeKind::Inside,
    );
}

/// The colour a ramp shows at `at`, for the preview only.
///
/// Straight interpolation in sRGB, which is what the renderer and the PDF both
/// do, so the strip agrees with the page. Before the first stop and after the
/// last it holds that stop, which is the same extension both of them apply.
fn sample(stops: &[tessera_document::paint::Stop], at: f32) -> Color {
    let first = &stops[0];
    if at <= first.at {
        return first.colour.clone();
    }
    for pair in stops.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if at <= b.at {
            let span = b.at - a.at;
            let t = if span <= 0.0 { 0.0 } else { (at - a.at) / span };
            let [r0, g0, b0, a0] = a.colour.to_rgb_f32();
            let [r1, g1, b1, a1] = b.colour.to_rgb_f32();
            let mix = |x: f32, y: f32| x + (y - x) * t;
            return Color::Rgb {
                r: mix(r0, r1),
                g: mix(g0, g1),
                b: mix(b0, b1),
                a: mix(a0, a1),
            };
        }
    }
    stops[stops.len() - 1].colour.clone()
}

/// Common dash patterns, in multiples of the stroke's own width.
///
/// Relative to the width so that a dashed hairline and a dashed 6 pt rule read
/// as the same pattern rather than the thick one looking almost solid.
const DASH_PRESETS: [(&str, &[f64]); 3] = [
    ("Solid", &[]),
    ("Dashed", &[3.0, 2.0]),
    ("Dotted", &[0.0, 2.0]),
];

fn stroke_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::nodes::{LineCap, LineJoin, Stroke, StrokeAlign};

    let mut on = frame.stroke.is_some();
    if ui.checkbox(&mut on, "Stroked").changed() {
        // Turning it on gives the stroke the model's own default: what
        // everything drew before the extra properties existed.
        let stroke = on.then(|| Stroke::new(Color::BLACK, 1.0));
        apply(state, Command::SetStroke { id, stroke });
        return;
    }

    let Some(existing) = frame.stroke.clone() else {
        return;
    };
    let mut stroke = existing.clone();
    let unit = state.prefs.unit;

    measure(ui, "Weight", &mut stroke.width, unit);

    let [r, g, b, a] = stroke.color.to_rgb_f32();
    let mut rgba = [r, g, b, a];
    ui.horizontal(|ui| {
        let (spot, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        crate::icons::paint(
            ui.painter(),
            spot,
            crate::icons::Icon::Palette,
            Theme::TEXT_MUTED,
        );
        ui.colored_label(Theme::TEXT_MUTED, "Colour");
        if fill_picker(ui, &mut rgba) {
            stroke.color = Color::Rgb {
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: rgba[3],
            };
        }
    });

    // Alignment is the one stroke property that changes geometry rather than
    // appearance, which is why the model carries it and why it sits first.
    segmented(
        ui,
        "Align",
        &mut stroke.align,
        &[
            ("Centre", StrokeAlign::Center),
            ("Inside", StrokeAlign::Inside),
            ("Outside", StrokeAlign::Outside),
        ],
    );

    segmented(
        ui,
        "Cap",
        &mut stroke.cap,
        &[
            ("Butt", LineCap::Butt),
            ("Round", LineCap::Round),
            ("Square", LineCap::Square),
        ],
    );

    segmented(
        ui,
        "Join",
        &mut stroke.join,
        &[
            ("Miter", LineJoin::Miter),
            ("Round", LineJoin::Round),
            ("Bevel", LineJoin::Bevel),
        ],
    );

    // Shown only when it means something. A miter limit on a rounded join is
    // a control that does nothing, which is worse than one that is absent.
    if stroke.join == LineJoin::Miter {
        ui.horizontal(|ui| {
            ui.colored_label(Theme::TEXT_MUTED, "Miter limit");
            ui.add(
                egui::DragValue::new(&mut stroke.miter_limit)
                    .speed(0.1)
                    .range(1.0..=100.0),
            );
        });
    }

    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Dashes");
        for (label, pattern) in DASH_PRESETS {
            let scaled: Vec<f64> = pattern.iter().map(|d| d * stroke.width.max(0.1)).collect();
            let selected = dashes_match(&stroke.dashes, &scaled);
            if ui.selectable_label(selected, label).clicked() {
                stroke.dashes = scaled;
            }
        }
    });

    if stroke.is_dashed() {
        measure(ui, "Dash offset", &mut stroke.dash_offset, unit);
    }

    if stroke != existing {
        // The whole struct, so an edit is one undo entry rather than one per
        // property touched.
        apply(
            state,
            Command::SetStroke {
                id,
                stroke: Some(stroke),
            },
        );
    }
}

fn dashes_match(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// A small square button drawn as a Lucide glyph.
///
/// `tooltip` is not decoration: an icon says what it means only to someone who
/// already knows, so every one of these carries its own name.
pub(crate) fn icon_button(
    ui: &mut Ui,
    icon: crate::icons::Icon,
    tooltip: &str,
    active: bool,
) -> bool {
    let size = Vec2::splat(Theme::TOOL_SIZE * 0.72);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if active || response.hovered() {
        ui.painter().rect_filled(
            rect,
            3.0,
            if active {
                Theme::ACCENT
            } else {
                Theme::HOVER_BG
            },
        );
    }
    let tint = if active {
        Theme::TEXT_PRIMARY
    } else {
        Theme::TEXT_MUTED
    };
    crate::icons::paint(ui.painter(), rect.shrink(4.0), icon, tint);

    response.on_hover_text(tooltip).clicked()
}

/// [`group_label`], for another module in the view.
pub(crate) fn group_label_pub(ui: &mut Ui, text: &str) {
    group_label(ui, text);
}

/// What is in a picture box: the file, its state, and its real resolution.
fn graphic_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::graphic::Fit;
    use tessera_document::links::Status;
    use tessera_document::nodes::FrameKind;

    let FrameKind::Graphic { placed } = &frame.kind else {
        return;
    };

    let Some(placement) = placed else {
        ui.colored_label(Theme::TEXT_MUTED, "Empty");
        if ui.button("Place artwork...").clicked() {
            crate::file_ops::place(state);
        }
        return;
    };

    let link = state.active().document().links.get(placement.link).cloned();
    let Some(link) = link else {
        ui.colored_label(Theme::ERROR, "The link is missing from the document");
        return;
    };

    // The file, by name. The whole path is usually too long for the rail and
    // the name is what a person recognises; the path is on the tooltip.
    let name = link
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| link.path.to_string_lossy().into_owned());
    ui.label(&name)
        .on_hover_text(link.path.to_string_lossy().into_owned());

    // What the disk says, now. **Three** states rather than two: "the file has
    // changed" is the one the previous codebase never drew, and the reason
    // somebody could send a printer last week's photograph.
    let status = link.status();
    let (word, colour) = match status {
        Status::Fine => ("Up to date", Theme::TEXT_MUTED),
        Status::Modified => ("Modified on disk", Theme::ACCENT),
        Status::Missing => ("Missing", Theme::ERROR),
    };
    ui.colored_label(colour, word);

    if status != Status::Fine
        && ui
            .button("Relink...")
            .on_hover_text("Choose the file this should point at")
            .clicked()
    {
        crate::file_ops::place(state);
    }

    // The effective resolution, which is the number a printer cares about: a
    // 300ppi photograph at twice its size is a 150ppi photograph.
    let drawn = {
        let a = placement.inner.apply(tessera_geometry::DocPoint::ZERO);
        let b = placement.inner.apply(tessera_geometry::DocPoint {
            x: link.natural.0,
            y: link.natural.1,
        });
        ((b.x - a.x).abs(), (b.y - a.y).abs())
    };
    let pixels = (link.natural.0 as u32, link.natural.1 as u32);
    if let Some((x, y)) = tessera_render::images::effective_ppi(pixels, drawn) {
        let worst = x.min(y);
        let colour = if worst < state.prefs.minimum_ppi {
            Theme::ERROR
        } else {
            Theme::TEXT_MUTED
        };
        // Two figures when they differ, because a stretched placement really
        // does have two and one would hide it.
        let shown = if (x - y).abs() < 0.5 {
            format!("{worst:.0} ppi")
        } else {
            format!("{x:.0} x {y:.0} ppi")
        };
        ui.colored_label(colour, shown).on_hover_text(format!(
            "Effective resolution. Reported below {:.0} ppi.",
            state.prefs.minimum_ppi
        ));
    }

    group_label(ui, "Fit");
    ui.horizontal(|ui| {
        for (label, how) in [
            ("Proportionally", Fit::Proportionally),
            ("Fill", Fit::FillProportionally),
            ("Stretch", Fit::Stretch),
            ("Centre", Fit::Centre),
        ] {
            if ui.small_button(label).clicked() {
                apply(state, Command::RefitArtwork { id, fit: how });
            }
        }
    });
    if ui.button("Fit frame to artwork").clicked() {
        apply(state, Command::FitFrameToArtwork { id });
    }
}

/// Opacity and blend mode: what happens to the object's paint once it is
/// painted.
///
/// **Not** in the Fill section, deliberately. A fill colour's alpha and an
/// object's opacity look alike in a panel and are different facts — one makes
/// the fill translucent and leaves the stroke solid, the other composites the
/// whole object at once — and putting them side by side under one heading is
/// how a person comes to believe they are the same control.
fn effects_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::blending::BlendMode;

    let mut blend = frame.blend;
    let mut changed = false;

    // Shown as a percentage, which is how a person says it; stored as the
    // fraction, which is what every renderer wants. Converted here, once, at
    // the edge.
    let mut percent = blend.alpha() * 100.0;
    changed |= field(ui, "Opacity", |ui| {
        ui.add(
            egui::Slider::new(&mut percent, 0.0..=100.0)
                .suffix("%")
                .fixed_decimals(0),
        )
        .changed()
    });
    if changed {
        blend.opacity = percent / 100.0;
    }

    // A drop-down rather than a segmented row: four modes will not fit across
    // a 292-point panel as words, and abbreviating them would make the reader
    // learn a code for something they use rarely.
    let before = blend.mode;
    field(ui, "Blend", |ui| {
        egui::ComboBox::from_id_salt(("blend-mode", id))
            .selected_text(blend.mode.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for mode in BlendMode::ALL {
                    ui.selectable_value(&mut blend.mode, mode, mode.label());
                }
            });
    });
    changed |= blend.mode != before;

    // What an object at no opacity actually means, said plainly. It is still
    // selectable and still in the layers panel, and somebody who cannot see it
    // will otherwise think it has gone.
    if blend.is_invisible() {
        ui.colored_label(
            Theme::TEXT_MUTED,
            "Invisible. Still selectable, and still on its layer.",
        );
    }

    if changed && blend != frame.blend {
        apply(state, Command::SetBlending { id, blend });
    }

    shadow_controls(ui, state, id, frame);
}

/// The shadow an object casts: whether, how far, how soft, and what colour.
fn shadow_controls(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::shadow::{MOST_BLUR, Shadow};

    group_label(ui, "Drop shadow");

    let mut on = frame.shadow.is_some();
    if ui.checkbox(&mut on, "Casts a shadow").changed() {
        // Switching off keeps nothing, because the model says nothing: a
        // shadow either exists or it does not. Switching on gets the shadow
        // that reads as depth rather than as an effect.
        let shadow = if on { Some(Shadow::TYPICAL) } else { None };
        apply(state, Command::SetShadow { id, shadow });
        return;
    }

    let Some(existing) = &frame.shadow else {
        return;
    };
    let mut shadow = existing.clone();
    let mut changed = false;
    let unit = state.prefs.unit;

    let (x, y) = pair(
        ui,
        ("Offset X", |ui: &mut Ui| {
            measure_bare(ui, &mut shadow.offset.0, unit)
        }),
        ("Y", |ui: &mut Ui| {
            measure_bare(ui, &mut shadow.offset.1, unit)
        }),
    );
    changed |= x || y;

    // In points rather than in the document's unit: a blur is not a measurement
    // on the page, it is how soft an edge is, and reading it in millimetres
    // invites somebody to try to line it up with something.
    changed |= field(ui, "Blur", |ui| {
        ui.add(
            egui::Slider::new(&mut shadow.blur, 0.0..=MOST_BLUR)
                .suffix(" pt")
                .fixed_decimals(1),
        )
        .changed()
    });

    // The colour carries how much of the shadow shows, in its alpha, so the
    // picker offers alpha here where the fill's does not.
    let [r, g, b, a] = shadow.colour.to_rgb_f32();
    let mut rgba = [r, g, b, a];
    if field(ui, "Colour", |ui| shadow_picker(ui, &mut rgba)) {
        shadow.colour = Color::Rgb {
            r: rgba[0],
            g: rgba[1],
            b: rgba[2],
            a: rgba[3],
        };
        changed = true;
    }

    // Said where a person can read it, because a shadow that appears on screen
    // and not in the export is exactly the kind of surprise that reaches a
    // printer.
    ui.colored_label(Theme::TEXT_MUTED, "Not written to PDF yet.")
        .on_hover_text(
            "A blurred shadow in a PDF needs a rasterised soft mask. \
             Milestone 6 owns export quality and adds it.",
        );

    if changed {
        apply(
            state,
            Command::SetShadow {
                id,
                shadow: Some(shadow),
            },
        );
    }
}

/// A colour picker that offers alpha.
///
/// Separate from [`fill_picker`], which deliberately does not: a fill's alpha
/// and its object's opacity are different facts and offering both in one place
/// is how a person comes to believe they are the same control. A shadow has no
/// such pair — its alpha *is* how much of it shows.
fn shadow_picker(ui: &mut Ui, rgba: &mut [f32; 4]) -> bool {
    let mut colour = egui::Rgba::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    let changed = egui::widgets::color_picker::color_edit_button_rgba(
        ui,
        &mut colour,
        egui::widgets::color_picker::Alpha::OnlyBlend,
    )
    .changed();
    if changed {
        *rgba = [colour.r(), colour.g(), colour.b(), colour.a()];
    }
    changed
}

/// How text in other frames runs around this one.
///
/// On every kind of object, not only text frames: a picture is the thing most
/// often wrapped, and it is the obstacle that carries the setting.
fn wrap_controls(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::nodes::TextWrap;

    let mut on = frame.wrap != TextWrap::None;
    let mut standoff = frame.wrap.standoff().unwrap_or_default();
    let mut changed = false;

    if ui.checkbox(&mut on, "Text runs around this").changed() {
        changed = true;
    }
    if on {
        let unit = state.prefs.unit;
        group_label(ui, "Standoff");
        let (a, b) = pair(
            ui,
            ("Top", |ui: &mut Ui| {
                measure_bare(ui, &mut standoff.top, unit)
            }),
            ("Bottom", |ui: &mut Ui| {
                measure_bare(ui, &mut standoff.bottom, unit)
            }),
        );
        let (c, d) = pair(
            ui,
            ("Left", |ui: &mut Ui| {
                measure_bare(ui, &mut standoff.left, unit)
            }),
            ("Right", |ui: &mut Ui| {
                measure_bare(ui, &mut standoff.right, unit)
            }),
        );
        changed |= a || b || c || d;
    }

    if changed {
        let wrap = if on {
            TextWrap::Bounds { standoff }
        } else {
            TextWrap::None
        };
        apply(state, Command::SetTextWrap { id, wrap });
    }
}

/// Columns, gutter, inset: the frame's geometry rather than the text's.
///
/// Under Character and Paragraph because it is a third thing: those describe
/// the words, this describes the box they are poured into.
fn text_frame_controls(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::nodes::{FrameKind, TextLayout};

    let FrameKind::Text { layout, .. } = frame.kind else {
        return;
    };
    let mut wanted: TextLayout = layout;
    let unit = state.prefs.unit;
    let mut changed = false;

    subheading(ui, crate::icons::Icon::TextFrame, "Frame");

    let mut columns = f64::from(wanted.columns.max(1));
    let (a, b) = pair(
        ui,
        ("Columns", |ui: &mut Ui| {
            ui.add(
                egui::DragValue::new(&mut columns)
                    .speed(0.1)
                    .range(1.0..=20.0),
            )
            .changed()
        }),
        ("Gutter", |ui: &mut Ui| {
            measure_bare(ui, &mut wanted.gutter, unit)
        }),
    );
    if a {
        wanted.columns = columns.round().clamp(1.0, 20.0) as u8;
    }
    changed |= a || b;

    group_label(ui, "Inset");
    let (c, d) = pair(
        ui,
        ("Top", |ui: &mut Ui| {
            measure_bare(ui, &mut wanted.inset.top, unit)
        }),
        ("Bottom", |ui: &mut Ui| {
            measure_bare(ui, &mut wanted.inset.bottom, unit)
        }),
    );
    let (e, f) = pair(
        ui,
        ("Left", |ui: &mut Ui| {
            measure_bare(ui, &mut wanted.inset.left, unit)
        }),
        ("Right", |ui: &mut Ui| {
            measure_bare(ui, &mut wanted.inset.right, unit)
        }),
    );
    changed |= c || d || e || f;

    // Only offered when there is a grid to lock to. A switch that does
    // nothing until a setting three panels away is turned on is a switch that
    // reads as broken.
    if state.active().document().setup.baseline_grid.is_some() {
        let mut locked = wanted.lock_to_grid;
        if ui.checkbox(&mut locked, "Lock to baseline grid").changed() {
            wanted.lock_to_grid = locked;
            changed = true;
        }
    }

    // Where the text sits when it does not fill the frame. Icons rather than
    // a list: it is the same choice as horizontal alignment and reads the
    // same way, turned a quarter turn.
    field(ui, "Vertical", |ui| {
        use tessera_document::nodes::VerticalJustify as V;
        for (icon, tip, which) in [
            (crate::icons::Icon::AlignTop, "Top", V::Top),
            (crate::icons::Icon::AlignMiddleV, "Centre", V::Centre),
            (crate::icons::Icon::AlignBottom, "Bottom", V::Bottom),
            (crate::icons::Icon::AlignJustify, "Justify", V::Justify),
        ] {
            if icon_button(ui, icon, tip, wanted.vertical == which) {
                wanted.vertical = which;
                changed = true;
            }
        }
    });

    if changed {
        apply(state, Command::SetTextLayout { id, layout: wanted });
    }
}

/// A quiet label naming a group of fields inside a section.
///
/// Not a section heading: it does not collapse and it carries no icon. The
/// difference in weight is what says one is a level above the other.
fn group_label(ui: &mut Ui, text: &str) {
    ui.add_space(Theme::SPACE_1);
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(Theme::TYPE_SM)
                .color(Theme::TEXT_MUTED),
        )
        .selectable(false),
    );
}

/// A heading inside a section, with the glyph that names what follows.
fn subheading(ui: &mut Ui, icon: crate::icons::Icon, label: &str) {
    ui.add_space(Theme::SPACING_SM);
    ui.horizontal(|ui| {
        let size = Vec2::splat(12.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        crate::icons::paint(ui.painter(), rect, icon, Theme::TEXT_MUTED);
        ui.colored_label(Theme::TEXT_MUTED, label);
    });
}

/// A row of mutually exclusive choices, the shape a three-way property wants.
fn segmented<T: PartialEq + Copy>(ui: &mut Ui, label: &str, value: &mut T, options: &[(&str, T)]) {
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, label);
        for (text, candidate) in options {
            if ui.selectable_label(*value == *candidate, *text).clicked() {
                *value = *candidate;
            }
        }
    });
}

/// What the typography controls act on: the text selection if there is one,
/// otherwise the whole story.
///
/// InDesign's rule, and the reason the section is useful before a caret
/// exists — select the frame and the controls format all of its text. A caret
/// with no selection is not the whole story: it is a caret, and character
/// formatting there would have nothing to act on, so the range stays empty and
/// the controls read the run typing will join.
fn format_target(
    state: &TesseraApp,
    id: tessera_document::ids::FrameId,
    story: StoryId,
) -> std::ops::Range<usize> {
    let whole = 0..state
        .active()
        .document()
        .story(story)
        .map_or(0, |s| s.text.len());
    match &state.active().editing {
        Some((editing, buffer)) if *editing == id => {
            buffer.selection_range().unwrap_or_else(|| {
                let at = buffer.cursor().position;
                at..at
            })
        }
        _ => whole,
    }
}

/// A character property the user just changed, as a format stating only it.
///
/// Only the changed field, never the whole shown struct. Sending everything
/// would stamp the shown values onto every run in the range and flatten the
/// variation the panel was showing as blank — an inspector that destroys what
/// it displays.
fn set_character(
    state: &mut TesseraApp,
    story: StoryId,
    range: std::ops::Range<usize>,
    format: CharacterFormat,
) {
    apply(
        state,
        Command::SetCharacterFormat {
            story,
            range,
            format,
        },
    );
}

fn set_paragraph(
    state: &mut TesseraApp,
    story: StoryId,
    range: std::ops::Range<usize>,
    format: ParagraphFormat,
) {
    apply(
        state,
        Command::SetParagraphFormat {
            story,
            range,
            format,
        },
    );
}

/// A number field for a property that may have no single value to show.
///
/// `None` draws as a blank field with a hint, which is what the panel says
/// when the runs disagree. Typing into it sets every run in the range.
/// A labelled control: the label in a fixed column, then the control.
///
/// The reason every panel now lines up. Each control used to lay out its own
/// `horizontal(label, widget)`, so the fields started wherever the label
/// happened to end — different in every row and different again in every
/// panel — and a long label pushed its field off the edge instead of
/// wrapping. The column is fixed, the label is clipped rather than allowed to
/// push, and the control begins at the same x in every row of the application.
pub(crate) fn field<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    labelled(ui, label, Theme::LABEL_COLUMN, add)
}

/// A labelled control with a chosen label width.
///
/// The control is given **all the room that is left**, rather than sizing
/// itself and leaving the rest of the panel blank to its right. A 292-point
/// panel with a 64-point label and a field that draws at its natural 60 was
/// throwing away more than half its width on every row.
fn labelled<R>(ui: &mut Ui, label: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        ui.painter().text(
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            Theme::TEXT_MUTED,
        );
        ui.style_mut().spacing.slider_width = ui.available_width();
        ui.scope(|ui| {
            let room = ui.available_width();
            ui.set_min_width(room);
            ui.spacing_mut().interact_size.x = room;
            add(ui)
        })
        .inner
    })
    .inner
}

/// Two labelled controls in one row.
///
/// **X and Y are one fact, not two.** So are a width and a height, an inside
/// and an outside margin, a space before and a space after. Stacking them
/// makes a panel twice as tall as it needs to be and hides the relationship
/// that makes them easy to read; side by side, the pair is one line and the
/// eye compares them without moving.
pub(crate) fn pair<A, B>(
    ui: &mut Ui,
    first: (&str, impl FnOnce(&mut Ui) -> A),
    second: (&str, impl FnOnce(&mut Ui) -> B),
) -> (A, B) {
    // A shorter label column inside a pair: each half has half the room, and
    // the full column would leave nothing for the control.
    const NARROW: f32 = 36.0;

    let mut out = (None, None);
    ui.horizontal(|ui| {
        let half = (ui.available_width() - Theme::SPACE_2) / 2.0;
        ui.scope(|ui| {
            ui.set_max_width(half);
            out.0 = Some(labelled(ui, first.0, NARROW, first.1));
        });
        ui.scope(|ui| {
            ui.set_max_width(half);
            out.1 = Some(labelled(ui, second.0, NARROW, second.1));
        });
    });
    (
        out.0.expect("the first half drew"),
        out.1.expect("the second half drew"),
    )
}

/// A section heading that opens and shuts, remembering which it was.
///
/// One component, used by the rail and by the inspector's own sections. A
/// panel that shows Transform, Fill, Stroke, Text and Styles at once is a
/// column nobody reads to the end of — which is the same complaint the Object
/// menu earned before it grew submenus.
pub(crate) fn section_heading(
    ui: &mut Ui,
    state: &mut TesseraApp,
    icon: crate::icons::Icon,
    title: &'static str,
) -> bool {
    let was = state.sections.is_open(title);
    let now = section_heading_with(ui, icon, title, was);
    if now != was {
        state.sections.set_open(title, now);
    }
    now
}

/// The same heading, for a caller that keeps the open state itself.
///
/// Returns what the state should now be. The rail's sections are the panels
/// the Window menu opens and shut, so their flag lives with the panel — a
/// heading with a second, private record of the same fact is two records that
/// can disagree, and on the first frame they did.
pub(crate) fn section_heading_with(
    ui: &mut Ui,
    icon: crate::icons::Icon,
    title: &str,
    open: bool,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), Theme::ROW), Sense::click());
    let painter = ui.painter_at(rect);

    if response.hovered() {
        painter.rect_filled(rect, Theme::RADIUS, Theme::HOVER_BG);
    }

    let caret = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - 5.0),
        Vec2::splat(10.0),
    );
    crate::icons::paint_rotated(
        &painter,
        caret,
        crate::icons::Icon::ChevronRight,
        Theme::TEXT_MUTED,
        if open { 90.0 } else { 0.0 },
        1.0,
    );

    let glyph = egui::Rect::from_min_size(
        egui::pos2(caret.right() + Theme::SPACE_1, rect.center().y - 6.0),
        Vec2::splat(12.0),
    );
    crate::icons::paint(&painter, glyph, icon, Theme::TEXT_MUTED);

    painter.text(
        egui::pos2(glyph.right() + Theme::SPACE_2, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(Theme::TYPE_MD),
        Theme::TEXT_PRIMARY,
    );

    if response.clicked() { !open } else { open }
}

/// A measurement control with no label of its own.
fn measure_bare(ui: &mut Ui, points: &mut f64, unit: Unit) -> bool {
    let mut shown = unit.from_points(*points);
    let changed = ui
        .add(
            egui::DragValue::new(&mut shown)
                .speed(0.25)
                .custom_formatter(move |v, _| format!("{v:.2} {}", unit.suffix()))
                .custom_parser(move |text| {
                    Unit::parse_to_points(text, unit).map(|p| unit.from_points(p))
                }),
        )
        .changed();
    if changed {
        *points = unit.to_points(shown);
    }
    changed
}

/// An optional number with no label of its own.
fn optional_number_bare(
    ui: &mut Ui,
    shown: Option<f32>,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> Option<f32> {
    match shown {
        Some(value) => {
            let mut edited = f64::from(value);
            let suffix = suffix.to_string();
            ui.add(
                egui::DragValue::new(&mut edited)
                    .speed(speed)
                    .range(range)
                    .custom_formatter(move |v, _| format!("{v:.2}{suffix}")),
            )
            .changed()
            .then_some(edited as f32)
        }
        None => {
            // Mixed. A zero here would be a lie the user cannot see through,
            // so the field shows its absence and offers the way to resolve it.
            ui.button("Mixed")
                .on_hover_text("The selection has more than one value. Click to unify.")
                .clicked()
                .then(|| *range.start() as f32)
        }
    }
}

fn optional_number(
    ui: &mut Ui,
    label: &str,
    shown: Option<f32>,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> Option<f32> {
    let mut changed = None;
    field(ui, label, |ui| {
        match shown {
            Some(value) => {
                let mut edited = f64::from(value);
                let suffix = suffix.to_string();
                if ui
                    .add(
                        egui::DragValue::new(&mut edited)
                            .speed(speed)
                            .range(range)
                            .custom_formatter(move |v, _| format!("{v:.2}{suffix}")),
                    )
                    .changed()
                {
                    changed = Some(edited as f32);
                }
            }
            None => {
                // Mixed. A zero here would be a lie the user cannot see
                // through, so the field shows its absence and offers the way
                // to resolve it.
                if ui
                    .button("Mixed")
                    .on_hover_text("The selection has more than one value. Click to unify.")
                    .clicked()
                {
                    changed = Some(*range.start() as f32);
                }
            }
        }
    });
    changed
}

fn text_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    // `id` identifies which frame's caret decides the target range; the story
    // is what everything here actually edits.
    let tessera_document::nodes::FrameKind::Text { story, .. } = &frame.kind else {
        return;
    };
    let story = *story;

    // No text box here. The text is on the canvas, where it is set; a copy of
    // it in the inspector grows with the story until it drives every control
    // below it off the panel, and a person editing a page is looking at the
    // page.
    let target = format_target(state, id, story);
    let (shown, paragraph) = {
        let doc = state.active().document();
        let Some(s) = doc.story(story) else {
            return;
        };
        (
            s.common_format(target.clone(), doc),
            s.common_paragraph_format(target.clone()),
        )
    };

    // At a caret, formatting is held until there is text to put it on — so the
    // panel has to show what is held, or choosing red would leave the swatch
    // black and look like nothing happened.
    let shown = match &state.active().editing {
        Some((editing, buffer)) if *editing == id && target.start >= target.end => {
            buffer.pending().over(&shown)
        }
        _ => shown,
    };

    // Which faces this document names that this machine has not got. parley
    // substitutes silently, which is right for drawing and wrong for the
    // person holding the file.
    let missing = {
        let key = state.active;
        let TesseraApp {
            documents, shaper, ..
        } = state;
        let open = &documents[key];
        match open.document().story(story) {
            Some(s) => shaper.missing_families(s, open.document()),
            None => Vec::new(),
        }
    };

    subheading(ui, crate::icons::Icon::CaseSensitive, "Character");

    // Family, size and leading are **not** here. They are the three a person
    // changes while typing, so they live in the control bar, and a control
    // that appears in two places is two places to read a different answer
    // from. What is left is what the bar has no room for.
    //
    // The families a story asks for and the system does not have are still
    // reported here, because that is a fault to be seen rather than a control
    // to be used.
    if !missing.is_empty() {
        ui.colored_label(Theme::ERROR, format!("Missing: {}", missing.join(", ")));
    }

    // Tracking in thousandths of an em, the unit every type specimen uses.
    if let Some(tracking) = optional_number(
        ui,
        "Tracking",
        Some(shown.tracking.unwrap_or(0.0)),
        1.0,
        -200.0..=800.0,
        "/1000 em",
    ) {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                tracking: Some(tracking),
                ..CharacterFormat::default()
            },
        );
    }

    // Case. A display transform, not an edit: the story keeps what was typed,
    // so turning All Caps off gives back the original capitals rather than a
    // sentence that has forgotten where they were.
    let mut case_change = None;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Case");
        for (label, case, hint) in [
            ("aa", Case::Normal, "As typed"),
            ("AA", Case::Upper, "All capitals"),
            ("Aa", Case::SmallCaps, "Small capitals"),
            ("aa\u{0332}", Case::Lower, "All lower case"),
        ] {
            if ui
                .selectable_label(shown.case == Some(case), label)
                .on_hover_text(hint)
                .clicked()
            {
                case_change = Some(case);
            }
        }
    });
    if let Some(case) = case_change {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                case: Some(case),
                ..CharacterFormat::default()
            },
        );
    }

    // Baseline shift: a superscript sits above the line it belongs to without
    // making that line taller.
    if let Some(shift) = optional_number(
        ui,
        "Baseline",
        Some(shown.baseline_shift.unwrap_or(0.0)),
        0.25,
        -200.0..=200.0,
        " pt",
    ) {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                baseline_shift: Some(shift),
                ..CharacterFormat::default()
            },
        );
    }

    // Text colour. Distinct from the frame's fill, which is the box behind the
    // glyphs — setting that and expecting the letters to change is the mistake
    // the two controls sitting apart is meant to prevent.
    let shown_colour = shown.colour.clone().unwrap_or(Color::BLACK);
    let [r, g, b, a] = shown_colour.to_rgb_f32();
    let mut rgba = [r, g, b, a];
    ui.horizontal(|ui| {
        let (spot, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        crate::icons::paint(
            ui.painter(),
            spot,
            crate::icons::Icon::Palette,
            Theme::TEXT_MUTED,
        );
        ui.colored_label(Theme::TEXT_MUTED, "Colour");
        if fill_picker(ui, &mut rgba) {
            set_character(
                state,
                story,
                target.clone(),
                CharacterFormat {
                    colour: Some(Color::Rgb {
                        r: rgba[0],
                        g: rgba[1],
                        b: rgba[2],
                        a: rgba[3],
                    }),
                    ..CharacterFormat::default()
                },
            );
        }
    });

    // Weight and slant on one row. Bold and Italic are toggles rather than a
    // list, because that is how they are used: the numbered weights stay for
    // the faces that have them, but the pair a person reaches for constantly
    // should be one click and recognisable without reading.
    let mut weight_change = None;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Weight");
        let bold = shown.weight.is_some_and(|w| w >= 600);
        if icon_button(ui, crate::icons::Icon::Bold, "Bold", bold) {
            // Off returns to 400 rather than to inherit: a toggle that cleared
            // the property would leave a run bold whenever its style was.
            weight_change = Some(if bold { 400 } else { 700 });
        }
        let italic = shown.italic == Some(true);
        if icon_button(ui, crate::icons::Icon::Italic, "Italic", italic) {
            set_character(
                state,
                story,
                target.clone(),
                CharacterFormat {
                    // `Some(false)`, not `None`: `None` means inherit and would
                    // leave the text italic when its style says so.
                    italic: Some(!italic),
                    ..CharacterFormat::default()
                },
            );
        }
        ui.separator();
        for (label, weight) in [("300", 300u16), ("400", 400), ("500", 500), ("700", 700)] {
            if ui
                .selectable_label(shown.weight == Some(weight), label)
                .on_hover_text(match weight {
                    300 => "Light",
                    400 => "Regular",
                    500 => "Medium",
                    _ => "Bold",
                })
                .clicked()
            {
                weight_change = Some(weight);
            }
        }
    });
    if let Some(weight) = weight_change {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                weight: Some(weight),
                ..CharacterFormat::default()
            },
        );
    }

    // --- the paragraph half

    text_frame_controls(ui, state, id, frame);

    subheading(ui, crate::icons::Icon::Pilcrow, "Paragraph");

    let mut alignment_change = None;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Align");
        for (icon, name, alignment) in [
            (crate::icons::Icon::AlignLeft, "Left", Alignment::Left),
            (
                crate::icons::Icon::AlignCentreH,
                "Centre",
                Alignment::Centre,
            ),
            (crate::icons::Icon::AlignRight, "Right", Alignment::Right),
            (
                crate::icons::Icon::AlignJustify,
                "Justify",
                Alignment::Justify,
            ),
        ] {
            if icon_button(ui, icon, name, paragraph.alignment == Some(alignment)) {
                alignment_change = Some(alignment);
            }
        }
    });
    if let Some(alignment) = alignment_change {
        set_paragraph(
            state,
            story,
            target.clone(),
            ParagraphFormat {
                alignment: Some(alignment),
                ..ParagraphFormat::default()
            },
        );
    }

    // Drop cap. Zero lines is no drop cap, which is why the row reads as a
    // count rather than as a switch with a count beside it.
    if let Some(lines) = optional_number(
        ui,
        "Drop cap lines",
        Some(f32::from(paragraph.drop_cap_lines.unwrap_or(0))),
        1.0,
        0.0..=10.0,
        "",
    ) {
        set_paragraph(
            state,
            story,
            target.clone(),
            ParagraphFormat {
                drop_cap_lines: Some(lines.round().clamp(0.0, 10.0) as u8),
                ..ParagraphFormat::default()
            },
        );
    }
    if paragraph.drop_cap_lines.unwrap_or(0) > 0
        && let Some(chars) = optional_number(
            ui,
            "Drop cap letters",
            Some(f32::from(paragraph.drop_cap_characters.unwrap_or(1))),
            1.0,
            1.0..=10.0,
            "",
        )
    {
        set_paragraph(
            state,
            story,
            target.clone(),
            ParagraphFormat {
                drop_cap_characters: Some(chars.round().clamp(1.0, 10.0) as u8),
                ..ParagraphFormat::default()
            },
        );
    }

    // Hyphenation. English only for now: `hypher` holds its patterns per
    // language and a story has no language to pick one with.
    let hyphenating = paragraph.hyphenate == Some(true);
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Hyphenate");
        if ui
            .selectable_label(hyphenating, "Break words")
            .on_hover_text("English patterns")
            .clicked()
        {
            set_paragraph(
                state,
                story,
                target.clone(),
                ParagraphFormat {
                    // `Some(false)`, not `None`: `None` means inherit, and a
                    // toggle built on it could never turn anything off.
                    hyphenate: Some(!hyphenating),
                    ..ParagraphFormat::default()
                },
            );
        }
    });

    /// Which field of a `ParagraphFormat` a row writes.
    type Set = fn(&mut ParagraphFormat, f32);

    // Paired by meaning: an indent against the opposite indent, a space
    // before against the space after. Five rows in a column said nothing about
    // which of them belong together.
    group_label(ui, "Indents");
    /// Two paragraph measurements shown side by side: their labels, what they
    /// currently read, and where each writes back to.
    struct Paired(
        &'static str,
        &'static str,
        Option<f32>,
        Option<f32>,
        Set,
        Set,
    );

    let indents = [
        Paired(
            "Left",
            "Right",
            paragraph.indent_left,
            paragraph.indent_right,
            |f: &mut ParagraphFormat, v| f.indent_left = Some(v),
            |f: &mut ParagraphFormat, v| f.indent_right = Some(v),
        ),
        Paired(
            "First",
            "Before",
            paragraph.indent_first,
            paragraph.space_before,
            |f: &mut ParagraphFormat, v| f.indent_first = Some(v),
            |f: &mut ParagraphFormat, v| f.space_before = Some(v),
        ),
    ];
    for Paired(la, lb, ra, rb, sa, sb) in indents {
        let (a, b) = pair(
            ui,
            (la, |ui: &mut Ui| {
                optional_number_bare(ui, Some(ra.unwrap_or(0.0)), 0.25, 0.0..=1440.0, " pt")
            }),
            (lb, |ui: &mut Ui| {
                optional_number_bare(ui, Some(rb.unwrap_or(0.0)), 0.25, 0.0..=1440.0, " pt")
            }),
        );
        for (value, set) in [(a, sa), (b, sb)] {
            if let Some(value) = value {
                let mut format = ParagraphFormat::default();
                set(&mut format, value);
                set_paragraph(state, story, target.clone(), format);
            }
        }
    }

    for (label, read, set) in [(
        "Space after",
        paragraph.space_after,
        (|f: &mut ParagraphFormat, v| f.space_after = Some(v)) as Set,
    )] {
        // Shown as 0 rather than blank: an indent nobody has set is not
        // ambiguous, it is zero.
        let Some(value) = optional_number(
            ui,
            label,
            Some(read.unwrap_or(0.0)),
            0.25,
            -720.0..=720.0,
            " pt",
        ) else {
            continue;
        };
        let mut format = ParagraphFormat::default();
        set(&mut format, value);
        set_paragraph(state, story, target.clone(), format);
    }

    style_rows(ui, state, story, target);
}

/// The font menu, with the faces this machine lacks marked.
///
/// The list is built inside the closure, so it is only enumerated when the menu
/// is actually open — the scan costs tens of milliseconds and a closed menu
/// should not pay it every frame.
fn family_picker(
    ui: &mut Ui,
    state: &mut TesseraApp,
    shown: Option<&str>,
    missing: &[String],
) -> Option<String> {
    let mut chosen = None;
    field(ui, "Family", |ui| {
        let label = shown.unwrap_or("Mixed");
        egui::ComboBox::from_id_salt("family")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for family in state.shaper.families() {
                    if ui
                        .selectable_label(shown == Some(family.as_str()), family)
                        .clicked()
                    {
                        chosen = Some(family.clone());
                    }
                }
            });
    });

    for family in missing {
        ui.colored_label(
            Theme::ERROR,
            format!("{family} is not installed — a substitute is shown"),
        );
    }

    chosen
}

fn frame_section(ui: &mut Ui, frame: &tessera_document::nodes::Frame) {
    let tessera_document::nodes::FrameKind::Group(children) = &frame.kind else {
        return;
    };
    ui.colored_label(
        Theme::TEXT_MUTED,
        format!("{} objects grouped", children.len()),
    );
}

fn fill_picker(ui: &mut Ui, rgba: &mut [f32; 4]) -> bool {
    let mut colour = egui::Rgba::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    let changed = egui::widgets::color_picker::color_edit_button_rgba(
        ui,
        &mut colour,
        egui::widgets::color_picker::Alpha::Opaque,
    )
    .changed();
    if changed {
        *rgba = [colour.r(), colour.g(), colour.b(), colour.a()];
    }
    changed
}

// --- document setup ----------------------------------------------------

/// The inspector with nothing selected: the document's own properties.
///
/// InDesign shows the same thing in the same place, and it is the one part of
/// its Properties panel worth keeping wholesale — with nothing selected, the
/// document *is* the selection.
pub fn document_setup(ui: &mut Ui, state: &mut TesseraApp) {
    let unit = state.prefs.unit;
    let mut setup = state.active().document().setup;
    let page = state.first_page_bounds();
    let (mut width, mut height) = (page.width, page.height);

    ui.label("Page");

    // The preset names a pair of numbers the user recognises; the model still
    // stores only a width and a height. "Custom" is not a value — it is what
    // no preset matching looks like.
    let current = PagePreset::matching(width, height);
    let mut wanted = None;
    egui::ComboBox::from_id_salt("page-preset")
        .selected_text(current.map_or("Custom", PagePreset::name))
        .show_ui(ui, |ui| {
            for preset in PagePreset::ALL {
                if ui
                    .selectable_label(current == Some(preset), preset.name())
                    .clicked()
                {
                    wanted = Some(preset);
                }
            }
        });
    if let Some(preset) = wanted {
        // Applied in the orientation the page already has, so choosing A4 for
        // a landscape document does not silently turn it upright.
        let (w, h) = preset.size();
        let (w, h) = Orientation::of(width, height).apply(w, h);
        apply(
            state,
            Command::SetPageSize {
                width: w,
                height: h,
            },
        );
        return;
    }

    let orientation = Orientation::of(width, height);
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Orientation");
        for (label, which) in [
            ("Portrait", Orientation::Portrait),
            ("Landscape", Orientation::Landscape),
        ] {
            if ui.selectable_label(orientation == which, label).clicked() && orientation != which {
                let (w, h) = which.apply(width, height);
                wanted = None;
                apply(
                    state,
                    Command::SetPageSize {
                        width: w,
                        height: h,
                    },
                );
            }
        }
    });

    let mut resized = false;
    egui::Grid::new("page-size").num_columns(2).show(ui, |ui| {
        resized |= measure(ui, "W", &mut width, unit);
        resized |= measure(ui, "H", &mut height, unit);
        ui.end_row();
    });
    if resized {
        apply(state, Command::SetPageSize { width, height });
        return;
    }

    let mut changed = false;

    ui.add_space(Theme::SPACING_SM);
    changed |= ui
        .checkbox(&mut setup.facing_pages, "Facing pages")
        .changed();

    // The labels change with the binding, because the fields themselves mean
    // something different: with facing pages on, the wide margin is the one
    // against the spine and swaps sides between left-hand and right-hand
    // pages. Calling it "Left" then would be a lie on half the document.
    let (near, far) = if setup.facing_pages {
        ("Inside", "Outside")
    } else {
        ("Left", "Right")
    };

    // Four edges are two pairs, not four rows: top against bottom and one
    // side against the other are the comparisons a person actually makes.
    let edges = |ui: &mut Ui,
                 title: &str,
                 v: (&mut f64, &mut f64),
                 h: ((&str, &mut f64), (&str, &mut f64))| {
        ui.add_space(Theme::SPACE_3);
        group_label(ui, title);
        let (a, b) = pair(
            ui,
            ("Top", |ui: &mut Ui| measure_bare(ui, v.0, unit)),
            ("Bottom", |ui: &mut Ui| measure_bare(ui, v.1, unit)),
        );
        let (c, d) = pair(
            ui,
            (h.0.0, |ui: &mut Ui| measure_bare(ui, h.0.1, unit)),
            (h.1.0, |ui: &mut Ui| measure_bare(ui, h.1.1, unit)),
        );
        a || b || c || d
    };

    changed |= edges(
        ui,
        "Margins",
        (&mut setup.margins.top, &mut setup.margins.bottom),
        (
            (near, &mut setup.margins.inside),
            (far, &mut setup.margins.outside),
        ),
    );
    changed |= edges(
        ui,
        "Bleed",
        (&mut setup.bleed.top, &mut setup.bleed.bottom),
        (
            ("Left", &mut setup.bleed.left),
            ("Right", &mut setup.bleed.right),
        ),
    );
    // Column guides, next to the margins they subdivide.
    ui.add_space(Theme::SPACE_3);
    group_label(ui, "Columns");
    let mut count = f64::from(setup.columns.max(1));
    let (i, j) = pair(
        ui,
        ("Count", |ui: &mut Ui| {
            ui.add(
                egui::DragValue::new(&mut count)
                    .speed(0.1)
                    .range(1.0..=20.0),
            )
            .changed()
        }),
        ("Gutter", |ui: &mut Ui| {
            measure_bare(ui, &mut setup.column_gutter, unit)
        }),
    );
    if i {
        setup.columns = count.round().clamp(1.0, 20.0) as u8;
    }
    changed |= i || j;

    // The baseline grid, with the document's other page-wide rhythms.
    ui.add_space(Theme::SPACE_3);
    group_label(ui, "Baseline grid");
    let mut on = setup.baseline_grid.is_some();
    if ui.checkbox(&mut on, "Use a baseline grid").changed() {
        setup.baseline_grid = on.then_some(tessera_document::nodes::BaselineGrid {
            start: 0.0,
            // Twelve on twelve: a grid that matches the default leading, so
            // turning it on changes nothing until something is set against it.
            step: 12.0,
        });
        changed = true;
    }
    if let Some(mut grid) = setup.baseline_grid {
        let (g, h) = pair(
            ui,
            ("Start", |ui: &mut Ui| {
                measure_bare(ui, &mut grid.start, unit)
            }),
            ("Every", |ui: &mut Ui| {
                measure_bare(ui, &mut grid.step, unit)
            }),
        );
        if g || h {
            // A step of zero is a grid with every line in one place, which no
            // caller can use and the flow would have to guard against.
            grid.step = grid.step.max(0.1);
            setup.baseline_grid = Some(grid);
            changed = true;
        }
    }

    changed |= edges(
        ui,
        "Slug",
        (&mut setup.slug.top, &mut setup.slug.bottom),
        (
            ("Left", &mut setup.slug.left),
            ("Right", &mut setup.slug.right),
        ),
    );

    if changed {
        // One command for the whole struct: a page-setup edit is one undo
        // entry, not one per field touched.
        apply(state, Command::SetDocumentSetup(setup));
    }

    ui.add_space(Theme::SPACING_LG);
    ui.colored_label(
        Theme::TEXT_MUTED,
        format!("Measurements in {}", unit_name(unit)),
    );
}

fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::Millimetres => "millimetres",
        Unit::Points => "points",
        Unit::Pixels => "pixels",
        Unit::Inches => "inches",
        Unit::Picas => "picas",
    }
}

/// A numeric field holding a measurement.
///
/// The document stores points; this shows and edits the user's preferred unit
/// and converts at the edge, which is the only place a conversion belongs.
fn measure(ui: &mut Ui, label: &str, points: &mut f64, unit: Unit) -> bool {
    let mut shown = unit.from_points(*points);
    let changed = field(ui, label, |ui| {
        ui.add(
            egui::DragValue::new(&mut shown)
                .speed(0.25)
                // Typing `12mm` into a field showing points converts it.
                // This is D5: a unit is parsed, never moded, so the same
                // keystrokes never mean two different things.
                .custom_formatter(move |v, _| format!("{v:.2} {}", unit.suffix()))
                .custom_parser(move |text| {
                    Unit::parse_to_points(text, unit).map(|p| unit.from_points(p))
                }),
        )
        .changed()
    });
    if changed {
        *points = unit.to_points(shown);
    }
    changed
}

// --- status bar --------------------------------------------------------

/// The zoom levels the step buttons move between.
///
/// A ladder rather than a multiplier, so the steps land on the round numbers
/// a person names — 50%, 100%, 200% — instead of 70.7% and 141.4%.
const ZOOM_LADDER: [f64; 13] = [
    0.05, 0.10, 0.25, 0.50, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 16.0,
];

/// The next rung above or below `current`.
///
/// Clamped at both ends: at 1600% there is nowhere further to go, and
/// wrapping round to 5% would be a surprise rather than a convenience.
pub fn stepped_zoom(current: f64, up: bool) -> f64 {
    let last = ZOOM_LADDER[ZOOM_LADDER.len() - 1];
    if up {
        ZOOM_LADDER
            .iter()
            .find(|z| **z > current + 1e-9)
            .copied()
            .unwrap_or(last)
    } else {
        ZOOM_LADDER
            .iter()
            .rev()
            .find(|z| **z < current - 1e-9)
            .copied()
            .unwrap_or(ZOOM_LADDER[0])
    }
}

pub fn status_bar(ui: &mut Ui, state: &mut TesseraApp) {
    ui.horizontal(|ui| {
        match &state.status {
            Some(s) if s.is_error => ui.colored_label(Theme::ERROR, &s.message),
            Some(s) => ui.colored_label(Theme::TEXT_MUTED, &s.message),
            None => ui.colored_label(Theme::TEXT_MUTED, state.active_tool.label()),
        };

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut percent = state.active().view.zoom * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut percent)
                        .speed(1.0)
                        .range(5.0..=1600.0)
                        .suffix("%"),
                )
                .changed()
            {
                state.active_mut().view.zoom = percent / 100.0;
            }
            if glyph_button(ui, crate::icons::Icon::ZoomIn, "Zoom in").clicked() {
                let next = stepped_zoom(state.active().view.zoom, true);
                state.active_mut().view.zoom = next;
            }
            if glyph_button(ui, crate::icons::Icon::ZoomOut, "Zoom out").clicked() {
                let next = stepped_zoom(state.active().view.zoom, false);
                state.active_mut().view.zoom = next;
            }
            if ui
                .small_button("Fit")
                .on_hover_text("Zoom to fit")
                .clicked()
            {
                // The viewport fits the page whenever this is false, which is
                // the same path the very first frame takes.
                state.active_mut().fitted = false;
            }

            ui.separator();

            page_navigator(ui, state);
        });
    });
}

/// The two style pickers, and the way a style comes into existence.
///
/// A style is defined *from* what the panel is currently showing. That is the
/// only route that needs no dialog, and it is how a designer actually works:
/// set a paragraph until it looks right, then name it.
fn style_rows(ui: &mut Ui, state: &mut TesseraApp, story: StoryId, target: std::ops::Range<usize>) {
    let Some(current) = state.active().document().story(story).cloned() else {
        return;
    };
    let (character_style, _) = current.common_character_style(target.clone());
    let (paragraph_style, _) = current.common_paragraph_style(target.clone());

    let characters: Vec<(CharacterStyleId, String)> = state
        .active()
        .document()
        .character_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();
    let paragraphs: Vec<(ParagraphStyleId, String)> = state
        .active()
        .document()
        .paragraph_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();

    subheading(ui, crate::icons::Icon::Palette, "Styles");

    // --- paragraph styles

    let mut attach_paragraph = None;
    let mut define_paragraph = false;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Paragraph style");
        let label = paragraph_style
            .and_then(|id| paragraphs.iter().find(|(p, _)| *p == id))
            .map_or("None", |(_, name)| name.as_str())
            .to_string();
        egui::ComboBox::from_id_salt("paragraph-style")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(paragraph_style.is_none(), "None")
                    .clicked()
                {
                    attach_paragraph = Some(None);
                }
                for (id, name) in &paragraphs {
                    if ui
                        .selectable_label(paragraph_style == Some(*id), name)
                        .clicked()
                    {
                        attach_paragraph = Some(Some(*id));
                    }
                }
            });
        define_paragraph = icon_button(
            ui,
            crate::icons::Icon::Plus,
            "Define a paragraph style from what is shown",
            false,
        );
    });

    if let Some(style) = attach_paragraph {
        apply(
            state,
            Command::SetParagraphStyleOf {
                story,
                range: target.clone(),
                style,
            },
        );
    }
    if define_paragraph {
        let mut format = current.common_paragraph_format(target.clone());
        // The character half goes inside the paragraph format, which is where
        // the cascade reads it from.
        format.character = current.common_format(target.clone(), state.active().document());
        apply(
            state,
            Command::DefineParagraphStyle(ParagraphStyle {
                name: format!("Paragraph style {}", paragraphs.len() + 1),
                based_on: None,
                format,
            }),
        );
    }

    // --- character styles

    let mut attach_character = None;
    let mut define_character = false;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Character style");
        let label = character_style
            .and_then(|id| characters.iter().find(|(c, _)| *c == id))
            .map_or("None", |(_, name)| name.as_str())
            .to_string();
        egui::ComboBox::from_id_salt("character-style")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(character_style.is_none(), "None")
                    .clicked()
                {
                    attach_character = Some(None);
                }
                for (id, name) in &characters {
                    if ui
                        .selectable_label(character_style == Some(*id), name)
                        .clicked()
                    {
                        attach_character = Some(Some(*id));
                    }
                }
            });
        define_character = icon_button(
            ui,
            crate::icons::Icon::Plus,
            "Define a character style from what is shown",
            false,
        );
    });

    if let Some(style) = attach_character {
        apply(
            state,
            Command::SetCharacterStyleOf {
                story,
                range: target.clone(),
                style,
            },
        );
    }
    if define_character {
        let format = current.common_format(target.clone(), state.active().document());
        apply(
            state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: format!("Character style {}", characters.len() + 1),
                based_on: None,
                format,
            }),
        );
    }

    overrides_row(ui, state, story, target, character_style, paragraph_style);
}

/// What the text says over and above its styles, and the four things you can do
/// about it.
///
/// InDesign's `+`, Clear Overrides, Redefine Style and Break Link to Style. The
/// row is absent when there is nothing to say — no overrides and no attached
/// style means all four buttons would be no-ops.
fn overrides_row(
    ui: &mut Ui,
    state: &mut TesseraApp,
    story: StoryId,
    target: std::ops::Range<usize>,
    character_style: Option<CharacterStyleId>,
    paragraph_style: Option<ParagraphStyleId>,
) {
    let Some(current) = state.active().document().story(story).cloned() else {
        return;
    };
    let character_overrides = current.has_character_overrides(target.clone());
    let paragraph_overrides = current.has_paragraph_overrides(target.clone());
    let attached = character_style.is_some() || paragraph_style.is_some();

    if !character_overrides && !paragraph_overrides && !attached {
        return;
    }

    ui.add_space(Theme::SPACING_MD);
    if character_overrides || paragraph_overrides {
        ui.colored_label(
            Theme::ERROR,
            "+ this text states formatting of its own, over its styles",
        );
    }

    ui.horizontal_wrapped(|ui| {
        if character_overrides && ui.button("Clear character overrides").clicked() {
            apply(
                state,
                Command::ClearCharacterOverrides {
                    story,
                    range: target.clone(),
                },
            );
        }
        if paragraph_overrides && ui.button("Clear paragraph overrides").clicked() {
            apply(
                state,
                Command::ClearParagraphOverrides {
                    story,
                    range: target.clone(),
                },
            );
        }
    });

    ui.horizontal_wrapped(|ui| {
        // Redefine needs both a style to move and a difference to move it to.
        if character_overrides
            && let Some(id) = character_style
            && ui
                .button("Redefine character style")
                .on_hover_text("Make the style say what this text says")
                .clicked()
        {
            apply(
                state,
                Command::RedefineCharacterStyle {
                    id,
                    story,
                    range: target.clone(),
                },
            );
        }
        if (paragraph_overrides || character_overrides)
            && let Some(id) = paragraph_style
            && ui
                .button("Redefine paragraph style")
                .on_hover_text("Make the style say what this text says")
                .clicked()
        {
            apply(
                state,
                Command::RedefineParagraphStyle {
                    id,
                    story,
                    range: target.clone(),
                },
            );
        }
    });

    ui.horizontal_wrapped(|ui| {
        if character_style.is_some()
            && ui
                .button("Break character link")
                .on_hover_text("Detach the style, keeping how this looks")
                .clicked()
        {
            apply(
                state,
                Command::BreakCharacterStyleLink {
                    story,
                    range: target.clone(),
                },
            );
        }
        if paragraph_style.is_some()
            && ui
                .button("Break paragraph link")
                .on_hover_text("Detach the style, keeping how this looks")
                .clicked()
        {
            apply(
                state,
                Command::BreakParagraphStyleLink {
                    story,
                    range: target.clone(),
                },
            );
        }
    });
}

/// Which page the document is turned to.
///
/// The first page of the current spread, because a spread is what is looked at
/// and a page is what is operated on. `None` only for a document with no
/// spreads at all, which nothing can produce.
/// The top-left of the spread being looked at, which every measurement in the
/// interface is taken from.
///
/// The left-hand page of the spread, so that a two-page spread measures from
/// the outside edge of its verso rather than from the fold.
pub fn current_spread_origin(state: &TesseraApp) -> tessera_geometry::DocPoint {
    let doc = state.active().document();
    let bounds = current_page(state)
        .and_then(|p| doc.pages.get(p))
        .map(|p| p.bounds)
        .unwrap_or_else(|| doc.first_page_bounds());
    tessera_geometry::DocPoint {
        x: bounds.x,
        y: bounds.y,
    }
}

pub fn current_page(state: &TesseraApp) -> Option<tessera_document::ids::PageId> {
    let doc = state.active().document();
    let at = state
        .active()
        .current_spread
        .min(doc.spread_order.len().saturating_sub(1));
    doc.spread_order
        .get(at)
        .map(|s| doc.pages_of(*s))
        .and_then(|pages| pages.first().copied())
}

/// Previous, "3 of 12", next — the reading InDesign shows in the same corner.
///
/// Milestone 1.5 recorded this as partial: it could say how many pages there
/// were and not which one you were on, because there was only ever one.
fn page_navigator(ui: &mut Ui, state: &mut TesseraApp) {
    let spreads = state.active().document().spread_order.len();
    let pages = state.active().document().page_ids().count();
    // Clamped rather than trusted: removing the last spread leaves the index
    // pointing past the end until something moves it.
    let at = state.active().current_spread.min(spreads.saturating_sub(1));

    // Right to left in this layout, so the controls are built in reverse and
    // read forwards.
    if glyph_button(ui, crate::icons::Icon::ChevronRight, "Next spread").clicked()
        && at + 1 < spreads
    {
        state.active_mut().current_spread = at + 1;
        state.active_mut().fitted = false;
    }

    let first = state
        .active()
        .document()
        .spread_order
        .get(at)
        .map(|s| state.active().document().pages_of(*s))
        .and_then(|p| p.first().copied());
    let number = first
        .and_then(|page| state.active().document().page_ids().position(|p| p == page))
        .map_or(1, |i| i + 1);

    ui.colored_label(Theme::TEXT_MUTED, format!("{number} of {pages}"));

    if glyph_button(ui, crate::icons::Icon::ChevronLeft, "Previous spread").clicked() && at > 0 {
        state.active_mut().current_spread = at - 1;
        state.active_mut().fitted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::{Frame, FrameKind};
    use tessera_geometry::{DocRect, Transform};

    fn rect_frame() -> Frame {
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            transform: Transform::IDENTITY,
            kind: FrameKind::Rectangle,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
        }
    }

    #[test]
    fn zooming_in_lands_on_the_next_round_number() {
        assert_eq!(stepped_zoom(1.0, true), 1.5);
        assert_eq!(stepped_zoom(1.0, false), 0.75);
    }

    #[test]
    fn zoom_steps_stop_at_the_ends_rather_than_wrapping() {
        // At 1600% there is nowhere further to go, and jumping back to 5%
        // would be a surprise rather than a convenience.
        assert_eq!(stepped_zoom(16.0, true), 16.0);
        assert_eq!(stepped_zoom(0.05, false), 0.05);
    }

    #[test]
    fn a_zoom_between_two_rungs_moves_to_the_nearer_one_in_that_direction() {
        assert_eq!(stepped_zoom(1.2, true), 1.5);
        assert_eq!(stepped_zoom(1.2, false), 1.0);
    }

    #[test]
    fn the_chain_carries_a_width_change_across_to_the_height() {
        let (w, h) = constrained((100.0, 50.0), (200.0, 50.0), true);
        assert_eq!(
            (w, h),
            (200.0, 100.0),
            "doubling the width doubled the height"
        );
    }

    #[test]
    fn the_chain_carries_a_height_change_across_to_the_width() {
        let (w, h) = constrained((100.0, 50.0), (100.0, 25.0), false);
        assert_eq!((w, h), (50.0, 25.0), "halving the height halved the width");
    }

    #[test]
    fn the_chain_leaves_a_zero_side_alone_rather_than_collapsing_its_partner() {
        // A zero has no ratio. Carrying it across would silently destroy the
        // other dimension, and the object with it.
        let (w, h) = constrained((0.0, 50.0), (30.0, 50.0), true);
        assert_eq!((w, h), (30.0, 50.0));
    }

    #[test]
    fn the_chain_is_off_until_asked_for() {
        assert!(!TesseraApp::headless().constrain_proportions);
    }

    #[test]
    fn the_default_reference_point_is_the_centre() {
        // Scaling and rotating about the middle is what a user expects when
        // they have not said otherwise.
        assert_eq!(TesseraApp::headless().anchor, Anchor::Centre);
    }

    #[test]
    fn the_sections_that_can_be_absent_come_last() {
        // This is what makes D1 true. Hiding a section moves everything below
        // it, so the ones that apply to every frame must sit above the ones
        // that do not — then hiding never moves anything reached for often.
        let frame = rect_frame();
        let last_present = Section::ALL
            .iter()
            .rposition(|s| s.applies_to(&frame))
            .expect("some section applies");
        if let Some(first_absent) = Section::ALL.iter().position(|s| !s.applies_to(&frame)) {
            assert!(
                first_absent > last_present,
                "an absent section sits above a present one, so hiding it                  would move the present one"
            );
        }
    }

    #[test]
    fn transform_fill_and_stroke_apply_to_every_frame() {
        let frame = rect_frame();
        for section in [Section::Transform, Section::Fill, Section::Stroke] {
            assert!(section.applies_to(&frame), "{section:?} must never move");
        }
    }

    #[test]
    fn the_text_section_belongs_only_to_a_text_frame() {
        assert!(!Section::Text.applies_to(&rect_frame()));
    }

    #[test]
    fn the_frame_section_belongs_only_to_a_group() {
        assert!(!Section::Frame.applies_to(&rect_frame()));
    }

    #[test]
    fn every_section_has_a_title() {
        for section in Section::ALL {
            assert!(!section.title().is_empty(), "{section:?} has no title");
        }
    }

    // --- what the typography controls act on ----------------------------

    /// A text frame holding `text`, with nothing being edited.
    fn a_text_frame(text: &str) -> (TesseraApp, tessera_document::ids::FrameId, StoryId) {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        let FrameKind::Text { story, .. } =
            state.active().document().frame(id).expect("frame").kind
        else {
            panic!("a text frame shows a story");
        };
        (state, id, story)
    }

    #[test]
    fn with_no_caret_the_controls_act_on_the_whole_story() {
        // InDesign's rule, and what makes the section useful before a caret
        // exists: select the frame, set the family, all of the text follows.
        let (state, id, story) = a_text_frame("the quick brown fox");
        assert_eq!(format_target(&state, id, story), 0..19);
    }

    #[test]
    fn with_a_selection_the_controls_act_on_the_selection() {
        let (mut state, id, story) = a_text_frame("the quick brown fox");
        let mut buffer = tessera_text::edit::EditBuffer::new(
            state.active().document().story(story).cloned().unwrap(),
        );
        buffer.select(4..9);
        state.active_mut().editing = Some((id, buffer));

        assert_eq!(format_target(&state, id, story), 4..9);
    }

    #[test]
    fn a_caret_with_no_selection_is_not_the_whole_story() {
        // A caret is a caret. Treating it as "everything" would mean clicking
        // into a frame and nudging the size box restyled text the user never
        // selected.
        let (mut state, id, story) = a_text_frame("the quick brown fox");
        let mut buffer = tessera_text::edit::EditBuffer::new(
            state.active().document().story(story).cloned().unwrap(),
        );
        buffer.set_cursor(7);
        state.active_mut().editing = Some((id, buffer));

        assert_eq!(format_target(&state, id, story), 7..7);
    }

    #[test]
    fn a_caret_in_another_frame_does_not_narrow_this_ones_target() {
        let (mut state, first, first_story) = a_text_frame("the quick brown fox");
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let second = state.active().selection.single().expect("selected");
        let mut buffer = tessera_text::edit::EditBuffer::new(Default::default());
        buffer.select(0..0);
        state.active_mut().editing = Some((second, buffer));

        assert_eq!(
            format_target(&state, first, first_story),
            0..19,
            "the caret is elsewhere, so this frame formats whole"
        );
    }

    // --- page navigation ----------------------------------------------------

    #[test]
    fn turning_the_page_is_not_a_change_to_the_document() {
        // View state: where somebody is looking is not part of what they are
        // making, so it must not need saving or land in undo.
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddPage);
        state.active_mut().dirty = false;

        state.active_mut().current_spread = 1;

        assert!(!state.active().dirty);
    }

    #[test]
    fn the_current_page_follows_the_current_spread() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddPage);
        let second = state
            .active()
            .document()
            .page_ids()
            .nth(1)
            .expect("a second page");

        state.active_mut().current_spread = 1;
        assert_eq!(current_page(&state), Some(second));
    }

    #[test]
    fn a_current_spread_past_the_end_still_names_a_page() {
        // Removing the last spread leaves the index pointing past the end
        // until something moves it, and the status bar draws before anything
        // does.
        let mut state = TesseraApp::headless();
        state.active_mut().current_spread = 99;

        assert!(
            current_page(&state).is_some(),
            "it clamps rather than showing nothing"
        );
    }
}
