# Milestone 1.5 Phase A — Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the ten small, invisible pieces that phases B and C stand on — units, affine decomposition, anchors, an icon cache, theme tokens, a preferences store, a per-document container, the command invariant, autosave, and a performance guard.

**Architecture:** Nothing in this phase appears on screen and nothing changes the `.tessera` format. Each task is a self-contained addition to an existing crate, lands with its tests in one commit, and is depended upon by later phases. The two refactors (tasks 7 and 8) are mechanical and are done now because they widen with every milestone.

**Tech Stack:** Rust 2024, egui 0.35, kurbo 0.13, serde + serde_json, slotmap, proptest. No new dependencies except `directories` in task 6.

**Spec:** [`docs/superpowers/specs/2026-09-03-instrument-milestone-design.md`](../specs/2026-09-03-instrument-milestone-design.md)

**Roadmap:** [`ROADMAP.md`](../../../ROADMAP.md), milestone 1.5 phase A. Task numbers here map to the roadmap's A-labels, noted per task.

## Global Constraints

- `unsafe_code = "forbid"` at the workspace level. No exceptions.
- Document units are **points, 1/72 inch**, throughout. Never store anything else.
- Tests land in the **same commit** as the code they test.
- **No silent fallbacks.** Every error path states its cause; a failure the user should know about reaches the status bar.
- Crate dependencies point **downward only**: `geometry` → `color` → `io` → `text` → `document` → `layout` → `render` → `pdf` → `ui` → `app`.
- Workspace dependencies are declared once in the root `Cargo.toml` under `[workspace.dependencies]`; a crate writes `name.workspace = true`.
- GPU-backed tests run alone and in the foreground, never inside `cargo test --workspace`. Every command in this plan targets a single crate with `-p` for that reason.
- `cargo clippy --workspace --all-targets -- -D warnings` must be clean before each commit.

---

### Task 1: The `Unit` type (roadmap A1)

Every numeric field in the application will parse and display through this. It goes in `tessera_geometry` because that crate is the bottom of the graph and has no dependencies of its own.

**Files:**
- Create: `crates/tessera_geometry/src/unit.rs`
- Modify: `crates/tessera_geometry/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `crates/tessera_geometry/src/unit.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `Unit` — enum with variants `Millimetres`, `Points`, `Pixels`, `Inches`, `Picas`; derives `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`
  - `Unit::points_per(self) -> f64`
  - `Unit::to_points(self, value: f64) -> f64`
  - `Unit::from_points(self, points: f64) -> f64`
  - `Unit::suffix(self) -> &'static str`
  - `Unit::parse_to_points(text: &str, current: Unit) -> Option<f64>`
  - `Unit::format(self, points: f64) -> String`

- [ ] **Step 1: Write the failing tests**

Create `crates/tessera_geometry/src/unit.rs` with only the test module for now:

```rust
//! Units of measure.
//!
//! The document stores points and nothing else. This type converts at the
//! edges — parsing what a user types and formatting what a field shows — so
//! that no conversion is scattered through the interface.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inch_is_seventy_two_points() {
        assert_eq!(Unit::Inches.to_points(1.0), 72.0);
    }

    #[test]
    fn a_millimetre_is_the_metric_share_of_an_inch() {
        let expected = 72.0 / 25.4;
        assert!((Unit::Millimetres.to_points(1.0) - expected).abs() < 1e-12);
    }

    #[test]
    fn a_pica_is_twelve_points() {
        assert_eq!(Unit::Picas.to_points(1.0), 12.0);
    }

    #[test]
    fn every_unit_round_trips_through_points() {
        for unit in Unit::ALL {
            for value in [0.0, 1.0, 12.5, -3.25, 1234.5678] {
                let there_and_back = unit.from_points(unit.to_points(value));
                assert!(
                    (there_and_back - value).abs() < 1e-9,
                    "{unit:?} lost {value} (got {there_and_back})"
                );
            }
        }
    }

    #[test]
    fn a_bare_number_is_read_in_the_current_unit() {
        assert_eq!(
            Unit::parse_to_points("10", Unit::Picas),
            Some(120.0)
        );
    }

    #[test]
    fn a_suffix_overrides_the_current_unit() {
        assert_eq!(Unit::parse_to_points("1in", Unit::Millimetres), Some(72.0));
        assert_eq!(Unit::parse_to_points("12 pt", Unit::Inches), Some(12.0));
        assert_eq!(Unit::parse_to_points("72px", Unit::Millimetres), Some(72.0));
    }

    #[test]
    fn picas_and_points_are_written_together() {
        // 1p6 is one pica and six points, which is the notation a compositor
        // uses and the one InDesign accepts.
        assert_eq!(Unit::parse_to_points("1p6", Unit::Points), Some(18.0));
        assert_eq!(Unit::parse_to_points("3p", Unit::Points), Some(36.0));
        assert_eq!(Unit::parse_to_points("p6", Unit::Points), Some(6.0));
    }

    #[test]
    fn leading_dots_and_whitespace_are_accepted() {
        assert_eq!(Unit::parse_to_points("  .5in ", Unit::Points), Some(36.0));
    }

    #[test]
    fn nonsense_is_rejected_rather_than_guessed() {
        assert_eq!(Unit::parse_to_points("", Unit::Points), None);
        assert_eq!(Unit::parse_to_points("wide", Unit::Points), None);
        assert_eq!(Unit::parse_to_points("12qq", Unit::Points), None);
    }

    #[test]
    fn formatting_trims_the_noise_off_a_round_number() {
        assert_eq!(Unit::Millimetres.format(Unit::Millimetres.to_points(210.0)), "210 mm");
        assert_eq!(Unit::Points.format(12.5), "12.5 pt");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tessera_geometry unit::`
Expected: FAIL to compile, `cannot find type Unit in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/tessera_geometry/src/unit.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Unit {
    Millimetres,
    Points,
    Pixels,
    Inches,
    Picas,
}

impl Unit {
    /// Every unit, for iteration in tests and in a unit picker.
    pub const ALL: [Unit; 5] = [
        Unit::Millimetres,
        Unit::Points,
        Unit::Pixels,
        Unit::Inches,
        Unit::Picas,
    ];

    /// How many points one of this unit is worth.
    ///
    /// A pixel is 1/72 inch, so it equals a point. That is PDF's user space
    /// and the honest choice for a tool whose output is print; the 96-per-inch
    /// pixel the web uses would make on-screen numbers disagree with exported
    /// ones.
    pub fn points_per(self) -> f64 {
        match self {
            Unit::Points | Unit::Pixels => 1.0,
            Unit::Inches => 72.0,
            Unit::Picas => 12.0,
            Unit::Millimetres => 72.0 / 25.4,
        }
    }

    pub fn to_points(self, value: f64) -> f64 {
        value * self.points_per()
    }

    pub fn from_points(self, points: f64) -> f64 {
        points / self.points_per()
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Millimetres => "mm",
            Unit::Points => "pt",
            Unit::Pixels => "px",
            Unit::Inches => "in",
            Unit::Picas => "p",
        }
    }

    /// Read a field's text as a measurement in points.
    ///
    /// A bare number is in `current`. A suffix overrides it. `1p6` is the
    /// compositor's picas-and-points notation. Anything else is rejected —
    /// guessing at input the user did not mean is how a layout silently moves.
    pub fn parse_to_points(text: &str, current: Unit) -> Option<f64> {
        let text = text.trim().to_ascii_lowercase();
        if text.is_empty() {
            return None;
        }

        // Picas-and-points, before the suffix table, because `p` is a prefix
        // of nothing but is an infix here. Both halves must be plain digits,
        // which is what keeps `px` and `pt` out of this branch.
        if let Some((picas, points)) = text.split_once('p') {
            let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit() || c == '.');
            if digits(picas) && points.chars().all(|c| c.is_ascii_digit()) {
                let picas: f64 = if picas.is_empty() { 0.0 } else { picas.parse().ok()? };
                let points: f64 = if points.is_empty() { 0.0 } else { points.parse().ok()? };
                return Some(picas * 12.0 + points);
            }
        }

        for unit in [Unit::Millimetres, Unit::Points, Unit::Pixels, Unit::Inches] {
            if let Some(number) = text.strip_suffix(unit.suffix()) {
                return Some(unit.to_points(number.trim().parse().ok()?));
            }
        }

        Some(current.to_points(text.parse().ok()?))
    }

    /// Render a measurement for a field, without trailing zeroes.
    pub fn format(self, points: f64) -> String {
        let value = self.from_points(points);
        let mut text = format!("{value:.3}");
        if text.contains('.') {
            text = text.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        format!("{text} {}", self.suffix())
    }
}
```

- [ ] **Step 4: Export it**

In `crates/tessera_geometry/src/lib.rs`, add `pub mod unit;` beside the other module declarations and `Unit` to the re-export line:

```rust
pub mod spaces;
pub mod transform;
pub mod unit;
pub mod view;

pub use spaces::{DocPoint, DocRect, ScreenPoint};
pub use transform::Transform;
pub use unit::Unit;
pub use view::ViewTransform;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p tessera_geometry unit::`
Expected: PASS, 9 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/tessera_geometry/src/unit.rs crates/tessera_geometry/src/lib.rs
git commit -m "Add the unit of measure every numeric field will parse through"
```

---

### Task 2: Affine decomposition (roadmap A3)

`Transform::rotation_degrees()` assumes no shear. Phase C introduces shear, which makes that assumption false, and four modules read it to place handles. This adds the honest decomposition beside it so callers can migrate one at a time.

**Files:**
- Modify: `crates/tessera_geometry/src/transform.rs`
- Test: inline `#[cfg(test)]` module in the same file

**Interfaces:**
- Consumes: `Transform` and its `coefficients: [f64; 6]`, `Transform::from_affine`, `Transform::to_affine`.
- Produces:
  - `Decomposition` — struct with public fields `scale_x: f64`, `scale_y: f64`, `shear_degrees: f64`, `rotation_degrees: f64`, `translation: (f64, f64)`; derives `Debug, Clone, Copy, PartialEq`
  - `Transform::decompose(self) -> Decomposition`
  - `Transform::from_decomposition(d: Decomposition) -> Transform`

The decomposition is **translate · rotate · shear · scale**, in that order. kurbo's coefficients are `[a, b, c, d, e, f]` mapping `x' = a·x + c·y + e` and `y' = b·x + d·y + f`, which gives `a = sx·cosθ`, `b = sx·sinθ`, `c = sy·(m·cosθ − sinθ)`, `d = sy·(m·sinθ + cosθ)` where `m` is the shear factor. Inverting: `sx = hypot(a, b)`, `θ = atan2(b, a)`, `det = a·d − b·c`, `sy = det / sx`, `m = (a·c + b·d) / det`.

- [ ] **Step 1: Write the failing tests**

Append to the existing `#[cfg(test)] mod tests` in `crates/tessera_geometry/src/transform.rs`:

```rust
    #[test]
    fn the_identity_decomposes_to_nothing() {
        let d = Transform::IDENTITY.decompose();
        assert!((d.scale_x - 1.0).abs() < 1e-12);
        assert!((d.scale_y - 1.0).abs() < 1e-12);
        assert!(d.shear_degrees.abs() < 1e-12);
        assert!(d.rotation_degrees.abs() < 1e-12);
        assert_eq!(d.translation, (0.0, 0.0));
    }

    #[test]
    fn a_pure_rotation_reports_only_rotation() {
        let t = Transform::rotate_about(30.0, DocPoint::ZERO);
        let d = t.decompose();
        assert!((d.rotation_degrees - 30.0).abs() < 1e-9);
        assert!((d.scale_x - 1.0).abs() < 1e-9);
        assert!((d.scale_y - 1.0).abs() < 1e-9);
        assert!(d.shear_degrees.abs() < 1e-9);
    }

    #[test]
    fn decomposition_agrees_with_the_old_rotation_reader_when_there_is_no_shear() {
        for degrees in [0.0, 15.0, 90.0, 179.0, -47.5] {
            let t = Transform::rotate_about(degrees, DocPoint { x: 3.0, y: 7.0 });
            assert!(
                (t.decompose().rotation_degrees - t.rotation_degrees()).abs() < 1e-9,
                "disagreed at {degrees} degrees"
            );
        }
    }

    #[test]
    fn a_transform_recomposes_to_itself() {
        for (sx, sy, shear, rot, tx, ty) in [
            (1.0, 1.0, 0.0, 0.0, 0.0, 0.0),
            (2.0, 3.0, 0.0, 45.0, 10.0, -4.0),
            (1.5, 0.5, 20.0, -30.0, -100.0, 250.0),
            (-2.0, 1.0, 0.0, 0.0, 0.0, 0.0),
        ] {
            let built = Transform::from_decomposition(Decomposition {
                scale_x: sx,
                scale_y: sy,
                shear_degrees: shear,
                rotation_degrees: rot,
                translation: (tx, ty),
            });
            let again = Transform::from_decomposition(built.decompose());
            for i in 0..6 {
                assert!(
                    (built.coefficients[i] - again.coefficients[i]).abs() < 1e-9,
                    "coefficient {i} drifted for {sx},{sy},{shear},{rot}: \
                     {} vs {}",
                    built.coefficients[i],
                    again.coefficients[i]
                );
            }
        }
    }

    #[test]
    fn a_collapsed_transform_reports_zero_rather_than_dividing_by_it() {
        let flat = Transform::from_affine(kurbo::Affine::new([0.0, 0.0, 0.0, 0.0, 5.0, 6.0]));
        let d = flat.decompose();
        assert_eq!(d.scale_x, 0.0);
        assert_eq!(d.scale_y, 0.0);
        assert_eq!(d.rotation_degrees, 0.0);
        assert_eq!(d.shear_degrees, 0.0);
        assert_eq!(d.translation, (5.0, 6.0));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tessera_geometry transform::`
Expected: FAIL to compile, `no method named decompose` and `cannot find struct Decomposition`.

- [ ] **Step 3: Write the implementation**

Add to `crates/tessera_geometry/src/transform.rs`, above the test module:

```rust
/// A transform read back as the four things a user adjusts.
///
/// The order is **translate · rotate · shear · scale**. Any other order gives
/// different numbers for the same matrix, so it is fixed here and nowhere
/// else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decomposition {
    pub scale_x: f64,
    pub scale_y: f64,
    pub shear_degrees: f64,
    pub rotation_degrees: f64,
    pub translation: (f64, f64),
}

impl Decomposition {
    pub const IDENTITY: Self = Self {
        scale_x: 1.0,
        scale_y: 1.0,
        shear_degrees: 0.0,
        rotation_degrees: 0.0,
        translation: (0.0, 0.0),
    };
}

impl Transform {
    /// Read this transform as scale, shear, rotation and translation.
    ///
    /// Replaces [`Transform::rotation_degrees`], which is correct only while
    /// no shear exists. That method stays until its last caller has moved.
    pub fn decompose(self) -> Decomposition {
        let [a, b, c, d, e, f] = self.coefficients;

        let scale_x = a.hypot(b);
        let determinant = a * d - b * c;

        // A transform that collapses a shape to a line or a point has no
        // rotation or shear to report. Saying so beats dividing by zero and
        // reporting NaN into a numeric field.
        if scale_x == 0.0 || determinant == 0.0 {
            return Decomposition {
                scale_x: 0.0,
                scale_y: 0.0,
                shear_degrees: 0.0,
                rotation_degrees: 0.0,
                translation: (e, f),
            };
        }

        Decomposition {
            scale_x,
            scale_y: determinant / scale_x,
            shear_degrees: ((a * c + b * d) / determinant).atan().to_degrees(),
            rotation_degrees: b.atan2(a).to_degrees(),
            translation: (e, f),
        }
    }

    /// Build a transform from the parts [`Transform::decompose`] reports.
    pub fn from_decomposition(d: Decomposition) -> Self {
        let (sin, cos) = d.rotation_degrees.to_radians().sin_cos();
        let m = d.shear_degrees.to_radians().tan();
        Self {
            coefficients: [
                d.scale_x * cos,
                d.scale_x * sin,
                d.scale_y * (m * cos - sin),
                d.scale_y * (m * sin + cos),
                d.translation.0,
                d.translation.1,
            ],
        }
    }
}
```

- [ ] **Step 4: Export it**

In `crates/tessera_geometry/src/lib.rs`, change the transform re-export:

```rust
pub use transform::{Decomposition, Transform};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p tessera_geometry transform::`
Expected: PASS. The whole existing transform suite must still pass — `rotation_degrees` is untouched.

- [ ] **Step 6: Commit**

```bash
git add crates/tessera_geometry/src/transform.rs crates/tessera_geometry/src/lib.rs
git commit -m "Read a transform back as scale, shear, rotation and translation"
```

---

### Task 3: The nine-point anchor (roadmap A4)

Pure geometry, no UI. Phase C's reference-point proxy and on-canvas anchor mark both resolve through this.

**Files:**
- Create: `crates/tessera_geometry/src/anchor.rs`
- Modify: `crates/tessera_geometry/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `crates/tessera_geometry/src/anchor.rs`

**Interfaces:**
- Consumes: `DocPoint`, `DocRect` from task 1's crate; `Transform::rotate_about`, `Transform::scale_about`.
- Produces:
  - `Anchor` — enum with variants `TopLeft`, `TopCentre`, `TopRight`, `MiddleLeft`, `Centre`, `MiddleRight`, `BottomLeft`, `BottomCentre`, `BottomRight`; derives `Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize` with `Centre` as `#[default]`
  - `Anchor::ALL: [Anchor; 9]`
  - `Anchor::in_rect(self, rect: DocRect) -> DocPoint`
  - `Anchor::scale(self, rect: DocRect, sx: f64, sy: f64) -> Transform`
  - `Anchor::rotate(self, rect: DocRect, degrees: f64) -> Transform`
  - `Anchor::flip(self, rect: DocRect, horizontal: bool, vertical: bool) -> Transform`

- [ ] **Step 1: Write the failing tests**

Create `crates/tessera_geometry/src/anchor.rs` containing only:

```rust
//! The nine points a transform can be anchored to.

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> DocRect {
        DocRect { x: 100.0, y: 200.0, width: 40.0, height: 60.0 }
    }

    fn close(a: DocPoint, b: DocPoint) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn each_anchor_lands_where_its_name_says() {
        let r = rect();
        assert!(close(Anchor::TopLeft.in_rect(r), DocPoint { x: 100.0, y: 200.0 }));
        assert!(close(Anchor::TopCentre.in_rect(r), DocPoint { x: 120.0, y: 200.0 }));
        assert!(close(Anchor::TopRight.in_rect(r), DocPoint { x: 140.0, y: 200.0 }));
        assert!(close(Anchor::MiddleLeft.in_rect(r), DocPoint { x: 100.0, y: 230.0 }));
        assert!(close(Anchor::Centre.in_rect(r), DocPoint { x: 120.0, y: 230.0 }));
        assert!(close(Anchor::MiddleRight.in_rect(r), DocPoint { x: 140.0, y: 230.0 }));
        assert!(close(Anchor::BottomLeft.in_rect(r), DocPoint { x: 100.0, y: 260.0 }));
        assert!(close(Anchor::BottomCentre.in_rect(r), DocPoint { x: 120.0, y: 260.0 }));
        assert!(close(Anchor::BottomRight.in_rect(r), DocPoint { x: 140.0, y: 260.0 }));
    }

    #[test]
    fn the_anchor_is_the_one_point_a_scale_leaves_alone() {
        let r = rect();
        for anchor in Anchor::ALL {
            let fixed = anchor.in_rect(r);
            let moved = anchor.scale(r, 3.0, 0.5).apply(fixed);
            assert!(close(fixed, moved), "{anchor:?} moved its own anchor point");
        }
    }

    #[test]
    fn the_anchor_is_the_one_point_a_rotation_leaves_alone() {
        let r = rect();
        for anchor in Anchor::ALL {
            let fixed = anchor.in_rect(r);
            let moved = anchor.rotate(r, 37.0).apply(fixed);
            assert!(close(fixed, moved), "{anchor:?} moved its own anchor point");
        }
    }

    #[test]
    fn a_horizontal_flip_about_the_left_edge_sends_the_right_edge_out_past_it() {
        let r = rect();
        let flipped = Anchor::MiddleLeft.flip(r, true, false);
        let right_edge = DocPoint { x: 140.0, y: 230.0 };
        let landed = flipped.apply(right_edge);
        assert!(close(landed, DocPoint { x: 60.0, y: 230.0 }));
    }

    #[test]
    fn flipping_twice_is_doing_nothing() {
        let r = rect();
        let once = Anchor::Centre.flip(r, true, true);
        let there_and_back = once.then(once);
        let p = DocPoint { x: 123.0, y: 234.0 };
        assert!(close(there_and_back.apply(p), p));
    }

    #[test]
    fn the_default_anchor_is_the_centre() {
        assert_eq!(Anchor::default(), Anchor::Centre);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tessera_geometry anchor::`
Expected: FAIL to compile, `cannot find type Anchor in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/tessera_geometry/src/anchor.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::spaces::{DocPoint, DocRect};
use crate::transform::Transform;

/// The point a transform holds still.
///
/// Scaling, rotating and flipping all need one, and which one it is changes
/// the result entirely. Choosing it is the user's decision, so it is a value
/// rather than an assumption buried in each operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    TopCentre,
    TopRight,
    MiddleLeft,
    #[default]
    Centre,
    MiddleRight,
    BottomLeft,
    BottomCentre,
    BottomRight,
}

impl Anchor {
    /// Reading order, which is the order a three-by-three proxy draws them in.
    pub const ALL: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::TopCentre,
        Anchor::TopRight,
        Anchor::MiddleLeft,
        Anchor::Centre,
        Anchor::MiddleRight,
        Anchor::BottomLeft,
        Anchor::BottomCentre,
        Anchor::BottomRight,
    ];

    /// Where this anchor sits in a given rectangle.
    pub fn in_rect(self, rect: DocRect) -> DocPoint {
        let (fx, fy) = self.fractions();
        DocPoint {
            x: rect.x + rect.width * fx,
            y: rect.y + rect.height * fy,
        }
    }

    /// How far along each axis this anchor sits, from 0 to 1.
    fn fractions(self) -> (f64, f64) {
        match self {
            Anchor::TopLeft => (0.0, 0.0),
            Anchor::TopCentre => (0.5, 0.0),
            Anchor::TopRight => (1.0, 0.0),
            Anchor::MiddleLeft => (0.0, 0.5),
            Anchor::Centre => (0.5, 0.5),
            Anchor::MiddleRight => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 1.0),
            Anchor::BottomCentre => (0.5, 1.0),
            Anchor::BottomRight => (1.0, 1.0),
        }
    }

    pub fn scale(self, rect: DocRect, sx: f64, sy: f64) -> Transform {
        Transform::scale_about(sx, sy, self.in_rect(rect))
    }

    pub fn rotate(self, rect: DocRect, degrees: f64) -> Transform {
        Transform::rotate_about(degrees, self.in_rect(rect))
    }

    /// A flip is a scale by minus one, which is why it belongs here rather
    /// than as an operation of its own.
    pub fn flip(self, rect: DocRect, horizontal: bool, vertical: bool) -> Transform {
        self.scale(
            rect,
            if horizontal { -1.0 } else { 1.0 },
            if vertical { -1.0 } else { 1.0 },
        )
    }
}
```

- [ ] **Step 4: Export it**

In `crates/tessera_geometry/src/lib.rs`:

```rust
pub mod anchor;
```

and add to the re-exports:

```rust
pub use anchor::Anchor;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p tessera_geometry anchor::`
Expected: PASS, 6 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/tessera_geometry/src/anchor.rs crates/tessera_geometry/src/lib.rs
git commit -m "Add the nine-point anchor every transform resolves about"
```

---

### Task 4: Parse each icon once (roadmap A9)

`icons::paint` parses SVG path data into a `BezPath` on every call, which means once per icon per frame. Phase C takes the set to roughly sixty.

**Files:**
- Modify: `crates/tessera_ui/src/icons.rs`
- Test: inline `#[cfg(test)]` module in the same file

**Interfaces:**
- Consumes: the existing `Icon` enum and its `paths(self) -> &'static [&'static str]`.
- Produces: `Icon::geometry(self) -> &'static [kurbo::BezPath]`. The signatures of `paint` and `paint_rotated` do not change.

- [ ] **Step 1: Write the failing test**

Append to `crates/tessera_ui/src/icons.rs`, creating the `#[cfg(test)] mod tests` block if it does not exist:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses_to_at_least_one_subpath() {
        for icon in Icon::ALL {
            let geometry = icon.geometry();
            assert!(
                !geometry.is_empty(),
                "{icon:?} produced no geometry — its path data is malformed"
            );
            for path in geometry {
                assert!(
                    path.elements().len() > 1,
                    "{icon:?} produced an empty subpath"
                );
            }
        }
    }

    #[test]
    fn the_same_icon_hands_back_the_same_allocation() {
        // Parsing on every paint is what this cache exists to stop, so the
        // test pins the pointer rather than the contents.
        let first = Icon::Select.geometry().as_ptr();
        let second = Icon::Select.geometry().as_ptr();
        assert_eq!(first, second);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p tessera_ui icons::`
Expected: FAIL to compile, `no method named geometry` and `no associated item named ALL`.

- [ ] **Step 3: Add `Hash` and `ALL` to `Icon`**

In `crates/tessera_ui/src/icons.rs`, extend the derive on `Icon` and add the list. Every variant currently in the enum must appear in `ALL`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Select,
    Rectangle,
    Ellipse,
    Line,
    Pen,
    Text,
    Hand,
    Grab,
    Rotate,
    Move,
    Scale,
    TextCursor,
    TextFrame,
    Crosshair,
}

impl Icon {
    /// Every icon, so that a test can prove all of their path data parses.
    pub const ALL: [Icon; 14] = [
        Icon::Select,
        Icon::Rectangle,
        Icon::Ellipse,
        Icon::Line,
        Icon::Pen,
        Icon::Text,
        Icon::Hand,
        Icon::Grab,
        Icon::Rotate,
        Icon::Move,
        Icon::Scale,
        Icon::TextCursor,
        Icon::TextFrame,
        Icon::Crosshair,
    ];
}
```

- [ ] **Step 4: Add the cache**

Add to `crates/tessera_ui/src/icons.rs`:

```rust
use std::collections::HashMap;
use std::sync::OnceLock;

/// Parsed path data, built on first use and shared thereafter.
///
/// The paths are static text and never change, so parsing them per paint was
/// pure waste — one allocation per icon per frame, and the tool strip alone
/// draws a dozen.
static GEOMETRY: OnceLock<HashMap<Icon, Vec<BezPath>>> = OnceLock::new();

impl Icon {
    /// This icon's outlines, in the 24×24 Lucide grid.
    pub fn geometry(self) -> &'static [BezPath] {
        GEOMETRY
            .get_or_init(|| {
                Icon::ALL
                    .into_iter()
                    .map(|icon| {
                        let parsed = icon
                            .paths()
                            .iter()
                            .map(|data| {
                                BezPath::from_svg(data).unwrap_or_else(|error| {
                                    // Path data is a compile-time constant in
                                    // this file. A failure here is a typo in
                                    // the source, not a runtime condition, and
                                    // saying which icon is what makes it
                                    // findable.
                                    panic!("icon {icon:?} has malformed path data: {error}")
                                })
                            })
                            .collect();
                        (icon, parsed)
                    })
                    .collect()
            })
            .get(&self)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}
```

- [ ] **Step 5: Route `paint` through the cache**

In `crates/tessera_ui/src/icons.rs`, find where `paint` (line 179) and `paint_rotated` (line 190) call `BezPath::from_svg` on the result of `paths()`, and replace that parsing with a loop over `icon.geometry()`. The transform, colour and stroke handling around it is unchanged — only the source of the `BezPath` moves.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p tessera_ui icons::`
Expected: PASS, 2 tests.

- [ ] **Step 7: Check the tool strip still draws**

Run: `cargo run -p tessera_app --release`
Expected: every tool icon renders exactly as before. Quit.

- [ ] **Step 8: Commit**

```bash
git add crates/tessera_ui/src/icons.rs
git commit -m "Parse each Lucide icon once instead of on every paint"
```

---

### Task 5: Theme tokens for light and dark (roadmap A8)

`theme.rs` holds one dark palette as associated constants. This introduces a `Palette` value with a light and a dark instance and a test asserting contrast, without changing a single call site — the existing constants keep working by delegating to the dark palette. Phase C does the wiring.

**Files:**
- Modify: `crates/tessera_ui/src/theme.rs`
- Test: inline `#[cfg(test)]` module in the same file

**Interfaces:**
- Consumes: `egui::Color32`.
- Produces:
  - `Palette` — struct with `panel_bg`, `panel_bg_alt`, `canvas_bg`, `border`, `text_primary`, `text_muted`, `accent`, `selection`, `error`, `frame_edge`, all `Color32`; derives `Debug, Clone, Copy, PartialEq`
  - `Palette::DARK: Palette`, `Palette::LIGHT: Palette`
  - `contrast_ratio(a: Color32, b: Color32) -> f64` — public, so the test and any later audit share one implementation

- [ ] **Step 1: Write the failing tests**

Append to `crates/tessera_ui/src/theme.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG AA for body text.
    const AA_TEXT: f64 = 4.5;
    /// WCAG AA for user interface components and large text.
    const AA_COMPONENT: f64 = 3.0;

    #[test]
    fn black_on_white_is_the_maximum_ratio() {
        let ratio = contrast_ratio(Color32::BLACK, Color32::WHITE);
        assert!((ratio - 21.0).abs() < 0.01, "got {ratio}");
    }

    #[test]
    fn a_colour_against_itself_has_no_contrast() {
        assert!((contrast_ratio(Color32::from_rgb(0x40, 0x50, 0x60),
                                Color32::from_rgb(0x40, 0x50, 0x60)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn every_palette_reads_at_wcag_aa() {
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            for (label, fg, bg) in [
                ("primary on panel", p.text_primary, p.panel_bg),
                ("primary on alt panel", p.text_primary, p.panel_bg_alt),
                ("muted on panel", p.text_muted, p.panel_bg),
                ("muted on alt panel", p.text_muted, p.panel_bg_alt),
                ("error on panel", p.error, p.panel_bg),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= AA_TEXT,
                    "{name}: {label} is {ratio:.2}:1, below the {AA_TEXT}:1 text minimum"
                );
            }

            for (label, fg, bg) in [
                ("border on panel", p.border, p.panel_bg),
                ("accent on panel", p.accent, p.panel_bg),
                ("selection on canvas", p.selection, p.canvas_bg),
                ("frame edge on canvas", p.frame_edge, p.canvas_bg),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= AA_COMPONENT,
                    "{name}: {label} is {ratio:.2}:1, below the {AA_COMPONENT}:1 \
                     component minimum"
                );
            }
        }
    }

    #[test]
    fn the_existing_constants_still_name_the_dark_palette() {
        assert_eq!(Theme::PANEL_BG, Palette::DARK.panel_bg);
        assert_eq!(Theme::TEXT_PRIMARY, Palette::DARK.text_primary);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tessera_ui theme::`
Expected: FAIL to compile, `cannot find struct Palette` and `cannot find function contrast_ratio`.

- [ ] **Step 3: Write the implementation**

Add to `crates/tessera_ui/src/theme.rs`:

```rust
/// One complete set of interface colours.
///
/// Both palettes are defined here and both are contrast-tested, so a light
/// theme cannot rot while only the dark one is looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub panel_bg: Color32,
    pub panel_bg_alt: Color32,
    pub canvas_bg: Color32,
    pub border: Color32,
    pub text_primary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub error: Color32,
    pub frame_edge: Color32,
}

impl Palette {
    pub const DARK: Self = Self {
        panel_bg: Color32::from_rgb(0x24, 0x25, 0x28),
        panel_bg_alt: Color32::from_rgb(0x2C, 0x2D, 0x31),
        canvas_bg: Color32::from_rgb(0x18, 0x19, 0x1B),
        border: Color32::from_rgb(0x5C, 0x5F, 0x66),
        text_primary: Color32::from_rgb(0xE6, 0xE6, 0xE8),
        text_muted: Color32::from_rgb(0xA2, 0xA5, 0xAD),
        accent: Color32::from_rgb(0x6E, 0xA8, 0xFF),
        selection: Color32::from_rgb(0x6E, 0xA8, 0xFF),
        error: Color32::from_rgb(0xFF, 0x8B, 0x7D),
        frame_edge: Color32::from_rgb(0x7A, 0x7D, 0x85),
    };

    pub const LIGHT: Self = Self {
        panel_bg: Color32::from_rgb(0xF4, 0xF5, 0xF7),
        panel_bg_alt: Color32::from_rgb(0xE9, 0xEA, 0xED),
        canvas_bg: Color32::from_rgb(0xBC, 0xBF, 0xC4),
        border: Color32::from_rgb(0x6B, 0x6E, 0x76),
        text_primary: Color32::from_rgb(0x1A, 0x1B, 0x1E),
        text_muted: Color32::from_rgb(0x54, 0x57, 0x5E),
        accent: Color32::from_rgb(0x1B, 0x5C, 0xC4),
        selection: Color32::from_rgb(0x1B, 0x5C, 0xC4),
        error: Color32::from_rgb(0xA8, 0x24, 0x18),
        frame_edge: Color32::from_rgb(0x5E, 0x61, 0x68),
    };
}

/// A channel's share of perceived luminance, per WCAG 2.1.
fn channel_luminance(value: u8) -> f64 {
    let s = value as f64 / 255.0;
    if s <= 0.03928 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(c: Color32) -> f64 {
    0.2126 * channel_luminance(c.r())
        + 0.7152 * channel_luminance(c.g())
        + 0.0722 * channel_luminance(c.b())
}

/// The WCAG contrast ratio between two colours, from 1.0 to 21.0.
///
/// Public because a designer's eye is not a check that can fail in CI, and
/// this is the one that can.
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let (a, b) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}
```

- [ ] **Step 4: Point the existing constants at the dark palette**

In `crates/tessera_ui/src/theme.rs`, change each colour constant on `Theme` to read from `Palette::DARK` rather than repeating a literal. For example:

```rust
    pub const PANEL_BG: Color32 = Palette::DARK.panel_bg;
    pub const PANEL_BG_ALT: Color32 = Palette::DARK.panel_bg_alt;
    pub const CANVAS_BG: Color32 = Palette::DARK.canvas_bg;
    pub const BORDER: Color32 = Palette::DARK.border;
    pub const TEXT_PRIMARY: Color32 = Palette::DARK.text_primary;
    pub const TEXT_MUTED: Color32 = Palette::DARK.text_muted;
    pub const ACCENT: Color32 = Palette::DARK.accent;
    pub const SELECTION: Color32 = Palette::DARK.selection;
    pub const ERROR: Color32 = Palette::DARK.error;
    pub const FRAME_EDGE: Color32 = Palette::DARK.frame_edge;
```

The non-colour constants (`SPACING_SM`, `RADIUS`, `TOOL_SIZE`, `HANDLE_SIZE`, `CURSOR_SIZE`, `REFERENCE_MARK`, `CURSOR_ON_DARK`, `CURSOR_ON_LIGHT`) stay exactly as they are.

Note that several dark values changed: `BORDER`, `TEXT_MUTED`, `ACCENT`, `SELECTION`, `ERROR` and `FRAME_EDGE` were lightened to reach AA. That is the point of the test.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p tessera_ui theme::`
Expected: PASS, 4 tests. If `every_palette_reads_at_wcag_aa` fails, adjust the offending colour in the palette until it passes — do not lower the threshold.

- [ ] **Step 6: Look at the application**

Run: `cargo run -p tessera_app --release`
Expected: the interface looks as before but slightly higher contrast. Quit.

- [ ] **Step 7: Commit**

```bash
git add crates/tessera_ui/src/theme.rs
git commit -m "Define both palettes as tokens and assert their contrast"
```

---

### Task 6: The preferences store (roadmap A2)

Phase A introduces two preferences — the working unit and the theme — and there is nowhere to put them. This is that place, and autosave in task 9 uses the same directory.

**Files:**
- Create: `crates/tessera_ui/src/prefs.rs`
- Modify: `crates/tessera_ui/src/lib.rs`, `crates/tessera_ui/Cargo.toml`, root `Cargo.toml`
- Test: inline `#[cfg(test)]` module in `crates/tessera_ui/src/prefs.rs`

**Interfaces:**
- Consumes: `tessera_geometry::Unit`, `tessera_io::write_atomic`, `tessera_io::IoError`.
- Produces:
  - `Preferences` — struct with `version: u32`, `unit: Unit`, `theme: ThemeChoice`; derives `Debug, Clone, PartialEq, Serialize, Deserialize`, and `Default`
  - `ThemeChoice` — enum `Dark`, `Light`; derives `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default` with `Dark` as `#[default]`
  - `Preferences::PATH_VERSION: u32`
  - `Preferences::directory() -> Option<PathBuf>`
  - `Preferences::load_from(path: &Path) -> (Preferences, Option<String>)`
  - `Preferences::save_to(&self, path: &Path) -> Result<(), IoError>`

`load_from` returns the preferences **and** an optional complaint rather than a `Result`, because a missing or damaged preferences file must never stop the application from starting — but it must also never be swallowed. The caller puts the complaint in the status bar.

- [ ] **Step 1: Add the dependency**

Run: `cargo add --package tessera_ui directories`

Then move the resulting line out of `crates/tessera_ui/Cargo.toml` and into the root `Cargo.toml` under `[workspace.dependencies]`, keeping the version `cargo` selected, and put `directories.workspace = true` in the crate manifest instead. This is the workspace's single-source-of-truth convention.

- [ ] **Step 2: Write the failing tests**

Create `crates/tessera_ui/src/prefs.rs` containing only:

```rust
//! What the application remembers between runs.

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("tessera-prefs-test-{name}.json"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn preferences_round_trip_through_a_file() {
        let path = temp_file("round-trip");
        let written = Preferences {
            version: Preferences::PATH_VERSION,
            unit: Unit::Millimetres,
            theme: ThemeChoice::Light,
        };
        written.save_to(&path).expect("save failed");

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(read, written);
        assert_eq!(complaint, None);
    }

    #[test]
    fn a_missing_file_gives_defaults_without_complaining() {
        let (read, complaint) = Preferences::load_from(&temp_file("absent"));
        assert_eq!(read, Preferences::default());
        assert_eq!(
            complaint, None,
            "a first run is not an error and must not look like one"
        );
    }

    #[test]
    fn a_damaged_file_gives_defaults_and_says_so() {
        let path = temp_file("damaged");
        std::fs::write(&path, b"{ this is not json").unwrap();

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(read, Preferences::default());
        let complaint = complaint.expect("a damaged file must be reported, never swallowed");
        assert!(
            complaint.contains("preferences"),
            "the complaint must name what failed, got: {complaint}"
        );
    }

    #[test]
    fn a_file_from_a_future_version_gives_defaults_and_says_so() {
        let path = temp_file("future");
        std::fs::write(
            &path,
            br#"{"version":9999,"unit":"Points","theme":"Dark"}"#,
        )
        .unwrap();

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(read, Preferences::default());
        assert!(complaint.is_some());
    }

    #[test]
    fn the_default_unit_is_millimetres() {
        // The unit most of the world lays out pages in.
        assert_eq!(Preferences::default().unit, Unit::Millimetres);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p tessera_ui prefs::`
Expected: FAIL to compile, `cannot find type Preferences in this scope`.

- [ ] **Step 4: Write the implementation**

Insert above the test module in `crates/tessera_ui/src/prefs.rs`:

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tessera_geometry::Unit;
use tessera_io::{IoError, write_atomic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

/// What the application remembers between runs.
///
/// Deliberately not document data: a preference travels with the person, not
/// with the file, so opening someone else's layout must not change the units
/// you work in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    pub version: u32,
    pub unit: Unit,
    pub theme: ThemeChoice,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: Self::PATH_VERSION,
            unit: Unit::Millimetres,
            theme: ThemeChoice::default(),
        }
    }
}

impl Preferences {
    /// Bumped when the shape of this file changes incompatibly.
    pub const PATH_VERSION: u32 = 1;

    const FILE_NAME: &'static str = "preferences.json";

    /// Where preferences live on this platform, if the platform will say.
    pub fn directory() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "Tessera")
            .map(|dirs| dirs.config_dir().to_path_buf())
    }

    /// The preferences file itself.
    pub fn path() -> Option<PathBuf> {
        Self::directory().map(|dir| dir.join(Self::FILE_NAME))
    }

    /// Read preferences, and say what went wrong if anything did.
    ///
    /// Never fails. A first run, a damaged file and a file from a newer
    /// Tessera all yield defaults — but only the first is silent, because the
    /// other two mean the user's settings were just discarded and they are
    /// entitled to know.
    pub fn load_from(path: &Path) -> (Self, Option<String>) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None);
            }
            Err(error) => {
                return (
                    Self::default(),
                    Some(format!("could not read preferences: {error}")),
                );
            }
        };

        match serde_json::from_slice::<Self>(&bytes) {
            Ok(prefs) if prefs.version == Self::PATH_VERSION => (prefs, None),
            Ok(prefs) => (
                Self::default(),
                Some(format!(
                    "preferences were written by a newer Tessera \
                     (version {}, this build reads {}); defaults restored",
                    prefs.version,
                    Self::PATH_VERSION
                )),
            ),
            Err(error) => (
                Self::default(),
                Some(format!("preferences are damaged: {error}; defaults restored")),
            ),
        }
    }

    /// Write preferences, creating the directory if it is not there yet.
    pub fn save_to(&self, path: &Path) -> Result<(), IoError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IoError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let json = serde_json::to_vec_pretty(self).map_err(|error| IoError::Write {
            path: path.to_path_buf(),
            source: std::io::Error::other(error),
        })?;

        write_atomic(path, &json)
    }
}
```

- [ ] **Step 5: Declare the module**

In `crates/tessera_ui/src/lib.rs`, add `pub mod prefs;` beside the other module declarations.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p tessera_ui prefs::`
Expected: PASS, 5 tests.

- [ ] **Step 7: Commit**

```bash
git add crates/tessera_ui/src/prefs.rs crates/tessera_ui/src/lib.rs \
        crates/tessera_ui/Cargo.toml Cargo.toml Cargo.lock
git commit -m "Give the application somewhere to remember a preference"
```

---

### Task 7: The open-document container (roadmap A5)

`TesseraApp` holds `document`, `history`, `resolved`, `view` and `selection` as flat fields. All five are per-document. Moving them into a container now is mechanical; at milestone 7 it would touch every call site written between here and there.

**Files:**
- Create: `crates/tessera_ui/src/open_document.rs`
- Modify: `crates/tessera_ui/src/app.rs`, `crates/tessera_ui/src/lib.rs`, and every call site the compiler names
- Test: `crates/tessera_ui/tests/milestone_0.rs` must pass unchanged in behaviour

**Interfaces:**
- Consumes: `Document`, `History`, `tessera_layout::cache::ResolveCache`, `ViewTransform`, `crate::selection::Selection`.
- Produces:
  - `OpenDocument` — struct with a **private** `document` field and public `history`, `resolved`, `view`, `selection`, plus whatever file path and dirty flag `TesseraApp` currently carries
  - `OpenDocument::new() -> Self`
  - `OpenDocument::document(&self) -> &Document`
  - `OpenDocument::document_mut(&mut self) -> &mut Document` — `pub(crate)`, callable only from `command.rs` (task 8 asserts this)
  - `TesseraApp::active(&self) -> &OpenDocument`
  - `TesseraApp::active_mut(&mut self) -> &mut OpenDocument`

- [ ] **Step 1: Read the current struct**

Open `crates/tessera_ui/src/app.rs` and write down every field of `TesseraApp`. The five named above move; everything else — `shaper`, `active_tool`, `drag`, the clipboard, the status — stays, because it belongs to the application rather than to a document. Any field holding the current file path or a dirty flag moves too.

- [ ] **Step 2: Write the failing test**

Create `crates/tessera_ui/tests/open_document.rs`:

```rust
use tessera_ui::app::TesseraApp;

#[test]
fn a_fresh_application_has_exactly_one_open_document() {
    let app = TesseraApp::headless();
    assert_eq!(app.open_count(), 1);
    assert!(app.active().document().spread_ids().count() >= 1);
}

#[test]
fn the_active_document_is_reachable_for_reading_and_for_editing() {
    let mut app = TesseraApp::headless();
    let before = app.active().document().revision();
    app.active_mut().view.zoom = app.active().view.zoom;
    assert_eq!(app.active().document().revision(), before);
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p tessera_ui --test open_document`
Expected: FAIL to compile, `no method named active`.

- [ ] **Step 4: Write the container**

Create `crates/tessera_ui/src/open_document.rs`:

```rust
//! One document, open in the application.
//!
//! Everything here is per-document: two open files have two histories, two
//! views and two selections. Keeping them together is what makes the second
//! document a data change rather than a rewrite — and it is far cheaper to do
//! now, while there are few call sites, than at the milestone that adds tabs.

use std::path::PathBuf;

use tessera_document::document::Document;
use tessera_document::history::History;
use tessera_geometry::ViewTransform;

use crate::selection::Selection;

pub struct OpenDocument {
    /// Private on purpose. Every mutation goes through `Command`, and
    /// `command.rs` is the only module that may reach the mutable form.
    document: Document,

    pub history: History,
    pub resolved: tessera_layout::cache::ResolveCache,
    pub view: ViewTransform,
    pub selection: Selection,
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

impl OpenDocument {
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The mutable document.
    ///
    /// **Only `crate::command` may call this.** Routing every change through
    /// the command layer is what keeps undo whole and what lets the command
    /// palette reach everything; a direct edit here would be invisible to
    /// both. `tests/command_invariant.rs` holds the line.
    pub(crate) fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }
}
```

Give `OpenDocument` a `new()` that builds each field the way `TesseraApp::new` and `TesseraApp::headless` currently build them, and move the `UNDO_LIMIT` history construction with it.

- [ ] **Step 5: Restructure `TesseraApp`**

In `crates/tessera_ui/src/app.rs`, replace the five moved fields with:

```rust
    pub documents: slotmap::SlotMap<DocumentKey, OpenDocument>,
    pub active: DocumentKey,
```

adding `slotmap::new_key_type! { pub struct DocumentKey; }` beside it, and:

```rust
impl TesseraApp {
    pub fn active(&self) -> &OpenDocument {
        &self.documents[self.active]
    }

    pub fn active_mut(&mut self) -> &mut OpenDocument {
        &mut self.documents[self.active]
    }

    pub fn open_count(&self) -> usize {
        self.documents.len()
    }
}
```

- [ ] **Step 6: Follow the compiler**

Run: `cargo check -p tessera_ui`

Fix every error by replacing `state.document` with `state.active().document()`, `state.selection` with `state.active().selection`, and so on. In `command.rs`, the mutable form is `state.active_mut().document_mut()`. Change nothing else — this task adds no behaviour.

- [ ] **Step 7: Run the whole crate's tests**

Run: `cargo test -p tessera_ui`
Expected: PASS, including `milestone_0.rs` unchanged. If milestone 0's acceptance test fails, the refactor changed behaviour and must be corrected rather than the test.

- [ ] **Step 8: Perform milestone 0's sentence by hand**

Run: `cargo run -p tessera_app --release`

Draw a rectangle, give it a fill, draw a text frame, type into it, save, quit, reopen, confirm both survived, export a PDF. This refactor touches the spine, and the spine is checked by hand.

- [ ] **Step 9: Commit**

```bash
git add crates/tessera_ui/src/open_document.rs crates/tessera_ui/src/app.rs \
        crates/tessera_ui/src/lib.rs crates/tessera_ui/tests/open_document.rs
git add -u
git commit -m "Gather the per-document state into one container"
```

---

### Task 8: Hold the command invariant (roadmap A6)

Undo, the command palette and any later scripting all rest on every mutation going through `Command`. It erodes silently — one direct edit and undo has a hole in it — so it is asserted rather than intended.

**Files:**
- Create: `crates/tessera_ui/tests/command_invariant.rs`
- Modify: whichever modules the test catches

**Interfaces:**
- Consumes: `OpenDocument::document_mut` from task 7.
- Produces: nothing at runtime. The deliverable is the test.

The check reads the crate's own source. That is unusual, and it is the honest tool for the job: Rust's visibility rules cannot express "only this one sibling module", so the compiler cannot hold this line and something else has to.

- [ ] **Step 1: Write the failing test**

Create `crates/tessera_ui/tests/command_invariant.rs`:

```rust
//! Every mutation goes through `Command`.
//!
//! Rust's visibility rules can restrict a method to a crate but not to one
//! sibling module, so the compiler cannot hold this line. This test does.
//! If it fails, the fix is to route the change through a `Command` variant —
//! not to add the offending file to the list below.

use std::path::Path;

/// The only files allowed to reach the mutable document.
const PERMITTED: &[&str] = &["command.rs", "open_document.rs"];

#[test]
fn only_the_command_layer_reaches_the_mutable_document() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    visit(&src, &mut |path, contents| {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if PERMITTED.contains(&name.as_str()) {
            return;
        }
        for (number, line) in contents.lines().enumerate() {
            if line.contains("document_mut(") {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "these edit the document outside the command layer, so undo cannot \
         see them:\n{}",
        offenders.join("\n")
    );
}

fn visit(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    for entry in std::fs::read_dir(dir).expect("src is readable") {
        let path = entry.expect("entry is readable").path();
        if path.is_dir() {
            visit(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let contents = std::fs::read_to_string(&path).expect("file is readable");
            f(&path, &contents);
        }
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p tessera_ui --test command_invariant`

Expected: it either passes immediately — task 7 having already routed everything through `command.rs` — or it names the files that still edit directly.

- [ ] **Step 3: Route any offender through a command**

For each file the test names, replace the direct edit with an existing `Command` variant, or add a new variant to `command.rs` with its own unit test in that file's existing `#[cfg(test)]` module. Do not add the file to `PERMITTED`.

- [ ] **Step 4: Run it again**

Run: `cargo test -p tessera_ui --test command_invariant`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/tessera_ui/tests/command_invariant.rs
git add -u
git commit -m "Assert that every mutation goes through the command layer"
```

---

### Task 9: Autosave and crash recovery (roadmap A10)

The cross-cutting rule is that the application never loses a user's work. A crash currently loses everything since the last manual save.

**Files:**
- Create: `crates/tessera_ui/src/recovery.rs`
- Modify: `crates/tessera_ui/src/lib.rs`, `crates/tessera_ui/src/app.rs`
- Test: inline `#[cfg(test)]` module in `crates/tessera_ui/src/recovery.rs`

**Interfaces:**
- Consumes: `Preferences::directory` from task 6, `OpenDocument` from task 7, the existing save and load functions in `crates/tessera_ui/src/file_ops.rs`, `Document::revision`.
- Produces:
  - `Recovery` — struct with `last_saved_revision: u64`, `last_write: std::time::Instant`
  - `Recovery::INTERVAL: std::time::Duration` — thirty seconds
  - `Recovery::path() -> Option<PathBuf>`
  - `Recovery::due(&self, revision: u64, now: Instant) -> bool`
  - `Recovery::pending() -> Option<PathBuf>`
  - `Recovery::discard()`

- [ ] **Step 1: Write the failing tests**

Create `crates/tessera_ui/src/recovery.rs` containing only:

```rust
//! Keeping the user's work across a crash.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn nothing_is_due_when_the_document_has_not_changed() {
        let now = Instant::now();
        let r = Recovery { last_saved_revision: 7, last_write: now };
        assert!(!r.due(7, now + Duration::from_secs(600)));
    }

    #[test]
    fn nothing_is_due_before_the_interval_has_passed() {
        let now = Instant::now();
        let r = Recovery { last_saved_revision: 7, last_write: now };
        assert!(!r.due(8, now + Duration::from_secs(1)));
    }

    #[test]
    fn a_changed_document_is_due_once_the_interval_has_passed() {
        let now = Instant::now();
        let r = Recovery { last_saved_revision: 7, last_write: now };
        assert!(r.due(8, now + Recovery::INTERVAL + Duration::from_millis(1)));
    }

    #[test]
    fn the_interval_is_not_so_long_that_a_crash_costs_real_work() {
        assert!(Recovery::INTERVAL <= Duration::from_secs(60));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tessera_ui recovery::`
Expected: FAIL to compile, `cannot find type Recovery in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/tessera_ui/src/recovery.rs`:

```rust
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::prefs::Preferences;

/// When the autosave copy was last written, and for which revision.
///
/// The revision comparison is what stops an idle application rewriting the
/// same bytes every thirty seconds.
pub struct Recovery {
    pub last_saved_revision: u64,
    pub last_write: Instant,
}

impl Recovery {
    /// Long enough not to interrupt, short enough that a crash costs seconds.
    pub const INTERVAL: Duration = Duration::from_secs(30);

    const FILE_NAME: &'static str = "recovery.tessera";

    pub fn new(revision: u64) -> Self {
        Self {
            last_saved_revision: revision,
            last_write: Instant::now(),
        }
    }

    pub fn path() -> Option<PathBuf> {
        Preferences::directory().map(|dir| dir.join(Self::FILE_NAME))
    }

    /// Whether a copy is owed: the document has moved on, and enough time has
    /// passed.
    pub fn due(&self, revision: u64, now: Instant) -> bool {
        revision != self.last_saved_revision
            && now.duration_since(self.last_write) >= Self::INTERVAL
    }

    /// A recovery file left behind by a previous run, if there is one.
    pub fn pending() -> Option<PathBuf> {
        Self::path().filter(|p| p.exists())
    }

    /// Remove the recovery file, after a clean quit or once it is recovered.
    pub fn discard() {
        if let Some(path) = Self::path() {
            let _ = std::fs::remove_file(path);
        }
    }
}
```

- [ ] **Step 4: Declare the module**

In `crates/tessera_ui/src/lib.rs`, add `pub mod recovery;`.

- [ ] **Step 5: Wire it into the frame loop**

In `crates/tessera_ui/src/app.rs`, add a `recovery: Recovery` field to `TesseraApp`, and once per frame — after input is handled — check `due` against the active document's revision and `Instant::now()`. When it is due, write the document through the existing save path in `file_ops.rs` to `Recovery::path()`, then update `last_saved_revision` and `last_write`.

**This must not schedule a repaint.** The check happens on frames that were going to be drawn anyway; an idle application stays idle. That is the performance invariant in the spec, §6.

A failed autosave reaches the status bar as an error, like every other file failure. It must never be silent.

- [ ] **Step 6: Offer recovery on launch**

Where `TesseraApp` is constructed, call `Recovery::pending()`. When it returns a path, load that document instead of a blank one and set the status bar to say the previous session was recovered and is unsaved. Call `Recovery::discard()` on a clean quit and after a successful manual save.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p tessera_ui`
Expected: PASS.

- [ ] **Step 8: Perform the sentence by hand**

Run: `cargo run -p tessera_app --release`

Draw a rectangle. Wait forty seconds. Kill the process from Task Manager rather than quitting. Relaunch. Expected: the rectangle is there and the status bar says the session was recovered.

- [ ] **Step 9: Commit**

```bash
git add crates/tessera_ui/src/recovery.rs crates/tessera_ui/src/lib.rs \
        crates/tessera_ui/src/app.rs
git commit -m "Keep the user's work across a crash"
```

---

### Task 10: The performance guard (roadmap A7)

The spec states a 16.7 ms interactive budget with nothing measuring it. This measures the two things on that path that grow with document size — resolving the document and building the scene — and fails loudly on an order-of-magnitude regression.

**Files:**
- Create: `crates/tessera_render/tests/perf_guard.rs`
- Test: that file

**Interfaces:**
- Consumes: `tessera_document::document::Document`, `tessera_document::nodes::{Frame, FrameKind}`, `tessera_color::Color`, `tessera_geometry::{DocRect, Transform, ViewTransform}`, `tessera_text::shape::Shaper`, `tessera_layout::resolve(doc: &Document, shaper: &mut Shaper) -> ResolvedDocument`, `tessera_render::scene::build_scene(&ResolvedDocument, ViewTransform, DocRect) -> Scene`.
- Produces: nothing. The deliverable is the guard.

`crates/tessera_render/Cargo.toml` already carries `tessera_document`, `tessera_layout`, `tessera_text`, `tessera_color` and `tessera_geometry` under `[dev-dependencies]`, so no manifest change is needed. `Frame` has exactly five fields — `bounds`, `transform`, `kind`, `fill`, `stroke` — and does not implement `Default`, so all five are written out.

The ceiling is deliberately loose. A tight one turns a shared CI runner into a flaky test, and a flaky performance test gets muted, which is worse than none. This catches accidental quadratic behaviour, which is what actually happens.

- [ ] **Step 1: Write the test**

Create `crates/tessera_render/tests/perf_guard.rs`:

```rust
//! An order-of-magnitude guard on the interactive path.
//!
//! The budget in the spec is 16.7 ms per frame. This does not assert that —
//! a shared CI runner cannot hold a tight bound without becoming flaky, and a
//! flaky performance test gets muted. It asserts that resolving and building a
//! 500-frame document has not become ten times slower, which is what a
//! regression actually looks like.
//!
//! No GPU: `build_scene` is CPU work, so this runs in the ordinary suite.

use std::time::Instant;

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::{DocRect, Transform, ViewTransform};
use tessera_text::shape::Shaper;

const FRAMES: usize = 500;
const CEILING_MILLIS: u128 = 250;

fn crowded_document() -> Document {
    let mut document = Document::new();
    let layer = document.default_layer().expect("a new document has a layer");

    for i in 0..FRAMES {
        let across = (i % 25) as f64;
        let down = (i / 25) as f64;
        document.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: 10.0 + across * 22.0,
                    y: 10.0 + down * 28.0,
                    width: 18.0,
                    height: 24.0,
                },
                transform: Transform::IDENTITY,
                kind: FrameKind::Rectangle,
                fill: Color::BLACK,
                stroke: None,
            },
        );
    }

    document
}

#[test]
fn resolving_and_building_five_hundred_frames_stays_fast() {
    let document = crowded_document();
    let page = document.first_page_bounds();
    let view = ViewTransform::default();
    let mut shaper = Shaper::new();

    // One untimed pass, so that lazily built caches are not charged to the
    // measurement.
    let warm = tessera_layout::resolve(&document, &mut shaper);
    let _ = tessera_render::scene::build_scene(&warm, view, page);

    let started = Instant::now();
    let resolved = tessera_layout::resolve(&document, &mut shaper);
    let _scene = tessera_render::scene::build_scene(&resolved, view, page);
    let elapsed = started.elapsed();

    println!(
        "resolve + build_scene over {FRAMES} frames: {:.2} ms",
        elapsed.as_secs_f64() * 1000.0
    );

    assert!(
        elapsed.as_millis() < CEILING_MILLIS,
        "took {} ms for {FRAMES} frames, ceiling is {CEILING_MILLIS} ms — \
         something on the interactive path has become far slower",
        elapsed.as_millis()
    );
}
```

- [ ] **Step 2: Run it and read the number**

Run: `cargo test -p tessera_render --test perf_guard -- --nocapture`
Expected: PASS, printing the measured time. **Write that number in the commit message** — it is the baseline every later comparison is against.

- [ ] **Step 3: Commit**

```bash
git add crates/tessera_render/tests/perf_guard.rs
git commit -m "Guard the interactive path against an order-of-magnitude regression"
```

---

## Closing the phase

- [ ] **Run the full non-GPU suite**

```bash
cargo test -p tessera_geometry -p tessera_color -p tessera_io -p tessera_text -p tessera_document -p tessera_layout -p tessera_pdf -p tessera_ui
```

Expected: every test passes. GPU tests stay out — run `cargo test -p tessera_render --test gpu_render` separately, alone and in the foreground.

- [ ] **Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Perform milestone 0's sentence one more time**

Phase A refactored the spine in task 7. Launch, draw, type, save, quit, reopen, export. Not the test suite — the sentence.

- [ ] **Tick phase A in the roadmap**

Mark A1 through A10 in `ROADMAP.md`, and mark the two cross-cutting requirements this phase delivers: *every mutation goes through `Command`* and *performance is measured, not asserted*. Commit that with the phase.

- [ ] **Write the phase B plan**

Phase B is one format version bump carrying page geometry, facing pages, guides, `ColorRef`, spread rendering, migration tests, the document inspector and the PDF boxes. It gets its own plan, written once phase A has landed and its interfaces are real rather than predicted.
