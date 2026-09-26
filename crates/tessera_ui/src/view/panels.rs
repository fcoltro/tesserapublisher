//! The tool strip, the inspector and the status bar.

use egui::{Sense, Ui, Vec2};
use tessera_color::Color;
use tessera_document::ids::StoryId;
use tessera_document::nodes::{Orientation, PagePreset};
use tessera_document::paint::Paint;
use tessera_geometry::{Anchor, Unit};
use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyle, CharacterStyleId, ListFormat, ListKind,
    ParagraphFormat, ParagraphRule, ParagraphStyle, ParagraphStyleId,
};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;
use crate::tools::Tool;

// --- tool strip --------------------------------------------------------

pub fn tool_strip(ui: &mut Ui, state: &mut TesseraApp) {
    ui.vertical(|ui| {
        ui.add_space(Theme::space_1());
        for tool in Tool::ALL {
            if matches!(
                tool,
                Tool::Rectangle | Tool::Text | Tool::Scissors | Tool::Hand
            ) {
                ui.add_space(Theme::space_1());
                ui.separator();
                ui.add_space(Theme::space_1());
            }
            let shortcut = crate::actions::all()
                .iter()
                .find(|a| a.run == crate::actions::Run::PickTool(tool))
                .and_then(|a| state.prefs.shortcuts.chord(a))
                .map(|c| c.label());
            let name = shortcut.map_or_else(
                || tool.label().to_owned(),
                |chord| format!("{} ({chord})", tool.label()),
            );
            if tool_button(ui, tool, state.active_tool == tool, name).clicked() {
                crate::actions::run(state, crate::actions::Run::PickTool(tool));
            }
        }
    });
}

/// Icons come from Lucide, painted through `egui::Painter` from path data
/// rather than loaded as assets — so they stay crisp at any DPI and re-tint
/// with the theme. See [`crate::icons`].
fn tool_button(ui: &mut Ui, tool: Tool, active: bool, name: String) -> egui::Response {
    let size = Vec2::splat(Theme::TOOL_SIZE);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let bg = if active {
        Theme::accent_soft()
    } else if response.hovered() {
        Theme::hover_bg()
    } else {
        Theme::panel_bg()
    };
    let fg = if active || response.hovered() {
        Theme::text_primary()
    } else {
        Theme::text_muted()
    };

    ui.painter().rect_filled(rect, Theme::RADIUS, bg);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            Theme::RADIUS,
            egui::Stroke::new(1.5, Theme::text_primary()),
            egui::StrokeKind::Inside,
        );
    }
    crate::icons::paint(ui.painter(), rect, tool.icon(), fg);

    // Selected rather than merely labelled: which tool is *active* is the one
    // thing a strip of identical squares does not say out loud, and a person
    // choosing a tool needs to know they already have it.
    crate::icons::named_toggle(response, name, egui::WidgetType::RadioButton, active)
}

// --- inspector ---------------------------------------------------------

/// The inspector's sections, in the order they are drawn.
///
/// Content-specific controls come first: selecting text should expose type,
/// and selecting artwork should expose fitting. Shared appearance follows
/// in a consistent order, with occasional adjustments collapsed initially.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Transform,
    Style,
    Fill,
    Stroke,
    Corners,
    Effects,
    Text,
    Frame,
    Graphic,
    Wrap,
    /// A path's text, when it is a path.
    PathText,
}

impl Section {
    /// Display order, from the selected content to its appearance.
    pub const ALL: [Section; 11] = [
        Section::Text,
        Section::Graphic,
        Section::Transform,
        Section::Fill,
        Section::Stroke,
        Section::Style,
        Section::Corners,
        Section::Effects,
        Section::Wrap,
        Section::Frame,
        Section::PathText,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Transform => "Transform",
            Section::Fill => "Fill",
            Section::Stroke => "Stroke",
            Section::Corners => "Corners",
            Section::Text => "Text",
            Section::Frame => "Frame",
            Section::Wrap => "Text wrap",
            Section::Graphic => "Artwork",
            Section::Effects => "Effects",
            Section::Style => "Object style",
            Section::PathText => "Type on a path",
        }
    }

    /// Whether this section says anything about `frame`.
    pub fn applies_to(self, frame: &tessera_document::nodes::Frame) -> bool {
        use tessera_document::nodes::FrameKind;
        match self {
            // Every frame has a place, a fill and a stroke — even when the
            // stroke is None, which is a value the section can set.
            Section::Transform
            | Section::Fill
            | Section::Stroke
            | Section::Effects
            | Section::Style => true,
            Section::Text => matches!(frame.kind, FrameKind::Text { .. }),
            Section::Frame => matches!(frame.kind, FrameKind::Group(_)),
            // Rectangles and the frames that are rectangles. An ellipse has no
            // corners to cut, and a group is a box round other things rather
            // than a shape of its own \— offering the control there would ask a
            // question with no answer.
            Section::Corners => matches!(
                frame.kind,
                FrameKind::Rectangle | FrameKind::Text { .. } | FrameKind::Graphic { .. }
            ),
            // Every kind of object. A picture is the thing most often
            // wrapped, and it is the obstacle that carries the setting.
            Section::Wrap => true,
            Section::Graphic => matches!(frame.kind, FrameKind::Graphic { .. }),
            Section::PathText => matches!(frame.kind, FrameKind::Path(_)),
        }
    }
}

pub fn inspector(ui: &mut Ui, state: &mut TesseraApp) {
    // No heading: the rail draws one, and printing a second underneath it was
    // the word "Properties" twice in a column 292 points wide.
    if state.active().selection.is_empty() {
        context_heading(
            ui,
            "Document setup",
            "Page size, margins and output settings",
        );
        document_setup(ui, state);
        return;
    }

    // Geometry fields edit one frame. With several selected there is no single
    // value to show, and silently editing only the first would be worse than
    // saying so.
    let Some(id) = state.active().selection.single() else {
        crate::view::selection_panel::several(ui, state);
        return;
    };
    let Some(frame) = state.active().document().frame(id).cloned() else {
        ui.colored_label(Theme::text_muted(), "No selection");
        return;
    };

    crate::view::selection_panel::object_header(ui, state, id, &frame);
    // The header's quick actions can take the object away — deleted,
    // hidden, locked out of the selection — and the rest is about it.
    if state.active().document().frame(id).is_none()
        || state.active().selection.single() != Some(id)
    {
        return;
    }
    ui.horizontal(|ui| {
        fill_stroke_proxy(ui, state, id, &frame, 36.0);
        ui.label(
            egui::RichText::new("Fill & stroke")
                .small()
                .color(Theme::text_muted()),
        );
    });

    for section in Section::ALL {
        if !section.applies_to(&frame) {
            continue;
        }
        // Air above each heading: the band separates a section from the one
        // before it, and the space is what keeps the bands from reading as
        // a ladder of bars.
        ui.add_space(Theme::space_2());
        if !section_heading(ui, state, section.title()) {
            continue;
        }
        ui.add_space(Theme::space_1());
        if section == Section::Text {
            text_section(ui, state, id, &frame);
        } else {
            property_body(ui, |ui| match section {
                Section::Transform => transform_section(ui, state, id, &frame),
                Section::Fill => fill_section(ui, state, id, &frame),
                Section::Stroke => stroke_section(ui, state, id, &frame),
                Section::Corners => corners_section(ui, state, id, &frame),
                Section::Text => text_section(ui, state, id, &frame),
                Section::Frame => frame_section(ui, &frame),
                Section::Wrap => wrap_controls(ui, state, id, &frame),
                Section::Graphic => graphic_section(ui, state, id, &frame),
                Section::Effects => effects_section(ui, state, id, &frame),
                Section::Style => object_style_section(ui, state, id, &frame),
                Section::PathText => path_text_section(ui, state, id),
            });
        }
    }
}

/// What the panel is describing — "Text frame", "Document setup" — named as
/// InDesign names it at the head of its Properties panel. The note that says
/// where the rest lives is the name's tooltip rather than a paragraph under
/// it on every selection.
fn context_heading(ui: &mut Ui, title: &str, description: &str) {
    ui.add(egui::Label::new(egui::RichText::new(title).strong()).selectable(false))
        .on_hover_text(description);
    ui.add_space(Theme::space_1());
}

/// The control bar's appearance: the fill and stroke proxy, then the object's
/// opacity — what InDesign's control bar carries after the geometry, and what
/// LayoutPro's bar ends with. Edits go through the same commands as the
/// panel's, so the two cannot disagree about what a change means.
pub(crate) fn appearance_row(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    fill_stroke_proxy(ui, state, id, frame, crate::view::control::PROXY);
    crate::view::control::separator(ui);
    crate::view::control::glyph(ui, crate::icons::Icon::Opacity, "Opacity");
    let mut blend = frame.blend;
    let mut percent = blend.alpha() * 100.0;
    if crate::icons::speak_as(
        ui.add(
            egui::DragValue::new(&mut percent)
                .range(0.0..=100.0)
                .speed(0.5)
                .suffix("%")
                .fixed_decimals(0),
        ),
        "Opacity",
    )
    .changed()
    {
        blend.opacity = percent / 100.0;
        apply(state, Command::SetBlending { id, blend });
    }
}

/// The fill and stroke proxy: two overlapping swatches with their three keys.
///
/// The arrangement every drawing tool since MacDraw has used, and the one
/// place InDesign's design is worth copying exactly — a shape so familiar that
/// it needs no label.
///
/// `side` is the whole proxy's height: the panel has room for 36 points, the
/// control bar for its own row.
fn fill_stroke_proxy(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
    side: f32,
) {
    // The stroke square sits down and right of the fill by a little over a
    // third of its side, which is what reads as "behind" at any size.
    let offset = (side * 0.28).round();
    let swatch = side - offset;

    let (rect, _) = ui.allocate_exact_size(Vec2::splat(swatch + offset), Sense::hover());
    let painter = ui.painter();

    // Through the swatch table: a fill that names a swatch is the swatch's
    // colour, and drawn as the name it drew the magenta of an undefined one.
    let doc = state.active().document();
    let to_colour = |c: &Color| {
        let [r, g, b, a] = doc.resolve_colour(c).to_rgb_f32();
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
        egui::Rect::from_min_size(rect.min + Vec2::splat(offset), Vec2::splat(swatch));
    let fill_rect = egui::Rect::from_min_size(rect.min, Vec2::splat(swatch));

    let stroke_colour = frame
        .stroke
        .as_ref()
        .map_or(Theme::panel_bg(), |s| to_colour(&s.color));
    painter.rect_filled(stroke_rect, 2.0, Theme::panel_bg_solid());
    painter.rect_filled(stroke_rect, 2.0, stroke_colour);
    painter.rect_filled(stroke_rect.shrink(6.0), 1.0, Theme::panel_bg());
    painter.rect_stroke(
        stroke_rect,
        2.0,
        egui::Stroke::new(1.0, Theme::border()),
        egui::StrokeKind::Inside,
    );

    // One colour on a 26-point button. The section below draws the real
    // ramp, where there is room for it.
    // Opaque behind both swatches: a fill with alpha is judged against a known
    // ground, not against whatever the page happens to be showing.
    painter.rect_filled(fill_rect, 2.0, Theme::panel_bg_solid());
    painter.rect_filled(fill_rect, 2.0, to_colour(&frame.fill.representative()));
    painter.rect_stroke(
        fill_rect,
        2.0,
        egui::Stroke::new(1.0, Theme::border()),
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
    ui.add_space(Theme::space_1());
}

/// A small icon button, for the places a word would be worse than a picture.
fn glyph_button(ui: &mut Ui, icon: crate::icons::Icon, tip: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(Theme::control_height()), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
    }
    crate::icons::paint(ui.painter(), rect, icon, Theme::text_primary());
    crate::icons::named(response, tip)
}

/// The nine-point reference proxy, at a size the caller has room for.
///
/// **The size is asked for rather than assumed.** It was assumed once, at 45,
/// and the control bar holding it was shorter than that; a widget that draws
/// past the panel holding it does not overflow visibly — it is clipped. The
/// bottom row of points simply was not there, which reads as the proxy being
/// stuck rather than as the proxy being cut. The bar now derives its height
/// from the size it asks for (`control::HEIGHT`).
///
/// Returns whether the anchor changed.
pub fn reference_proxy_sized(ui: &mut Ui, anchor: &mut Anchor, side: f32) -> bool {
    let step = side / 3.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let mut changed = false;

    for (i, candidate) in Anchor::ALL.iter().enumerate() {
        let (col, row) = ((i % 3) as f32, (i / 3) as f32);
        let cell = egui::Rect::from_min_size(
            rect.min + Vec2::new(col * step, row * step),
            Vec2::splat(step),
        );
        let response = ui.interact(cell, ui.id().with(("anchor", i)), Sense::click());
        let response = crate::icons::reads_as(
            response,
            candidate.label(),
            egui::WidgetType::RadioButton,
            Some(*candidate == *anchor),
        );
        if response.clicked() {
            *anchor = *candidate;
            changed = true;
        }

        let selected = *candidate == *anchor;
        let colour = if selected {
            Theme::accent()
        } else if response.hovered() {
            Theme::text_primary()
        } else {
            Theme::text_muted()
        };
        // Scaled with the grid: a fixed 4-point dot in a small proxy is a
        // blob that touches its neighbours.
        let dot = step / if selected { 3.75 } else { 7.5 };
        ui.painter().circle_filled(cell.center(), dot, colour);
    }

    ui.painter().rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, Theme::border()),
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
    if reference_proxy_sized(ui, &mut anchor, crate::view::control::PROXY) {
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
    chain_button(ui, state);
    crate::view::control::separator(ui);

    let d = frame.transform.decompose();
    let mut rotation = d.rotation_degrees;
    let turned = angle_inline(ui, &mut rotation);

    apply_geometry(
        state,
        id,
        GeometryEdit {
            moved: moved.then_some((x - origin.x, y - origin.y)),
            resized: (w_changed || h_changed).then_some((was_w, was_h, bounds, w_changed)),
            turned: turned.then_some(rotation - d.rotation_degrees),
        },
    );
}

/// The chain between width and height, as a toggle.
fn chain_button(ui: &mut Ui, state: &mut TesseraApp) {
    let chain = state.constrain_proportions;
    if icon_button(
        ui,
        if chain {
            crate::icons::Icon::Link2
        } else {
            crate::icons::Icon::Unlink2
        },
        "Constrain proportions",
        chain,
    ) {
        state.constrain_proportions = !chain;
    }
}

/// What a geometry editor changed, read off its fields: a move by `(dx, dy)`,
/// a new size from `(was width, was height, bounds, width was the one edited)`,
/// a turn by so many degrees.
#[derive(Default)]
struct GeometryEdit {
    moved: Option<(f64, f64)>,
    resized: Option<(f64, f64, tessera_geometry::DocRect, bool)>,
    turned: Option<f64>,
}

/// Carry out a [`GeometryEdit`], the same way from the control bar and from
/// the panel: one place that knows a size under the chain keeps its
/// proportions, and that a turn is about the reference point.
fn apply_geometry(state: &mut TesseraApp, id: tessera_document::ids::FrameId, edit: GeometryEdit) {
    if let Some((dx, dy)) = edit.moved {
        apply(state, Command::TranslateSelection { dx, dy });
    }
    if let Some((was_w, was_h, mut bounds, w_changed)) = edit.resized {
        if state.constrain_proportions {
            let (w, h) = constrained((was_w, was_h), (bounds.width, bounds.height), w_changed);
            bounds.width = w;
            bounds.height = h;
        }
        apply(state, Command::SetBounds { id, bounds });
    }
    if let Some(degrees) = edit.turned {
        apply(
            state,
            Command::TransformAbout {
                id,
                anchor: state.anchor,
                scale: (1.0, 1.0),
                rotate: degrees,
                shear: 0.0,
            },
        );
    }
}

/// The panel's Transform: position, size and turn, then scale and shear.
fn transform_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    // Read from one decomposition and written back as deltas about the
    // reference point, so the fields, the handles and the proxy mean one
    // thing rather than three.
    // Position, size and turn at the head of the section, as InDesign's
    // Properties panel has them: the reference point on the left, X and Y,
    // then W and H with the chain. The control bar has the same fields; the
    // panel is where somebody who works from it looks for them, and a
    // Transform section that could scale and shear but not move was missing
    // its first line.
    let unit = state.prefs.unit;
    let d = frame.transform.decompose();
    let origin = frame.transform.apply(state.anchor.in_rect(frame.bounds));
    let (mut x, mut y) = (origin.x, origin.y);
    let mut bounds = frame.bounds;
    let (was_w, was_h) = (bounds.width, bounds.height);
    let mut anchor = state.anchor;
    let (mut moved, mut w_changed, mut h_changed) = (false, false, false);
    ui.horizontal(|ui| {
        let side = 2.0 * Theme::control_height() + ui.spacing().item_spacing.y;
        if reference_proxy_sized(ui, &mut anchor, side) {
            state.anchor = anchor;
        }
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                moved |= measure_inline(ui, "X", &mut x, unit);
                moved |= measure_inline(ui, "Y", &mut y, unit);
            });
            ui.horizontal_wrapped(|ui| {
                w_changed = measure_inline(ui, "W", &mut bounds.width, unit);
                h_changed = measure_inline(ui, "H", &mut bounds.height, unit);
                chain_button(ui, state);
            });
        });
    });
    let mut rotation = d.rotation_degrees;
    let turned = angle(ui, (crate::icons::Icon::Angle, "Rotation"), &mut rotation);
    if moved || w_changed || h_changed || turned {
        apply_geometry(
            state,
            id,
            GeometryEdit {
                moved: moved.then_some((x - origin.x, y - origin.y)),
                resized: (w_changed || h_changed).then_some((was_w, was_h, bounds, w_changed)),
                turned: turned.then_some(rotation - d.rotation_degrees),
            },
        );
        return;
    }
    let anchor = state.anchor;

    let (mut sx, mut sy) = (d.scale_x * 100.0, d.scale_y * 100.0);
    let (a, b) = pair(
        ui,
        ((crate::icons::Icon::ScaleX, "Scale X"), |ui: &mut Ui| {
            percent_bare(ui, &mut sx)
        }),
        ((crate::icons::Icon::ScaleY, "Scale Y"), |ui: &mut Ui| {
            percent_bare(ui, &mut sy)
        }),
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
    if angle(ui, (crate::icons::Icon::Shear, "Shear"), &mut shear) {
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
/// The control bar's counterpart to [`measure_bare`], which lays a label and a
/// field out as a grid row. Same parser, same formatter, same unit rule.
fn measure_inline(ui: &mut Ui, label: &str, points: &mut f64, unit: Unit) -> bool {
    crate::view::control::label(ui, label);
    // The letter is InDesign's and reads at a glance. Spoken, "W" is a letter
    // and not a field, so a screen reader is given the word.
    let spoken = match label {
        "W" => "Width",
        "H" => "Height",
        other => other,
    };
    let mut shown = unit.from_points(*points);
    let changed = crate::icons::speak_as(
        ui.add(
            egui::DragValue::new(&mut shown)
                .speed(0.25)
                .custom_formatter(move |v, _| format!("{v:.2} {}", unit.suffix()))
                .custom_parser(move |text| {
                    Unit::parse_to_points(text, unit).map(|p| unit.from_points(p))
                }),
        ),
        spoken,
    )
    .changed();
    if changed {
        *points = unit.to_points(shown);
    }
    changed
}

/// An angle in a row.
fn angle_inline(ui: &mut Ui, degrees: &mut f64) -> bool {
    crate::view::control::glyph(ui, crate::icons::Icon::Angle, "Rotation");
    crate::icons::speak_as(
        ui.add(egui::DragValue::new(degrees).speed(0.5).suffix("\u{00B0}")),
        "Rotation",
    )
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
    let common = match &state.active().editing {
        Some((editing, buffer)) if *editing == id && target.is_empty() => {
            buffer.pending().over(&common)
        }
        _ => common,
    };

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
    crate::view::control::glyph(ui, crate::icons::Icon::TypeSize, "Size");
    if crate::icons::speak_as(
        ui.add(
            egui::DragValue::new(&mut size)
                .speed(0.25)
                .range(0.1..=2000.0)
                .suffix(" pt"),
        ),
        "Size",
    )
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
    crate::view::control::glyph(ui, crate::icons::Icon::LineSpacing, "Leading");
    if crate::icons::speak_as(
        ui.add(
            egui::DragValue::new(&mut leading)
                .speed(0.02)
                .range(0.5..=4.0),
        ),
        "Leading",
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
    // The page being worked on, not the first: the bar's fields resize
    // **this page**, which is how a gatefold or a wider cover is made. The
    // document's size for every page is in Properties.
    let page = state.current_page();
    let bounds = page
        .and_then(|p| state.active().document().pages.get(p))
        .map_or_else(
            || state.active().document().first_page_bounds(),
            |p| p.bounds,
        );
    let unit = state.prefs.unit;

    let mut size = (bounds.width, bounds.height);
    let mut changed = measure_inline(ui, "W", &mut size.0, unit);
    changed |= measure_inline(ui, "H", &mut size.1, unit);
    if changed {
        match page {
            Some(page) => apply(
                state,
                Command::SetPageSizeOf {
                    page,
                    width: size.0,
                    height: size.1,
                },
            ),
            None => apply(
                state,
                Command::SetPageSize {
                    width: size.0,
                    height: size.1,
                },
            ),
        }
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

/// An angle field, in degrees.
fn angle<'a>(ui: &mut Ui, label: impl Into<FieldLabel<'a>>, value: &mut f64) -> bool {
    // An unsheared frame's shear comes out of its matrix as -0.0, and the
    // field would print the sign of nothing. `-0.0 == 0.0`, so this clears it.
    if *value == 0.0 {
        *value = 0.0;
    }
    property_field(ui, label, |ui| {
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
    //
    // None is among them because it is what a new shape has, and a panel
    // that called it "Solid" over a half-checkered swatch was describing the
    // storage rather than the page. The model keeps no-fill as a solid at no
    // alpha (`command::NO_FILL`), so it is read off the alpha here, and the
    // colour under it is what comes back when the fill is turned on.
    let unfilled = matches!(&frame.fill, Paint::Solid(c) if c.to_rgb_f32()[3] <= 0.0);
    let chosen = match &frame.fill {
        Paint::Solid(_) if unfilled => 0,
        Paint::Solid(_) => 1,
        Paint::Gradient(g) => match g.ramp {
            Ramp::Linear { .. } => 2,
            Ramp::Radial => 3,
        },
    };
    let mut want = chosen;
    segmented(
        ui,
        "Fill type",
        &mut want,
        &[("None", 0), ("Solid", 1), ("Linear", 2), ("Radial", 3)],
    );
    if want == 0 && chosen != 0 {
        apply(state, Command::ClearFill(id));
        return;
    }
    if want != chosen {
        // Switching keeps whatever the other kind can carry: a gradient turned
        // solid takes a colour from its ramp, and a solid turned gradient ramps
        // from the colour it already was rather than from an unrelated black.
        // Out of None, that colour comes back at full strength.
        let base = if unfilled {
            let [r, g, b, _] = frame.fill.representative().to_rgb_f32();
            Color::Rgb { r, g, b, a: 1.0 }
        } else {
            frame.fill.representative()
        };
        let paint = match want {
            1 => Paint::Solid(base),
            other => {
                let ramp = if other == 2 {
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
                                colour: base,
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
        // Nothing to colour: the type row above is the whole of it.
        Paint::Solid(_) if unfilled => {}
        Paint::Solid(colour) => {
            let [r, g, b, a] = colour.to_rgb_f32();
            let mut rgba = [r, g, b, a];
            if property_field(ui, "Fill colour", |ui| {
                fill_picker(ui, &mut rgba, "Fill colour")
            }) {
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
        changed |= property_field(ui, (crate::icons::Icon::Angle, "Angle"), |ui| {
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

    group_label(ui, "Gradient stops");
    let mut remove = None;
    for (index, stop) in stops.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.separator();
            ui.label(format!("Stop {}", index + 1));
            let mut rgba = stop.colour.to_rgb_f32();
            let name = format!("Stop {} colour", index + 1);
            if property_field(ui, "Colour", |ui| fill_picker(ui, &mut rgba, &name)) {
                stop.colour = Color::Rgb {
                    r: rgba[0],
                    g: rgba[1],
                    b: rgba[2],
                    a: rgba[3],
                };
                changed = true;
            }
            let mut percent = stop.at * 100.0;
            if property_field(ui, "Position along gradient", |ui| {
                ui.add(
                    egui::DragValue::new(&mut percent)
                        .speed(0.5)
                        .range(0.0..=100.0)
                        .suffix("%"),
                )
                .changed()
            }) {
                stop.at = percent / 100.0;
                changed = true;
            }
            if index >= 2
                && crate::view::panel_ui::action(ui, crate::icons::Icon::Trash, "Remove stop")
                    .clicked()
            {
                remove = Some(index);
            }
        });
    }

    if let Some(at) = remove {
        stops.remove(at);
        changed = true;
    }

    if crate::view::panel_ui::action(ui, crate::icons::Icon::Plus, "Add colour stop").clicked() {
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
    // Opaque behind the whole strip, before any band is drawn: a ramp is
    // judged by eye, and a stop carrying alpha needs a known ground under it.
    ui.painter().rect_filled(rect, 2.0, Theme::panel_bg_solid());

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
        egui::Stroke::new(1.0, Theme::border()),
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

/// How the corners are cut.
fn corners_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use tessera_document::corners::CornerShape;

    let mut corners = frame.corners;
    let mut changed = false;

    let options: Vec<_> = CornerShape::ALL
        .into_iter()
        .map(|shape| (shape.label(), shape))
        .collect();
    changed |= property_choice(ui, "Corner shape", &mut corners.shape, &options);

    // **One field while the corners agree, four when they do not.** Four
    // fields for the commonest case is three fields of noise; one field for a
    // frame with different corners would be a control that silently flattens
    // them the first time it is touched.
    let same = linked_group_heading(
        ui,
        egui::Id::new(("corner-link", state.active, id)),
        "Corner radii",
        corners.is_uniform(),
    );

    let unit = state.prefs.unit;
    if same {
        let mut radius = corners.radii[0];
        if property_field(ui, (crate::icons::Icon::CornerRadius, "Radius"), |ui| {
            measure_bare(ui, &mut radius, unit)
        }) {
            corners.radii = [radius; 4];
            changed = true;
        }
    } else {
        // One local per corner rather than indexing the array inside two
        // closures: `pair` takes both halves at once, and two closures cannot
        // hold the same array mutably.
        let (mut tl, mut tr) = (corners.radii[0], corners.radii[1]);
        let (mut br, mut bl) = (corners.radii[2], corners.radii[3]);

        let (a, b) = pair(
            ui,
            (
                (crate::icons::Icon::CornerTopLeft, "Top left"),
                |ui: &mut Ui| measure_bare(ui, &mut tl, unit),
            ),
            (
                (crate::icons::Icon::CornerTopRight, "Top right"),
                |ui: &mut Ui| measure_bare(ui, &mut tr, unit),
            ),
        );
        let (c, d) = pair(
            ui,
            (
                (crate::icons::Icon::CornerBottomLeft, "Bottom left"),
                |ui: &mut Ui| measure_bare(ui, &mut bl, unit),
            ),
            (
                (crate::icons::Icon::CornerBottomRight, "Bottom right"),
                |ui: &mut Ui| measure_bare(ui, &mut br, unit),
            ),
        );
        if a || b || c || d {
            corners.radii = [tl, tr, br, bl];
            changed = true;
        }
    }

    // Said only when it is actually happening. A permanent note about a limit
    // nobody has reached is a line of chrome that teaches people to stop
    // reading notes.
    let fitted = corners.effective(frame.bounds);
    if fitted != corners.radii.map(|r| r.max(0.0)) {
        note_line(
            ui,
            "Trimmed to half the shorter side. The number is kept, so making \
             the frame bigger brings the corner back.",
        );
    }

    if changed {
        apply(state, Command::SetCorners { id, corners });
    }
}

/// A quiet line of explanation under a control.
fn note_line(ui: &mut Ui, text: &str) {
    ui.colored_label(Theme::text_muted(), text);
}

/// Type on a path: put a story on the path, say where along it the text
/// runs and how it sits, open the words in the story editor, or take the
/// text off. The words themselves are edited in the story editor rather
/// than on the curve — a caret that follows a circle is a gesture this
/// does not have yet, and a plain box over the story is honest.
fn path_text_section(ui: &mut Ui, state: &mut TesseraApp, id: tessera_document::ids::FrameId) {
    use tessera_document::path_text::PathTextAlign;

    let Some(current) = state.active().document().path_text(id).copied() else {
        note_line(ui, "The path carries no text.");
        if crate::view::panel_ui::action(ui, crate::icons::Icon::Plus, "Add text to path").clicked()
        {
            put_text_on_path(state, id);
        }
        return;
    };
    let mut edited = current;
    let mut changed = false;

    let mut start = (current.start * 100.0) as f32;
    let mut end = (current.end * 100.0) as f32;
    let (from, to) = pair(
        ui,
        ("Start position", |ui: &mut Ui| {
            percent_of(ui, &mut start, 0.0..=100.0)
        }),
        ("End position", |ui: &mut Ui| {
            percent_of(ui, &mut end, 0.0..=100.0)
        }),
    );
    if from {
        edited.start = f64::from(start) / 100.0;
    }
    if to {
        edited.end = f64::from(end) / 100.0;
    }
    changed |= from || to;
    changed |= property_choice(
        ui,
        "Align text to path",
        &mut edited.align,
        &[
            ("Baseline", PathTextAlign::Baseline),
            ("Centre", PathTextAlign::Centre),
            ("Ascender", PathTextAlign::Ascender),
            ("Descender", PathTextAlign::Descender),
        ],
    );

    let mut flip = current.flip;
    if ui
        .checkbox(&mut flip, "Flip text direction")
        .on_hover_text("Run the other way, on the other side of the path")
        .changed()
    {
        edited.flip = flip;
        changed = true;
    }

    if changed {
        apply(
            state,
            Command::SetPathText {
                id,
                text: Some(edited),
            },
        );
    }

    ui.vertical(|ui| {
        if crate::view::panel_ui::action(ui, crate::icons::Icon::TextCursor, "Edit text…").clicked()
        {
            let mut window = std::mem::take(&mut state.story_editor);
            window.open(state);
            state.story_editor = window;
        }
        if crate::view::panel_ui::action(ui, crate::icons::Icon::Trash, "Remove path text")
            .clicked()
        {
            apply(state, Command::SetPathText { id, text: None });
        }
    });
}

/// Put a story on the path frame `id` and open its words for editing —
/// or, if it already carries one, just open the words.
pub(crate) fn put_text_on_path(state: &mut TesseraApp, id: tessera_document::ids::FrameId) {
    if state.active().document().path_text(id).is_none() {
        apply(
            state,
            Command::PutTextOnPath {
                id,
                text: "Type on a path".to_string(),
            },
        );
    }
    let mut window = std::mem::take(&mut state.story_editor);
    window.open(state);
    state.story_editor = window;
}

fn stroke_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    use crate::icons::Icon;
    use tessera_document::nodes::{LineCap, LineJoin, Stroke, StrokeAlign};

    let mut on = frame.stroke.is_some();
    if ui.checkbox(&mut on, "Enable stroke").changed() {
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

    // Weight and colour on one row, as InDesign's Appearance row has them:
    // two things that describe one line, read together.
    let [r, g, b, a] = stroke.color.to_rgb_f32();
    let mut rgba = [r, g, b, a];
    let recoloured = property_field(ui, (Icon::StrokeWeight, "Stroke weight"), |ui| {
        ui.horizontal(|ui| {
            let swatch = 2.0 * Theme::control_height();
            ui.spacing_mut().interact_size.x =
                (ui.available_width() - swatch - ui.spacing().item_spacing.x).max(VALUE_BOX);
            // Two fields in the row, so the row's name cannot go to its first
            // widget alone: the weight takes it and the swatch has its own.
            let weight = ui.next_auto_id();
            measure_bare(ui, &mut stroke.width, unit);
            crate::icons::speak_field_as(ui.ctx(), weight, "Stroke weight");
            fill_picker(ui, &mut rgba, "Stroke colour")
        })
        .inner
    });
    if recoloured {
        stroke.color = Color::Rgb {
            r: rgba[0],
            g: rgba[1],
            b: rgba[2],
            a: rgba[3],
        };
    }

    // Alignment is the one stroke property that changes geometry rather than
    // appearance, which is why the model carries it and why it sits first.
    segmented(
        ui,
        "Stroke alignment",
        &mut stroke.align,
        &[
            ("Centre", StrokeAlign::Center),
            ("Inside", StrokeAlign::Inside),
            ("Outside", StrokeAlign::Outside),
        ],
    );

    icon_choices(
        ui,
        "Line ends",
        &mut stroke.cap,
        &[
            (
                Icon::CapButt,
                "Butt cap",
                "Flat end at the path endpoint",
                LineCap::Butt,
            ),
            (
                Icon::CapRound,
                "Round cap",
                "Rounded end extending beyond the endpoint",
                LineCap::Round,
            ),
            (
                Icon::CapSquare,
                "Projecting square cap",
                "Square end extending beyond the endpoint",
                LineCap::Square,
            ),
        ],
    );

    icon_choices(
        ui,
        "Line joins",
        &mut stroke.join,
        &[
            (
                Icon::JoinMiter,
                "Miter join",
                "Sharp corner where the strokes meet",
                LineJoin::Miter,
            ),
            (
                Icon::JoinRound,
                "Round join",
                "Rounded corner where the strokes meet",
                LineJoin::Round,
            ),
            (
                Icon::JoinBevel,
                "Bevel join",
                "Flat diagonal corner where the strokes meet",
                LineJoin::Bevel,
            ),
        ],
    );

    // Shown only when it means something. A miter limit on a rounded join is
    // a control that does nothing, which is worse than one that is absent.
    if stroke.join == LineJoin::Miter {
        property_field(ui, "Miter limit", |ui| {
            ui.add(
                egui::DragValue::new(&mut stroke.miter_limit)
                    .speed(0.1)
                    .range(1.0..=100.0),
            );
        });
    }

    let mut pattern = DASH_PRESETS.iter().position(|(_, dashes)| {
        let scaled: Vec<_> = dashes.iter().map(|d| d * stroke.width.max(0.1)).collect();
        dashes_match(&stroke.dashes, &scaled)
    });
    if icon_choices(
        ui,
        "Line pattern",
        &mut pattern,
        &[
            (
                Icon::StrokeSolid,
                "Solid stroke",
                "Continuous line",
                Some(0),
            ),
            (
                Icon::StrokeDashed,
                "Dashed stroke",
                "Repeating dashes",
                Some(1),
            ),
            (
                Icon::StrokeDotted,
                "Dotted stroke",
                "Repeating dots; round caps give round dots",
                Some(2),
            ),
        ],
    ) && let Some(index) = pattern
    {
        stroke.dashes = DASH_PRESETS[index]
            .1
            .iter()
            .map(|d| d * stroke.width.max(0.1))
            .collect();
        // Zero-length dashes only draw as dots when they have round caps.
        if index == 2 {
            stroke.cap = LineCap::Round;
        }
    }
    if pattern.is_none() {
        crate::view::panel_ui::hint(ui, "Custom dash pattern");
    }

    if stroke.is_dashed() {
        property_field(ui, "Dash offset", |ui| {
            measure_bare(ui, &mut stroke.dash_offset, unit)
        });
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
    let size = Vec2::splat(Theme::control_height());
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if active || response.hovered() {
        ui.painter().rect_filled(
            rect,
            Theme::RADIUS,
            if active {
                Theme::selected_bg()
            } else {
                Theme::hover_bg()
            },
        );
    }
    let tint = if active {
        Theme::text_primary()
    } else {
        Theme::text_muted()
    };
    crate::icons::paint(ui.painter(), rect, icon, tint);

    crate::icons::named_toggle(response, tooltip, egui::WidgetType::Button, active).clicked()
}

/// [`group_label`], for another module in the view.
pub(crate) fn group_label_pub(ui: &mut Ui, text: &str) {
    group_label(ui, text);
}

/// One paragraph rule, above or below: on or off, and when on, its weight,
/// offset, width, indents and colour. Returns whether anything changed.
///
/// `inheritable` offers "Inherit" — `None` — which only a style can mean; see
/// [`tab_stops_editor`] for why the inspector never shows it.
pub(crate) fn paragraph_rule_editor(
    ui: &mut Ui,
    label: &str,
    rule: &mut Option<ParagraphRule>,
    inheritable: bool,
) -> bool {
    use tessera_text::story::RuleWidth;

    let mut changed = false;
    let mut inherit = false;
    let on = rule.as_ref().is_some_and(|r| r.on);

    ui.horizontal(|ui| {
        ui.colored_label(Theme::text_muted(), label);
        let response = ui.selectable_label(on, if on { "On" } else { "Off" });
        if crate::icons::named_toggle(response, label, egui::WidgetType::Checkbox, on).clicked() {
            match rule.as_mut() {
                // Keep the settings: off and back on should not mean set up
                // again.
                Some(r) => r.on = !r.on,
                None => *rule = Some(ParagraphRule::default()),
            }
            changed = true;
        }
        if inheritable && rule.is_some() && ui.small_button("Inherit").clicked() {
            inherit = true;
        }
    });
    if inherit {
        *rule = None;
        return true;
    }
    let Some(r) = rule.as_mut().filter(|r| r.on) else {
        return changed;
    };

    let number =
        |ui: &mut Ui, value: &mut f32, speed: f64, range: std::ops::RangeInclusive<f64>| {
            let mut edited = f64::from(*value);
            if ui
                .add(
                    egui::DragValue::new(&mut edited)
                        .speed(speed)
                        .range(range)
                        .custom_formatter(|v, _| format!("{v:.2} pt")),
                )
                .changed()
            {
                *value = edited as f32;
                true
            } else {
                false
            }
        };
    let (a, b) = pair(
        ui,
        ("Weight", |ui: &mut Ui| {
            number(ui, &mut r.weight, 0.25, 0.0..=100.0)
        }),
        ("Offset", |ui: &mut Ui| {
            number(ui, &mut r.offset, 0.25, -100.0..=100.0)
        }),
    );
    changed |= a || b;
    let (a, b) = pair(
        ui,
        ("Left", |ui: &mut Ui| {
            number(ui, &mut r.indent_left, 0.25, -720.0..=720.0)
        }),
        ("Right", |ui: &mut Ui| {
            number(ui, &mut r.indent_right, 0.25, -720.0..=720.0)
        }),
    );
    changed |= a || b;

    field(ui, "Width", |ui| {
        for (width, text) in [(RuleWidth::Column, "Column"), (RuleWidth::Text, "Text")] {
            if ui.selectable_label(r.width == width, text).clicked() && r.width != width {
                r.width = width;
                changed = true;
            }
        }
    });

    // The colour: the text's own, or one of the rule's. Shown as the text's
    // black when it has none, and set the moment the picker moves.
    field(ui, "Colour", |ui| {
        let own = r.colour.is_some();
        if ui
            .selectable_label(!own, "Text")
            .on_hover_text("The colour of the text it belongs to")
            .clicked()
            && own
        {
            r.colour = None;
            changed = true;
        }
        let [cr, cg, cb, ca] = r.colour.clone().unwrap_or(Color::BLACK).to_rgb_f32();
        let mut rgba = [cr, cg, cb, ca];
        if fill_picker(ui, &mut rgba, "Rule colour") {
            r.colour = Some(Color::Rgb {
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: rgba[3],
            });
            changed = true;
        }
    });

    changed
}

/// Stylistic set numbers out of whatever somebody typed: "1 3, 7" is sets
/// one, three and seven. Out-of-range numbers are dropped, because a set
/// that no font can have is not one to keep.
pub(crate) fn parse_sets(text: &str) -> Vec<u8> {
    let mut sets: Vec<u8> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=20).contains(n))
        .collect();
    sets.sort_unstable();
    sets.dedup();
    sets
}

/// A paragraph as a list item: none, a bullet or a number, and the shape of
/// the marker. Returns `(changed, hang)`: whether the list changed, and
/// whether a hanging indent was asked for — which is the caller's to write,
/// because it is three other fields.
///
/// `inheritable` offers "Inherit" — `None` — which only a style can mean.
pub(crate) fn list_editor(
    ui: &mut Ui,
    list: &mut Option<ListFormat>,
    inheritable: bool,
) -> (bool, bool) {
    use tessera_text::story::Numbering;

    let mut changed = false;
    let mut inherit = false;
    let mut hang = false;
    group_label(ui, "List");
    let stated = list.is_some();
    let l = list.get_or_insert_with(|| ListFormat {
        kind: ListKind::None,
        ..ListFormat::default()
    });

    text_field(ui, crate::icons::Icon::List, "List type", |ui| {
        ui.horizontal_wrapped(|ui| {
            for (kind, text) in [
                (ListKind::None, "None"),
                (ListKind::Bullet, "Bullet"),
                (ListKind::Number, "Number"),
            ] {
                if ui.selectable_label(l.kind == kind, text).clicked() && l.kind != kind {
                    l.kind = kind;
                    changed = true;
                }
            }
            if inheritable && stated && ui.small_button("Inherit").clicked() {
                inherit = true;
            }
        });
    });

    match l.kind {
        ListKind::None => {}
        ListKind::Bullet => {
            field(ui, "Bullet", |ui| {
                let mut bullet = l.bullet.to_string();
                let response = ui.add(egui::TextEdit::singleline(&mut bullet).desired_width(24.0));
                crate::icons::named(response.clone(), "Bullet character");
                if response.changed()
                    && let Some(ch) = bullet.chars().last()
                    && ch != l.bullet
                {
                    l.bullet = ch;
                    changed = true;
                }
                for (ch, name) in [
                    ('\u{2022}', "Bullet"),
                    ('\u{2013}', "En dash"),
                    ('\u{25E6}', "White bullet"),
                ] {
                    if ui
                        .selectable_label(l.bullet == ch, ch.to_string())
                        .on_hover_text(name)
                        .clicked()
                        && l.bullet != ch
                    {
                        l.bullet = ch;
                        changed = true;
                    }
                }
            });
        }
        ListKind::Number => {
            let numberings = [
                (Numbering::Arabic, "1, 2, 3"),
                (Numbering::LowerAlpha, "a, b, c"),
                (Numbering::UpperAlpha, "A, B, C"),
                (Numbering::LowerRoman, "i, ii, iii"),
                (Numbering::UpperRoman, "I, II, III"),
            ];
            text_field(ui, crate::icons::Icon::List, "Number format", |ui| {
                let shown = numberings
                    .iter()
                    .find(|(n, _)| *n == l.numbering)
                    .map_or("1, 2, 3", |(_, label)| *label);
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt("list-numbering")
                        .width(ui.available_width())
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (numbering, label) in numberings {
                                if ui
                                    .selectable_label(l.numbering == numbering, label)
                                    .clicked()
                                    && l.numbering != numbering
                                {
                                    l.numbering = numbering;
                                    changed = true;
                                }
                            }
                        })
                        .response,
                    "Numbering",
                    egui::WidgetType::ComboBox,
                    None,
                );
            });
            field(ui, "Suffix", |ui| {
                let mut suffix = l.suffix.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut suffix)
                        .desired_width(24.0)
                        .hint_text("."),
                );
                crate::icons::named(response.clone(), "After the number");
                if response.changed() && suffix != l.suffix {
                    l.suffix = suffix;
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                let response = ui.selectable_label(l.restart, "Restart at 1");
                if crate::icons::named_toggle(
                    response,
                    "Restart numbering at this paragraph",
                    egui::WidgetType::Checkbox,
                    l.restart,
                )
                .clicked()
                {
                    l.restart = !l.restart;
                    changed = true;
                }
            });
        }
    }
    if l.kind != ListKind::None
        && !inheritable
        && ui
            .small_button("Apply hanging indent")
            .on_hover_text("Left indent 18 pt, first line \u{2212}18 pt, a stop at 18 pt")
            .clicked()
    {
        hang = true;
    }

    if inherit {
        *list = None;
        return (true, hang);
    }
    if inheritable && !stated && !changed {
        *list = None;
    }
    (changed, hang)
}

/// A percentage field: a drag value with a `%` suffix. Returns whether it
/// changed.
fn percent_of(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f64>) -> bool {
    let mut edited = f64::from(*value);
    if ui
        .add(
            egui::DragValue::new(&mut edited)
                .speed(1.0)
                .range(range)
                .custom_formatter(|v, _| format!("{v:.0}%")),
        )
        .changed()
    {
        *value = edited as f32;
        true
    } else {
        false
    }
}

/// InDesign's Justification dialog, in two rows: how far the spaces and the
/// letters of a justified line may be squeezed or stretched. Returns whether
/// anything changed.
pub(crate) fn justification_editor(
    ui: &mut Ui,
    value: &mut Option<tessera_text::story::Justification>,
    inheritable: bool,
) -> bool {
    let mut changed = false;
    let mut inherit = false;
    let stated = value.is_some();
    let j = value.get_or_insert_with(Default::default);

    if inheritable && stated && ui.small_button("Inherit").clicked() {
        inherit = true;
    }
    for (label, values, range) in [
        (
            "Word spacing",
            [&mut j.word_min, &mut j.word_desired, &mut j.word_max],
            0.0..=1000.0,
        ),
        (
            "Letter spacing",
            [&mut j.letter_min, &mut j.letter_desired, &mut j.letter_max],
            -100.0..=500.0,
        ),
        (
            "Glyph width",
            [&mut j.glyph_min, &mut j.glyph_desired, &mut j.glyph_max],
            50.0..=200.0,
        ),
    ] {
        ui.label(label);
        ui.columns(3, |columns| {
            for ((column, value), name) in columns
                .iter_mut()
                .zip(values)
                .zip(["Min", "Desired", "Max"])
            {
                column.label(egui::RichText::new(name).small().color(Theme::text_muted()));
                column.spacing_mut().interact_size.x = column.available_width();
                changed |= percent_of(column, value, range.clone());
            }
        });
    }
    // Kept in order: a minimum above its maximum is not a setting anyone
    // means, and the breaker would only refuse to squeeze.
    if changed {
        j.word_min = j.word_min.min(j.word_desired);
        j.word_max = j.word_max.max(j.word_desired);
        j.letter_min = j.letter_min.min(j.letter_desired);
        j.letter_max = j.letter_max.max(j.letter_desired);
        j.glyph_min = j.glyph_min.min(j.glyph_desired);
        j.glyph_max = j.glyph_max.max(j.glyph_desired);
    }

    if inherit {
        *value = None;
        return true;
    }
    if inheritable && !stated && !changed {
        *value = None;
    }
    changed
}

/// Where a word may be hyphenated: the counts InDesign's Hyphenation dialog
/// has. Returns whether anything changed.
pub(crate) fn hyphenation_editor(
    ui: &mut Ui,
    value: &mut Option<tessera_text::story::Hyphenation>,
    inheritable: bool,
) -> bool {
    let mut changed = false;
    let mut inherit = false;
    let stated = value.is_some();
    group_label(ui, "Hyphenation");
    let h = value.get_or_insert_with(Default::default);

    let count = |ui: &mut Ui, value: &mut u8, range: std::ops::RangeInclusive<f64>| {
        let mut edited = f64::from(*value);
        if ui
            .add(egui::DragValue::new(&mut edited).speed(0.1).range(range))
            .changed()
        {
            *value = edited.round() as u8;
            true
        } else {
            false
        }
    };
    let (a, b) = pair(
        ui,
        ("Words of", |ui: &mut Ui| {
            count(ui, &mut h.min_word, 2.0..=25.0)
        }),
        ("Before", |ui: &mut Ui| {
            count(ui, &mut h.min_before, 1.0..=15.0)
        }),
    );
    changed |= a || b;
    let (a, b) = pair(
        ui,
        ("After", |ui: &mut Ui| {
            count(ui, &mut h.min_after, 1.0..=15.0)
        }),
        ("In a row", |ui: &mut Ui| {
            count(ui, &mut h.limit, 0.0..=25.0)
        }),
    );
    changed |= a || b;
    ui.horizontal(|ui| {
        let response = ui.selectable_label(h.capitalised, "Capitalised words");
        if crate::icons::named_toggle(
            response.on_hover_text("Whether a word beginning with a capital may be broken"),
            "Hyphenate capitalised words",
            egui::WidgetType::Checkbox,
            h.capitalised,
        )
        .clicked()
        {
            h.capitalised = !h.capitalised;
            changed = true;
        }
        if inheritable && stated && ui.small_button("Inherit").clicked() {
            inherit = true;
        }
    });

    if inherit {
        *value = None;
        return true;
    }
    if inheritable && !stated && !changed {
        *value = None;
    }
    changed
}

/// What a column break may not part: the paragraph from the next one, or its
/// own lines from each other. Returns whether anything changed.
///
/// `inheritable` offers "Inherit" — `None` — which only a style can mean.
pub(crate) fn keep_options_editor(
    ui: &mut Ui,
    keep: &mut Option<tessera_text::story::KeepOptions>,
    inheritable: bool,
) -> bool {
    use tessera_text::story::KeepTogether;

    let mut changed = false;
    let mut inherit = false;
    group_label(ui, "Keep");
    let stated = keep.is_some();
    let k = keep.get_or_insert_with(Default::default);

    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut k.with_next, "Keep with next paragraph")
            .on_hover_text("Keep the last line with the next paragraph")
            .changed()
        {
            changed = true;
        }
        if inheritable && stated && ui.small_button("Inherit").clicked() {
            inherit = true;
        }
    });

    text_field(ui, crate::icons::Icon::Link2, "Keep lines together", |ui| {
        let all = matches!(k.together, KeepTogether::All);
        let ends = matches!(k.together, KeepTogether::Ends { .. });
        let choices = [
            (KeepTogether::Off, "Off", !all && !ends),
            (KeepTogether::All, "All lines", all),
            (
                KeepTogether::Ends { start: 2, end: 2 },
                "First and last lines",
                ends,
            ),
        ];
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("keep-lines")
                .width(ui.available_width())
                .selected_text(if all {
                    "All lines"
                } else if ends {
                    "First and last lines"
                } else {
                    "Off"
                })
                .show_ui(ui, |ui| {
                    for (choice, text, selected) in choices {
                        if ui.selectable_label(selected, text).clicked() && !selected {
                            k.together = choice;
                            changed = true;
                        }
                    }
                })
                .response,
            "Keep lines together",
            egui::WidgetType::ComboBox,
            None,
        );
    });
    if let KeepTogether::Ends { start, end } = &mut k.together {
        let count = |ui: &mut Ui, value: &mut u8| {
            let mut edited = f64::from(*value);
            if ui
                .add(
                    egui::DragValue::new(&mut edited)
                        .speed(0.1)
                        .range(1.0..=10.0),
                )
                .changed()
            {
                *value = edited.round() as u8;
                true
            } else {
                false
            }
        };
        let (a, b) = pair(
            ui,
            ("Start", |ui: &mut Ui| count(ui, start)),
            ("End", |ui: &mut Ui| count(ui, end)),
        );
        changed |= a || b;
    }

    if inherit {
        *keep = None;
        return true;
    }
    // Opened and untouched, a style still inherits.
    if inheritable && !stated && !changed {
        *keep = None;
    }
    changed
}

/// The tab stops of a paragraph or a style: one row per stop, and a way to
/// add one. Returns whether anything changed.
///
/// A row is the stop's position, how text sits against it, and its leader.
/// The position is from the left edge of the column, as the model has it and
/// as InDesign's ruler shows it.
///
/// `inheritable` offers "Inherit" — clearing the list to `None` — which only
/// a style can mean: a paragraph's own `None` is indistinguishable from its
/// style's answer, so the inspector never shows it.
pub(crate) fn tab_stops_editor(
    ui: &mut Ui,
    stops: &mut Option<Vec<tessera_text::story::TabStop>>,
    inheritable: bool,
) -> bool {
    use tessera_text::story::{TabAlignment, TabStop};

    let mut changed = false;
    group_label(ui, "Tab stops");

    let list = stops.get_or_insert_with(Vec::new);
    let mut remove = None;
    for (i, stop) in list.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(format!("Tab stop {}", i + 1));
                if icon_button(ui, crate::icons::Icon::Trash, "Remove this tab stop", false) {
                    remove = Some(i);
                }
            });
            let mut position = f64::from(stop.position);
            if field(ui, "Position", |ui| {
                ui.add(
                    egui::DragValue::new(&mut position)
                        .speed(0.25)
                        .range(0.0..=1440.0)
                        .custom_formatter(|v, _| format!("{v:.2} pt")),
                )
                .on_hover_text("Position from the left edge of the column")
                .changed()
            }) {
                stop.position = position as f32;
                changed = true;
            }

            let alignments = [
                (TabAlignment::Left, "Left"),
                (TabAlignment::Centre, "Centre"),
                (TabAlignment::Right, "Right"),
                (TabAlignment::Decimal, "Decimal"),
            ];
            let shown = alignments
                .iter()
                .find(|(a, _)| *a == stop.alignment)
                .map_or("Left", |(_, label)| *label);
            field(ui, "Align", |ui| {
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt(("tab-alignment", i))
                        .selected_text(shown)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for (alignment, label) in alignments {
                                if ui
                                    .selectable_label(stop.alignment == alignment, label)
                                    .clicked()
                                    && stop.alignment != alignment
                                {
                                    stop.alignment = alignment;
                                    changed = true;
                                }
                            }
                        })
                        .response,
                    "Align",
                    egui::WidgetType::ComboBox,
                    None,
                )
            });

            // The leader is one character; the field takes the last one typed
            // so that typing over a dot with a dash needs no deleting first.
            let mut leader: String = stop.leader.map(String::from).unwrap_or_default();
            let response = field(ui, "Leader", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut leader)
                        .desired_width(20.0)
                        .hint_text("·"),
                )
            });
            crate::icons::named(response.clone(), format!("Leader of tab stop {}", i + 1));
            if response.changed() {
                let next = leader.chars().last();
                if next != stop.leader {
                    stop.leader = next;
                    changed = true;
                }
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }

    let mut inherit = false;
    ui.horizontal(|ui| {
        if crate::view::panel_ui::action(ui, crate::icons::Icon::Plus, "Add tab stop").clicked() {
            // Half an inch past the last one, which is where the default
            // stop would have been anyway.
            let last = list.iter().map(|s| s.position).fold(0.0, f32::max);
            list.push(TabStop::at(last + 36.0));
            changed = true;
        }
        if inheritable && ui.small_button("Inherit").clicked() {
            inherit = true;
        }
    });
    if inherit {
        *stops = None;
        return true;
    }
    // A style whose editor was opened and touched nothing should still say
    // `None` — inherit — not "no stops". The inspector keeps the empty list,
    // because there it is the only way to say "no stops".
    if inheritable && stops.as_ref().is_some_and(|l| l.is_empty()) {
        *stops = None;
    }
    changed
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
        crate::view::panel_ui::hint(ui, "Place an image or illustration in this frame.");
        if crate::view::panel_ui::action(ui, crate::icons::Icon::PlaceImage, "Place artwork…")
            .clicked()
        {
            crate::file_ops::place(state);
        }
        return;
    };

    let link = state.active().document().links.get(placement.link).cloned();
    let Some(link) = link else {
        ui.colored_label(Theme::error(), "The link is missing from the document");
        return;
    };

    // The file, by name. The whole path is usually too long for the rail and
    // the name is what a person recognises; the path is on the tooltip.
    let name = link
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| link.path.to_string_lossy().into_owned());
    ui.add(egui::Label::new(&name).wrap())
        .on_hover_text(link.path.to_string_lossy().into_owned());

    // What the disk says, now. **Three** states rather than two: "the file has
    // changed" is the one the previous codebase never drew, and the reason
    // somebody could send a printer last week's photograph.
    let status = link.status();
    let (word, colour) = match status {
        Status::Fine => ("Up to date", Theme::text_muted()),
        Status::Modified => ("Modified on disk", Theme::accent()),
        Status::Missing => ("Missing", Theme::error()),
    };
    ui.colored_label(colour, word);

    // The link, not this frame: every frame showing the file follows, which
    // is what the Links panel does too. Placing a new file into this frame
    // alone would leave the same picture's other frames pointing at the old
    // path, and the panel counting two files where there is one.
    if status != Status::Fine
        && ui
            .button("Relink...")
            .on_hover_text("Choose the file this should point at")
            .clicked()
        && let Some(path) = crate::file_ops::pick_artwork()
    {
        crate::command::apply(
            state,
            crate::command::Command::Relink {
                link: placement.link,
                path,
            },
        );
        state.links.recheck();
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
    if let Some((x, y)) = tessera_document::graphic::effective_ppi(pixels, drawn) {
        let worst = x.min(y);
        let colour = if worst < state.prefs.minimum_ppi {
            Theme::error()
        } else {
            Theme::text_muted()
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

    group_label(ui, "Artwork fitting");
    for (label, icon, how, hint) in [
        (
            "Fit artwork",
            crate::icons::Icon::Scale,
            Fit::Proportionally,
            "Show the entire artwork without changing its proportions.",
        ),
        (
            "Fill frame",
            crate::icons::Icon::PictureFrame,
            Fit::FillProportionally,
            "Fill the frame proportionally; edges may be cropped.",
        ),
        (
            "Stretch artwork",
            crate::icons::Icon::Scale,
            Fit::Stretch,
            "Fill the frame by changing the artwork's proportions.",
        ),
        (
            "Centre artwork",
            crate::icons::Icon::Move,
            Fit::Centre,
            "Centre the artwork without resizing it.",
        ),
    ] {
        if crate::view::panel_ui::action(ui, icon, label)
            .on_hover_text(hint)
            .clicked()
        {
            apply(state, Command::RefitArtwork { id, fit: how });
        }
    }
    ui.separator();
    if ui
        .add_sized(
            [ui.available_width(), Theme::row()],
            egui::Button::new("Fit frame to artwork").wrap(),
        )
        .clicked()
    {
        apply(state, Command::FitFrameToArtwork { id });
    }
}

/// Which named appearance the object follows, and whether it has departed from
/// it.
///
/// The "differs" mark is **computed by comparing the object to its style**, not
/// looked up, so it cannot be wrong. That is the same fact the cascade uses to
/// decide what to leave alone when the style changes, asked from the other side.
fn object_style_section(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: tessera_document::ids::FrameId,
    frame: &tessera_document::nodes::Frame,
) {
    let listed: Vec<(tessera_document::ids::ObjectStyleId, String)> = state
        .active()
        .document()
        .object_style_order
        .iter()
        .filter_map(|style| {
            state
                .active()
                .document()
                .object_styles
                .get(*style)
                .map(|s| (*style, s.name.clone()))
        })
        .collect();

    if listed.is_empty() {
        ui.colored_label(Theme::text_muted(), "No object styles yet.")
            .on_hover_text("Make one in the Styles panel, under Object");
        return;
    }

    let current = frame
        .style
        .and_then(|s| state.active().document().object_styles.get(s))
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "None".to_string());

    let mut chosen = frame.style;
    property_field(ui, "Style", |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt(("object-style", id))
                .selected_text(&current)
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, None, "None");
                    for (style, name) in &listed {
                        ui.selectable_value(&mut chosen, Some(*style), name);
                    }
                })
                .response
                .on_hover_text(&current),
            "Style",
            egui::WidgetType::ComboBox,
            None,
        );
    });

    if chosen != frame.style {
        match chosen {
            // Attaching writes what the style states; detaching writes nothing,
            // because the object already holds its own appearance and taking it
            // away would be a change nobody asked for.
            Some(style) => apply(state, Command::ApplyObjectStyle { id, style }),
            None => apply(state, Command::DetachObjectStyle { id }),
        }
        return;
    }

    // The overrides, and the way back. InDesign shows a `+` beside the style
    // name; this says which properties, because "differs somehow" sends a person
    // looking through every control to find out where.
    let Some(overrides) = state.active().document().object_overrides(id) else {
        return;
    };
    if overrides.is_empty() {
        ui.colored_label(Theme::text_muted(), "Following its style");
        return;
    }

    let mut departed: Vec<&str> = Vec::new();
    if overrides.fill.is_some() {
        departed.push("fill");
    }
    if overrides.stroke.is_some() {
        departed.push("stroke");
    }
    if overrides.blend.is_some() {
        departed.push("opacity");
    }
    if overrides.shadow.is_some() {
        departed.push("shadow");
    }
    if overrides.wrap.is_some() {
        departed.push("text wrap");
    }

    ui.colored_label(Theme::accent(), format!("Own {}", departed.join(", ")))
        .on_hover_text("These stay as they are when the style changes");
    if ui
        .button("Follow the style again")
        .on_hover_text("Put every property back to what the style says")
        .clicked()
    {
        apply(state, Command::ClearObjectOverrides { id });
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
    changed |= property_slider(ui, (crate::icons::Icon::Opacity, "Opacity"), |ui| {
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
    property_field(ui, (crate::icons::Icon::Blend, "Blend"), |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt(("blend-mode", id))
                .selected_text(blend.mode.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for mode in BlendMode::ALL {
                        ui.selectable_value(&mut blend.mode, mode, mode.label());
                    }
                })
                .response,
            "Blend",
            egui::WidgetType::ComboBox,
            None,
        );
    });
    changed |= blend.mode != before;

    // What an object at no opacity actually means, said plainly. It is still
    // selectable and still in the layers panel, and somebody who cannot see it
    // will otherwise think it has gone.
    if blend.is_invisible() {
        ui.colored_label(
            Theme::text_muted(),
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
        ("Offset Y", |ui: &mut Ui| {
            measure_bare(ui, &mut shadow.offset.1, unit)
        }),
    );
    changed |= x || y;

    // In points rather than in the document's unit: a blur is not a measurement
    // on the page, it is how soft an edge is, and reading it in millimetres
    // invites somebody to try to line it up with something.
    changed |= property_slider(ui, (crate::icons::Icon::Blur, "Blur"), |ui| {
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
    if property_field(ui, "Colour", |ui| shadow_picker(ui, &mut rgba)) {
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
    ui.colored_label(Theme::text_muted(), "Not written to PDF yet.")
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

/// A shadow's colour, with its alpha: separate from [`fill_picker`], which
/// deliberately offers none — a fill's alpha and its object's opacity are
/// different facts, and offering both in one place is how a person comes to
/// believe they are the same control. A shadow has no such pair: its alpha
/// *is* how much of it shows.
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
    use tessera_document::nodes::{TextWrap, WrapTo};

    #[derive(PartialEq, Clone, Copy)]
    enum How {
        Off,
        Bounds,
        Contour,
        Jump,
    }
    let mut how = match frame.wrap {
        TextWrap::None => How::Off,
        TextWrap::Bounds { .. } => How::Bounds,
        TextWrap::Contour { .. } => How::Contour,
        TextWrap::Jump => How::Jump,
    };
    let mut standoff = frame.wrap.standoff().unwrap_or_default();
    let mut sides = frame.wrap.sides();
    let mut changed = false;

    changed |= icon_choices(
        ui,
        "Wrap mode",
        &mut how,
        &[
            (
                crate::icons::Icon::WrapNone,
                "No wrap",
                "Text runs beneath the frame",
                How::Off,
            ),
            (
                crate::icons::Icon::WrapBounds,
                "Around the box",
                "Text stops at the frame's edges",
                How::Bounds,
            ),
            (
                crate::icons::Icon::WrapContour,
                "Around the shape",
                "Text follows the outline of what is inside",
                How::Contour,
            ),
            (
                crate::icons::Icon::WrapJump,
                "Jump over",
                "Text skips the whole band the frame sits in",
                How::Jump,
            ),
        ],
    );
    let unit = state.prefs.unit;
    match how {
        How::Bounds => {
            changed |= linked_edges(
                ui,
                egui::Id::new(("wrap-link", state.active, id)),
                "Distance from text",
                ["Top", "Bottom", "Left", "Right"],
                [
                    &mut standoff.top,
                    &mut standoff.bottom,
                    &mut standoff.left,
                    &mut standoff.right,
                ],
                unit,
            );
        }
        How::Contour => {
            // One distance all round: a contour has no top and no left.
            let mut all = standoff.top;
            if property_field(ui, "Distance from text", |ui| {
                measure_bare(ui, &mut all, unit)
            }) {
                standoff = tessera_document::nodes::Insets {
                    top: all,
                    bottom: all,
                    left: all,
                    right: all,
                };
                changed = true;
            }
        }
        How::Off | How::Jump => {}
    }
    // Which side the text runs on. Not for a jump, which has no sides.
    if matches!(how, How::Bounds | How::Contour) {
        let name = |s: WrapTo| match s {
            WrapTo::Largest => "Largest area",
            WrapTo::Both => "Both sides",
            WrapTo::Left => "Left side",
            WrapTo::Right => "Right side",
        };
        property_field(ui, "Wrap to", |ui| {
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt(("wrap-to", id))
                    .width(ui.available_width())
                    .selected_text(name(sides))
                    .show_ui(ui, |ui| {
                        for choice in [WrapTo::Largest, WrapTo::Both, WrapTo::Left, WrapTo::Right] {
                            if ui
                                .selectable_value(&mut sides, choice, name(choice))
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    })
                    .response,
                "Wrap to",
                egui::WidgetType::ComboBox,
                None,
            );
        });
    }

    if changed {
        let wrap = match how {
            How::Off => TextWrap::None,
            How::Bounds => TextWrap::Bounds { standoff, sides },
            How::Contour => TextWrap::Contour {
                standoff: standoff.top,
                sides,
            },
            How::Jump => TextWrap::Jump,
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

    group_label(ui, "Frame");

    let mut columns = f64::from(wanted.columns.max(1));
    let (a, b) = pair(
        ui,
        ((crate::icons::Icon::Columns, "Columns"), |ui: &mut Ui| {
            ui.add(
                egui::DragValue::new(&mut columns)
                    .speed(0.1)
                    .range(1.0..=20.0),
            )
            .changed()
        }),
        ((crate::icons::Icon::Gutter, "Gutter"), |ui: &mut Ui| {
            measure_bare(ui, &mut wanted.gutter, unit)
        }),
    );
    if a {
        wanted.columns = columns.round().clamp(1.0, 20.0) as u8;
    }
    changed |= a || b;

    changed |= linked_edges(
        ui,
        egui::Id::new(("inset-link", state.active, id)),
        "Inset",
        ["Top", "Bottom", "Left", "Right"],
        [
            &mut wanted.inset.top,
            &mut wanted.inset.bottom,
            &mut wanted.inset.left,
            &mut wanted.inset.right,
        ],
        unit,
    );

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
    text_label(ui, crate::icons::Icon::TextFrame, "Vertical alignment");
    use tessera_document::nodes::VerticalJustify as V;
    let placements = [
        (crate::icons::Icon::AlignTop, "Top", V::Top),
        (crate::icons::Icon::AlignMiddleV, "Centre", V::Centre),
        (crate::icons::Icon::AlignBottom, "Bottom", V::Bottom),
        (crate::icons::Icon::DistributeV, "Justify", V::Justify),
    ];
    let toggles = placements.map(|(icon, name, which)| (icon, name, wanted.vertical == which));
    if let Some(i) = icon_toggles(ui, &toggles) {
        wanted.vertical = placements[i].2;
        changed = true;
    }

    if changed {
        apply(state, Command::SetTextLayout { id, layout: wanted });
    }
}

/// A quiet label naming a group of fields inside a section.
///
/// Not a section heading: it does not collapse and it carries no icon. The
/// difference in weight is what says one is a level above the other.
pub(crate) fn group_label(ui: &mut Ui, text: &str) {
    ui.add_space(Theme::space_2());
    ui.add(egui::Label::new(group_text(text)).selectable(false));
}

/// A readable sentence-case label for a subgroup of controls.
fn group_text(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(Theme::TYPE_SM)
        .strong()
        .color(Theme::text_primary())
}

/// Linking is UI state, scoped to this document/object and group. Toggling it
/// never modifies the document; the next edit supplies the shared value.
fn linked_group_heading(ui: &mut Ui, id: egui::Id, title: &str, default: bool) -> bool {
    let mut linked = ui.ctx().data_mut(|data| *data.get_temp_mut_or(id, default));
    ui.add_space(Theme::space_2());
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(group_text(title)).selectable(false));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::splat(Theme::control_height()), Sense::click());
            if response.clicked() {
                linked = !linked;
            }
            if linked || response.hovered() {
                ui.painter()
                    .rect_filled(rect, Theme::RADIUS, Theme::selected_bg());
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    Theme::RADIUS,
                    egui::Stroke::new(1.0, Theme::focus()),
                    egui::StrokeKind::Inside,
                );
            }
            crate::icons::paint(
                ui.painter(),
                rect,
                if linked {
                    crate::icons::Icon::Link2
                } else {
                    crate::icons::Icon::Unlink2
                },
                if linked {
                    Theme::text_primary()
                } else {
                    Theme::text_muted()
                },
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    ui.is_enabled(),
                    linked,
                    format!("Link {title} values"),
                )
            });
            response.on_hover_text(if linked {
                "Linked: editing one value changes all. Click to edit independently."
            } else {
                "Link values: the next value you edit will apply to all sides."
            });
        });
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, linked));
    linked
}

fn propagate_linked_edit(values: &mut [f64; 4], edited: [bool; 4], linked: bool) -> bool {
    let Some(index) = edited.iter().position(|changed| *changed) else {
        return false;
    };
    if linked {
        *values = [values[index]; 4];
    }
    true
}

pub(crate) fn linked_edges(
    ui: &mut Ui,
    id: egui::Id,
    title: &str,
    labels: [&str; 4],
    values: [&mut f64; 4],
    unit: Unit,
) -> bool {
    let linked = linked_group_heading(ui, id, title, false);
    let [mut top, mut bottom, mut left, mut right] = values.each_ref().map(|value| **value);
    let (a, b) = pair(
        ui,
        (labels[0], |ui: &mut Ui| measure_bare(ui, &mut top, unit)),
        (labels[1], |ui: &mut Ui| measure_bare(ui, &mut bottom, unit)),
    );
    let (c, d) = pair(
        ui,
        (labels[2], |ui: &mut Ui| measure_bare(ui, &mut left, unit)),
        (labels[3], |ui: &mut Ui| measure_bare(ui, &mut right, unit)),
    );
    let mut edited = [top, bottom, left, right];
    let changed = propagate_linked_edit(&mut edited, [a, b, c, d], linked);
    for (target, value) in values.into_iter().zip(edited) {
        *target = value;
    }
    changed
}

/// A quiet surface groups related controls without another heading icon.
/// A group's fields, spaced as the panel spaces them.
///
/// **Flat**, as InDesign's Properties panel is. The groups were cards — a
/// filled, bordered box each — and a panel of boxes inside a panel reads as
/// clutter before it reads as structure. The heading and the rule under the
/// group say where one ends and the next begins, and say it more quietly.
fn property_body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = Vec2::splat(Theme::space_2());
        ui.spacing_mut().interact_size.y = Theme::row();
        add(ui)
    })
    .inner
}

/// A named group: its heading, its fields, and the rule that closes it.
fn property_card(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    property_body(ui, |ui| {
        ui.add(egui::Label::new(egui::RichText::new(title).strong()).selectable(false));
        add(ui);
    });
    ui.add_space(Theme::space_2());
    ui.separator();
}

/// What names a field: a word, or a glyph with the word beside it.
///
/// The glyph is for the reader who has learned it — leading, tracking, an
/// indent — and the word stays for the one who has not, and for the screen
/// reader, which cannot read a picture. A field whose meaning has no honest
/// picture (X, Y, a miter limit) keeps the bare word.
#[derive(Clone, Copy)]
pub(crate) enum FieldLabel<'a> {
    Word(&'a str),
    Glyph(crate::icons::Icon, &'a str),
}

impl<'a> From<&'a str> for FieldLabel<'a> {
    fn from(word: &'a str) -> Self {
        Self::Word(word)
    }
}

impl<'a> From<(crate::icons::Icon, &'a str)> for FieldLabel<'a> {
    fn from((icon, word): (crate::icons::Icon, &'a str)) -> Self {
        Self::Glyph(icon, word)
    }
}

impl FieldLabel<'_> {
    /// Lay the label and the field out together.
    ///
    /// A word sits on its own line above the field, because a word beside a
    /// field leaves the field no width. A glyph sits on the field's line at
    /// its left, InDesign's row, and the word becomes the glyph's tooltip
    /// and its accessible name — half the height, and the eye finds the
    /// glyph without reading.
    fn field<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
        match self {
            Self::Word(word) => {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = Theme::space_1();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(word).small().color(Theme::text_muted()),
                        )
                        .wrap(),
                    );
                    ui.spacing_mut().interact_size.x = ui.available_width().min(WIDEST_ROW);
                    let field = ui.next_auto_id();
                    let added = add(ui);
                    crate::icons::speak_field_as(ui.ctx(), field, word);
                    added
                })
                .inner
            }
            Self::Glyph(icon, word) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = Theme::space_1();
                    let (rect, response) = ui
                        .allocate_exact_size(Vec2::splat(Theme::FIELD_GLYPH_SIZE), Sense::hover());
                    crate::icons::paint(ui.painter(), rect, icon, Theme::text_muted());
                    crate::icons::reads_as(response, word, egui::WidgetType::Label, None)
                        .on_hover_text(word);
                    ui.spacing_mut().interact_size.x = ui.available_width().min(WIDEST_ROW);
                    let field = ui.next_auto_id();
                    let added = add(ui);
                    crate::icons::speak_field_as(ui.ctx(), field, word);
                    added
                })
                .inner
            }
        }
    }
}

pub(crate) fn property_field<'a, R>(
    ui: &mut Ui,
    label: impl Into<FieldLabel<'a>>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    label.into().field(ui, add)
}

fn property_slider<'a, R>(
    ui: &mut Ui,
    label: impl Into<FieldLabel<'a>>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    property_field(ui, label, |ui| {
        ui.spacing_mut().interact_size.x = VALUE_BOX;
        ui.spacing_mut().slider_width =
            (ui.available_width() - VALUE_BOX - ui.spacing().item_spacing.x).max(0.0);
        add(ui)
    })
}

fn text_label(ui: &mut Ui, icon: crate::icons::Icon, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(Theme::ICON_SIZE), Sense::hover());
        crate::icons::paint(ui.painter(), rect, icon, Theme::text_muted());
        ui.add(
            egui::Label::new(
                egui::RichText::new(label)
                    .small()
                    .color(Theme::text_muted()),
            )
            .wrap(),
        );
    });
}

/// Labels above inputs stay readable even at the dock's minimum width.
fn text_field<R>(
    ui: &mut Ui,
    icon: crate::icons::Icon,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    property_field(ui, (icon, label), add)
}

/// What every toggle in the panel draws behind its picture.
///
/// Flat until it matters, as InDesign's icon rows are: nothing around an idle
/// choice, a filled square under the one that is on, a tint under the
/// pointer, and the focus ring over any of them. A row of nine boxed buttons
/// is nine borders to read past before the pictures.
pub(crate) fn paint_toggle_frame(ui: &Ui, rect: egui::Rect, response: &egui::Response, on: bool) {
    let painter = ui.painter_at(rect);
    let fill = if on {
        Some(Theme::selected_bg())
    } else if response.hovered() {
        Some(Theme::hover_bg())
    } else {
        None
    };
    if let Some(fill) = fill {
        painter.rect_filled(rect, Theme::RADIUS, fill);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            Theme::RADIUS,
            egui::Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }
}

/// A row of icon-only toggles: bold, italic, underline, strike; the four
/// alignments. These are the most learned pictures in any editor, so the word
/// goes to the tooltip and the accessible name, as a glyph-captioned field's
/// does. The index of the toggle clicked, if one was.
///
/// Packed from the left in cells a little wider than the icon, as InDesign's
/// are — nine alignments fit its row that way. Spread across the panel, four
/// icons sat a hand's width apart and read as four unrelated buttons rather
/// than one choice.
fn icon_toggles(ui: &mut Ui, toggles: &[(crate::icons::Icon, &str, bool)]) -> Option<usize> {
    let cell = Vec2::new(
        Theme::control_height() + Theme::space_1(),
        Theme::control_height(),
    );
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (index, &(icon, name, on)) in toggles.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
            paint_toggle_frame(ui, rect, &response, on);
            crate::icons::paint(&ui.painter_at(rect), rect, icon, Theme::text_primary());
            if crate::icons::named_toggle(response, name, egui::WidgetType::Button, on)
                .on_hover_text(name)
                .clicked()
            {
                clicked = Some(index);
            }
        }
    });
    clicked
}

fn property_disclosure(ui: &mut Ui, state: &mut TesseraApp, title: &'static str) -> bool {
    // No gap above: the heading's own rule is the break between sections,
    // and a gap as well spent a row's height on air for every closed one.
    let open = section_heading(ui, state, title);
    if open {
        ui.add_space(Theme::space_2());
    }
    open
}

/// A row of mutually exclusive choices, the shape a three-way property wants.
fn segmented<T: PartialEq + Copy>(ui: &mut Ui, label: &str, value: &mut T, options: &[(&str, T)]) {
    let widest = options
        .iter()
        .map(|(label, _)| {
            ui.painter()
                .layout_no_wrap(
                    (*label).to_owned(),
                    egui::TextStyle::Button.resolve(ui.style()),
                    Theme::text_primary(),
                )
                .size()
                .x
                + 2.0 * ui.spacing().button_padding.x
        })
        .fold(0.0_f32, f32::max);
    let needed = widest * options.len() as f32
        + ui.spacing().item_spacing.x * options.len().saturating_sub(1) as f32;
    if needed > ui.available_width() {
        property_choice(ui, label, value, options);
        return;
    }
    ui.label(
        egui::RichText::new(label)
            .small()
            .color(Theme::text_muted()),
    );
    ui.columns(options.len(), |columns| {
        for (column, (text, candidate)) in columns.iter_mut().zip(options) {
            let selected = *value == *candidate;
            if column
                .add_sized(
                    [column.available_width(), Theme::row()],
                    egui::Button::new(*text)
                        .selected(selected)
                        .fill(if selected {
                            Theme::accent_soft()
                        } else {
                            Theme::field_bg()
                        }),
                )
                .clicked()
            {
                *value = *candidate;
            }
        }
    });
}

fn property_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[(&str, T)],
) -> bool {
    let before = *value;
    property_field(ui, label, |ui| {
        let shown = options
            .iter()
            .find(|(_, candidate)| candidate == value)
            .map_or("Custom", |(label, _)| *label);
        let response = egui::ComboBox::from_id_salt(label)
            .width(ui.available_width())
            .truncate()
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (label, candidate) in options {
                    ui.selectable_value(value, *candidate, *label);
                }
            })
            .response;
        crate::icons::reads_as(response, label, egui::WidgetType::ComboBox, None);
    });
    before != *value
}

/// A choice made by picture — line ends, joins, a dash pattern — on one row:
/// its name at the left in the label column, as InDesign's Stroke panel
/// writes "Cap" and "Join", and the pictures packed after it. The name above
/// the row cost a line per choice for a word the row could carry.
pub(crate) fn icon_choices<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[(crate::icons::Icon, &str, &str, T)],
) -> bool {
    let before = *value;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(Theme::LABEL_COLUMN, Theme::control_height()),
                Sense::hover(),
            );
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(Theme::TYPE_SM),
                Theme::text_muted(),
            );
            ui.spacing_mut().item_spacing.x = 1.0;
            let cell = Vec2::new(
                Theme::control_height() + Theme::space_1(),
                Theme::control_height(),
            );
            for (icon, name, hint, candidate) in options {
                let selected = *value == *candidate;
                let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
                paint_toggle_frame(ui, rect, &response, selected);
                crate::icons::paint(&ui.painter_at(rect), rect, *icon, Theme::text_primary());
                let response = crate::icons::reads_as(
                    response,
                    *name,
                    egui::WidgetType::RadioButton,
                    Some(selected),
                )
                .on_hover_text(format!("{name}\n{hint}"));
                if response.clicked() {
                    *value = *candidate;
                }
            }
        });
    });
    before != *value
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

/// The paragraph formatting the text in hand states: the selection's, or the
/// caret's paragraph, or the whole story when a text frame is selected — its
/// own overrides and nothing it inherits, so a style made from it states what
/// somebody set rather than every default. `None` with no text frame selected.
pub(crate) fn stated_paragraph_format(state: &TesseraApp) -> Option<ParagraphFormat> {
    let (story, range) = text_in_hand(state)?;
    Some(
        state
            .active()
            .document()
            .story(story)?
            .common_paragraph_format(range),
    )
}

/// The text formatting would land on: the story, and the selection in it or
/// the caret — or the whole story when the frame is selected and not being
/// typed in. `None` with no single text frame selected.
pub(crate) fn text_in_hand(state: &TesseraApp) -> Option<(StoryId, std::ops::Range<usize>)> {
    let id = state.active().selection.single()?;
    let tessera_document::nodes::FrameKind::Text { story, .. } =
        state.active().document().frame(id)?.kind
    else {
        return None;
    };
    Some((story, format_target(state, id, story)))
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
    labelled(ui, label, Theme::LABEL_COLUMN, false, add)
}

/// A labelled **slider**, which is the one control that needs its row split.
///
/// Separate from [`field`] rather than detected inside it, because a `Ui`
/// cannot be asked what is about to be drawn into it. Calling `field` with a
/// slider is the mistake this name exists to make visible, and
/// `a_slider_row_fits_the_panel_it_is_in` is the test that catches it.
pub(crate) fn slider_field<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    labelled(ui, label, Theme::LABEL_COLUMN, true, add)
}

/// How wide one row of a properties panel may grow.
///
/// A ceiling rather than a size. Rows fill whatever they are given, and this
/// only stops "whatever they are given" being a whole 4K screen when the
/// surrounding container has not decided its own width yet.
const WIDEST_ROW: f32 = 420.0;

/// How wide a slider’s value box is allowed to be.
///
/// Sliders are the one control that draws *two* things: a rail, sized from
/// `spacing.slider_width`, and a value box beside it, sized from
/// `spacing.interact_size.x`. Everything else here draws one.
const VALUE_BOX: f32 = 52.0;

/// A labelled control with a chosen label width.
///
/// The control is given **all the room that is left**, rather than sizing
/// itself and leaving the rest of the panel blank to its right. A 292-point
/// panel with a 64-point label and a field that draws at its natural 60 was
/// throwing away more than half its width on every row.
///
/// `for_slider` is the exception to that, and it exists because giving a
/// slider all the room twice over is not the same as giving it all the room.
/// See [`slider_field`].
fn labelled<R>(
    ui: &mut Ui,
    label: &str,
    width: f32,
    for_slider: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(width);
                ui.add(
                    egui::Label::new(egui::RichText::new(label).color(Theme::text_muted())).wrap(),
                )
                .on_hover_text(label);
            },
        );
        // Clamped, and **not** fed back into a minimum. `available_width` in a
        // resizable window is however wide the window currently is, so a row
        // that answers "I need all of it" ratchets: the window grows, the row
        // grows with it, and it can never be dragged smaller again. That is
        // what made Preferences open at the size of the screen and refuse to be
        // adjusted. The control fills the row by being *sized*, never by
        // asserting a minimum.
        let room = ui.available_width().min(WIDEST_ROW);
        let gap = ui.spacing().item_spacing.x;

        // A slider spends the row on a rail *and* a value box. Sized as though
        // it were one control, each half took the whole row and the row came
        // out twice the width of the panel — which ran the properties panel
        // off the right of the window the moment anything with an opacity was
        // selected. Splitting the room is the whole fix.
        let (rail, box_) = if for_slider {
            ((room - VALUE_BOX - gap).max(32.0), VALUE_BOX)
        } else {
            (room, room)
        };
        ui.style_mut().spacing.slider_width = rail;

        ui.scope(|ui| {
            ui.spacing_mut().interact_size.x = box_;
            let field = ui.next_auto_id();
            let added = add(ui);
            crate::icons::speak_field_as(ui.ctx(), field, label);
            added
        })
        .inner
    })
    .inner
}

/// Two equal-width fields with labels above their values. Keeping the labels
/// out of the value row leaves room for units even in a narrow dock.
pub(crate) fn pair<'a, A, B>(
    ui: &mut Ui,
    first: (impl Into<FieldLabel<'a>>, impl FnOnce(&mut Ui) -> A),
    second: (impl Into<FieldLabel<'a>>, impl FnOnce(&mut Ui) -> B),
) -> (A, B) {
    fn cell<R>(ui: &mut Ui, label: FieldLabel<'_>, add: impl FnOnce(&mut Ui) -> R) -> R {
        label.field(ui, add)
    }
    // Two to a row while each half has room for a glyph and a number. A dock
    // dragged narrower than that stacks them rather than letting the second
    // run past the edge — the glyphs stay the size they were drawn for.
    // Room for a number with its unit, "12.70 mm", not just a bare value.
    let half = Theme::FIELD_GLYPH_SIZE + Theme::space_1() + 64.0;
    if ui.available_width() < 2.0 * half + ui.spacing().item_spacing.x {
        let a = cell(ui, first.0.into(), first.1);
        return (a, cell(ui, second.0.into(), second.1));
    }
    ui.columns(2, |columns| {
        (
            cell(&mut columns[0], first.0.into(), first.1),
            cell(&mut columns[1], second.0.into(), second.1),
        )
    })
}

/// A section heading that opens and shuts, remembering which it was.
///
/// One component, used by the rail and by the inspector's own sections. A
/// panel that shows Transform, Fill, Stroke, Text and Styles at once is a
/// column nobody reads to the end of — which is the same complaint the Object
/// menu earned before it grew submenus.
pub(crate) fn section_heading(ui: &mut Ui, state: &mut TesseraApp, title: &'static str) -> bool {
    let was = state.sections.is_open(title);
    let now = section_heading_with(ui, title, was);
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
pub(crate) fn section_heading_with(ui: &mut Ui, title: &str, open: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), Theme::control_height()),
        Sense::click(),
    );
    let painter = ui.painter_at(rect);

    // A quiet divider and a semibold title establish the group without
    // making every heading look like another input field.
    painter.hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, Theme::rule()),
    );
    if response.hovered() {
        painter.rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            Theme::RADIUS,
            egui::Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }

    // Spectrum's ten-pixel chevron, at ten pixels: the twenty-pixel one
    // halved was the softest thing in the panel.
    let caret = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - Theme::space_2() - 10.0,
            rect.center().y - 5.0,
        ),
        Vec2::splat(10.0),
    );
    crate::icons::paint_rotated(
        &painter,
        caret,
        crate::icons::Icon::Disclosure,
        Theme::text_muted(),
        if open { 90.0 } else { 0.0 },
    );

    // The heading face at the body size: semibold where the theme installed
    // it, and whatever the heading style resolves to where it did not — a
    // test context has no such face, and naming one it lacks is a panic.
    let font = egui::FontId {
        size: Theme::TYPE_MD,
        family: ui
            .style()
            .text_styles
            .get(&egui::TextStyle::Heading)
            .map_or(egui::FontFamily::Proportional, |f| f.family.clone()),
    };
    let text_left = rect.left() + Theme::space_2();
    let galley = ui.fonts_mut(|fonts| {
        let mut job =
            egui::text::LayoutJob::simple_singleline(title.to_owned(), font, Theme::text_primary());
        job.wrap.max_width = (caret.left() - Theme::space_2() - text_left).max(0.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        fonts.layout_job(job)
    });
    painter.galley(
        egui::pos2(text_left, rect.center().y - galley.size().y / 2.0),
        galley,
        Theme::text_primary(),
    );

    // **Painted text is invisible to a screen reader.** The title above goes
    // straight to the painter, so nothing in the widget tree carries it — a
    // disclosure that reveals half the inspector announced itself as an unnamed
    // button. It says its own name and whether it is open.
    let response = crate::icons::reads_as(
        response.on_hover_text(title),
        title,
        egui::WidgetType::CollapsingHeader,
        Some(open),
    );

    if response.clicked() { !open } else { open }
}

/// A measurement control with no label of its own.
pub(crate) fn measure_bare(ui: &mut Ui, points: &mut f64, unit: Unit) -> bool {
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

    use crate::icons::Icon;
    ui.spacing_mut().item_spacing.y = Theme::space_2();
    ui.spacing_mut().interact_size.y = Theme::row();
    crate::view::panel_ui::hint(
        ui,
        match &state.active().editing {
            Some((editing, _)) if *editing == id && target.is_empty() => {
                "Character changes apply to new typing."
            }
            Some((editing, _)) if *editing == id => "Formatting the selected text.",
            _ => "Formatting all text in this story.",
        },
    );
    ui.add_space(Theme::space_2());
    property_card(ui, "Character", |ui| {
        // These controls also work on a selected frame, before entering its text.
        // The same target and pending format as the other controls keep both
        // entry points in sync with the toolbar while typing.
        let family = text_field(ui, Icon::Text, "Font family", |ui| {
            let width = ui.available_width().min(WIDEST_ROW);
            family_menu(ui, state, shown.family.as_deref(), width)
        });
        if let Some(family) = family {
            set_character(
                state,
                story,
                target.clone(),
                CharacterFormat {
                    family: Some(family),
                    ..CharacterFormat::default()
                },
            );
        }
        let mut size = f64::from(shown.size.unwrap_or(12.0));
        let mut leading = f64::from(shown.line_height.unwrap_or(1.2));
        let (size_changed, leading_changed) = pair(
            ui,
            ((Icon::TypeSize, "Size"), |ui: &mut Ui| {
                ui.add(
                    egui::DragValue::new(&mut size)
                        .speed(0.25)
                        .range(0.1..=2000.0)
                        .suffix(" pt"),
                )
                .changed()
            }),
            ((Icon::LineSpacing, "Line height"), |ui: &mut Ui| {
                ui.add(
                    egui::DragValue::new(&mut leading)
                        .speed(0.02)
                        .range(0.5..=4.0)
                        .suffix(" ×"),
                )
                .changed()
            }),
        );
        if size_changed || leading_changed {
            set_character(
                state,
                story,
                target.clone(),
                CharacterFormat {
                    size: size_changed.then_some(size as f32),
                    line_height: leading_changed.then_some(leading as f32),
                    ..CharacterFormat::default()
                },
            );
        }
        if !missing.is_empty() {
            ui.colored_label(Theme::error(), format!("Missing: {}", missing.join(", ")));
        }

        // Text colour. Distinct from the frame's fill, which is the box behind the
        // glyphs — setting that and expecting the letters to change is the mistake
        // the two controls sitting apart is meant to prevent.
        let shown_colour = shown.colour.clone().unwrap_or(Color::BLACK);
        let [r, g, b, a] = shown_colour.to_rgb_f32();
        let mut rgba = [r, g, b, a];
        if text_field(ui, Icon::Palette, "Text colour", |ui| {
            fill_picker(ui, &mut rgba, "Text colour")
        }) {
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

        // Weight and slant on one row. Bold and Italic are toggles rather than a
        // list, because that is how they are used: the numbered weights stay for
        // the faces that have them, but the pair a person reaches for constantly
        // should be one click and recognisable without reading.
        let mut weight_change = None;
        let bold = shown.weight.is_some_and(|w| w >= 600);
        let italic = shown.italic == Some(true);
        let underline = shown.underline.as_ref().is_some_and(|d| d.on);
        let strike = shown.strikethrough.as_ref().is_some_and(|d| d.on);
        match icon_toggles(
            ui,
            &[
                (Icon::Bold, "Bold", bold),
                (Icon::Italic, "Italic", italic),
                (Icon::Underline, "Underline", underline),
                (Icon::Strikethrough, "Strike", strike),
            ],
        ) {
            // Off returns to 400 rather than to inherit: a toggle that cleared
            // the property would leave a run bold whenever its style was.
            Some(0) => weight_change = Some(if bold { 400 } else { 700 }),
            Some(1) => set_character(
                state,
                story,
                target.clone(),
                CharacterFormat {
                    // `Some(false)`, not `None`: `None` means inherit and would
                    // leave the text italic when its style says so.
                    italic: Some(!italic),
                    ..CharacterFormat::default()
                },
            ),
            // Underline and strikethrough, the same way. Off keeps the
            // decoration's settings and states `on: false`, for the reason
            // italic states `Some(false)`.
            Some(which @ (2 | 3)) => {
                let (current, on) = if which == 3 {
                    (&shown.strikethrough, strike)
                } else {
                    (&shown.underline, underline)
                };
                let mut decoration = current.clone().unwrap_or_default();
                decoration.on = !on;
                let mut format = CharacterFormat::default();
                if which == 3 {
                    format.strikethrough = Some(decoration);
                } else {
                    format.underline = Some(decoration);
                }
                set_character(state, story, target.clone(), format);
            }
            _ => {}
        }
        text_field(ui, Icon::Bold, "Font weight", |ui| {
            let current = shown.weight.unwrap_or(400);
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("text-weight")
                    .width(ui.available_width())
                    .selected_text(match current {
                        300 => "Light",
                        400 => "Regular",
                        500 => "Medium",
                        600 => "Semibold",
                        700 => "Bold",
                        _ => "Custom",
                    })
                    .show_ui(ui, |ui| {
                        for (label, weight) in [
                            ("Light", 300),
                            ("Regular", 400),
                            ("Medium", 500),
                            ("Semibold", 600),
                            ("Bold", 700),
                        ] {
                            if ui.selectable_label(current == weight, label).clicked() {
                                weight_change = Some(weight);
                            }
                        }
                    })
                    .response,
                "Font weight",
                egui::WidgetType::ComboBox,
                None,
            );
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
    });

    property_card(ui, "Paragraph", |ui| {
        let alignments = [
            (Icon::TextAlignLeft, "Align left", Alignment::Left),
            (Icon::TextAlignCentre, "Align centre", Alignment::Centre),
            (Icon::TextAlignRight, "Align right", Alignment::Right),
            (Icon::TextAlignJustify, "Justify", Alignment::Justify),
        ];
        let toggles = alignments
            .map(|(icon, name, alignment)| (icon, name, paragraph.alignment == Some(alignment)));
        if let Some(alignment) = icon_toggles(ui, &toggles).map(|i| alignments[i].2) {
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

        group_label(ui, "Indents");
        let (left, right) = pair(
            ui,
            ((crate::icons::Icon::IndentLeft, "Left"), |ui: &mut Ui| {
                optional_number_bare(
                    ui,
                    Some(paragraph.indent_left.unwrap_or(0.0)),
                    0.25,
                    0.0..=1440.0,
                    " pt",
                )
            }),
            ((crate::icons::Icon::IndentRight, "Right"), |ui: &mut Ui| {
                optional_number_bare(
                    ui,
                    Some(paragraph.indent_right.unwrap_or(0.0)),
                    0.25,
                    0.0..=1440.0,
                    " pt",
                )
            }),
        );
        let first = text_field(ui, Icon::Indent, "First line", |ui| {
            optional_number_bare(
                ui,
                Some(paragraph.indent_first.unwrap_or(0.0)),
                0.25,
                -1440.0..=1440.0,
                " pt",
            )
        });
        group_label(ui, "Paragraph spacing");
        let (before, after) = pair(
            ui,
            (
                (crate::icons::Icon::SpaceBefore, "Before"),
                |ui: &mut Ui| {
                    optional_number_bare(
                        ui,
                        Some(paragraph.space_before.unwrap_or(0.0)),
                        0.25,
                        0.0..=1440.0,
                        " pt",
                    )
                },
            ),
            ((crate::icons::Icon::SpaceAfter, "After"), |ui: &mut Ui| {
                optional_number_bare(
                    ui,
                    Some(paragraph.space_after.unwrap_or(0.0)),
                    0.25,
                    -720.0..=720.0,
                    " pt",
                )
            }),
        );
        if left.is_some()
            || right.is_some()
            || first.is_some()
            || before.is_some()
            || after.is_some()
        {
            set_paragraph(
                state,
                story,
                target.clone(),
                ParagraphFormat {
                    indent_left: left,
                    indent_right: right,
                    indent_first: first,
                    space_before: before,
                    space_after: after,
                    ..ParagraphFormat::default()
                },
            );
        }
    });

    if property_disclosure(ui, state, "Character options") {
        // Tracking in thousandths of an em, the unit every type specimen uses.
        if let Some(tracking) = text_field(ui, Icon::LetterSpacing, "Tracking", |ui| {
            optional_number_bare(
                ui,
                Some(shown.tracking.unwrap_or(0.0)),
                1.0,
                -200.0..=800.0,
                " /1000 em",
            )
        }) {
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

        // Kerning: the font's table, or every pair judged from the glyphs'
        // shapes. Stated either way, so a style that says optical can be
        // overridden back to metrics on a range.
        ui.horizontal_wrapped(|ui| {
            use tessera_text::story::Kerning;
            ui.colored_label(Theme::text_muted(), "Kerning");
            let current = shown.kerning.unwrap_or(Kerning::Metrics);
            for (label, kerning, hint) in [
                (
                    "Metrics",
                    Kerning::Metrics,
                    "The pairs the font's designer set",
                ),
                (
                    "Optical",
                    Kerning::Optical,
                    "Every pair judged from the shapes of its letters",
                ),
            ] {
                if ui
                    .selectable_label(current == kerning, label)
                    .on_hover_text(hint)
                    .clicked()
                    && current != kerning
                {
                    set_character(
                        state,
                        story,
                        target.clone(),
                        CharacterFormat {
                            kerning: Some(kerning),
                            ..CharacterFormat::default()
                        },
                    );
                }
            }
        });

        // Case. A display transform, not an edit: the story keeps what was typed,
        // so turning All Caps off gives back the original capitals rather than a
        // sentence that has forgotten where they were.
        text_field(ui, Icon::CaseSensitive, "Letter case", |ui| {
            let current = shown.case.unwrap_or(Case::Normal);
            let choices = [
                (Case::Normal, "As typed"),
                (Case::Upper, "Uppercase"),
                (Case::SmallCaps, "Small caps"),
                (Case::Lower, "Lowercase"),
            ];
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("text-case")
                    .width(ui.available_width())
                    .selected_text(choices.iter().find(|(case, _)| *case == current).unwrap().1)
                    .show_ui(ui, |ui| {
                        for (case, label) in choices {
                            if ui.selectable_label(current == case, label).clicked()
                                && current != case
                            {
                                set_character(
                                    state,
                                    story,
                                    target.clone(),
                                    CharacterFormat {
                                        case: Some(case),
                                        ..Default::default()
                                    },
                                );
                            }
                        }
                    })
                    .response,
                "Case",
                egui::WidgetType::ComboBox,
                None,
            );
        });

        // The kern at the caret: between the character before it and the one
        // after. Only with a caret and not a selection, because a kern is about
        // one pair — a range has many, and tracking is the control for a range.
        if target.is_empty()
            && let Some(kern) = state
                .active()
                .editing
                .as_ref()
                .and_then(|(_, buffer)| buffer.kern_at_cursor())
            && let Some(edited) = optional_number(ui, "Kern", Some(kern), 1.0, -1000.0..=1000.0, "")
            && let Some((_, buffer)) = state.active_mut().editing.as_mut()
        {
            // undo-bracketed: written the way a keystroke is, straight into the
            // story, inside the entry the editing session opened.
            buffer.kern_by(edited - kern);
            let updated = buffer.story().clone();
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                *s = updated;
            }
            state.active_mut().dirty = true;
        }

        // The language: what the hyphenation patterns are chosen by, and what
        // the font is told. A choice, not a toggle, so it states `Some` always;
        // the document default is English.
        text_field(ui, Icon::Text, "Language", |ui| {
            use tessera_text::story::LANGUAGES;
            let current = shown.language.as_deref().unwrap_or("en");
            let name = LANGUAGES
                .iter()
                .find(|(code, _)| *code == current)
                .map_or(current, |(_, name)| *name);
            let mut chosen = None;
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("text-language")
                    .width(ui.available_width())
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (code, name) in LANGUAGES {
                            if ui.selectable_label(current == *code, *name).clicked()
                                && current != *code
                            {
                                chosen = Some((*code).to_string());
                            }
                        }
                    })
                    .response,
                "Language",
                egui::WidgetType::ComboBox,
                None,
            );
            if let Some(code) = chosen {
                set_character(
                    state,
                    story,
                    target.clone(),
                    CharacterFormat {
                        language: Some(code),
                        ..CharacterFormat::default()
                    },
                );
            }
        });

        // Baseline shift: a superscript sits above the line it belongs to without
        // making that line taller.
        if let Some(shift) = text_field(ui, Icon::BaselineShift, "Baseline shift", |ui| {
            optional_number_bare(
                ui,
                Some(shown.baseline_shift.unwrap_or(0.0)),
                0.25,
                -200.0..=200.0,
                " pt",
            )
        }) {
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
    }

    if property_disclosure(ui, state, "OpenType features") {
        // OpenType features. Each row states `Some(..)` either way, for the
        // reason italic does: `None` would inherit, and off has to mean off.
        // What a font lacks it ignores, so a control here can never make text
        // disappear — it can only fail to change it.
        crate::view::panel_ui::hint(ui, "Availability depends on the selected font.");
        let mut feature_change: Option<CharacterFormat> = None;
        ui.horizontal_wrapped(|ui| {
            let mut common = shown.ligatures != Some(false);
            if ui
                .checkbox(&mut common, "Common ligatures")
                .on_hover_text("fi, fl and the others the font sets by default")
                .clicked()
            {
                feature_change = Some(CharacterFormat {
                    ligatures: Some(common),
                    ..CharacterFormat::default()
                });
            }
            let mut discretionary = shown.discretionary_ligatures == Some(true);
            if ui
                .checkbox(&mut discretionary, "Discretionary ligatures")
                .on_hover_text("The decorative ones: st, ct, Th")
                .clicked()
            {
                feature_change = Some(CharacterFormat {
                    discretionary_ligatures: Some(discretionary),
                    ..CharacterFormat::default()
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            use tessera_text::story::FigureCase;
            ui.colored_label(Theme::text_muted(), "Number style");
            for (label, case, hint) in [
                ("Lining", FigureCase::Lining, "As tall as capitals"),
                (
                    "Old-style",
                    FigureCase::OldStyle,
                    "With ascenders and descenders",
                ),
            ] {
                if ui
                    .selectable_label(shown.figure_case == Some(case), label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    feature_change = Some(CharacterFormat {
                        figure_case: Some(case),
                        ..CharacterFormat::default()
                    });
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            use tessera_text::story::FigureWidth;
            ui.colored_label(Theme::text_muted(), "Number width");
            for (label, width, hint) in [
                (
                    "Proportional",
                    FigureWidth::Proportional,
                    "Each its own width",
                ),
                (
                    "Tabular",
                    FigureWidth::Tabular,
                    "All one width, for columns",
                ),
            ] {
                if ui
                    .selectable_label(shown.figure_width == Some(width), label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    feature_change = Some(CharacterFormat {
                        figure_width: Some(width),
                        ..CharacterFormat::default()
                    });
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let mut fractions = shown.fractions == Some(true);
            if ui
                .checkbox(&mut fractions, "Fractions")
                .on_hover_text("1/2 set as a fraction, where the font can")
                .clicked()
            {
                feature_change = Some(CharacterFormat {
                    fractions: Some(fractions),
                    ..CharacterFormat::default()
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(Theme::text_muted(), "Stylistic sets");
            let mut sets = shown
                .stylistic_sets
                .as_ref()
                .map(|s| s.iter().map(u8::to_string).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            let response = ui.add(
                egui::TextEdit::singleline(&mut sets)
                    .desired_width(60.0)
                    .hint_text("1 3 7"),
            );
            crate::icons::named(response.clone(), "Stylistic sets, by number");
            // Written when the field is left, not on every keystroke: half a
            // number is not a set.
            if response.lost_focus() {
                let parsed = parse_sets(&sets);
                if Some(&parsed) != shown.stylistic_sets.as_ref() {
                    feature_change = Some(CharacterFormat {
                        stylistic_sets: Some(parsed),
                        ..CharacterFormat::default()
                    });
                }
            }
        });
        if let Some(format) = feature_change {
            set_character(state, story, target.clone(), format);
        }
    }

    if property_disclosure(ui, state, "Drop caps") {
        // Drop cap. Zero lines is no drop cap, which is why the row reads as a
        // count rather than as a switch with a count beside it.
        // "Lines" and "Letters", under a heading that already says "Drop
        // caps": the full names wrapped to two lines in the label column.
        if let Some(lines) = optional_number(
            ui,
            "Lines",
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
                "Letters",
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
    }
    if property_disclosure(ui, state, "Line breaking") {
        // Hyphenation. English only for now: `hypher` holds its patterns per
        // language and a story has no language to pick one with.
        let mut hyphenating = paragraph.hyphenate == Some(true);
        ui.horizontal_wrapped(|ui| {
            if ui
                .checkbox(&mut hyphenating, "Hyphenate words")
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
                        hyphenate: Some(hyphenating),
                        ..ParagraphFormat::default()
                    },
                );
            }
        });

        // The composer: which breaker chooses the lines.
        {
            use tessera_text::story::Composer;
            let current = paragraph.composer.unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(Theme::text_muted(), "Composer");
                for (choice, label, hint) in [
                    (
                        Composer::SingleLine,
                        "Single-line",
                        "Each line as far as it fits",
                    ),
                    (
                        Composer::Paragraph,
                        "Paragraph",
                        "The whole paragraph's breaks weighed together, for the evenest spacing",
                    ),
                ] {
                    if ui
                        .selectable_label(current == choice, label)
                        .on_hover_text(hint)
                        .clicked()
                        && current != choice
                    {
                        set_paragraph(
                            state,
                            story,
                            target.clone(),
                            ParagraphFormat {
                                composer: Some(choice),
                                ..ParagraphFormat::default()
                            },
                        );
                    }
                }
            });
        }
        if paragraph.hyphenate == Some(true) {
            let mut hyphenation = paragraph.hyphenation;
            if hyphenation_editor(ui, &mut hyphenation, false) {
                set_paragraph(
                    state,
                    story,
                    target.clone(),
                    ParagraphFormat {
                        hyphenation: Some(hyphenation.unwrap_or_default()),
                        ..ParagraphFormat::default()
                    },
                );
            }
        }
    }
    if property_disclosure(ui, state, "Tabs") {
        // Tab stops. Edited as a whole: the list is one value in the cascade,
        // so a change to any stop writes the whole list back.
        let mut stops = paragraph.tab_stops.clone();
        if tab_stops_editor(ui, &mut stops, false) {
            set_paragraph(
                state,
                story,
                target.clone(),
                ParagraphFormat {
                    // An empty list, never `None`: `None` would mean "leave it",
                    // and removing the last stop has to mean "no stops".
                    tab_stops: Some(stops.unwrap_or_default()),
                    ..ParagraphFormat::default()
                },
            );
        }
    }
    if property_disclosure(ui, state, "Paragraph rules") {
        // Paragraph rules. Each is one value in the cascade, written whole.
        for (label, above) in [("Rule above", true), ("Rule below", false)] {
            let mut rule = if above {
                paragraph.rule_above.clone()
            } else {
                paragraph.rule_below.clone()
            };
            if paragraph_rule_editor(ui, label, &mut rule, false) {
                let mut format = ParagraphFormat::default();
                // Switched off is `Some` with `on: false`, never `None`: `None`
                // would mean "leave it", and off has to mean off.
                let rule = Some(rule.unwrap_or(ParagraphRule {
                    on: false,
                    ..ParagraphRule::default()
                }));
                if above {
                    format.rule_above = rule;
                } else {
                    format.rule_below = rule;
                }
                set_paragraph(state, story, target.clone(), format);
            }
        }
    }
    if property_disclosure(ui, state, "Bullets and numbering") {
        // List: one value, written whole. "Hang" is a convenience over the
        // indents below — an item whose turnover lines up under its text is what
        // nearly every list wants, and setting two indents by hand to get it is
        // the kind of chore a control exists to spare.
        let mut list = paragraph.list.clone();
        let (changed, hang) = list_editor(ui, &mut list, false);
        if changed {
            set_paragraph(
                state,
                story,
                target.clone(),
                ParagraphFormat {
                    list: Some(list.unwrap_or(ListFormat {
                        kind: ListKind::None,
                        ..ListFormat::default()
                    })),
                    ..ParagraphFormat::default()
                },
            );
        }
        if hang {
            set_paragraph(
                state,
                story,
                target.clone(),
                ParagraphFormat {
                    indent_left: Some(18.0),
                    indent_first: Some(-18.0),
                    tab_stops: Some(vec![tessera_text::story::TabStop::at(18.0)]),
                    ..ParagraphFormat::default()
                },
            );
        }
    }
    if property_disclosure(ui, state, "Justification") {
        // Justification and hyphenation settings: each one value, written whole.
        let mut justification = paragraph.justification;
        if justification_editor(ui, &mut justification, false) {
            set_paragraph(
                state,
                story,
                target.clone(),
                ParagraphFormat {
                    justification: Some(justification.unwrap_or_default()),
                    ..ParagraphFormat::default()
                },
            );
        }
    }

    if property_disclosure(ui, state, "Keep options") {
        // Keep options: one value, written whole.
        let mut keep = paragraph.keep;
        if keep_options_editor(ui, &mut keep, false) {
            set_paragraph(
                state,
                story,
                target.clone(),
                ParagraphFormat {
                    // Stated, never `None`: everything switched off has to mean
                    // "keep nothing", whatever the style says.
                    keep: Some(keep.unwrap_or_default()),
                    ..ParagraphFormat::default()
                },
            );
        }
    }
    if property_disclosure(ui, state, "Text frame layout") {
        text_frame_controls(ui, state, id, frame);
    }
    if property_disclosure(ui, state, "Text styles") {
        style_rows(ui, state, story, target);
    }
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
    let chosen = field(ui, "Family", |ui| family_menu(ui, state, shown, 220.0));
    for family in missing {
        ui.colored_label(
            Theme::error(),
            format!("{family} is not installed — a substitute is shown"),
        );
    }
    chosen
}

fn family_menu(
    ui: &mut Ui,
    state: &mut TesseraApp,
    shown: Option<&str>,
    max_width: f32,
) -> Option<String> {
    let mut chosen = None;
    let label = shown.unwrap_or("Mixed");
    let recent = state.recent_fonts.clone();
    crate::icons::reads_as(
        egui::ComboBox::from_id_salt("family")
            .width(ui.available_width().min(max_width))
            .truncate()
            .selected_text(label)
            .show_ui(ui, |ui| {
                if !recent.is_empty() {
                    ui.label(
                        egui::RichText::new("Recent")
                            .small()
                            .color(Theme::text_muted()),
                    );
                    for family in &recent {
                        if ui
                            .selectable_label(shown == Some(family.as_str()), family)
                            .clicked()
                        {
                            chosen = Some(family.clone());
                        }
                    }
                    ui.separator();
                }
                for family in state.shaper.families() {
                    if ui
                        .selectable_label(shown == Some(family.as_str()), family)
                        .clicked()
                    {
                        chosen = Some(family.clone());
                    }
                }
            })
            .response,
        "Family",
        egui::WidgetType::ComboBox,
        None,
    );
    if let Some(family) = &chosen {
        remember_font(&mut state.recent_fonts, family);
    }
    chosen
}

/// How many families the menu's Recent list keeps.
const RECENT_FONTS: usize = 5;

/// Put `family` at the head of `recent`, once, keeping the list short.
pub(crate) fn remember_font(recent: &mut Vec<String>, family: &str) {
    recent.retain(|f| f != family);
    recent.insert(0, family.to_string());
    recent.truncate(RECENT_FONTS);
}

fn frame_section(ui: &mut Ui, frame: &tessera_document::nodes::Frame) {
    let tessera_document::nodes::FrameKind::Group(children) = &frame.kind else {
        return;
    };
    ui.colored_label(
        Theme::text_muted(),
        format!("{} objects grouped", children.len()),
    );
}

/// A colour, as a swatch that opens a picker.
///
/// A swatch, not a bar: InDesign's is a chip beside its field. A black bar the
/// width of the panel was the heaviest thing in it, and said no more than a
/// chip does.
/// A colour swatch that opens a picker. `name` is what a screen reader says
/// for it: a swatch is a patch of colour, and without one NVDA says only
/// "button".
pub(crate) fn fill_picker(ui: &mut Ui, rgba: &mut [f32; 4], name: &str) -> bool {
    ui.spacing_mut().interact_size =
        Vec2::new(2.0 * Theme::control_height(), Theme::control_height());
    let mut colour = egui::Rgba::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    let changed = crate::icons::speak_as(
        egui::widgets::color_picker::color_edit_button_rgba(
            ui,
            &mut colour,
            egui::widgets::color_picker::Alpha::Opaque,
        ),
        name,
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

    if section_heading(ui, state, "Page format") {
        let edited = property_body(ui, |ui| {
            // The preset names a pair of numbers the user recognises; the model still
            // stores only a width and a height. "Custom" is not a value — it is what
            // no preset matching looks like.
            let current = PagePreset::matching(width, height);
            let mut wanted = None;
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("page-preset")
                    .width(ui.available_width())
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
                    })
                    .response,
                "Page size",
                egui::WidgetType::ComboBox,
                None,
            );
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
                return true;
            }

            let orientation = Orientation::of(width, height);
            let mut desired_orientation = orientation;
            icon_choices(
                ui,
                "Orientation",
                &mut desired_orientation,
                &[
                    (
                        crate::icons::Icon::PagePortrait,
                        "Portrait",
                        "Taller than it is wide",
                        Orientation::Portrait,
                    ),
                    (
                        crate::icons::Icon::PageLandscape,
                        "Landscape",
                        "Wider than it is tall",
                        Orientation::Landscape,
                    ),
                ],
            );
            if desired_orientation != orientation {
                let (width, height) = desired_orientation.apply(width, height);
                apply(state, Command::SetPageSize { width, height });
                return true;
            }

            let (w_changed, h_changed) = pair(
                ui,
                ("Width", |ui: &mut Ui| measure_bare(ui, &mut width, unit)),
                ("Height", |ui: &mut Ui| measure_bare(ui, &mut height, unit)),
            );
            if w_changed || h_changed {
                apply(state, Command::SetPageSize { width, height });
                return true;
            }

            false
        });
        if edited {
            return;
        }
    }

    let mut changed = false;

    ui.add_space(Theme::space_3());
    let margins_open = section_heading(ui, state, "Margins & columns");
    // Four edges are two pairs, not four rows: top against bottom and one
    // side against the other are the comparisons a person actually makes.
    let document_key = state.active;
    let edges = |ui: &mut Ui,
                 title: &str,
                 v: (&mut f64, &mut f64),
                 h: ((&str, &mut f64), (&str, &mut f64))| {
        linked_edges(
            ui,
            egui::Id::new(("page-edge-link", document_key, title)),
            title,
            ["Top", "Bottom", h.0.0, h.1.0],
            [v.0, v.1, h.0.1, h.1.1],
            unit,
        )
    };

    if margins_open {
        property_body(ui, |ui| {
            changed |= ui
                .checkbox(&mut setup.facing_pages, "Facing pages")
                .changed();
            let (near, far) = if setup.facing_pages {
                ("Inside", "Outside")
            } else {
                ("Left", "Right")
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
            // Column guides, next to the margins they subdivide.
            ui.add_space(Theme::space_3());
            group_label(ui, "Columns");
            let mut count = f64::from(setup.columns.max(1));
            let (i, j) = pair(
                ui,
                ((crate::icons::Icon::Columns, "Count"), |ui: &mut Ui| {
                    ui.add(
                        egui::DragValue::new(&mut count)
                            .speed(0.1)
                            .range(1.0..=20.0),
                    )
                    .changed()
                }),
                ((crate::icons::Icon::Gutter, "Gutter"), |ui: &mut Ui| {
                    measure_bare(ui, &mut setup.column_gutter, unit)
                }),
            );
            if i {
                setup.columns = count.round().clamp(1.0, 20.0) as u8;
            }
            changed |= i || j;
        });
    }

    // The baseline grid, with the document's other page-wide rhythms.
    ui.add_space(Theme::space_3());
    if section_heading(ui, state, "Layout guides") {
        property_body(ui, |ui| {
            ui.label(
                egui::RichText::new("Keep text aligned across columns.")
                    .small()
                    .color(Theme::text_muted()),
            );
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
        });
    }

    ui.add_space(Theme::space_3());
    if section_heading(ui, state, "Print production") {
        property_body(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Bleed extends artwork beyond the trim. Slug adds space for production notes.",
                )
                .small()
                .color(Theme::text_muted()),
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
            changed |= edges(
                ui,
                "Slug",
                (&mut setup.slug.top, &mut setup.slug.bottom),
                (
                    ("Left", &mut setup.slug.left),
                    ("Right", &mut setup.slug.right),
                ),
            );
        });
    }

    if changed {
        // One command for the whole struct: a page-setup edit is one undo
        // entry, not one per field touched.
        apply(state, Command::SetDocumentSetup(setup));
    }

    ui.add_space(Theme::space_3());
    if section_heading(ui, state, "Colour management") {
        property_body(ui, |ui| {
            output_intent_controls(ui, state);
        });
    }

    ui.add_space(Theme::space_4());
    ui.colored_label(
        Theme::text_muted(),
        format!("Measurements in {}", unit_name(unit)),
    );
}

/// Which press the document is for, and whether it is being shown that way.
///
/// In document setup rather than in a preferences dialog, because *which press*
/// is a property of the job: it travels with the file, it is what the printer
/// needs to know, and it belongs beside the trim and the bleed which are the
/// same kind of fact. Whether you are *looking* through it is not, and reads as
/// the switch it is.
fn output_intent_controls(ui: &mut Ui, state: &mut TesseraApp) {
    use tessera_document::intent::Rendering;

    ui.add_space(Theme::space_4());
    group_label(ui, "Output intent");

    let intent = state.active().document().output_intent.clone();
    let Some(mut intent) = intent else {
        ui.colored_label(Theme::text_muted(), "No press chosen.")
            .on_hover_text(
                "Without one, colours are shown as an approximation \
                 rather than as they will print",
            );
        profile_picker(ui, state, None);
        return;
    };

    // The profile’s own name for itself, not the file it came from: profiles are
    // renamed, copied and re-supplied.
    ui.label(&intent.description);

    let mut showing = state.soft_proof.showing;
    if ui
        .checkbox(&mut showing, "Soft proof")
        .on_hover_text("Show the document as this press will reproduce it (Ctrl+Y)")
        .changed()
    {
        // Not a document edit, so not a command: which press is part of the job,
        // whether you are looking through it is a way of working.
        state.soft_proof.showing = showing;
    }

    // Why a proof asked for is not appearing. Somebody who ticked the box and saw
    // nothing change is entitled to know.
    if let Some(trouble) = state.soft_proof.trouble.clone() {
        ui.colored_label(Theme::error(), trouble);
    }

    let before = intent.rendering;
    property_field(ui, "Intent", |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("output-intent-rendering")
                .selected_text(intent.rendering.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for rendering in Rendering::ALL {
                        ui.selectable_value(&mut intent.rendering, rendering, rendering.label());
                    }
                })
                .response,
            "Rendering intent",
            egui::WidgetType::ComboBox,
            None,
        );
    });

    let changed = intent.rendering != before;
    profile_picker(ui, state, Some(&intent.description));

    if ui
        .button("Remove")
        .on_hover_text("The document stops being prepared for any particular press")
        .clicked()
    {
        apply(state, Command::SetOutputIntent(None));
        state.soft_proof.showing = false;
        return;
    }

    if changed {
        apply(state, Command::SetOutputIntent(Some(Box::new(intent))));
    }
}

/// The list of profiles a person can choose from.
///
/// **The standard spaces are built, and the presses are found.** An RGB working
/// space is defined by published numbers, so it is constructed here and is always
/// on offer. A CMYK profile is measured data and can only come from a file — and
/// the familiar ones belong to Adobe, so they are not bundled. They are read from
/// where the operating system and the other creative applications already keep
/// them, which means a document proofed here is proofed against the same bytes
/// the next application will use.
fn profile_picker(ui: &mut Ui, state: &mut TesseraApp, current: Option<&str>) {
    let choices = state.profiles.choices();
    let mut chosen: Option<crate::catalogue::Choice> = None;

    property_field(ui, "Profile", |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("output-intent-profile")
                .selected_text(current.unwrap_or("Choose..."))
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    let mut heading = "";
                    for choice in &choices {
                        // Grouped by where it came from, because "built in" and "on
                        // this machine" are different promises: one is always there,
                        // the other depends on what is installed.
                        let group = match choice {
                            crate::catalogue::Choice::Standard(_) => "Standard spaces",
                            crate::catalogue::Choice::Bundled(_) => "Shipped with Tessera",
                            crate::catalogue::Choice::Installed(_) => "On this machine",
                        };
                        if group != heading {
                            if !heading.is_empty() {
                                ui.separator();
                            }
                            ui.colored_label(Theme::text_muted(), group);
                            heading = group;
                        }

                        // The space beside the name, because that is the thing a
                        // person is choosing on: a CMYK entry is a press and an RGB
                        // one is not.
                        let label = format!("{}  · {}", choice.label(), choice.space());
                        let selected = current == Some(choice.label().as_str());
                        let mut response = ui.selectable_label(selected, label);
                        if let crate::catalogue::Choice::Standard(standard) = choice {
                            response = response.on_hover_text(standard.purpose());
                        }
                        if response.clicked() {
                            chosen = Some(choice.clone());
                        }
                    }
                })
                .response,
            "Output profile",
            egui::WidgetType::ComboBox,
            None,
        );
    });

    if let Some(choice) = chosen {
        crate::file_ops::adopt_output_intent(state, &choice);
        return;
    }

    ui.horizontal(|ui| {
        if ui
            .button("Browse...")
            .on_hover_text("Choose a profile from anywhere on disk")
            .clicked()
        {
            crate::file_ops::choose_output_intent(state);
        }
        // Offered rather than done automatically: somebody who has just installed
        // a profile knows they have, and re-scanning on every frame to catch it
        // would read the disk for nothing the rest of the time.
        if ui
            .small_button("Look again")
            .on_hover_text("Re-scan for profiles installed since Tessera started")
            .clicked()
        {
            state.profiles.refresh();
        }
    });

    let found = state.profiles.installed_count();
    if found == 0 {
        // Said plainly, because a person expecting the familiar CMYK presses and
        // not finding them should know it is the machine and not Tessera.
        ui.colored_label(
            Theme::text_muted(),
            "No profiles installed on this machine.",
        )
        .on_hover_text(
            "CMYK profiles are measured data and cannot be computed, so \
                 Tessera reads the ones the system and other applications \
                 install. Use Browse to point at one anywhere on disk.",
        );
    }
}

pub(crate) fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::Millimetres => "millimetres",
        Unit::Points => "points",
        Unit::Pixels => "pixels",
        Unit::Inches => "inches",
        Unit::Picas => "picas",
    }
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
        // Reserve the navigator and zoom controls before giving documents a
        // scrollable region; long names and status messages cannot push them out.
        let left_width = (ui.available_width() - 490.0).max(140.0);
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, 24.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let (message, tint) = match &state.status {
                    Some(s) => (
                        s.message.as_str(),
                        if s.is_error {
                            Theme::error()
                        } else {
                            Theme::text_muted()
                        },
                    ),
                    None => (state.active_tool.label(), Theme::text_muted()),
                };
                let text_width = ui
                    .painter()
                    .layout_no_wrap(
                        message.to_owned(),
                        egui::FontId::proportional(Theme::TYPE_SM),
                        tint,
                    )
                    .size()
                    .x;
                let message_width = text_width.min((left_width * 0.4).min(180.0));
                // Drawn in the size it was measured in. It was measured small
                // and drawn at the body size, so every message was a few
                // points wider than its room and read "New docum..." for ever.
                ui.add_sized(
                    egui::vec2(message_width, 20.0),
                    egui::Label::new(
                        egui::RichText::new(message)
                            .size(Theme::TYPE_SM)
                            .color(tint),
                    )
                    .truncate(),
                )
                .on_hover_text(message);
                ui.separator();
                super::document_tabs::show(ui, state);
            },
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The document’s state, always visible. Somebody who has to open a
            // panel to find out whether their document is sendable will open it
            // once, at the beginning, and never again.
            crate::view::preflight_panel::indicator(ui, state);
            ui.separator();

            let mut percent = state.active().view.zoom * 100.0;
            if crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut percent)
                        .speed(1.0)
                        .range(5.0..=1600.0)
                        .suffix("%"),
                ),
                "Zoom",
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

    group_label(ui, "Styles");

    // --- paragraph styles

    let mut attach_paragraph = None;
    let mut define_paragraph = false;
    text_label(ui, crate::icons::Icon::Pilcrow, "Paragraph style");
    ui.horizontal(|ui| {
        let label = paragraph_style
            .and_then(|id| paragraphs.iter().find(|(p, _)| *p == id))
            .map_or("None", |(_, name)| name.as_str())
            .to_string();
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("paragraph-style")
                .truncate()
                .width((ui.available_width() - 24.0 - ui.spacing().item_spacing.x).max(0.0))
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
                })
                .response,
            "Paragraph style",
            egui::WidgetType::ComboBox,
            None,
        );
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
    text_label(ui, crate::icons::Icon::CaseSensitive, "Character style");
    ui.horizontal(|ui| {
        let label = character_style
            .and_then(|id| characters.iter().find(|(c, _)| *c == id))
            .map_or("None", |(_, name)| name.as_str())
            .to_string();
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("character-style")
                .truncate()
                .width((ui.available_width() - 24.0 - ui.spacing().item_spacing.x).max(0.0))
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
                })
                .response,
            "Character style",
            egui::WidgetType::ComboBox,
            None,
        );
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

    ui.add_space(Theme::space_2());
    if character_overrides || paragraph_overrides {
        ui.colored_label(
            Theme::error(),
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

    ui.colored_label(Theme::text_muted(), format!("{number} of {pages}"));

    if glyph_button(ui, crate::icons::Icon::ChevronLeft, "Previous spread").clicked() && at > 0 {
        state.active_mut().current_spread = at - 1;
        state.active_mut().fitted = false;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn linked_edges_follow_whichever_side_was_edited() {
        for side in 0..4 {
            let mut values = [10.0, 20.0, 30.0, 40.0];
            values[side] = 7.5;
            let mut edited = [false; 4];
            edited[side] = true;
            assert!(propagate_linked_edit(&mut values, edited, true));
            assert_eq!(values, [7.5; 4]);
        }
    }

    #[test]
    fn unlinked_edges_and_unedited_linked_edges_keep_their_values() {
        let original = [10.0, 20.0, 30.0, 40.0];
        let mut values = original;
        assert!(propagate_linked_edit(
            &mut values,
            [true, false, false, false],
            false
        ));
        assert_eq!(values, original);
        assert!(!propagate_linked_edit(&mut values, [false; 4], true));
        assert_eq!(values, original, "linking alone must not normalize values");
    }

    #[test]
    fn changing_linked_page_margins_is_one_undoable_edit() {
        let mut state = TesseraApp::headless();
        let original = state.active().document().setup;
        let mut setup = original;
        let mut edges = [
            setup.margins.top,
            23.0,
            setup.margins.inside,
            setup.margins.outside,
        ];
        propagate_linked_edit(&mut edges, [false, true, false, false], true);
        setup.margins.top = edges[0];
        setup.margins.bottom = edges[1];
        setup.margins.inside = edges[2];
        setup.margins.outside = edges[3];
        apply(&mut state, Command::SetDocumentSetup(setup));
        assert_eq!(
            state.active().document().setup.margins,
            tessera_document::nodes::Margins::uniform(23.0)
        );
        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().setup, original);
    }

    /// Draw something in a context that is building an accessibility tree, and
    /// hand back what a screen reader would be given.
    ///
    /// **The tree is the evidence.** Asserting that `widget_info` is *called*
    /// would be asserting that the source says what it says; this asks egui what
    /// it actually built, which is the thing a screen reader reads.
    fn accessibility_tree(draw: impl FnMut(&mut Ui)) -> Vec<(String, Option<String>)> {
        let ctx = egui::Context::default();
        // The theme, fonts and all, as the window has it: a panel drawing its
        // heading in the interface's semibold is drawing in a family only
        // the theme installs.
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        // `run_ui` rather than `run`: egui 0.35 hands the application a root
        // `Ui` and panels nest inside it, which is the same shape `view::show`
        // is built around.
        let output = crate::headless_frame::frame(&ctx, egui::RawInput::default(), draw);
        let update = output
            .platform_output
            .accesskit_update
            .expect("accessibility was enabled, so there is a tree");
        update
            .nodes
            .iter()
            .map(|(_, node)| {
                // A node Tab can land on whose role egui never set: NVDA says
                // "unknown" for it, named or not.
                let role = if node.role() == egui::accesskit::Role::Unknown
                    && node.supports_action(egui::accesskit::Action::Focus)
                {
                    FOCUSABLE_UNKNOWN.to_string()
                } else {
                    format!("{:?}", node.role())
                };
                (role, node.label().map(ToString::to_string))
            })
            .collect()
    }

    /// What [`accessibility_tree`] calls a focusable node with no role.
    const FOCUSABLE_UNKNOWN: &str = "focusable, unknown";

    /// The roles a person can reach, focus and act on.
    ///
    /// `SpinButton` is a number field. It was missing from this list, and NVDA
    /// walking the Properties panel said "spin button, 264.58 mm" four times
    /// over with nothing to say which was X and which was H.
    const INTERACTIVE: [&str; 7] = [
        "Button",
        "RadioButton",
        "CheckBox",
        "ComboBox",
        "Link",
        "SpinButton",
        "ColorWell",
    ];

    #[test]
    fn every_tool_tells_a_screen_reader_its_name() {
        // **Nothing in Tessera did this before, and it was worse than expected.**
        // Removing the naming and running this again finds *zero* interactive
        // nodes, not a dozen unnamed ones: a response built by hand out of
        // `allocate_exact_size` never enters the accessibility tree at all
        // unless something gives it a `WidgetInfo`. So every tool, every panel
        // tab and every section disclosure was not merely anonymous to a screen
        // reader — it was invisible to one.
        let mut state = TesseraApp::headless();
        let named = accessibility_tree(|ui| tool_strip(ui, &mut state));

        let interactive: Vec<_> = named
            .iter()
            .filter(|(role, _)| INTERACTIVE.contains(&role.as_str()))
            .collect();

        // Counted, so that this fails loudly rather than quietly checking
        // nothing if egui's roles are ever renamed. A test that silently
        // inspects an empty list is not evidence.
        assert_eq!(
            interactive.len(),
            Tool::ALL.len(),
            "expected one reachable control per tool, found {interactive:?}"
        );

        for (role, label) in interactive {
            let label = label.as_deref().unwrap_or("");
            assert!(
                !label.is_empty(),
                "a {role} reached the accessibility tree with no name"
            );
        }
    }

    #[test]
    fn no_control_in_the_docked_panels_reaches_a_screen_reader_unnamed() {
        // Broader than the tool strip, and the guard that matters: this fails
        // the day somebody adds a hand-built clickable widget and forgets to
        // name it. `allocate_exact_size` plus a painter is how most of this
        // interface is drawn, and nothing about that spelling makes a name
        // appear — which is exactly why every one of them was missing.
        //
        // Nothing selected, a shape and a text frame: each puts different
        // sections in the panels and the control bar, and nothing selected
        // alone never drew a Transform, Fill or Stroke field to check.
        let mut shape = TesseraApp::headless();
        apply(
            &mut shape,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 60.0,
            }),
        );
        let (mut text, _, _) = a_text_frame("Words");
        no_control_reaches_a_screen_reader_unnamed(&mut TesseraApp::headless());
        no_control_reaches_a_screen_reader_unnamed(&mut text);
        let fields = no_control_reaches_a_screen_reader_unnamed(&mut shape);

        // And the names are the right ones: a selected shape's geometry, which
        // is what NVDA read out as five bare numbers. Checked by name, so a
        // Transform section that stopped drawing fails here rather than
        // passing for having no fields to name.
        for name in ["X", "Y", "Width", "Height", "Rotation", "Opacity"] {
            assert!(
                fields.iter().any(|f| f == name),
                "no number field is named {name:?}: {fields:?}"
            );
        }
    }

    #[test]
    fn no_control_in_a_dialog_reaches_a_screen_reader_unnamed() {
        // The dialogs are windows of their own, which the docked-panel test
        // never opens. NVDA found the New Document dialog's facing-pages box
        // as "check box, checked" and nothing more.
        let (mut state, _, _) = a_text_frame("Words");
        state.new_document.open = true;
        state.export.open = true;
        state.print.open = true;
        state.step.open = true;
        state.variables.open = true;
        state.contents.open = true;
        state.cross_reference.open = true;
        state.hyperlink.open = true;
        state.footnote_options.open = true;

        // Two frames: a window lays itself out unseen on its first.
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            crate::view::new_document::show(&ctx, &mut state);
            crate::view::export_dialog::show(&ctx, &mut state);
            crate::view::print_dialog::show(&ctx, &mut state);
            crate::view::step_repeat::show(&ctx, &mut state);
            crate::view::variables::show(&ctx, &mut state);
            crate::view::long_document::show(&ctx, &mut state);
            crate::view::cross_reference::show(&ctx, &mut state);
            crate::view::hyperlink::show(&ctx, &mut state);
            crate::view::footnote_options::show(&ctx, &mut state);
        };
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), &mut draw);
        let output = crate::headless_frame::frame(&ctx, egui::RawInput::default(), &mut draw);
        let nodes: Vec<(String, Option<String>)> = output
            .platform_output
            .accesskit_update
            .expect("accessibility was enabled, so there is a tree")
            .nodes
            .iter()
            .map(|(_, node)| {
                (
                    format!("{:?}", node.role()),
                    node.label().map(ToString::to_string),
                )
            })
            .collect();
        let interactive: Vec<_> = nodes
            .iter()
            .filter(|(role, _)| INTERACTIVE.contains(&role.as_str()))
            .collect();

        // The dialogs were drawn, or this checks nothing: the box NVDA
        // stumbled on is there, by name.
        assert!(
            interactive
                .iter()
                .any(|(_, label)| label.as_deref() == Some("Facing pages")),
            "the New Document dialog did not draw: {interactive:?}"
        );
        let nameless: Vec<_> = interactive
            .iter()
            .filter(|(_, label)| label.as_deref().unwrap_or("").is_empty())
            .collect();
        assert!(
            nameless.is_empty(),
            "{} control(s) in a dialog reached the accessibility tree with no name: \
             {nameless:?}",
            nameless.len()
        );
    }

    /// Draws everything docked round the canvas for `state`, fails on any
    /// control with no name, and hands back the number fields' names.
    fn no_control_reaches_a_screen_reader_unnamed(state: &mut TesseraApp) -> Vec<String> {
        let named = accessibility_tree(|ui| {
            crate::view::docks::show(ui, state);
            tool_strip(ui, state);
            crate::view::control::show(ui, state);
            status_bar(ui, state);
            crate::view::rulers::zero_point(ui, state);
            crate::view::rulers::unit_selector(ui, state);
        });

        // NVDA found two of these: the docks' splitter and the rulers' zero
        // point, each announced as "unknown" and nothing else.
        let unknown: Vec<_> = named
            .iter()
            .filter(|(role, _)| role == FOCUSABLE_UNKNOWN)
            .collect();
        assert!(
            unknown.is_empty(),
            "Tab lands on {} control(s) a screen reader can only call unknown: {unknown:?}",
            unknown.len()
        );
        assert!(
            named
                .iter()
                .any(|(role, label)| role == "Splitter"
                    && label.as_deref() == Some("Right dock width")),
            "the dock's splitter is not named, or egui's handle id moved: {named:?}"
        );

        let interactive: Vec<_> = named
            .iter()
            .filter(|(role, _)| INTERACTIVE.contains(&role.as_str()))
            .collect();

        // More than the tools alone, or the docked panels contributed nothing
        // and this is quietly checking the same strip twice.
        assert!(
            interactive.len() > Tool::ALL.len(),
            "the docked panels put no reachable control in the tree: {interactive:?}"
        );

        let nameless: Vec<_> = interactive
            .iter()
            .filter(|(_, label)| label.as_deref().unwrap_or("").is_empty())
            .collect();
        assert!(
            nameless.is_empty(),
            "{} control(s) reached the accessibility tree with no name: {nameless:?}",
            nameless.len()
        );
        interactive
            .iter()
            .filter(|(role, _)| role == "SpinButton")
            .filter_map(|(_, label)| label.clone())
            .collect()
    }

    #[test]
    fn tab_reaches_the_tools_from_the_keyboard() {
        // The other half of the requirement: named is not the same as
        // reachable. A control a screen reader can describe and a keyboard
        // cannot get to is one it can only describe.
        //
        // **This passed before the names were added**, because `Sense::click()`
        // is enough to make a widget focusable in egui — so it is a guard
        // against losing that, not evidence of having fixed it. Checked by
        // removing the naming and running it again, which is the only way to
        // know which of the two a green test is.
        let mut state = TesseraApp::headless();
        let ctx = egui::Context::default();

        // One frame to lay the strip out — nothing is focusable before it
        // exists — then a Tab into it.
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            tool_strip(ui, &mut state)
        });

        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let _ = crate::headless_frame::frame(&ctx, input, |ui| tool_strip(ui, &mut state));

        assert!(
            ctx.memory(|m| m.focused()).is_some(),
            "Tab reached nothing: the tool strip cannot be entered from the              keyboard, whatever its controls are called"
        );
    }

    #[test]
    fn a_tool_says_whether_it_is_the_one_in_use() {
        // A strip of identical squares does not say which one is active, and
        // that is the one thing somebody choosing a tool needs to know. The
        // pointer is the tool a fresh application starts in.
        let mut state = TesseraApp::headless();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let output = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            tool_strip(ui, &mut state)
        });
        let update = output.platform_output.accesskit_update.expect("a tree");
        let toggled = update
            .nodes
            .iter()
            .filter(|(_, node)| node.toggled().is_some())
            .count();
        assert_eq!(
            toggled,
            Tool::ALL.len(),
            "a tool that does not report whether it is selected leaves somebody              pressing keys to find out which one they are in"
        );
    }

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
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            anchor: None,
            style: None,
            hidden: false,
            locked: false,
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
    fn selecting_a_text_frame_exposes_its_font_without_entering_text_editing() {
        let (mut state, _, _) = a_text_frame("A heading");
        assert!(state.active().editing.is_none());
        let tree = accessibility_tree(|ui| inspector(ui, &mut state));
        assert!(
            tree.iter()
                .any(|(role, label)| role == "ComboBox" && label.as_deref() == Some("Family")),
            "a selected text frame must expose its font picker: {tree:?}"
        );
    }

    #[test]
    fn inspector_content_fits_a_narrow_dock() {
        for width in [208.0, 288.0] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            for mut state in [TesseraApp::headless(), a_text_frame("A heading").0] {
                let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
                    ui.set_width(width);
                    let left = ui.cursor().left();
                    inspector(ui, &mut state);
                    assert!(
                        ui.min_rect().right() <= left + width + 1.0,
                        "inspector grew beyond {width} points: {:?}",
                        ui.min_rect()
                    );
                });
            }
        }
    }

    #[test]
    fn expanded_object_properties_fit_narrow_docks() {
        for width in [208.0, 288.0] {
            for kind in [
                "rectangle",
                "ellipse",
                "empty artwork",
                "artwork",
                "path",
                "path text",
                "group",
                "table",
                "document",
            ] {
                let ctx = egui::Context::default();
                crate::theme::apply(&ctx);
                let mut state = object_properties_fixture(kind);
                for _ in 0..2 {
                    let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
                        ui.set_width(width);
                        let left = ui.cursor().left();
                        inspector(ui, &mut state);
                        assert!(
                            ui.min_rect().right() <= left + width + 1.0,
                            "{kind} overflows {width}: {:?}",
                            ui.min_rect()
                        );
                    });
                }
            }
        }
    }

    fn object_properties_fixture(kind: &str) -> TesseraApp {
        use tessera_document::{
            nodes::{Insets, Stroke, TextWrap, WrapTo},
            paint::{Gradient, Ramp, Stop},
        };
        let mut state = TesseraApp::headless();
        for section in Section::ALL {
            state.sections.set_open(section.title(), true);
        }
        for title in ["Layout guides", "Print production", "Colour management"] {
            state.sections.set_open(title, true);
        }
        if kind == "document" {
            state.active_mut().document_mut().setup.baseline_grid =
                Some(tessera_document::nodes::BaselineGrid {
                    start: 0.0,
                    step: 12.0,
                });
            return state;
        }
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().unwrap();
        let frame_kind = match kind {
            "ellipse" => FrameKind::Ellipse,
            "empty artwork" => FrameKind::Graphic { placed: None },
            "artwork" => {
                let link = state.active_mut().document_mut().links.insert(tessera_document::links::Link::new(
                    "a deliberately long artwork filename that should wrap in the inspector.png", (600.0, 400.0)));
                FrameKind::Graphic {
                    placed: Some(tessera_document::graphic::Placement {
                        link,
                        inner: Transform::IDENTITY,
                    }),
                }
            }
            "path" | "path text" => {
                let mut path = kurbo::BezPath::new();
                path.move_to((0.0, 0.0));
                path.line_to((200.0, 100.0));
                FrameKind::Path(path)
            }
            "group" => FrameKind::Group(Vec::new()),
            "table" => {
                let story = state
                    .active_mut()
                    .document_mut()
                    .stories
                    .insert(tessera_text::story::Story::new(""));
                FrameKind::Table(tessera_document::table::new(1, 1, 200.0, || story))
            }
            _ => FrameKind::Rectangle,
        };
        let style = if kind == "rectangle" {
            let mut style = tessera_document::object_style::ObjectStyle::new(
                "A deliberately long object style name to verify narrow panels",
            );
            style.format.fill = Some(Paint::Solid(Color::WHITE));
            let doc = state.active_mut().document_mut();
            let style = doc.object_styles.insert(style);
            doc.object_style_order.push(style);
            Some(style)
        } else {
            None
        };
        let frame = state.active_mut().document_mut().frame_mut(id).unwrap();
        frame.style = style;
        frame.kind = frame_kind;
        frame.fill = Paint::Gradient(Gradient::new(
            Ramp::Linear { angle: 45.0 },
            vec![
                Stop {
                    at: 0.0,
                    colour: Color::BLACK,
                },
                Stop {
                    at: 0.5,
                    colour: Color::WHITE,
                },
                Stop {
                    at: 1.0,
                    colour: Color::BLACK,
                },
            ],
        ));
        let mut stroke = Stroke::new(Color::BLACK, 2.0);
        stroke.dashes = vec![6.0, 4.0];
        frame.stroke = Some(stroke);
        frame.shadow = Some(tessera_document::shadow::Shadow::TYPICAL);
        frame.corners.radii = [1.0, 2.0, 3.0, 4.0];
        frame.wrap = TextWrap::Bounds {
            standoff: Insets::default(),
            sides: WrapTo::Both,
        };
        if kind == "path text" {
            apply(
                &mut state,
                Command::PutTextOnPath {
                    id,
                    text: "Type along this path".into(),
                },
            );
        }
        state
    }

    #[test]
    fn object_inspector_actions_update_the_selected_object() {
        for label in [
            "Enable stroke",
            "Casts a shadow",
            "Add colour stop",
            "Remove stop",
        ] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            ctx.enable_accesskit();
            let mut state = object_properties_fixture("rectangle");
            let id = state.active().selection.single().unwrap();
            let raw = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(288.0, 5000.0),
                )),
                ..Default::default()
            };
            let output = crate::headless_frame::frame(&ctx, raw(), |ui| inspector(ui, &mut state));
            let bounds = output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .and_then(|(_, node)| node.bounds())
                .expect("a named action with a hit target");
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            for pressed in [true, false] {
                let mut input = raw();
                input.events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ];
                let _ = crate::headless_frame::frame(&ctx, input, |ui| inspector(ui, &mut state));
            }
            let frame = state.active().document().frame(id).unwrap();
            match label {
                "Enable stroke" => assert!(frame.stroke.is_none()),
                "Casts a shadow" => assert!(frame.shadow.is_none()),
                "Add colour stop" => assert_eq!(frame.fill.gradient().unwrap().stops().len(), 4),
                "Remove stop" => assert_eq!(frame.fill.gradient().unwrap().stops().len(), 2),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn stroke_icon_choices_apply_the_named_option_and_undo() {
        use tessera_document::nodes::{LineCap, LineJoin};
        for label in [
            "Butt cap",
            "Round cap",
            "Projecting square cap",
            "Miter join",
            "Round join",
            "Bevel join",
            "Solid stroke",
            "Dashed stroke",
            "Dotted stroke",
        ] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            ctx.enable_accesskit();
            let mut state = object_properties_fixture("rectangle");
            let id = state.active().selection.single().unwrap();
            let stroke = state
                .active_mut()
                .document_mut()
                .frame_mut(id)
                .unwrap()
                .stroke
                .as_mut()
                .unwrap();
            stroke.cap = LineCap::Square;
            stroke.join = LineJoin::Bevel;
            stroke.dashes = vec![1.0, 3.0, 2.0, 4.0];
            let before = stroke.clone();
            let mut expected = before.clone();
            match label {
                "Butt cap" => expected.cap = LineCap::Butt,
                "Round cap" => expected.cap = LineCap::Round,
                "Projecting square cap" => expected.cap = LineCap::Square,
                "Miter join" => expected.join = LineJoin::Miter,
                "Round join" => expected.join = LineJoin::Round,
                "Bevel join" => expected.join = LineJoin::Bevel,
                "Solid stroke" => expected.dashes.clear(),
                "Dashed stroke" => expected.dashes = vec![6.0, 4.0],
                "Dotted stroke" => {
                    expected.dashes = vec![0.0, 4.0];
                    expected.cap = LineCap::Round;
                }
                _ => unreachable!(),
            }
            let draw = |ui: &mut Ui, state: &mut TesseraApp| {
                let frame = state.active().document().frame(id).unwrap().clone();
                ui.set_width(208.0);
                property_body(ui, |ui| stroke_section(ui, state, id, &frame));
            };
            let output =
                crate::headless_frame::frame(&ctx, Default::default(), |ui| draw(ui, &mut state));
            assert_eq!(
                state.active().document().frame(id).unwrap().stroke.as_ref(),
                Some(&before),
                "drawing must preserve custom patterns"
            );
            let tree = output.platform_output.accesskit_update.unwrap();
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .and_then(|(_, node)| node.bounds())
                .expect("a named icon button");
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            for pressed in [true, false] {
                let input = egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    ..Default::default()
                };
                let _ = crate::headless_frame::frame(&ctx, input, |ui| draw(ui, &mut state));
            }
            assert_eq!(
                state.active().document().frame(id).unwrap().stroke.as_ref(),
                Some(&expected),
                "{label}"
            );
            if expected != before {
                apply(&mut state, Command::Undo);
                assert_eq!(
                    state.active().document().frame(id).unwrap().stroke.as_ref(),
                    Some(&before),
                    "one undo restores {label}"
                );
            }
        }
    }

    #[test]
    fn wrap_mode_glyphs_apply_the_named_mode_and_undo() {
        use tessera_document::nodes::TextWrap;
        for label in ["No wrap", "Around the box", "Around the shape", "Jump over"] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            ctx.enable_accesskit();
            let mut state = object_properties_fixture("rectangle");
            let id = state.active().selection.single().unwrap();
            let before = state.active().document().frame(id).unwrap().wrap;
            let draw = |ui: &mut Ui, state: &mut TesseraApp| {
                let frame = state.active().document().frame(id).unwrap().clone();
                ui.set_width(208.0);
                property_body(ui, |ui| wrap_controls(ui, state, id, &frame));
            };
            let output =
                crate::headless_frame::frame(&ctx, Default::default(), |ui| draw(ui, &mut state));
            let tree = output.platform_output.accesskit_update.unwrap();
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .and_then(|(_, node)| node.bounds())
                .expect("a named wrap glyph");
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            for pressed in [true, false] {
                let input = egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    ..Default::default()
                };
                let _ = crate::headless_frame::frame(&ctx, input, |ui| draw(ui, &mut state));
            }
            let after = &state.active().document().frame(id).unwrap().wrap;
            let matches = match label {
                "No wrap" => matches!(after, TextWrap::None),
                "Around the box" => matches!(after, TextWrap::Bounds { .. }),
                "Around the shape" => matches!(after, TextWrap::Contour { .. }),
                "Jump over" => matches!(after, TextWrap::Jump),
                _ => unreachable!(),
            };
            assert!(matches, "{label} left the wrap as {after:?}");
            if *after != before {
                apply(&mut state, Command::Undo);
                assert_eq!(
                    state.active().document().frame(id).unwrap().wrap,
                    before,
                    "one undo restores {label}"
                );
            }
        }
    }

    #[test]
    fn expanded_text_properties_fit_a_narrow_dock() {
        for width in [176.0, 208.0, 256.0, 288.0] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            let (mut state, id, story) = a_text_frame("A heading");
            state.sections = crate::app::Sections::default();
            for title in [
                "Character options",
                "OpenType features",
                "Drop caps",
                "Line breaking",
                "Tabs",
                "Paragraph rules",
                "Bullets and numbering",
                "Justification",
                "Keep options",
                "Text frame layout",
                "Text styles",
            ] {
                state.sections.set_open(title, true);
            }
            set_paragraph(
                &mut state,
                story,
                0..9,
                ParagraphFormat {
                    tab_stops: Some(vec![tessera_text::story::TabStop::at(36.0)]),
                    hyphenate: Some(true),
                    list: Some(ListFormat {
                        kind: ListKind::Number,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            );
            let frame = state.active().document().frame(id).unwrap().clone();
            for _ in 0..2 {
                let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
                    ui.set_width(width);
                    let left = ui.cursor().left();
                    text_section(ui, &mut state, id, &frame);
                    assert!(
                        ui.min_rect().right() <= left + width + 1.0,
                        "expanded text properties overflow {width}: {:?}",
                        ui.min_rect()
                    );
                });
            }
        }
    }

    #[test]
    fn labelled_text_controls_apply_and_remove_formatting() {
        for label in ["Bold", "Common ligatures", "Hyphenate words"] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            ctx.enable_accesskit();
            let (mut state, id, story) = a_text_frame("A heading");
            state.sections.set_open("OpenType features", true);
            state.sections.set_open("Line breaking", true);
            let frame = state.active().document().frame(id).unwrap().clone();
            let raw = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(288.0, 5000.0),
                )),
                ..Default::default()
            };
            let output = crate::headless_frame::frame(&ctx, raw(), |ui| {
                text_section(ui, &mut state, id, &frame)
            });
            let tree = output.platform_output.accesskit_update.unwrap();
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .and_then(|(_, node)| node.bounds())
                .expect("the control has a named hit target");
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            for enabled in [true, false] {
                for pressed in [true, false] {
                    let mut input = raw();
                    input.events = vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ];
                    let _ = crate::headless_frame::frame(&ctx, input, |ui| {
                        text_section(ui, &mut state, id, &frame)
                    });
                }
                let doc = state.active().document();
                let text = doc.story(story).unwrap();
                let character = text.common_format(0..9, doc);
                match label {
                    "Bold" => assert_eq!(character.weight, Some(if enabled { 700 } else { 400 })),
                    "Common ligatures" => assert_eq!(character.ligatures, Some(!enabled)),
                    "Hyphenate words" => {
                        assert_eq!(text.common_paragraph_format(0..9).hyphenate, Some(enabled))
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Draw the fill section once and return the names it shows, clicking the
    /// control named `click` first when there is one.
    fn fill_section_names(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        id: tessera_document::ids::FrameId,
        click: Option<&str>,
    ) -> Vec<String> {
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(288.0, 2000.0),
            )),
            ..Default::default()
        };
        let draw = |state: &mut TesseraApp, input| {
            let frame = state.active().document().frame(id).unwrap().clone();
            crate::headless_frame::frame(ctx, input, |ui| fill_section(ui, state, id, &frame))
                .platform_output
                .accesskit_update
                .unwrap()
        };
        let tree = draw(state, raw());
        if let Some(label) = click {
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .and_then(|(_, node)| node.bounds())
                .unwrap_or_else(|| panic!("no control named {label}"));
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            for pressed in [true, false] {
                let mut input = raw();
                input.events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ];
                draw(state, input);
            }
        }
        draw(state, raw())
            .nodes
            .iter()
            .filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned))
            .collect()
    }

    #[test]
    fn no_fill_reads_as_none_and_solid_brings_the_colour_back() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        let (mut state, id, _) = a_text_frame("Words");
        let red = Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: Paint::Solid(red.clone()),
            },
        );
        let fill = |state: &TesseraApp| state.active().document().frame(id).unwrap().fill.clone();

        // None: the fill goes to no alpha, and there is no colour to pick.
        let names = fill_section_names(&ctx, &mut state, id, Some("None"));
        assert_eq!(fill(&state).representative().to_rgb_f32()[3], 0.0);
        assert!(!names.iter().any(|n| n == "Fill colour"), "{names:?}");

        // Solid: the red that was under it, at full strength.
        let names = fill_section_names(&ctx, &mut state, id, Some("Solid"));
        assert_eq!(fill(&state), Paint::Solid(red));
        assert!(names.iter().any(|n| n == "Fill colour"), "{names:?}");
    }

    #[test]
    fn a_font_chosen_again_moves_to_the_head_of_recent_and_the_list_stays_short() {
        let mut recent = Vec::new();
        for family in ["A", "B", "C"] {
            remember_font(&mut recent, family);
        }
        remember_font(&mut recent, "A");
        assert_eq!(recent, ["A", "C", "B"]);
        for family in ["D", "E", "F", "G"] {
            remember_font(&mut recent, family);
        }
        assert_eq!(recent.len(), RECENT_FONTS);
        assert_eq!(recent[0], "G");
    }

    #[test]
    fn a_style_from_text_holds_what_the_text_states_and_no_more() {
        let (mut state, id, story) = a_text_frame("A heading");
        set_paragraph(
            &mut state,
            story,
            0..9,
            ParagraphFormat {
                alignment: Some(Alignment::Centre),
                ..Default::default()
            },
        );
        state.active_mut().selection.set(id);
        let format = stated_paragraph_format(&state).expect("a text frame is selected");
        assert_eq!(format.alignment, Some(Alignment::Centre));
        assert_eq!(format.indent_left, None, "only what the text states");
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

#[cfg(test)]
mod set_tests {
    #[test]
    fn sets_are_read_out_of_whatever_was_typed() {
        assert_eq!(super::parse_sets("1 3, 7"), vec![1, 3, 7]);
        assert_eq!(
            super::parse_sets("7 3 3 1"),
            vec![1, 3, 7],
            "sorted, once each"
        );
        assert_eq!(
            super::parse_sets("0 21 ss04"),
            vec![4],
            "out of range dropped"
        );
        assert!(super::parse_sets("").is_empty());
    }
}
