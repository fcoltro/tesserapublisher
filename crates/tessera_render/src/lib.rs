//! Turning a resolved document into pixels.
//!
//! Two outputs from one scene builder: a wgpu texture for the live viewport,
//! and a CPU pixel buffer for tests and page thumbnails. The pixel path is
//! what makes rendering regression-testable without a window.

pub mod headless;
pub mod scene;

pub use headless::{HeadlessRenderer, RenderError};
pub use scene::build_scene;
