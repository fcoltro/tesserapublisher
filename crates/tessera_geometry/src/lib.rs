//! Document and screen coordinate spaces, kept in distinct types.
//!
//! Confusing the two is the most common source of defects in a zoomable
//! canvas, so they are different types and the compiler enforces it.
//!
//! Document units are **points** (1/72 inch) throughout — the unit PDF uses,
//! so export needs no conversion.

pub mod anchor;
pub mod spaces;
pub mod transform;
pub mod unit;
pub mod view;

pub use anchor::Anchor;
pub use spaces::{DocPoint, DocRect, ScreenPoint};
pub use transform::{Decomposition, Transform};
pub use unit::Unit;
pub use view::ViewTransform;
