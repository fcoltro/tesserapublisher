use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// 2D Camera component and resource representing viewport transformation
#[derive(Component, Resource, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            pan_x: 60.0,
            pan_y: 60.0,
            zoom: 1.0,
            viewport_width: 1200.0,
            viewport_height: 800.0,
        }
    }
}

impl Camera {
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            pan_x: 60.0,
            pan_y: 60.0,
            zoom: 1.0,
            viewport_width,
            viewport_height,
        }
    }

    /// Maps a 2D screen coordinate (e.g. mouse cursor) to document coordinate space
    pub fn screen_to_document(&self, screen_x: f32, screen_y: f32) -> (f32, f32) {
        let doc_x = (screen_x - self.pan_x) / self.zoom;
        let doc_y = (screen_y - self.pan_y) / self.zoom;
        (doc_x, doc_y)
    }

    /// Maps a 2D document coordinate to screen pixel coordinates
    pub fn document_to_screen(&self, doc_x: f32, doc_y: f32) -> (f32, f32) {
        let screen_x = doc_x * self.zoom + self.pan_x;
        let screen_y = doc_y * self.zoom + self.pan_y;
        (screen_x, screen_y)
    }

    /// Translates the camera by (dx, dy) in screen pixels
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan_x += dx;
        self.pan_y += dy;
    }

    /// Zooms the camera by a multiplicative factor centered at a specific screen point
    pub fn zoom_at(&mut self, screen_x: f32, screen_y: f32, factor: f32) {
        // Point in document coordinates under the cursor before zoom
        let (doc_x, doc_y) = self.screen_to_document(screen_x, screen_y);

        // Clamp zoom level between 5% (0.05) and 2000% (20.0)
        let new_zoom = (self.zoom * factor).clamp(0.05, 20.0);

        // Adjust pan so the document point remains precisely under the screen cursor
        self.pan_x = screen_x - doc_x * new_zoom;
        self.pan_y = screen_y - doc_y * new_zoom;
        self.zoom = new_zoom;
    }

    /// Fits a page of given dimensions into the current viewport with margins
    pub fn fit_page(
        &mut self,
        page_width: f32,
        page_height: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) {
        self.viewport_width = viewport_width;
        self.viewport_height = viewport_height;

        let margin = 60.0;
        let available_w = (viewport_width - margin * 2.0).max(100.0);
        let available_h = (viewport_height - margin * 2.0).max(100.0);

        let scale_w = available_w / page_width.max(1.0);
        let scale_h = available_h / page_height.max(1.0);
        let optimal_zoom = scale_w.min(scale_h).clamp(0.1, 4.0);

        self.zoom = optimal_zoom;
        self.pan_x = (viewport_width - page_width * optimal_zoom) / 2.0;
        self.pan_y = (viewport_height - page_height * optimal_zoom) / 2.0;
    }

    /// Resets the camera to 100% scale and default margins
    pub fn reset(&mut self) {
        self.pan_x = 60.0;
        self.pan_y = 60.0;
        self.zoom = 1.0;
    }
}
