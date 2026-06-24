//! Native host-window editor presentation seam.
//!
//! This module is the replacement path for the experimental GTK child-surface
//! Scene View. The target architecture is:
//!
//! - a native host window owns the top-level editor presentation;
//! - engine Scene View regions are native WGPU-rendered surfaces;
//! - Web UI is embedded as panels, overlays, dock views, and input layers;
//! - no CPU readback and no OS child-surface movement.
//!
//! The current file intentionally contains the seam and state machine only.
//! Window/WebView transparency and platform-specific surface ownership will be
//! implemented behind this interface.

use engine_render_wgpu::SurfaceViewportRect;
use serde::Serialize;

use crate::scene_window;
use crate::wayland_embedded_compositor;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeHostPlatformPlan {
    pub backend: EditorCompositorBackend,
    pub host_api: &'static str,
    pub scene_surface: &'static str,
    pub web_ui: &'static str,
    pub status: &'static str,
    pub blocking_work: &'static [&'static str],
}

impl NativeHostPlatformPlan {
    #[allow(dead_code)]
    pub fn support(self) -> EditorCompositorSupport {
        EditorCompositorSupport {
            backend: self.backend,
            available: false,
            reason: self.status,
        }
    }
}

pub const WINDOWS_NATIVE_HOST_PLAN: NativeHostPlatformPlan = NativeHostPlatformPlan {
    backend: EditorCompositorBackend::WindowsWebView2,
    host_api: "DirectComposition visual tree",
    scene_surface: "D3D/DXGI composition swapchain or WGPU-compatible native surface",
    web_ui: "WebView2 CompositionController visual hosting",
    status: "Windows DirectComposition/WebView2 native host adapter is planned but not implemented yet; Scene View is unavailable without a no-CPU-readback adapter.",
    blocking_work: &[
        "create HWND-owned DirectComposition host tree",
        "connect Scene View WGPU output to a DirectComposition-compatible surface",
        "embed WebView2 through CompositionController visual hosting",
        "route DPI, pointer, keyboard, focus, accessibility, and resize through the host",
    ],
};

pub const MACOS_NATIVE_HOST_PLAN: NativeHostPlatformPlan = NativeHostPlatformPlan {
    backend: EditorCompositorBackend::MacosWkWebView,
    host_api: "NSWindow/NSView with Core Animation layer tree",
    scene_surface: "Metal-backed NSView/CAMetalLayer behind WGPU presentation",
    web_ui: "WKWebView/AppKit panel views",
    status: "macOS NSView/CAMetalLayer/WKWebView native host adapter is planned but not implemented yet; Scene View is unavailable without a no-CPU-readback adapter.",
    blocking_work: &[
        "create NSWindow/NSView root host adapter",
        "present Scene View through a Metal-backed native view/layer",
        "embed WKWebView panels in the same native view tree",
        "validate backing scale, color space, transparency, focus, and resize behavior",
    ],
};

pub fn platform_support() -> EditorCompositorSupport {
    #[cfg(target_os = "linux")]
    {
        linux_platform_support_for_runtime(wayland_embedded_compositor::is_wayland_session())
    }
    #[cfg(target_os = "windows")]
    {
        WINDOWS_NATIVE_HOST_PLAN.support()
    }
    #[cfg(target_os = "macos")]
    {
        MACOS_NATIVE_HOST_PLAN.support()
    }
    #[cfg(target_os = "android")]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::AndroidWebView,
            available: false,
            reason: "Android native host view with WebView panels is not implemented yet; Scene View is unavailable without a no-CPU-readback adapter.",
        }
    }
    #[cfg(target_os = "ios")]
    {
        EditorCompositorSupport {
            backend: EditorCompositorBackend::IosWkWebView,
            available: false,
            reason: "iOS native host view with WKWebView panels is not implemented yet; Scene View is unavailable without a no-CPU-readback adapter.",
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
            reason: "This platform has no native editor compositor adapter; Scene View is unavailable without a no-CPU-readback adapter.",
        }
    }
}

pub fn platform_support_for_window_handle(
    handle: raw_window_handle::RawWindowHandle,
) -> EditorCompositorSupport {
    #[cfg(target_os = "linux")]
    {
        match handle {
            raw_window_handle::RawWindowHandle::Xlib(_)
            | raw_window_handle::RawWindowHandle::Xcb(_) => {
                linux_platform_support_for_runtime(false)
            }
            raw_window_handle::RawWindowHandle::Wayland(_) => {
                linux_platform_support_for_runtime(true)
            }
            _ => EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: false,
                reason: "Linux native-host-window Scene View requires an X11/Xwayland GTK window handle.",
            },
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = handle;
        platform_support()
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_platform_support_for_runtime(is_wayland: bool) -> EditorCompositorSupport {
    if is_wayland {
        return EditorCompositorSupport {
            backend: EditorCompositorBackend::LinuxGtk,
            available: false,
            reason: "Linux native Wayland Scene View embedding is disabled.",
        };
    }

    EditorCompositorSupport {
        backend: EditorCompositorBackend::LinuxGtk,
        available: true,
        reason: "Linux GTK/X11 native-host-window adapter is available; the Scene View renders into the host-owned GTK/X11 surface.",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViewportPresentationMode {
    NativeHostWindow,
    WaylandEmbeddedCompositor,
    /// Legacy compatibility name for the native host-window architecture.
    EditorCompositor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum DirectScanoutSupport {
    No,
    Maybe,
    YesWhenUnobscured,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewportPresentationAdapter {
    pub mode: ViewportPresentationMode,
    pub available: bool,
    pub default: bool,
    /// Compatibility field for older frontend code. This means "no CPU readback",
    /// not guaranteed display-controller scanout.
    pub zero_copy: bool,
    pub experimental: bool,
    pub backend: &'static str,
    pub cpu_readback: bool,
    pub gpu_native_surface: bool,
    pub gpu_composited: bool,
    pub direct_scanout_possible: DirectScanoutSupport,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewportPresentationCapabilities {
    pub default_mode: Option<ViewportPresentationMode>,
    pub adapters: Vec<ViewportPresentationAdapter>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewportPresentationPlatformStatus {
    pub backend: &'static str,
    pub available: bool,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewportPresentationWaylandEmbeddedCompositorStatus {
    pub backend: &'static str,
    pub status: wayland_embedded_compositor::WaylandEmbeddedCompositorStatus,
    pub available: bool,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewportPresentationStatus {
    pub compositor_requested: bool,
    pub default_mode: Option<ViewportPresentationMode>,
    pub selected_backend: Option<&'static str>,
    pub adapters: Vec<ViewportPresentationAdapter>,
    pub platform_support: ViewportPresentationPlatformStatus,
    pub wayland_embedded_compositor: ViewportPresentationWaylandEmbeddedCompositorStatus,
    pub unavailable_reason: Option<String>,
}

pub fn presentation_capabilities(compositor_requested: bool) -> ViewportPresentationCapabilities {
    presentation_capabilities_for(
        compositor_requested,
        platform_support(),
        wayland_embedded_compositor::support(),
    )
}

pub fn presentation_capabilities_for(
    compositor_requested: bool,
    support: EditorCompositorSupport,
    wayland_support: wayland_embedded_compositor::WaylandEmbeddedCompositorSupport,
) -> ViewportPresentationCapabilities {
    let native_host_available = compositor_requested && support.available;
    let wayland_embedded_available = compositor_requested && wayland_support.available;
    let default_mode = if native_host_available {
        Some(ViewportPresentationMode::NativeHostWindow)
    } else if wayland_embedded_available {
        Some(ViewportPresentationMode::WaylandEmbeddedCompositor)
    } else {
        None
    };
    ViewportPresentationCapabilities {
        default_mode,
        adapters: vec![
            ViewportPresentationAdapter {
                mode: ViewportPresentationMode::WaylandEmbeddedCompositor,
                available: wayland_embedded_available,
                default: default_mode == Some(ViewportPresentationMode::WaylandEmbeddedCompositor),
                zero_copy: true,
                experimental: false,
                backend: wayland_embedded_compositor::BACKEND_ID,
                cpu_readback: false,
                gpu_native_surface: true,
                gpu_composited: true,
                direct_scanout_possible: DirectScanoutSupport::Maybe,
                reason: wayland_support.reason,
            },
            ViewportPresentationAdapter {
                mode: ViewportPresentationMode::NativeHostWindow,
                available: native_host_available,
                default: default_mode == Some(ViewportPresentationMode::NativeHostWindow),
                zero_copy: true,
                experimental: false,
                backend: support.backend.id(),
                cpu_readback: false,
                gpu_native_surface: true,
                gpu_composited: true,
                direct_scanout_possible: DirectScanoutSupport::Maybe,
                reason: support.reason,
            },
            ViewportPresentationAdapter {
                mode: ViewportPresentationMode::EditorCompositor,
                available: native_host_available,
                default: false,
                zero_copy: true,
                experimental: false,
                backend: support.backend.id(),
                cpu_readback: false,
                gpu_native_surface: true,
                gpu_composited: true,
                direct_scanout_possible: DirectScanoutSupport::Maybe,
                reason: "Legacy alias for native-host-window.",
            },
        ],
    }
}

pub fn presentation_status(compositor_requested: bool) -> ViewportPresentationStatus {
    presentation_status_for(
        compositor_requested,
        platform_support(),
        wayland_embedded_compositor::support(),
    )
}

pub fn presentation_status_for(
    compositor_requested: bool,
    support: EditorCompositorSupport,
    wayland_support: wayland_embedded_compositor::WaylandEmbeddedCompositorSupport,
) -> ViewportPresentationStatus {
    let capabilities =
        presentation_capabilities_for(compositor_requested, support, wayland_support);
    let selected_backend = capabilities
        .adapters
        .iter()
        .find(|adapter| adapter.default)
        .map(|adapter| adapter.backend);
    let unavailable_reason = scene_view_unavailable_reason(
        compositor_requested,
        support,
        wayland_support,
        capabilities.default_mode,
    );

    ViewportPresentationStatus {
        compositor_requested,
        default_mode: capabilities.default_mode,
        selected_backend,
        adapters: capabilities.adapters,
        platform_support: ViewportPresentationPlatformStatus {
            backend: support.backend.id(),
            available: support.available,
            reason: support.reason,
        },
        wayland_embedded_compositor: ViewportPresentationWaylandEmbeddedCompositorStatus {
            backend: wayland_embedded_compositor::BACKEND_ID,
            status: wayland_support.status,
            available: wayland_support.available,
            reason: wayland_support.reason,
        },
        unavailable_reason,
    }
}

fn wayland_embedded_status_id(
    status: wayland_embedded_compositor::WaylandEmbeddedCompositorStatus,
) -> &'static str {
    match status {
        wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::Available => "available",
        wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::Incomplete => "incomplete",
        wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::FeatureDisabled => {
            "feature-disabled"
        }
        wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::NotWayland => "not-wayland",
        wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::MissingBackend => {
            "missing-backend"
        }
    }
}

fn scene_view_unavailable_reason(
    compositor_requested: bool,
    support: EditorCompositorSupport,
    wayland_support: wayland_embedded_compositor::WaylandEmbeddedCompositorSupport,
    default_mode: Option<ViewportPresentationMode>,
) -> Option<String> {
    if default_mode.is_some() {
        return None;
    }

    if !compositor_requested {
        return Some(
            "Scene View unavailable because no no-CPU-readback viewport adapter is enabled."
                .to_owned(),
        );
    }

    Some(format!(
        "Scene View unavailable because native host {} is unavailable ({}) and {} is unavailable ({}: {}).",
        support.backend.id(),
        support.reason,
        wayland_embedded_compositor::BACKEND_ID,
        wayland_embedded_status_id(wayland_support.status),
        wayland_support.reason
    ))
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

    fn unavailable_wayland_support() -> wayland_embedded_compositor::WaylandEmbeddedCompositorSupport
    {
        wayland_embedded_compositor::WaylandEmbeddedCompositorSupport {
            status: wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::FeatureDisabled,
            available: false,
            reason: "feature disabled",
        }
    }

    fn available_wayland_support() -> wayland_embedded_compositor::WaylandEmbeddedCompositorSupport
    {
        wayland_embedded_compositor::WaylandEmbeddedCompositorSupport {
            status: wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::Available,
            available: true,
            reason: "available",
        }
    }

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

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_platform_support_disables_gtk_native_host_on_wayland() {
        let support = linux_platform_support_for_runtime(true);
        assert_eq!(support.backend, EditorCompositorBackend::LinuxGtk);
        assert!(!support.available);
        assert!(support.reason.contains("Wayland"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_platform_support_enables_gtk_native_host_on_x11() {
        let support = linux_platform_support_for_runtime(false);
        assert_eq!(support.backend, EditorCompositorBackend::LinuxGtk);
        assert!(support.available);
        assert!(support.reason.contains("X11"));
    }

    #[test]
    fn windows_native_host_plan_is_explicit_but_unavailable() {
        let support = WINDOWS_NATIVE_HOST_PLAN.support();
        assert_eq!(support.backend, EditorCompositorBackend::WindowsWebView2);
        assert!(!support.available);
        assert!(
            WINDOWS_NATIVE_HOST_PLAN
                .host_api
                .contains("DirectComposition")
        );
        assert!(WINDOWS_NATIVE_HOST_PLAN.web_ui.contains("WebView2"));
        assert!(!WINDOWS_NATIVE_HOST_PLAN.blocking_work.is_empty());
    }

    #[test]
    fn macos_native_host_plan_is_explicit_but_unavailable() {
        let support = MACOS_NATIVE_HOST_PLAN.support();
        assert_eq!(support.backend, EditorCompositorBackend::MacosWkWebView);
        assert!(!support.available);
        assert!(MACOS_NATIVE_HOST_PLAN.host_api.contains("NSWindow"));
        assert!(
            MACOS_NATIVE_HOST_PLAN
                .scene_surface
                .contains("CAMetalLayer")
        );
        assert!(!MACOS_NATIVE_HOST_PLAN.blocking_work.is_empty());
    }

    #[test]
    fn presentation_capabilities_have_no_default_when_explicitly_disabled() {
        let capabilities = presentation_capabilities_for(
            false,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: true,
                reason: "available",
            },
            unavailable_wayland_support(),
        );

        assert_eq!(capabilities.default_mode, None);
        assert!(capabilities.adapters.iter().all(|adapter| !adapter.default));
        assert!(
            capabilities
                .adapters
                .iter()
                .all(|adapter| !adapter.cpu_readback)
        );
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
            unavailable_wayland_support(),
        );

        assert_eq!(
            capabilities.default_mode,
            Some(ViewportPresentationMode::NativeHostWindow)
        );
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::NativeHostWindow
            && adapter.available
            && adapter.default
            && adapter.zero_copy
            && !adapter.cpu_readback
            && adapter.gpu_native_surface
            && adapter.gpu_composited
            && adapter.direct_scanout_possible == DirectScanoutSupport::Maybe));
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::EditorCompositor
            && adapter.available
            && !adapter.default));
    }

    #[test]
    fn presentation_capabilities_have_no_default_when_platform_adapter_is_missing() {
        let capabilities = presentation_capabilities_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::WindowsWebView2,
                available: false,
                reason: "not implemented",
            },
            unavailable_wayland_support(),
        );

        assert_eq!(capabilities.default_mode, None);
        assert!(capabilities.adapters.iter().all(|adapter| !adapter.default));
        assert!(
            capabilities
                .adapters
                .iter()
                .all(|adapter| adapter.backend != "webview-canvas")
        );
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::NativeHostWindow
            && !adapter.available
            && adapter.backend == "windows-webview2"));
    }

    #[test]
    fn presentation_status_reports_windows_unavailable_scene_view_reason() {
        let status = presentation_status_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::WindowsWebView2,
                available: false,
                reason: "not implemented",
            },
            unavailable_wayland_support(),
        );

        let unavailable_reason = status.unavailable_reason.unwrap();
        assert_eq!(status.default_mode, None);
        assert_eq!(status.selected_backend, None);
        assert_eq!(status.platform_support.backend, "windows-webview2");
        assert!(!status.platform_support.available);
        assert!(unavailable_reason.contains("windows-webview2"));
        assert!(unavailable_reason.contains("not implemented"));
        assert!(unavailable_reason.contains("feature-disabled"));
        assert!(unavailable_reason.contains("feature disabled"));
    }

    #[test]
    fn presentation_status_reports_wayland_feature_disabled_scene_view_reason() {
        let status = presentation_status_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: false,
                reason: "disabled on Wayland",
            },
            unavailable_wayland_support(),
        );

        let unavailable_reason = status.unavailable_reason.unwrap();
        assert_eq!(status.default_mode, None);
        assert_eq!(status.selected_backend, None);
        assert_eq!(
            status.wayland_embedded_compositor.status,
            wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::FeatureDisabled
        );
        assert!(!status.wayland_embedded_compositor.available);
        assert!(unavailable_reason.contains("linux-gtk"));
        assert!(unavailable_reason.contains("disabled on Wayland"));
        assert!(unavailable_reason.contains("wayland-embedded-compositor"));
        assert!(unavailable_reason.contains("feature-disabled"));
    }

    #[test]
    fn presentation_capabilities_prefer_native_host_over_wayland_backend_when_both_available() {
        let capabilities = presentation_capabilities_for(
            true,
            EditorCompositorSupport {
                backend: EditorCompositorBackend::LinuxGtk,
                available: true,
                reason: "native host available",
            },
            available_wayland_support(),
        );

        assert_eq!(
            capabilities.default_mode,
            Some(ViewportPresentationMode::NativeHostWindow)
        );
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::WaylandEmbeddedCompositor
            && adapter.available
            && !adapter.default
            && !adapter.experimental
            && adapter.zero_copy
            && !adapter.cpu_readback
            && adapter.gpu_native_surface
            && adapter.gpu_composited));
        assert!(capabilities.adapters.iter().any(|adapter| adapter.mode
            == ViewportPresentationMode::NativeHostWindow
            && adapter.default));
    }
}
