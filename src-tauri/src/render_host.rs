//! Hosts the vello surface inside the Tauri window.
//!
//! The GPU surface is created from the application window itself rather than a
//! platform-specific child window. Every webview backend Tauri targets
//! (WebView2, WKWebView, WebKitGTK) composites into a child surface layered
//! *above* the parent window, so a transparent webview lets native GPU content
//! show through. That keeps this file free of per-platform window code.
//!
//! Because the surface spans the whole window while the DOM reserves only part
//! of it for the canvas, the frontend reports its canvas rectangle and that
//! becomes the [`Viewport`] the document is offset and clipped to.

use std::sync::Mutex;

use tessera_renderer::{RenderScene, RendererInfo, VelloSurface, Viewport};

/// Owns the GPU surface and the viewport the document is drawn into.
///
/// Held as Tauri managed state alongside the ECS `AppState`.
#[derive(Default)]
pub struct RenderHost {
    surface: Mutex<Option<VelloSurface>>,
    viewport: Mutex<Option<Viewport>>,
    info: Mutex<RendererInfo>,
}

impl RenderHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the GPU pipeline has been brought up successfully.
    pub fn is_ready(&self) -> bool {
        self.surface
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// The most recent renderer status report.
    pub fn info(&self) -> RendererInfo {
        self.info
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Brings up wgpu and vello against `target`, sized to the whole window.
    ///
    /// Initialization is idempotent: calling it again once ready is a no-op, so
    /// a frontend remount cannot tear down a working pipeline.
    pub fn initialize(
        &self,
        target: impl Into<vello::wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> RendererInfo {
        let mut guard = match self.surface.lock() {
            Ok(guard) => guard,
            Err(_) => return RendererInfo::failed("renderer lock poisoned"),
        };

        if guard.is_some() {
            return self.info();
        }

        let info = match VelloSurface::new(target, width, height) {
            Ok(surface) => {
                let info = RendererInfo::active(surface.adapter_name());
                *guard = Some(surface);
                info
            }
            Err(err) => RendererInfo::failed(&err.to_string()),
        };

        if let Ok(mut slot) = self.info.lock() {
            *slot = info.clone();
        }
        info
    }

    /// Reconfigures the swapchain after the window changes size.
    pub fn resize(&self, width: u32, height: u32) {
        if let Ok(mut guard) = self.surface.lock() {
            if let Some(surface) = guard.as_mut() {
                surface.resize(width, height);
            }
        }
    }

    /// Records the canvas rectangle reported by the frontend, in physical pixels.
    pub fn set_viewport(&self, viewport: Viewport) {
        if let Ok(mut guard) = self.viewport.lock() {
            *guard = Some(viewport);
        }
    }

    /// Paints and presents one frame.
    ///
    /// Returns `Ok(false)` when nothing was presented, either because the
    /// pipeline is not up yet or because the swapchain skipped the frame.
    pub fn render(&self, scene: &RenderScene) -> Result<bool, String> {
        let mut guard = self
            .surface
            .lock()
            .map_err(|_| "renderer lock poisoned".to_string())?;
        let Some(surface) = guard.as_mut() else {
            return Ok(false);
        };

        // Without a reported canvas rect, fall back to the whole surface so the
        // document is still visible rather than clipped away entirely.
        let viewport = self
            .viewport
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .unwrap_or_else(|| {
                let (width, height) = surface.size();
                Viewport::full(width as f64, height as f64)
            });

        surface.render(scene, viewport).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_host_is_not_ready() {
        let host = RenderHost::new();
        assert!(!host.is_ready());
        assert!(!host.info().is_ready);
    }

    #[test]
    fn rendering_without_a_surface_presents_nothing() {
        // Guards against the frontend triggering a paint before init and getting
        // an error instead of a quiet no-op.
        let host = RenderHost::new();
        assert_eq!(host.render(&RenderScene::default()), Ok(false));
    }

    #[test]
    fn viewport_round_trips() {
        let host = RenderHost::new();
        let viewport = Viewport {
            x: 12.0,
            y: 24.0,
            width: 800.0,
            height: 600.0,
        };
        host.set_viewport(viewport);

        assert_eq!(host.viewport.lock().unwrap().unwrap(), viewport);
    }

    #[test]
    fn resizing_without_a_surface_is_harmless() {
        let host = RenderHost::new();
        host.resize(1920, 1080);
        assert!(!host.is_ready());
    }
}
