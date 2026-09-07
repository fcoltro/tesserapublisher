//! The checks themselves.
//!
//! Each is a function of the document and returns problems. Separate functions
//! rather than one walk, because a rule somebody wants to silence, test or
//! explain has to be a thing you can point at — and because a walk that checked
//! everything at once would have to be read in full to answer "what does
//! low-resolution actually mean here?".
//!
//! The shaper is threaded through only where text is measured. Everything else
//! takes the document alone, which is what keeps most of the file testable
//! without a font system.

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::nodes::FrameKind;
use tessera_document::paint::Paint;
use tessera_text::shape::Shaper;

use crate::{Limits, Problem, Report, Rule, Where};

/// Everything, in one call.
///
/// The order rules run in is the order their problems appear before sorting, and
/// it is chosen to read well: the things that stop the job, then the things to
/// look at.
pub fn check(doc: &Document, shaper: &mut Shaper, limits: Limits) -> Report {
    let mut problems = Vec::new();
    problems.extend(overset_text(doc, shaper));
    problems.extend(links(doc));
    problems.extend(unresolved_swatches(doc));
    problems.extend(resolution(doc, limits));
    problems.extend(colour_space(doc));
    problems.extend(outside_bleed(doc, limits));
    Report { problems }.sorted()
}

/// Stories longer than the frames they flow through.
///
/// **Only the tail of a thread is reported.** A story running through four
/// frames overflows the first three by design — that is what threading *is* —
/// and reporting each of them would turn a working chain into four errors. The
/// question is whether the text runs out of frames, which only the last frame
/// can answer.
pub fn overset_text(doc: &Document, shaper: &mut Shaper) -> Vec<Problem> {
    let mut out = Vec::new();

    for id in doc.paint_order() {
        let Some(frame) = doc.frame(id) else { continue };
        let FrameKind::Text { story, .. } = frame.kind else {
            continue;
        };
        // The tail: a frame with nothing after it in its chain.
        let chain = doc.thread_of(id);
        if chain.last() != Some(&id) {
            continue;
        }
        let Some(text) = doc.story(story) else {
            continue;
        };

        // A hair of tolerance: a story that exactly fills its frame is not
        // overset, and floating point should not decide otherwise.
        let height = shaper.shape(text, doc, frame.bounds.width).height;
        if height > frame.bounds.height + 0.5 {
            let over = height - frame.bounds.height;
            out.push(Problem {
                rule: Rule::OversetText,
                message: format!(
                    "Text overflows its frame by {over:.0} pt and will not be printed"
                ),
                at: Where::Frame(id),
            });
        }
    }
    out
}

/// Placed files that are gone, or that have changed since they were placed.
///
/// Two rules from one walk because they are one question asked of one thing, and
/// splitting them would read the link table twice to produce the same answer.
pub fn links(doc: &Document) -> Vec<Problem> {
    use tessera_document::links::Status;

    let mut out = Vec::new();
    for id in doc.paint_order() {
        let Some(frame) = doc.frame(id) else { continue };
        let FrameKind::Graphic { placed: Some(p) } = &frame.kind else {
            continue;
        };
        let Some(link) = doc.links.get(p.link) else {
            continue;
        };
        let name = file_name(&link.path);

        match link.status() {
            Status::Fine => {}
            Status::Missing => out.push(Problem {
                rule: Rule::MissingLink,
                message: format!("{name} is not where the document expects it"),
                at: Where::Frame(id),
            }),
            Status::Modified => out.push(Problem {
                rule: Rule::ModifiedLink,
                message: format!("{name} has changed on disk since it was placed"),
                at: Where::Frame(id),
            }),
        }
    }
    out
}

/// Artwork reproduced below the resolution asked for.
///
/// **Effective**, not natural: a 300ppi photograph at twice its size is a 150ppi
/// photograph, and the effective figure is the one a printer cares about. A
/// missing file is not reported here — it has no resolution, and it is already
/// an error under its own rule.
pub fn resolution(doc: &Document, limits: Limits) -> Vec<Problem> {
    use tessera_document::graphic::effective_ppi;
    use tessera_document::links::Status;
    use tessera_geometry::DocPoint;

    let mut out = Vec::new();
    for id in doc.paint_order() {
        let Some(frame) = doc.frame(id) else { continue };
        let FrameKind::Graphic { placed: Some(p) } = &frame.kind else {
            continue;
        };
        let Some(link) = doc.links.get(p.link) else {
            continue;
        };
        if link.status() == Status::Missing {
            continue;
        }

        // How big the artwork actually lands, after its transform inside the
        // frame. Measured from the placement rather than the frame, because a
        // cropped picture is drawn larger than the box it shows through.
        let a = p.inner.apply(DocPoint::ZERO);
        let b = p.inner.apply(DocPoint {
            x: link.natural.0,
            y: link.natural.1,
        });
        let drawn = ((b.x - a.x).abs(), (b.y - a.y).abs());
        let pixels = (link.natural.0 as u32, link.natural.1 as u32);

        let Some((x, y)) = effective_ppi(pixels, drawn) else {
            continue;
        };
        // The worse axis. A stretched placement really does have two, and
        // reporting the better one would pass artwork that prints badly.
        let worst = x.min(y);
        if worst < limits.minimum_ppi {
            out.push(Problem {
                rule: Rule::LowResolution,
                message: format!(
                    "{} is {worst:.0} ppi where {:.0} was asked for",
                    file_name(&link.path),
                    limits.minimum_ppi
                ),
                at: Where::Frame(id),
            });
        }
    }
    out
}

/// References to named colours the document no longer defines.
///
/// An unresolved swatch draws in an alarming magenta on purpose, so this rule is
/// mostly a way of finding them in a long document rather than a way of
/// discovering them. It is still an error: that magenta prints.
pub fn unresolved_swatches(doc: &Document) -> Vec<Problem> {
    let mut out = Vec::new();
    for id in doc.paint_order() {
        let Some(frame) = doc.frame(id) else { continue };

        for name in names_used(&frame.fill)
            .into_iter()
            .chain(frame.stroke.as_ref().and_then(|s| swatch_name(&s.color)))
        {
            if doc.swatch(&name).is_none() {
                out.push(Problem {
                    rule: Rule::UnresolvedSwatch,
                    message: format!("\"{name}\" is not a colour this document defines"),
                    at: Where::Frame(id),
                });
            }
        }
    }
    out
}

/// Colours in a space the chosen press cannot print.
///
/// **Only asked when a press has been chosen.** Without an output intent there
/// is nothing to be mismatched *with*, and reporting every RGB object in a
/// document nobody has said is for print would be the noise that gets preflight
/// switched off. The absence of an intent is its own, separate warning.
pub fn colour_space(doc: &Document) -> Vec<Problem> {
    let Some(intent) = &doc.output_intent else {
        return vec![Problem {
            rule: Rule::NoOutputIntent,
            message: "No press chosen, so colour cannot be checked against one".to_string(),
            at: Where::Document,
        }];
    };

    // Only a CMYK press can be mismatched against: an RGB output takes RGB
    // objects happily, and a CMYK object converts into it.
    if !intent.description.is_empty() && !is_cmyk(intent) {
        return Vec::new();
    }

    let mut out = Vec::new();
    for id in doc.paint_order() {
        let Some(frame) = doc.frame(id) else { continue };
        if let Some(space) = rgb_space(doc, &frame.fill) {
            out.push(Problem {
                rule: Rule::ColourSpaceMismatch,
                message: format!(
                    "An {space} fill will be converted for {}",
                    intent.description
                ),
                at: Where::Frame(id),
            });
        }
    }
    out
}

/// Objects over a page edge that stop short of the bleed.
///
/// The failure this catches is expensive and invisible on screen: a photograph
/// that reaches the trim exactly, printed on a press that cuts a millimetre out,
/// leaves a white sliver down one side of every copy. Switched off entirely when
/// the document has no bleed, because then every object over an edge would be
/// reported.
pub fn outside_bleed(doc: &Document, limits: Limits) -> Vec<Problem> {
    if limits.bleed <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for id in doc.paint_order() {
        let Some(page) = doc.page_of_frame(id) else {
            continue;
        };
        let Some(trim) = doc.pages.get(page).map(|p| p.bounds) else {
            continue;
        };
        let Some(bounds) = doc.visual_bounds(id) else {
            continue;
        };

        // Only objects that already cross an edge. One sitting inside the page
        // is not short of the bleed; it is simply not at the edge.
        let short = crosses_and_stops_short(bounds, trim, limits.bleed);
        if short {
            out.push(Problem {
                rule: Rule::OutsideBleed,
                message: format!(
                    "This reaches the page edge but not the {:.1} pt bleed, so a \
                     trimming press may leave a white edge",
                    limits.bleed
                ),
                at: Where::Frame(id),
            });
        }
    }
    out
}

// --- small shared pieces ---------------------------------------------------

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn swatch_name(colour: &Color) -> Option<String> {
    match colour {
        Color::Swatch { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Every swatch a paint refers to, gradient stops included.
fn names_used(paint: &Paint) -> Vec<String> {
    match paint {
        Paint::Solid(colour) => swatch_name(colour).into_iter().collect(),
        Paint::Gradient(g) => g
            .stops()
            .iter()
            .filter_map(|s| swatch_name(&s.colour))
            .collect(),
    }
}

/// Whether an output intent describes a CMYK press.
///
/// Read from the profile's own bytes rather than from its name: a profile called
/// "Coated" is not evidence of anything, and the header says so definitely.
fn is_cmyk(intent: &tessera_document::intent::OutputIntent) -> bool {
    // The colour space signature sits at offset 16 of every ICC profile.
    intent.profile.get(16..20) == Some(b"CMYK")
}

/// The name of the RGB-family space a paint is in, if it is in one.
///
/// Resolved first, so a swatch pointing at an RGB colour is caught. `None` for
/// CMYK, for Lab, and for a spot ink — a spot is a plate of its own and is not a
/// mismatch with anything.
fn rgb_space(doc: &Document, paint: &Paint) -> Option<&'static str> {
    let colour = match paint {
        Paint::Solid(c) => doc.resolve_colour(c),
        // A gradient is reported on its first RGB stop rather than on each,
        // because one object with a five-stop ramp is one thing to fix.
        Paint::Gradient(g) => g
            .stops()
            .iter()
            .map(|s| doc.resolve_colour(&s.colour))
            .find(|c| matches!(c, Color::Rgb { .. }))?,
    };
    matches!(colour, Color::Rgb { .. }).then_some("RGB")
}

/// Whether `bounds` crosses an edge of `trim` without reaching `bleed` past it.
fn crosses_and_stops_short(
    bounds: tessera_geometry::DocRect,
    trim: tessera_geometry::DocRect,
    bleed: f64,
) -> bool {
    let over_left = bounds.x < trim.x;
    let over_top = bounds.y < trim.y;
    let over_right = bounds.x + bounds.width > trim.x + trim.width;
    let over_bottom = bounds.y + bounds.height > trim.y + trim.height;

    (over_left && bounds.x > trim.x - bleed)
        || (over_top && bounds.y > trim.y - bleed)
        || (over_right && bounds.x + bounds.width < trim.x + trim.width + bleed)
        || (over_bottom && bounds.y + bounds.height < trim.y + trim.height + bleed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::blending::Blending;
    use tessera_document::ids::FrameId;
    use tessera_document::nodes::{Frame, Swatch, TextWrap};
    use tessera_geometry::{DocRect, Transform};

    fn a_document() -> Document {
        Document::new()
    }

    fn a_frame(bounds: DocRect, kind: FrameKind, fill: Paint) -> Frame {
        Frame {
            bounds,
            transform: Transform::IDENTITY,
            kind,
            fill,
            stroke: None,
            wrap: TextWrap::None,
            blend: Blending::PLAIN,
            shadow: None,
            style: None,
        }
    }

    fn add(doc: &mut Document, frame: Frame) -> FrameId {
        let layer = doc.default_layer().expect("a layer");
        doc.add_frame(layer, frame)
    }

    fn box_at(x: f64, y: f64, w: f64, h: f64) -> DocRect {
        DocRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    // --- swatches ----------------------------------------------------------

    #[test]
    fn a_reference_to_a_colour_that_was_deleted_is_reported() {
        let mut doc = a_document();
        add(
            &mut doc,
            a_frame(
                box_at(0.0, 0.0, 10.0, 10.0),
                FrameKind::Rectangle,
                Paint::Solid(Color::Swatch {
                    name: "Brand red".to_string(),
                    tint: 1.0,
                }),
            ),
        );

        let found = unresolved_swatches(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, Rule::UnresolvedSwatch);
        assert!(found[0].message.contains("Brand red"));
    }

    #[test]
    fn a_reference_to_a_colour_that_exists_is_not() {
        let mut doc = a_document();
        doc.set_swatch(Swatch::new("Brand red", Color::BLACK));
        add(
            &mut doc,
            a_frame(
                box_at(0.0, 0.0, 10.0, 10.0),
                FrameKind::Rectangle,
                Paint::Solid(Color::Swatch {
                    name: "Brand red".to_string(),
                    tint: 1.0,
                }),
            ),
        );
        assert!(unresolved_swatches(&doc).is_empty());
    }

    #[test]
    fn a_gradient_stop_naming_a_missing_swatch_is_reported_too() {
        // Stops hold colours, so a gradient built from swatches can break the
        // same way a fill can — and it is harder to spot by eye.
        use tessera_document::paint::{Gradient, Ramp, Stop};

        let mut doc = a_document();
        let ramp = Gradient::new(
            Ramp::Radial,
            vec![
                Stop {
                    at: 0.0,
                    colour: Color::BLACK,
                },
                Stop {
                    at: 1.0,
                    colour: Color::Swatch {
                        name: "Gone".to_string(),
                        tint: 1.0,
                    },
                },
            ],
        );
        add(
            &mut doc,
            a_frame(
                box_at(0.0, 0.0, 10.0, 10.0),
                FrameKind::Rectangle,
                Paint::Gradient(ramp),
            ),
        );

        let found = unresolved_swatches(&doc);
        assert_eq!(found.len(), 1, "a stop naming a missing swatch was missed");
    }

    // --- the bleed ---------------------------------------------------------

    #[test]
    fn an_object_reaching_the_trim_but_not_the_bleed_is_reported() {
        // The failure this exists for: a photograph that stops at the trim,
        // printed on a press that cuts a millimetre out, leaves a white sliver
        // down one side of every copy.
        let trim = box_at(0.0, 0.0, 100.0, 100.0);
        assert!(crosses_and_stops_short(
            box_at(-1.0, 10.0, 30.0, 30.0),
            trim,
            9.0
        ));
    }

    #[test]
    fn an_object_reaching_past_the_bleed_is_not() {
        let trim = box_at(0.0, 0.0, 100.0, 100.0);
        assert!(!crosses_and_stops_short(
            box_at(-12.0, 10.0, 40.0, 30.0),
            trim,
            9.0
        ));
    }

    #[test]
    fn an_object_inside_the_page_is_not_short_of_anything() {
        // It is not at the edge; it is simply not at the edge.
        let trim = box_at(0.0, 0.0, 100.0, 100.0);
        assert!(!crosses_and_stops_short(
            box_at(20.0, 20.0, 30.0, 30.0),
            trim,
            9.0
        ));
    }

    #[test]
    fn the_bleed_rule_is_off_when_the_document_has_no_bleed() {
        // Otherwise every object over an edge is reported and the rule is
        // noise — which is how a preflight comes to be switched off.
        let mut doc = a_document();
        add(
            &mut doc,
            a_frame(
                box_at(-50.0, -50.0, 20.0, 20.0),
                FrameKind::Rectangle,
                Paint::Solid(Color::BLACK),
            ),
        );
        assert!(outside_bleed(&doc, Limits::default()).is_empty());
    }

    // --- colour space ------------------------------------------------------

    #[test]
    fn a_document_with_no_press_says_so_once_rather_than_per_object() {
        // Reporting every RGB object in a document nobody has said is for print
        // is the noise that gets preflight switched off.
        let mut doc = a_document();
        for _ in 0..5 {
            add(
                &mut doc,
                a_frame(
                    box_at(0.0, 0.0, 10.0, 10.0),
                    FrameKind::Rectangle,
                    Paint::Solid(Color::Rgb {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                ),
            );
        }

        let found = colour_space(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, Rule::NoOutputIntent);
        assert_eq!(found[0].at, Where::Document);
    }

    #[test]
    fn a_cmyk_press_is_told_apart_from_an_rgb_one_by_the_profile_not_its_name() {
        // A profile called "Coated" is not evidence of anything. The header is.
        use tessera_document::intent::{OutputIntent, Rendering};

        let rgb = tessera_color::managed::OutputProfile::screen().expect("a profile");
        let intent = OutputIntent {
            description: "Coated something".to_string(),
            profile: rgb.bytes().to_vec(),
            rendering: Rendering::default(),
        };
        assert!(!is_cmyk(&intent), "an RGB profile was read as CMYK");
    }

    #[test]
    fn an_rgb_press_takes_rgb_objects_without_complaint() {
        use tessera_document::intent::{OutputIntent, Rendering};

        let mut doc = a_document();
        let rgb = tessera_color::managed::OutputProfile::screen().expect("a profile");
        doc.output_intent = Some(OutputIntent {
            description: rgb.description().to_string(),
            profile: rgb.bytes().to_vec(),
            rendering: Rendering::default(),
        });
        add(
            &mut doc,
            a_frame(
                box_at(0.0, 0.0, 10.0, 10.0),
                FrameKind::Rectangle,
                Paint::Solid(Color::Rgb {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }),
            ),
        );

        assert!(colour_space(&doc).is_empty());
    }

    // --- the whole run -----------------------------------------------------

    #[test]
    fn a_fresh_document_has_nothing_wrong_with_it_but_no_press() {
        // The one thing a new document is missing is a decision nobody has made
        // yet, and saying so is right. Saying anything else would be crying
        // wolf on the first frame.
        let doc = a_document();
        let mut shaper = Shaper::new();
        let report = check(&doc, &mut shaper, Limits::default());

        assert!(report.is_clear(), "a new document has an error: {report:?}");
        assert_eq!(report.warnings(), 1);
        assert_eq!(report.problems[0].rule, Rule::NoOutputIntent);
    }
}
