//! The tool strip, the inspector and the status bar.

use egui::{Sense, Ui, Vec2};
use tessera_color::Color;
use tessera_document::ids::StoryId;
use tessera_document::nodes::{Orientation, PagePreset};
use tessera_geometry::{Anchor, Unit};
use tessera_text::story::{
    Alignment, CharacterFormat, CharacterStyle, CharacterStyleId, ParagraphFormat,
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
fn optional_number(
    ui: &mut Ui,
    label: &str,
    shown: Option<f32>,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> Option<f32> {
    let mut changed = None;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, label);
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
    let tessera_document::nodes::FrameKind::Text { story } = &frame.kind else {
        return;
    };
    let story = *story;

    let mut text = state
        .active()
        .document()
        .story(story)
        .map(|s| s.text.clone())
        .unwrap_or_default();
    if ui.text_edit_multiline(&mut text).changed() {
        apply(state, Command::SetText { id, text });
    }

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

    if let Some(family) = family_picker(ui, state, shown.family.as_deref(), &missing) {
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

    if let Some(size) = optional_number(ui, "Size", shown.size, 0.25, 1.0..=1440.0, " pt") {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                size: Some(size),
                ..CharacterFormat::default()
            },
        );
    }

    // Leading as a multiple of the size rather than in points. A multiple
    // survives a size change, which is what a designer setting 1.2 means and
    // what points would silently break.
    if let Some(line_height) =
        optional_number(ui, "Leading", shown.line_height, 0.01, 0.5..=4.0, "×")
    {
        set_character(
            state,
            story,
            target.clone(),
            CharacterFormat {
                line_height: Some(line_height),
                ..CharacterFormat::default()
            },
        );
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

    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Weight");
        for (label, weight) in [
            ("Light", 300u16),
            ("Regular", 400),
            ("Medium", 500),
            ("Bold", 700),
        ] {
            if ui
                .selectable_label(shown.weight == Some(weight), label)
                .clicked()
            {
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
        }
    });

    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Style");
        if ui
            .selectable_label(shown.italic == Some(true), "Italic")
            .clicked()
        {
            set_character(
                state,
                story,
                target.clone(),
                CharacterFormat {
                    // A toggle: clicking an active Italic turns it off, which
                    // needs `Some(false)` rather than `None` — `None` means
                    // inherit and would leave it italic.
                    italic: Some(shown.italic != Some(true)),
                    ..CharacterFormat::default()
                },
            );
        }
    });

    // --- the paragraph half

    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Align");
        for (label, alignment) in [
            ("Left", Alignment::Left),
            ("Centre", Alignment::Centre),
            ("Right", Alignment::Right),
            ("Justify", Alignment::Justify),
        ] {
            if ui
                .selectable_label(paragraph.alignment == Some(alignment), label)
                .clicked()
            {
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
        }
    });

    // Indents and paragraph spacing have no controls yet, and the reason is
    // the same one that limits alignment: parley lays out a whole story as one
    // layout, so a per-paragraph measure or a gap between paragraphs cannot be
    // expressed. The model carries all five fields and the format migration
    // preserves them; what is missing is one layout per paragraph, which also
    // reaches the caret and is its own piece of work.
    //
    // A control that sets a value nothing draws is worse than one that is
    // absent, because it makes the software look broken rather than
    // unfinished.

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
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Family");
        let label = shown.unwrap_or("Mixed");
        egui::ComboBox::from_id_salt("family")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for family in state.shaper.families() {
                    if ui.selectable_label(shown == Some(family.as_str()), family).clicked() {
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

            // The page count. Navigation arrives with the pages panel at
            // milestone 3; this is the reading InDesign shows in the same
            // corner, and it is true today.
            let pages = state.active().document().page_ids().count();
            let spreads = state.active().document().spread_ids().count();
            ui.colored_label(
                Theme::TEXT_MUTED,
                format!(
                    "{pages} page{} in {spreads} spread{}",
                    if pages == 1 { "" } else { "s" },
                    if spreads == 1 { "" } else { "s" }
                ),
            );
        });
    });
}

/// The two style pickers, and the way a style comes into existence.
///
/// A style is defined *from* what the panel is currently showing. That is the
/// only route that needs no dialog, and it is how a designer actually works:
/// set a paragraph until it looks right, then name it.
fn style_rows(
    ui: &mut Ui,
    state: &mut TesseraApp,
    story: StoryId,
    target: std::ops::Range<usize>,
) {
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

    ui.add_space(Theme::SPACING_MD);

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
        define_paragraph = ui
            .button("New")
            .on_hover_text("Define a paragraph style from what is shown")
            .clicked();
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

    // The style's own fields, shown only while one is selected. Editing them
    // changes every paragraph drawn through the style, which is the whole
    // reason a style exists rather than a set of overrides.
    if let Some(id) = paragraph_style
        && let Some(existing) = state
            .active()
            .document()
            .paragraph_styles
            .get(id)
            .cloned()
    {
        let mut edited = existing.clone();
        ui.indent("paragraph-style-fields", |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(Theme::TEXT_MUTED, "Name");
                ui.text_edit_singleline(&mut edited.name);
            });
            if let Some(size) = optional_number(
                ui,
                "Style size",
                Some(edited.format.character.size.unwrap_or(12.0)),
                0.25,
                1.0..=1440.0,
                " pt",
            ) {
                edited.format.character.size = Some(size);
            }
            ui.horizontal(|ui| {
                ui.colored_label(Theme::TEXT_MUTED, "Style align");
                for (label, alignment) in [
                    ("Left", Alignment::Left),
                    ("Centre", Alignment::Centre),
                    ("Right", Alignment::Right),
                    ("Justify", Alignment::Justify),
                ] {
                    if ui
                        .selectable_label(edited.format.alignment == Some(alignment), label)
                        .clicked()
                    {
                        edited.format.alignment = Some(alignment);
                    }
                }
            });
        });
        if edited != existing {
            apply(state, Command::EditParagraphStyle { id, style: edited });
        }
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
        define_character = ui
            .button("New")
            .on_hover_text("Define a character style from what is shown")
            .clicked();
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

    if let Some(id) = character_style
        && let Some(existing) = state
            .active()
            .document()
            .character_styles
            .get(id)
            .cloned()
    {
        let mut edited = existing.clone();
        ui.indent("character-style-fields", |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(Theme::TEXT_MUTED, "Name");
                ui.text_edit_singleline(&mut edited.name);
            });
            if let Some(size) = optional_number(
                ui,
                "Style size",
                Some(edited.format.size.unwrap_or(12.0)),
                0.25,
                1.0..=1440.0,
                " pt",
            ) {
                edited.format.size = Some(size);
            }
        });
        if edited != existing {
            apply(state, Command::EditCharacterStyle { id, style: edited });
        }
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
            fill: Color::BLACK,
            stroke: None,
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
        let FrameKind::Text { story } = state.active().document().frame(id).expect("frame").kind
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

}
