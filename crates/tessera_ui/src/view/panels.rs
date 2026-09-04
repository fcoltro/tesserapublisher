//! The tool strip, the inspector and the status bar.

use egui::{Sense, Ui, Vec2};
use tessera_color::Color;
use tessera_document::nodes::{Orientation, PagePreset};
use tessera_geometry::{Anchor, Unit};

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
    Text,
    Frame,
}

impl Section {
    /// Display order. Universal sections first; see the type's note.
    pub const ALL: [Section; 5] = [
        Section::Transform,
        Section::Fill,
        Section::Stroke,
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
        }
    }

    /// Whether this section says anything about `frame`.
    pub fn applies_to(self, frame: &tessera_document::nodes::Frame) -> bool {
        use tessera_document::nodes::FrameKind;
        match self {
            // Every frame has a place, a fill and a stroke — even when the
            // stroke is None, which is a value the section can set.
            Section::Transform | Section::Fill | Section::Stroke => true,
            Section::Text => matches!(frame.kind, FrameKind::Text { .. }),
            Section::Frame => matches!(frame.kind, FrameKind::Group(_)),
        }
    }
}

pub fn inspector(ui: &mut Ui, state: &mut TesseraApp) {
    ui.heading("Properties");
    ui.separator();

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
        ui.add_space(Theme::SPACING_MD);
        ui.label(section.title());
        match section {
            Section::Transform => transform_section(ui, state, id, &frame),
            Section::Fill => fill_section(ui, state, id, &frame),
            Section::Stroke => stroke_section(ui, state, id, &frame),
            Section::Text => text_section(ui, state, id, &frame),
            Section::Frame => frame_section(ui, &frame),
        }
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

    painter.rect_filled(fill_rect, 2.0, to_colour(&frame.fill));
    painter.rect_stroke(
        fill_rect,
        2.0,
        egui::Stroke::new(1.0, Theme::BORDER),
        egui::StrokeKind::Inside,
    );

    ui.horizontal(|ui| {
        if ui
            .small_button("⇄")
            .on_hover_text("Swap fill and stroke (X)")
            .clicked()
        {
            apply(state, Command::SwapFillAndStroke(id));
        }
        if ui.small_button("◼").on_hover_text("Defaults (D)").clicked() {
            apply(state, Command::DefaultFillAndStroke(id));
        }
        if ui.small_button("∅").on_hover_text("No fill (/)").clicked() {
            apply(state, Command::ClearFill(id));
        }
    });
    ui.add_space(Theme::SPACING_SM);
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

fn transform_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    // Position is asked in document space and size in the frame's own space,
    // which is what each of them means. `bounds` is the frame's own box, so it
    // answers W and H directly — but it does not move when the frame does,
    // because a move is a change of placement.
    ui.horizontal(|ui| {
        let mut anchor = state.anchor;
        if reference_proxy(ui, &mut anchor) {
            state.anchor = anchor;
        }
        ui.colored_label(
            Theme::TEXT_MUTED,
            "Reference
point",
        );
    });
    ui.add_space(Theme::SPACING_SM);

    let unit = state.prefs.unit;
    let origin = frame.corners()[0];
    let (mut x, mut y) = (origin.x, origin.y);
    let mut bounds = frame.bounds;
    let (was_w, was_h) = (bounds.width, bounds.height);
    let mut moved = false;
    let (mut w_changed, mut h_changed) = (false, false);

    egui::Grid::new("bounds").num_columns(2).show(ui, |ui| {
        moved |= measure(ui, "X", &mut x, unit);
        moved |= measure(ui, "Y", &mut y, unit);
        ui.end_row();
        w_changed = measure(ui, "W", &mut bounds.width, unit);
        h_changed = measure(ui, "H", &mut bounds.height, unit);
        ui.end_row();
    });

    let mut chain = state.constrain_proportions;
    if ui.checkbox(&mut chain, "Constrain proportions").changed() {
        state.constrain_proportions = chain;
    }

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

    // Scale, rotation and shear are all read from one decomposition and
    // written back as deltas about the reference point, so the fields, the
    // handles and the proxy mean one thing rather than three.
    let d = frame.transform.decompose();
    let anchor = state.anchor;

    let (mut sx, mut sy) = (d.scale_x * 100.0, d.scale_y * 100.0);
    let mut scaled = false;
    egui::Grid::new("scale").num_columns(2).show(ui, |ui| {
        scaled |= percent(ui, "Scale X", &mut sx);
        scaled |= percent(ui, "Scale Y", &mut sy);
        ui.end_row();
    });
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

    let mut rotation = d.rotation_degrees;
    if angle(ui, "Rotation", &mut rotation) {
        apply(
            state,
            Command::TransformAbout {
                id,
                anchor,
                scale: (1.0, 1.0),
                rotate: rotation - d.rotation_degrees,
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

/// A percentage field.
fn percent(ui: &mut Ui, label: &str, value: &mut f64) -> bool {
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, label);
        ui.add(
            egui::DragValue::new(value)
                .speed(0.5)
                .fixed_decimals(1)
                .suffix("%"),
        )
        .changed()
    })
    .inner
}

/// An angle field, in degrees.
fn angle(ui: &mut Ui, label: &str, value: &mut f64) -> bool {
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, label);
        ui.add(egui::DragValue::new(value).speed(0.5).suffix("°"))
            .changed()
    })
    .inner
}

fn fill_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    // Every frame has a fill, a text frame included — its background. The
    // previous arrangement showed this only for non-text frames, so a text
    // frame's own fill was unreachable.
    let [r, g, b, a] = frame.fill.to_rgb_f32();
    let mut rgba = [r, g, b, a];
    if fill_picker(ui, &mut rgba) {
        apply(
            state,
            Command::SetFill {
                id,
                color: Color::Rgb {
                    r: rgba[0],
                    g: rgba[1],
                    b: rgba[2],
                    a: rgba[3],
                },
            },
        );
    }
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

fn text_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    let tessera_document::nodes::FrameKind::Text { story } = &frame.kind else {
        return;
    };
    let mut text = state
        .active()
        .document()
        .story(*story)
        .map(|s| s.text.clone())
        .unwrap_or_default();
    if ui.text_edit_multiline(&mut text).changed() {
        apply(state, Command::SetText { id, text });
    }
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

    ui.add_space(Theme::SPACING_MD);
    ui.label("Margins");
    egui::Grid::new("margins").num_columns(2).show(ui, |ui| {
        changed |= measure(ui, "Top", &mut setup.margins.top, unit);
        changed |= measure(ui, "Bottom", &mut setup.margins.bottom, unit);
        ui.end_row();
        changed |= measure(ui, near, &mut setup.margins.inside, unit);
        changed |= measure(ui, far, &mut setup.margins.outside, unit);
        ui.end_row();
    });

    ui.add_space(Theme::SPACING_MD);
    ui.label("Bleed");
    egui::Grid::new("bleed").num_columns(2).show(ui, |ui| {
        changed |= measure(ui, "Top", &mut setup.bleed.top, unit);
        changed |= measure(ui, "Bottom", &mut setup.bleed.bottom, unit);
        ui.end_row();
        changed |= measure(ui, "Left", &mut setup.bleed.left, unit);
        changed |= measure(ui, "Right", &mut setup.bleed.right, unit);
        ui.end_row();
    });

    ui.add_space(Theme::SPACING_MD);
    ui.label("Slug");
    egui::Grid::new("slug").num_columns(2).show(ui, |ui| {
        changed |= measure(ui, "Top", &mut setup.slug.top, unit);
        changed |= measure(ui, "Bottom", &mut setup.slug.bottom, unit);
        ui.end_row();
        changed |= measure(ui, "Left", &mut setup.slug.left, unit);
        changed |= measure(ui, "Right", &mut setup.slug.right, unit);
        ui.end_row();
    });

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
    let changed = ui
        .horizontal(|ui| {
            ui.colored_label(Theme::TEXT_MUTED, label);
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
        })
        .inner;
    if changed {
        *points = unit.to_points(shown);
    }
    changed
}

// --- status bar --------------------------------------------------------

pub fn status_bar(ui: &mut Ui, state: &TesseraApp) {
    ui.horizontal(|ui| {
        match &state.status {
            Some(s) if s.is_error => ui.colored_label(Theme::ERROR, &s.message),
            Some(s) => ui.colored_label(Theme::TEXT_MUTED, &s.message),
            None => ui.colored_label(Theme::TEXT_MUTED, state.active_tool.label()),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.colored_label(
                Theme::TEXT_MUTED,
                format!("{:.0}%", state.active().view.zoom * 100.0),
            );
        });
    });
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
            fill: Color::BLACK,
            stroke: None,
        }
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
}
