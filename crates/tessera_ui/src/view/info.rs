//! The Info panel, InDesign's (Window > Info): the numbers behind whatever
//! is happening, read off without opening anything.
//!
//! - where the pointer is, counted from the rulers' zero;
//! - the selection's position and size;
//! - while something is being dragged, how far and at what angle, and the
//!   measure tool's line once it is drawn;
//! - for text, how many characters, words, lines and paragraphs, and how
//!   many lines are overset;
//! - for a picture, its file, its pixels, and the resolution it prints at;
//! - for a fill, its colour's values.
//!
//! Everything shown is read, never kept: the panel is a window onto state
//! that lives elsewhere.

use egui::Ui;
use tessera_document::nodes::FrameKind;
use tessera_geometry::{DocPoint, Unit};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// One line of the panel: its label and what it reads.
pub type Row = (&'static str, String);

/// Every group the panel shows right now, each a heading and its rows.
pub fn rows(state: &mut TesseraApp) -> Vec<(&'static str, Vec<Row>)> {
    let unit = state.prefs.unit;
    let zero = state
        .ruler_origin
        .unwrap_or_else(|| super::rulers::natural_origin(state));
    let length = |points: f64| unit.format(points);
    let mut out = Vec::new();

    if let Some(at) = state.pointer_at {
        out.push((
            "Pointer",
            vec![("X", length(at.x - zero.x)), ("Y", length(at.y - zero.y))],
        ));
    }

    let selection: Vec<_> = state.active().selection.as_slice().to_vec();
    if let Some(bounds) = crate::align::bounding_box(
        &selection
            .iter()
            .filter_map(|id| state.active().document().visual_bounds(*id))
            .collect::<Vec<_>>(),
    ) {
        out.push((
            "Selection",
            vec![
                ("X", length(bounds.x - zero.x)),
                ("Y", length(bounds.y - zero.y)),
                ("W", length(bounds.width)),
                ("H", length(bounds.height)),
            ],
        ));
    }

    if let Some(drag) = &state.drag {
        let (dx, dy) = drag.delta();
        if dx != 0.0 || dy != 0.0 {
            out.push(("Moving", movement(dx, dy, unit)));
        }
    }
    if let Some(m) = state.measured {
        let (dx, dy) = m.across();
        out.push(("Measure", movement(dx, dy, unit)));
    }

    if let Some(id) = selection.first().copied().filter(|_| selection.len() == 1) {
        let overset = overset_lines(state, id);
        let lines = text_lines(state, id);
        let doc = state.active().document();
        let Some(frame) = doc.frame(id) else {
            return out;
        };
        match &frame.kind {
            FrameKind::Text { story, .. } => {
                if let Some(story) = doc.story(*story) {
                    out.push(("Text", text_counts(&story.text, lines, overset)));
                }
            }
            FrameKind::Graphic { placed: Some(p) } => {
                if let Some(link) = doc.links.get(p.link) {
                    let drawn = super::content::content_box(p.inner, link.natural);
                    let pixels = (link.natural.0 as u32, link.natural.1 as u32);
                    let mut rows = vec![
                        (
                            "File",
                            link.path
                                .file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                        ),
                        ("Pixels", format!("{} \u{00D7} {}", pixels.0, pixels.1)),
                    ];
                    if let Some((x, y)) = tessera_document::graphic::effective_ppi(
                        pixels,
                        (drawn.width, drawn.height),
                    ) {
                        rows.push(("Effective", format!("{:.0} \u{00D7} {:.0} ppi", x, y)));
                    }
                    out.push(("Picture", rows));
                }
            }
            _ => {}
        }
        if let Some(colour) = frame.fill.solid() {
            out.push(("Fill", vec![("Colour", describe(colour))]));
        }
    }
    out
}

/// A distance moved: the two legs, the length, and the angle, counter-
/// clockwise from the right as InDesign gives it.
fn movement(dx: f64, dy: f64, unit: Unit) -> Vec<Row> {
    let angle = (-dy).atan2(dx).to_degrees();
    vec![
        ("D", unit.format(dx.hypot(dy))),
        ("\u{0394}X", unit.format(dx)),
        ("\u{0394}Y", unit.format(dy)),
        ("Angle", format!("{angle:.1}\u{00B0}")),
    ]
}

/// The counts InDesign's Info panel gives a story.
pub fn text_counts(text: &str, lines: usize, overset: usize) -> Vec<Row> {
    let characters = text.chars().filter(|c| *c != '\n').count();
    let words = text.split_whitespace().count();
    let paragraphs = if text.is_empty() {
        0
    } else {
        text.split('\n').count()
    };
    let mut rows = vec![
        ("Characters", characters.to_string()),
        ("Words", words.to_string()),
        ("Lines", lines.to_string()),
        ("Paragraphs", paragraphs.to_string()),
    ];
    if overset > 0 {
        rows.push(("Overset", format!("{overset} lines")));
    }
    rows
}

/// A colour's values, in the space it is written in.
fn describe(colour: &tessera_color::Color) -> String {
    use tessera_color::Color;
    let pc = |v: f32| (v * 100.0).round();
    match colour {
        Color::Rgb { r, g, b, .. } => format!(
            "R {} G {} B {}",
            (r * 255.0).round(),
            (g * 255.0).round(),
            (b * 255.0).round()
        ),
        Color::Cmyk { c, m, y, k, .. } => {
            format!("C {} M {} Y {} K {}", pc(*c), pc(*m), pc(*y), pc(*k))
        }
        Color::Spot { name, tint, .. } => format!("{name} {}%", pc(*tint)),
        Color::Lab { l, a, b, .. } => format!("L {l:.0} a {a:.0} b {b:.0}"),
        Color::Swatch { name, tint } if *tint < 1.0 => format!("{name} {}%", pc(*tint)),
        Color::Swatch { name, .. } => name.clone(),
    }
}

/// How many lines of text a frame shows, as the layout set them.
fn text_lines(state: &mut TesseraApp, id: tessera_document::ids::FrameId) -> usize {
    state
        .resolve_active()
        .items
        .iter()
        .find(|i| i.frame == id)
        .map_or(0, |i| match &i.kind {
            tessera_layout::resolve::ResolvedKind::Text { shaped, .. } => shaped.lines.len(),
            _ => 0,
        })
}

/// How many lines of a frame's story did not fit, anywhere in its thread.
fn overset_lines(state: &mut TesseraApp, id: tessera_document::ids::FrameId) -> usize {
    let thread = state.active().document().thread_of(id);
    state
        .resolve_active()
        .items
        .iter()
        .filter(|i| thread.contains(&i.frame))
        .map(|i| match &i.kind {
            tessera_layout::resolve::ResolvedKind::Text { overset_lines, .. } => *overset_lines,
            _ => 0,
        })
        .sum()
}

/// The panel, docked in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let groups = rows(state);
    if groups.is_empty() {
        super::panel_ui::empty(
            ui,
            "Nothing to read yet",
            "Point at the page or choose something, and its numbers are shown here.",
        );
        return;
    }
    for (title, rows) in groups {
        ui.add_space(Theme::space_1());
        ui.label(egui::RichText::new(title).strong().size(Theme::TYPE_SM));
        egui::Grid::new(("info", title))
            .num_columns(2)
            .spacing([Theme::space_2(), 2.0])
            .show(ui, |ui| {
                for (label, value) in rows {
                    ui.colored_label(Theme::text_muted(), label);
                    ui.label(value);
                    ui.end_row();
                }
            });
    }
}

/// The pointer's place on the document, when it is over the canvas.
pub fn track(state: &mut TesseraApp, at: Option<DocPoint>) {
    state.pointer_at = at;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};
    use tessera_geometry::DocRect;

    #[test]
    fn a_story_is_counted_as_the_info_panel_counts_it() {
        let rows = text_counts("One two.\nThree", 2, 0);
        assert_eq!(
            rows,
            vec![
                ("Characters", "13".to_string()),
                ("Words", "3".to_string()),
                ("Lines", "2".to_string()),
                ("Paragraphs", "2".to_string()),
            ]
        );
        let overset = text_counts("x", 1, 4);
        assert_eq!(overset.last(), Some(&("Overset", "4 lines".to_string())));
    }

    #[test]
    fn a_move_reads_its_length_and_its_angle() {
        let rows = movement(30.0, -40.0, Unit::Points);
        assert_eq!(rows[0], ("D", "50 pt".to_string()));
        assert_eq!(rows[3], ("Angle", "53.1\u{00B0}".to_string()));
    }

    #[test]
    fn the_pointer_and_the_selection_are_read_from_the_rulers_zero() {
        let mut state = TesseraApp::headless();
        state.prefs.unit = Unit::Points;
        let page = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: page.x + 10.0,
                y: page.y + 20.0,
                width: 30.0,
                height: 40.0,
            }),
        );
        track(
            &mut state,
            Some(DocPoint {
                x: page.x + 5.0,
                y: page.y + 6.0,
            }),
        );
        let groups = rows(&mut state);
        let pointer = &groups.iter().find(|g| g.0 == "Pointer").unwrap().1;
        assert_eq!(pointer[0], ("X", "5 pt".to_string()));
        let selection = &groups.iter().find(|g| g.0 == "Selection").unwrap().1;
        assert_eq!(selection[2], ("W", "30 pt".to_string()));
    }
}
