//! Assets on disk, and what the document knows about them.
//!
//! **Linked, never embedded.** A layout that swallows its images is a layout
//! nobody can re-supply artwork for, and a package that cannot collect its
//! links is not a package. The document stores a path and what it last saw
//! there; the pixels stay on disk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What the document last knew about a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub path: PathBuf,
    /// The size the artwork wants to be, in points.
    ///
    /// Read from the file when it was placed. Kept here so a document opens
    /// and lays out without touching the disk — a missing image must not stop
    /// a page from being drawn, and the size is what the layout depends on.
    pub natural: (f64, f64),
    /// When the file was last modified, as seconds since the epoch.
    ///
    /// Compared against the file to tell **modified** from **fine**, which is
    /// the distinction the previous codebase never drew and the reason
    /// somebody could send a printer last week's photograph.
    #[serde(default)]
    pub modified: Option<u64>,
}

/// What a link is doing right now.
///
/// Not stored: it is a fact about the disk, and the disk changes without the
/// document being told. Recomputed when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The file is there and has not changed since it was placed.
    Fine,
    /// The file is there and has changed.
    Modified,
    /// The file is not there.
    Missing,
}

impl Link {
    pub fn new(path: impl Into<PathBuf>, natural: (f64, f64)) -> Self {
        Self {
            path: path.into(),
            natural,
            modified: None,
        }
    }

    /// What the disk says about this link — as it said it at most a couple
    /// of seconds ago, from [`tessera_io::seen`]. Layout asks this for every
    /// placed picture on every edit, and the disk is not asked each time.
    pub fn status(&self) -> Status {
        let tessera_io::seen::Seen::Present { modified } = tessera_io::seen::seen(&self.path)
        else {
            return Status::Missing;
        };
        let Some(placed) = self.modified else {
            // Placed before modification times were recorded, or by something
            // that could not read one. Present is the most that can be said.
            return Status::Fine;
        };
        match modified {
            Some(now) if now != placed => Status::Modified,
            _ => Status::Fine,
        }
    }
}

impl Link {
    /// Whether the artwork is drawn from a description rather than pixels,
    /// and so has no resolution to be short of. By extension, which is what
    /// decides how the file is read everywhere else in the application — see
    /// `tessera_render::images::is_svg`, which this agrees with by
    /// construction while there is one vector format.
    pub fn is_vector(&self) -> bool {
        self.path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    }
}

impl crate::document::Document {
    /// The resolution a placed picture is printed at, in pixels per inch —
    /// the worse of its two axes, since a stretched picture has two and the
    /// better one would pass artwork that prints badly.
    ///
    /// **Effective**: its pixels over the size it is drawn at on the page,
    /// through its placement in the frame *and* the frame's own transform.
    /// A frame scaled up by its transform draws its picture larger, and the
    /// placement alone would not see it. `None` for vector artwork, a frame
    /// showing none, or a picture drawn at no size.
    pub fn effective_ppi(&self, frame: crate::ids::FrameId) -> Option<f64> {
        use tessera_geometry::DocPoint;
        let frame = self.frame(frame)?;
        let crate::nodes::FrameKind::Graphic { placed: Some(p) } = &frame.kind else {
            return None;
        };
        let link = self.links.get(p.link)?;
        if link.is_vector() {
            return None;
        }
        let (w, h) = link.natural;
        let at = |x: f64, y: f64| frame.transform.apply(p.inner.apply(DocPoint { x, y }));
        let (o, across, down) = (at(0.0, 0.0), at(w, 0.0), at(0.0, h));
        let length = |a: DocPoint, b: DocPoint| (b.x - a.x).hypot(b.y - a.y);
        let drawn = (length(o, across), length(o, down));
        // A raster's natural size is its pixel count: placed at one pixel
        // to the point.
        let (x, y) = crate::graphic::effective_ppi((w as u32, h as u32), drawn)?;
        Some(x.min(y))
    }
}

/// A file's modification time, in seconds since the epoch.
pub fn modified_seconds(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("tessera-link-{name}"));
        std::fs::write(&path, contents).expect("write");
        path
    }

    #[test]
    fn a_file_that_is_not_there_is_missing() {
        let link = Link::new(
            std::env::temp_dir().join("tessera-nothing-here.png"),
            (10.0, 10.0),
        );
        assert_eq!(link.status(), Status::Missing);
    }

    #[test]
    fn a_file_that_is_there_and_unchanged_is_fine() {
        let path = a_file("fine", "pixels");
        let mut link = Link::new(&path, (10.0, 10.0));
        link.modified = std::fs::metadata(&path)
            .ok()
            .as_ref()
            .and_then(modified_seconds);

        assert_eq!(link.status(), Status::Fine);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_has_changed_since_it_was_placed_is_modified() {
        // The distinction the previous codebase never drew, and the reason
        // somebody could send a printer last week's photograph.
        let path = a_file("modified", "pixels");
        let mut link = Link::new(&path, (10.0, 10.0));
        link.modified = Some(0);

        assert_eq!(link.status(), Status::Modified);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_link_placed_before_times_were_recorded_is_not_called_modified() {
        // Present is the most that can be said about it, and crying wolf on
        // every old document would teach people to ignore the warning.
        let path = a_file("untimed", "pixels");
        let link = Link::new(&path, (10.0, 10.0));
        assert_eq!(link.modified, None);
        assert_eq!(link.status(), Status::Fine);
        let _ = std::fs::remove_file(&path);
    }

    /// A document with a picture box 72 points square showing `natural`
    /// pixels, stretched to fill it.
    fn a_placed(
        natural: (f64, f64),
        path: &str,
    ) -> (crate::document::Document, crate::ids::FrameId) {
        let mut doc = crate::document::Document::new();
        let layer = doc.default_layer().expect("a layer");
        let link = doc.add_link(Link::new(path, natural));
        let frame = doc.add_frame(
            layer,
            crate::nodes::Frame {
                bounds: tessera_geometry::DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 72.0,
                    height: 72.0,
                },
                kind: crate::nodes::FrameKind::Graphic { placed: None },
                transform: tessera_geometry::Transform::IDENTITY,
                fill: crate::paint::Paint::Solid(tessera_color::Color::WHITE),
                stroke: None,
                wrap: crate::nodes::TextWrap::None,
                blend: crate::blending::Blending::PLAIN,
                corners: crate::corners::Corners::SQUARE,
                shadow: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
            },
        );
        doc.place(frame, link, crate::graphic::Fit::Stretch);
        (doc, frame)
    }

    #[test]
    fn a_picture_is_printed_at_its_pixels_over_the_size_it_is_drawn() {
        // 300 pixels across an inch is 300 ppi; the same frame scaled to
        // twice its size by its transform draws it over two inches.
        let (mut doc, frame) = a_placed((300.0, 300.0), "photo.png");
        let at = doc.effective_ppi(frame).expect("a raster");
        assert!((at - 300.0).abs() < 0.01, "{at}");
        doc.frames[frame].transform =
            tessera_geometry::Transform::scale_about(2.0, 2.0, tessera_geometry::DocPoint::ZERO);
        let at = doc.effective_ppi(frame).expect("a raster");
        assert!(
            (at - 150.0).abs() < 0.01,
            "the frame's own scale counts: {at}"
        );
    }

    #[test]
    fn a_picture_stretched_one_way_prints_at_its_coarser_resolution() {
        // 300 pixels across the inch and 150 down it: the 150 is what shows.
        let (doc, frame) = a_placed((300.0, 150.0), "banner.png");
        let at = doc.effective_ppi(frame).expect("a raster");
        assert!((at - 150.0).abs() < 0.01, "{at}");
    }

    #[test]
    fn a_picture_in_a_group_is_still_a_use_of_its_file() {
        // Grouped with its caption, it went uncounted, and a relink onto a
        // file already linked left it naming a link that was gone.
        let (mut doc, picture) = a_placed((10.0, 10.0), "old.png");
        let old = doc.links.keys().next().expect("a link");
        let layer = doc.default_layer().expect("a layer");
        let caption = doc.add_frame(layer, doc.frames[picture].clone());
        doc.frames[caption].kind = crate::nodes::FrameKind::Rectangle;
        let group = doc.group(&[picture, caption]).expect("grouped");
        assert!(!doc.layers[layer].frames.contains(&picture), "inside it");

        assert_eq!(doc.frames_using(old), [picture]);
        let other = doc.add_link(Link::new("new.png", (10.0, 10.0)));
        let now = doc.relink(old, Link::new("new.png", (10.0, 10.0)));
        assert_eq!(now, other);
        assert_eq!(doc.frames_using(other), [picture]);
        assert!(doc.frames.contains_key(group));
    }

    #[test]
    fn a_drawing_has_no_resolution_to_be_short_of() {
        let (doc, frame) = a_placed((40.0, 20.0), "logo.SVG");
        assert!(doc.effective_ppi(frame).is_none());
    }

    #[test]
    fn a_relink_onto_a_file_already_linked_takes_the_pictures_on_hidden_layers_too() {
        // It re-pointed the frames being drawn and removed the old link, and
        // a picture on a hidden layer was left naming a link that was gone.
        let (mut doc, shown) = a_placed((10.0, 10.0), "old.png");
        let old = doc.links.keys().next().expect("a link");
        let hidden_layer = doc.add_layer("Hidden");
        let hidden = doc.add_frame(hidden_layer, doc.frames[shown].clone());
        doc.layers[hidden_layer].visible = false;
        let other = doc.add_link(Link::new("new.png", (10.0, 10.0)));

        assert_eq!(
            doc.frames_using(old),
            [shown, hidden],
            "both, hidden or not"
        );
        let now = doc.relink(old, Link::new("new.png", (10.0, 10.0)));
        assert_eq!(now, other, "joined the link to that file");
        assert!(doc.links.get(old).is_none());
        assert_eq!(doc.frames_using(other), [shown, hidden]);
    }
}
