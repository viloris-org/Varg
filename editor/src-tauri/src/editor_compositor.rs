//! Native host-window editor presentation seam.
//!
//! This module is the replacement path for the experimental GTK child-surface
//! Scene View. The target architecture is:
//!
//! - a native host window owns the top-level editor presentation;
//! - engine Scene View regions are native WGPU-rendered surfaces;
//! - Web UI is embedded as panels, overlays, dock views, and input layers;
//! - no GPU readback and no OS child-surface movement.
//!
//! The current file intentionally contains the seam and state machine only.
//! Window/WebView transparency and platform-specific surface ownership will be
//! implemented behind this interface.

use engine_render_wgpu::SurfaceViewportRect;
use serde::Serialize;

use crate::scene_window;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum EditorCompositorBackend {
    LinuxGtk,
    WindowsWebView2,
    MacosWkWebView,
    AndroidWebView,
    IosWkWebView,
    Unsupported,
}

impl EditorCompositorBackend {
    pub fn id(self) -> &'static str {
        match self {
            Self::LinuxGtk => "linux-gtk",
            Self::WindowsWebView2 => "windows-webview2",
            Self::MacosWkWebView => "macos-wkwebview",
            Self::AndroidWebView => "android-webview",
            Self::IosWkWebView => "ios-wkwebview",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditorCompositorSupport {
    pub backend: EditorCompositorBackend,
    pub available: bool,
    pub reason: &'static str,
}

pub fn platform_support() -> EditorCompositorSupport {
    #[cfg(target_os = "linux")]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::LinuxGtk,
            available: true,
            reason: "Cross-platform native host-window seam is active through the Linux GTK adapter; the host owns Scene View presentation and embeds Web UI panels.",
        }
    }
    #[cfg(target_os = "windows")]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::WindowsWebView2,
            available: false,
            reason: "Windows native host window with WebView2 dock/overlay views is not implemented yet; using canvas readback fallback for now.",
        }
    }
    #[cfg(target_os = "macos")]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::MacosWkWebView,
            available: false,
            reason: "macOS native host window with WKWebView dock/overlay views is not implemented yet; using canvas readback fallback for now.",
        }
    }
    #[cfg(target_os = "android")]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::AndroidWebView,
            available: false,
            reason: "Android native host view with WebView panels is not implemented yet; using canvas readback fallback for now.",
        }
    }
    #[cfg(target_os = "ios")]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::IosWkWebView,
            available: false,
            reason: "iOS native host view with WKWebView panels is not implemented yet; using canvas readback fallback for now.",
        }
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "windows",
        target_os = "macos",
        target_os = "android",
        target_os = "ios"
    )))]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::Unsupported,
            available: false,
            reason: "This platform has no native editor compositor adapter yet; using canvas readback fallback.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViewportPresentationMode {
    CanvasReadback,
    EmbeddedNativeExperimental,
    NativeHostWindow,
    /// Legacy compatibility name for the native host-window architecture.
    EditorCompositor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewportPresentationAdapter {
    pub mode: ViewportPresentationMode,
    pub available: bool,
    pub default: bool,
    pub zero_copy: bool,
    pub experimental: bool,
    pub backend: &'static str,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewportPresentationCapabilities {
    pub default_mode: ViewportPresentationMode,
    pub adapters: Vec<ViewportPresentationAdapter>,
}

pub fn presentation_capabilities(compositor_requested: bool) -> ViewportPresentationCapabilities {
    presentation_capabilities_for(
        compositor_requested,
        platform_support(),
        experimental_child_surface_available(),
    )
}

pub fn presentation_capabilities_for(
    compositor_requested: bool,
    support: EditorCompositorSupport,
    experimental_child_surface_available: bool,
) -> ViewportPresentationCapabilities {
    let native_host_available = compositor_requested && support.available;
    ViewportPresentationCapabilities {
        default_mode: if native_host_available {
            ViewportPresentationMode::NativeHostWindow
        } else {
            ViewportPresentationMode::CanvasReadback
        },
        adapters: vec![
            ViewportPresentationAdapter {
                mode: ViewportPresentationMode::CanvasReadback,
                available: true,
                default: !native_host_available,
                zero_copy: false,
                experimental: false,
                backend: "webview-canvas",
                reason: "Stable WebView-composited fallback. Copies pixels through readback, so it is not the final performance path.",
            },
            ViewportPresentationAdapter {
                mode: ViewportPresentationMode::EmbeddedNativeExperimental,
                available: experimental_child_surface_available,
                default: false,
                zero_copy: true,
                experimental: true,
                backend: "linux-gtk-child-surface",
                reason: "Legacy GTK child-surface adapter. It is disabled by default because Wayland/WebView child-surface ownership can crash or drift; use native-host-window for zero-copy.",
            },
            ViewportPresentationAdapter {
                mode: ViewportPresentationMode::NativeHostWindow,
                available: native_host_available,
                default: native_host_available,
                zero_copy: true,
                experimental: false,
                backend: support.backend.id(),
                reason: support.reason,
            },
            ViewportPresentationAdapter {
                mode: ViewportPresentationMode::EditorCompositor,
                available: native_host_available,
                default: false,
                zero_copy: true,
                experimental: false,
                backend: support.backend.id(),
                reason: "Legacy alias for native-host-window.",
            },
        ],
    }
}

pub fn experimental_child_surface_available() -> bool {
    cfg!(target_os = "linux") && experimental_child_surface_requested()
}

pub fn experimental_child_surface_requested() -> bool {
    std::env::var("ASTER_ENABLE_EXPERIMENTAL_CHILD_SURFACE").is_ok_and(|value| {
        matches!(
            value.as_str(),
            "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditorCompositorViewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl EditorCompositorViewport {
    pub fn from_scene_rect(rect: scene_window::SceneViewportRect) -> Self {
        let rect = rect.sanitized();
        Self {
            x: rect.x.max(0) as u32,
            y: rect.y.max(0) as u32,
            width: rect.width.max(1),
            height: rect.height.max(1),
        }
    }

    pub fn into_surface_rect(self) -> SurfaceViewportRect {
        SurfaceViewportRect::new(self.x, self.y, self.width, self.height)
    }
}

#[derive(Debug)]
pub struct EditorCompositor {
    viewport: EditorCompositorViewport,
}

impl Default for EditorCompositor {
    fn default() -> Self {
        Self {
            viewport: EditorCompositorViewport {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        }
    }
}

impl EditorCompositor {
    pub fn set_viewport(&mut self, viewport: EditorCompositorViewport) {
        self.viewport = viewport;
    }

    pub fn surface_viewport(&self) -> SurfaceViewportRect {
        self.viewport.into_surface_rect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_compositor_viewport_sanitizes_dom_rects_for_surface_use() {
        let viewport = EditorCompositorViewport::from_scene_rect(scene_window::SceneViewportRect {
            x: -12,
            y: 34,
            width: 0,
            height: 720,
        });

        assert_eq!(
            viewport,
            EditorCompositorViewport {
                x: 0,
                y: 34,
                width: 1,
                height: 720,
            }
        );
        assert_eq!(
            viewport.into_surface_rect(),
            SurfaceViewportRect::new(0, 34, 1, 720)
        );
    }

    #[test]
    fn platform_support_has_stable_backend_identifier() {
        let support = platform_support();
        assert!(!support.backend.id().is_empty());
        assert!(!support.reason.is_empty());
    }

    #[test]
    fn presentation_capabilities_fall_back_to_canvas_when_not_requested() {
        let capabilities = presentation_capabilities_for(
            false,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: true,
                reason: "available",
            },
            false,
        );

        assert_eq!(
            capabilities.default_mode,
            ViewportPresentationMode::CanvasReadback
        );
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::CanvasReadback
            && adapter.default));
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::NativeHostWindow
            && !adapter.available));
    }

    #[test]
    fn presentation_capabilities_select_compositor_when_requested_and_available() {
        let capabilities = presentation_capabilities_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: true,
                reason: "available",
            },
            false,
        );

        assert_eq!(
            capabilities.default_mode,
            ViewportPresentationMode::NativeHostWindow
        );
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::NativeHostWindow
            && adapter.available
            && adapter.default
            && adapter.zero_copy));
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::EditorCompositor
            && adapter.available
            && !adapter.default));
    }

    #[test]
    fn presentation_capabilities_fall_back_when_platform_adapter_is_missing() {
        let capabilities = presentation_capabilities_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::WindowsWebView2,
                available: false,
                reason: "not implemented",
            },
            false,
        );

        assert_eq!(
            capabilities.default_mode,
            ViewportPresentationMode::CanvasReadback
        );
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::NativeHostWindow
            && !adapter.available
            && adapter.backend == "windows-webview2"));
    }

    #[test]
    fn presentation_capabilities_keep_child_surface_unavailable_without_opt_in() {
        let capabilities = presentation_capabilities_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: true,
                reason: "available",
            },
            false,
        );

        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::EmbeddedNativeExperimental
            && !adapter.available
            && adapter.experimental));
    }

    #[test]
    fn presentation_capabilities_can_expose_child_surface_for_explicit_diagnostics() {
        let capabilities = presentation_capabilities_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: true,
                reason: "available",
            },
            true,
        );

        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::EmbeddedNativeExperimental
            && adapter.available
            && adapter.experimental
            && !adapter.default));
    }
}
