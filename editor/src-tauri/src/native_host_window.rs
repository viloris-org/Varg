//! Platform adapters for native host-window presentation.
//!
//! The target model is that native code owns the editor root window and embeds
//! Web UI as panels/overlays.

use std::num::NonZeroIsize;
use std::sync::mpsc;
use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::Manager;

use crate::editor_compositor;
use crate::scene_window;

pub struct NativeHostSceneTarget {
    pub surface: scene_window::SceneRawSurface,
    pub layout_mode: NativeHostLayoutMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeHostSceneRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl From<scene_window::SceneViewportRect> for NativeHostSceneRect {
    fn from(rect: scene_window::SceneViewportRect) -> Self {
        let rect = rect.sanitized();
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum NativeHostLayoutMode {
    /// Legacy Linux bridge: retrofit a host surface around Tauri's main WebView.
    BridgeTauriWebView,
    /// Target model: native host window owns root and embeds Web UI views.
    HostOwnedRoot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeHostBackend {
    X11,
    Wayland,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeHostWindowBackend {
    LinuxX11,
    LinuxWayland,
    Win32,
    AppKit,
    MobileRootView,
    UnsupportedDesktop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeHostRoute {
    LinuxBridge(NativeHostBackend),
    WindowsDirectComposition,
    MacosCoreAnimation,
    RootWindowSurface,
    UnsupportedDesktop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowsDirectCompositionHostPlan {
    platform: editor_compositor::NativeHostPlatformPlan,
    native_host_root: &'static str,
    scene_surface_route: &'static str,
    web_ui_route: &'static str,
}

impl WindowsDirectCompositionHostPlan {
    fn unavailable_error(self) -> String {
        platform_plan_error(
            self.platform,
            &[
                self.native_host_root,
                self.scene_surface_route,
                self.web_ui_route,
            ],
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MacosCoreAnimationHostPlan {
    platform: editor_compositor::NativeHostPlatformPlan,
    native_host_root: &'static str,
    scene_surface_route: &'static str,
    web_ui_route: &'static str,
}

impl MacosCoreAnimationHostPlan {
    fn unavailable_error(self) -> String {
        platform_plan_error(
            self.platform,
            &[
                self.native_host_root,
                self.scene_surface_route,
                self.web_ui_route,
            ],
        )
    }
}

const WINDOWS_DIRECTCOMPOSITION_HOST_PLAN: WindowsDirectCompositionHostPlan =
    WindowsDirectCompositionHostPlan {
        platform: editor_compositor::WINDOWS_NATIVE_HOST_PLAN,
        native_host_root: "native host root: HWND-owned DirectComposition visual tree",
        scene_surface_route: "scene route: WGPU output presented through a DXGI/DirectComposition surface",
        web_ui_route: "web UI route: WebView2 CompositionController attached as hosted visuals",
    };

const MACOS_CORE_ANIMATION_HOST_PLAN: MacosCoreAnimationHostPlan = MacosCoreAnimationHostPlan {
    platform: editor_compositor::MACOS_NATIVE_HOST_PLAN,
    native_host_root: "native host root: NSWindow/NSView-owned Core Animation layer tree",
    scene_surface_route: "scene route: WGPU output presented through CAMetalLayer",
    web_ui_route: "web UI route: WKWebView/AppKit panels embedded in the native view tree",
};

pub fn install_bridge_host_on_main_thread(
    app: &tauri::AppHandle,
) -> Result<NativeHostLayoutMode, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let handle = window
        .window_handle()
        .map_err(|error| format!("main window handle: {error}"))?
        .as_raw();
    match native_host_route(native_host_window_backend(handle)) {
        NativeHostRoute::LinuxBridge(NativeHostBackend::X11) => {
            ensure_linux_host_surface_on_main_thread(app, NativeHostBackend::X11)
        }
        NativeHostRoute::LinuxBridge(NativeHostBackend::Wayland) => {
            ensure_linux_host_surface_on_main_thread(app, NativeHostBackend::Wayland)
        }
        NativeHostRoute::WindowsDirectComposition => {
            Err(WINDOWS_DIRECTCOMPOSITION_HOST_PLAN.unavailable_error())
        }
        NativeHostRoute::MacosCoreAnimation => {
            Err(MACOS_CORE_ANIMATION_HOST_PLAN.unavailable_error())
        }
        NativeHostRoute::RootWindowSurface => Ok(NativeHostLayoutMode::HostOwnedRoot),
        NativeHostRoute::UnsupportedDesktop => Err(format!(
            "native host window Scene View does not support this desktop backend yet: {handle:?}"
        )),
    }
}

fn native_host_window_backend(handle: RawWindowHandle) -> NativeHostWindowBackend {
    match handle {
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => NativeHostWindowBackend::LinuxX11,
        RawWindowHandle::Wayland(_) => NativeHostWindowBackend::LinuxWayland,
        RawWindowHandle::Win32(_) => NativeHostWindowBackend::Win32,
        RawWindowHandle::AppKit(_) => NativeHostWindowBackend::AppKit,
        RawWindowHandle::UiKit(_) | RawWindowHandle::AndroidNdk(_) => {
            NativeHostWindowBackend::MobileRootView
        }
        _ => NativeHostWindowBackend::UnsupportedDesktop,
    }
}

fn native_host_route(backend: NativeHostWindowBackend) -> NativeHostRoute {
    match backend {
        NativeHostWindowBackend::LinuxX11 => NativeHostRoute::LinuxBridge(NativeHostBackend::X11),
        NativeHostWindowBackend::LinuxWayland => {
            NativeHostRoute::LinuxBridge(NativeHostBackend::Wayland)
        }
        NativeHostWindowBackend::Win32 => NativeHostRoute::WindowsDirectComposition,
        NativeHostWindowBackend::AppKit => NativeHostRoute::MacosCoreAnimation,
        NativeHostWindowBackend::MobileRootView => NativeHostRoute::RootWindowSurface,
        NativeHostWindowBackend::UnsupportedDesktop => NativeHostRoute::UnsupportedDesktop,
    }
}

pub fn main_window_scene_target(app: &tauri::AppHandle) -> Result<NativeHostSceneTarget, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let handle = window
        .window_handle()
        .map_err(|error| format!("main window handle: {error}"))?
        .as_raw();
    match native_host_route(native_host_window_backend(handle)) {
        NativeHostRoute::LinuxBridge(NativeHostBackend::X11) => {
            create_linux_host_surface(app.clone(), NativeHostBackend::X11)
        }
        NativeHostRoute::LinuxBridge(NativeHostBackend::Wayland) => {
            create_linux_host_surface(app.clone(), NativeHostBackend::Wayland)
        }
        NativeHostRoute::WindowsDirectComposition => create_windows_host_scene_target(),
        NativeHostRoute::MacosCoreAnimation => create_macos_host_scene_target(),
        NativeHostRoute::RootWindowSurface => create_root_window_scene_surface(app),
        NativeHostRoute::UnsupportedDesktop => Err(format!(
            "native host window Scene View does not support this desktop backend yet: {handle:?}"
        )),
    }
}

pub fn resize_main_window_scene_surface(
    app: tauri::AppHandle,
    rect: NativeHostSceneRect,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = mpsc::channel();
        app.clone()
            .run_on_main_thread(move || {
                let result = resize_linux_host_surface_on_main_thread(&app, rect);
                let _ = tx.send(result);
            })
            .map_err(|error| format!("schedule native host surface resize: {error}"))?;
        return rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| format!("native host surface resize timed out: {error}"))?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        let _ = rect;
        Ok(())
    }
}

fn platform_plan_error(
    plan: editor_compositor::NativeHostPlatformPlan,
    host_boundaries: &[&str],
) -> String {
    let mut blocking_work = Vec::with_capacity(host_boundaries.len() + plan.blocking_work.len());
    blocking_work.extend_from_slice(host_boundaries);
    blocking_work.extend_from_slice(plan.blocking_work);
    format!(
        "{} Blocking work: {}",
        plan.status,
        blocking_work.join("; ")
    )
}

#[cfg(target_os = "windows")]
fn create_windows_host_scene_target() -> Result<NativeHostSceneTarget, String> {
    Err(WINDOWS_DIRECTCOMPOSITION_HOST_PLAN.unavailable_error())
}

#[cfg(not(target_os = "windows"))]
fn create_windows_host_scene_target() -> Result<NativeHostSceneTarget, String> {
    Err("Windows native host adapter can only run on Windows.".to_owned())
}

#[cfg(target_os = "macos")]
fn create_macos_host_scene_target() -> Result<NativeHostSceneTarget, String> {
    Err(MACOS_CORE_ANIMATION_HOST_PLAN.unavailable_error())
}

#[cfg(not(target_os = "macos"))]
fn create_macos_host_scene_target() -> Result<NativeHostSceneTarget, String> {
    Err("macOS native host adapter can only run on macOS.".to_owned())
}

fn create_root_window_scene_surface(
    app: &tauri::AppHandle,
) -> Result<NativeHostSceneTarget, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let handle = window
        .window_handle()
        .map_err(|error| format!("main window handle: {error}"))?
        .as_raw();
    let surface = match handle {
        RawWindowHandle::Win32(handle) => scene_window::SceneRawSurface::Win32 {
            hwnd: handle.hwnd.get(),
            hinstance: handle.hinstance.map(NonZeroIsize::get),
        },
        RawWindowHandle::AppKit(handle) => scene_window::SceneRawSurface::AppKit {
            ns_view: handle.ns_view.as_ptr() as usize,
        },
        RawWindowHandle::UiKit(handle) => scene_window::SceneRawSurface::UiKit {
            ui_view: handle.ui_view.as_ptr() as usize,
            ui_view_controller: handle.ui_view_controller.map(|ptr| ptr.as_ptr() as usize),
        },
        RawWindowHandle::AndroidNdk(handle) => scene_window::SceneRawSurface::AndroidNdk {
            a_native_window: handle.a_native_window.as_ptr() as usize,
        },
        other => {
            return Err(format!(
                "native host root surface does not support this desktop backend yet: {other:?}"
            ));
        }
    };
    Ok(NativeHostSceneTarget {
        surface,
        layout_mode: NativeHostLayoutMode::HostOwnedRoot,
    })
}

#[cfg(target_os = "linux")]
fn create_linux_host_surface(
    app: tauri::AppHandle,
    backend: NativeHostBackend,
) -> Result<NativeHostSceneTarget, String> {
    let (tx, rx) = mpsc::channel();
    app.clone()
        .run_on_main_thread(move || {
            let result = create_linux_host_surface_on_main_thread(&app, backend);
            let _ = tx.send(result);
        })
        .map_err(|error| format!("schedule native host surface creation: {error}"))?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|error| format!("native host surface creation timed out: {error}"))?
}

#[cfg(target_os = "linux")]
fn ensure_linux_host_surface_on_main_thread(
    app: &tauri::AppHandle,
    backend: NativeHostBackend,
) -> Result<NativeHostLayoutMode, String> {
    create_linux_host_surface_on_main_thread(app, backend).map(|target| target.layout_mode)
}

#[cfg(not(target_os = "linux"))]
fn ensure_linux_host_surface_on_main_thread(
    _app: &tauri::AppHandle,
    _backend: NativeHostBackend,
) -> Result<NativeHostLayoutMode, String> {
    Err("native host window adapter is not implemented on this platform yet".to_owned())
}

#[cfg(not(target_os = "linux"))]
fn create_linux_host_surface(
    _app: tauri::AppHandle,
    _backend: NativeHostBackend,
) -> Result<NativeHostSceneTarget, String> {
    Err("native host window adapter is not implemented on this platform yet".to_owned())
}

#[cfg(not(target_os = "linux"))]
fn ensure_linux_host_surface(
    _app: tauri::AppHandle,
    _backend: NativeHostBackend,
) -> Result<NativeHostLayoutMode, String> {
    Err("native host window adapter is not implemented on this platform yet".to_owned())
}

#[cfg(target_os = "linux")]
fn create_linux_host_surface_on_main_thread(
    app: &tauri::AppHandle,
    backend: NativeHostBackend,
) -> Result<NativeHostSceneTarget, String> {
    use gtk::prelude::*;

    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let vbox = window
        .default_vbox()
        .map_err(|error| format!("main GTK vbox: {error}"))?;
    let vbox_widget: gtk::Widget = vbox.upcast();
    let _overlay = ensure_native_host_root(&vbox_widget)?;

    let drawing = find_named_widget(&vbox_widget, HOST_DRAWING_NAME)
        .and_then(|widget| widget.downcast::<gtk::DrawingArea>().ok())
        .ok_or_else(|| "native host drawing surface is missing".to_owned())?;
    drawing.show_all();
    drawing.realize();
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }

    let surface = gtk_drawing_area_raw_surface(&drawing, backend)?;
    Ok(NativeHostSceneTarget {
        surface,
        layout_mode: NativeHostLayoutMode::HostOwnedRoot,
    })
}

#[cfg(target_os = "linux")]
fn resize_linux_host_surface_on_main_thread(
    app: &tauri::AppHandle,
    rect: NativeHostSceneRect,
) -> Result<(), String> {
    use gtk::prelude::*;

    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let vbox = window
        .default_vbox()
        .map_err(|error| format!("main GTK vbox: {error}"))?;
    let vbox_widget: gtk::Widget = vbox.upcast();
    let overlay = ensure_native_host_root(&vbox_widget)?;
    let drawing = find_named_widget(&vbox_widget, HOST_DRAWING_NAME)
        .and_then(|widget| widget.downcast::<gtk::DrawingArea>().ok())
        .ok_or_else(|| "native host drawing surface is missing".to_owned())?;

    let allocation = overlay.allocation();
    let right = (allocation.width() - rect.x - rect.width as i32).max(0);
    let bottom = (allocation.height() - rect.y - rect.height as i32).max(0);
    drawing.set_margin_start(rect.x.max(0));
    drawing.set_margin_top(rect.y.max(0));
    drawing.set_margin_end(right);
    drawing.set_margin_bottom(bottom);
    drawing.set_size_request(rect.width.max(1) as i32, rect.height.max(1) as i32);
    drawing.queue_resize();
    drawing.show_all();
    Ok(())
}

#[cfg(target_os = "linux")]
const HOST_OVERLAY_NAME: &str = "aster-native-host-overlay";
#[cfg(target_os = "linux")]
const HOST_DRAWING_NAME: &str = "aster-native-host-scene-surface";

#[cfg(target_os = "linux")]
fn ensure_native_host_root(vbox_widget: &gtk::Widget) -> Result<gtk::Overlay, String> {
    use gtk::prelude::*;

    if let Some(widget) = find_named_widget(vbox_widget, HOST_OVERLAY_NAME) {
        return widget
            .downcast::<gtk::Overlay>()
            .map_err(|_| "native host overlay has unexpected GTK type".to_owned());
    }

    let vbox = vbox_widget
        .clone()
        .downcast::<gtk::Box>()
        .map_err(|_| "main GTK root has unexpected type".to_owned())?;
    let children = vbox.children();
    for child in &children {
        vbox.remove(child);
    }

    let drawing = gtk::DrawingArea::new();
    drawing.set_widget_name(HOST_DRAWING_NAME);
    drawing.set_has_window(true);
    drawing.set_hexpand(true);
    drawing.set_vexpand(true);
    drawing.set_halign(gtk::Align::Fill);
    drawing.set_valign(gtk::Align::Fill);
    drawing.set_no_show_all(false);

    let overlay = gtk::Overlay::new();
    overlay.set_widget_name(HOST_OVERLAY_NAME);
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_halign(gtk::Align::Fill);
    overlay.set_valign(gtk::Align::Fill);
    overlay.add(&drawing);

    for child in children {
        child.set_hexpand(true);
        child.set_vexpand(true);
        child.set_halign(gtk::Align::Fill);
        child.set_valign(gtk::Align::Fill);
        overlay.add_overlay(&child);
        overlay.set_overlay_pass_through(&child, false);
    }

    vbox.pack_start(&overlay, true, true, 0);
    overlay.show_all();
    Ok(overlay)
}

#[cfg(target_os = "linux")]
fn find_named_widget(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    use gtk::prelude::*;

    if root.widget_name().as_str() == name {
        return Some(root.clone());
    }
    let container = root.clone().downcast::<gtk::Container>().ok()?;
    for child in container.children() {
        if let Some(found) = find_named_widget(&child, name) {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn gtk_drawing_area_raw_surface(
    drawing: &gtk::DrawingArea,
    backend: NativeHostBackend,
) -> Result<scene_window::SceneRawSurface, String> {
    use gtk::prelude::*;

    let gdk_window = drawing
        .window()
        .ok_or_else(|| "GTK drawing area did not realize a native GDK window".to_owned())?;
    if !gdk_window.ensure_native() {
        return Err("GTK drawing area could not create a native surface".to_owned());
    }
    let display = gdk_window.display();

    match backend {
        NativeHostBackend::Wayland => {
            let wl_display = unsafe {
                gdk_wayland_sys::gdk_wayland_display_get_wl_display(
                    display.as_ptr() as *mut gdk_wayland_sys::GdkWaylandDisplay
                )
            };
            let wl_surface = unsafe {
                gdk_wayland_sys::gdk_wayland_window_get_wl_surface(
                    gdk_window.as_ptr() as *mut gdk_wayland_sys::GdkWaylandWindow
                )
            };
            if wl_display.is_null() || wl_surface.is_null() {
                return Err("GTK did not expose Wayland native surface handles".to_owned());
            }
            Ok(scene_window::SceneRawSurface::Wayland {
                display: wl_display as usize,
                surface: wl_surface as usize,
            })
        }
        NativeHostBackend::X11 => {
            let xdisplay = unsafe {
                gdk_x11_sys::gdk_x11_display_get_xdisplay(
                    display.as_ptr() as *mut gdk_x11_sys::GdkX11Display
                )
            };
            let xid = unsafe {
                gdk_x11_sys::gdk_x11_window_get_xid(
                    gdk_window.as_ptr() as *mut gdk_x11_sys::GdkX11Window
                )
            };
            if xdisplay.is_null() || xid == 0 {
                return Err("GTK did not expose X11 native surface handles".to_owned());
            }
            Ok(scene_window::SceneRawSurface::Xlib {
                display: xdisplay as usize,
                window: xid as u64,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_host_route_classifies_desktop_backends() {
        assert_eq!(
            native_host_route(NativeHostWindowBackend::LinuxX11),
            NativeHostRoute::LinuxBridge(NativeHostBackend::X11)
        );
        assert_eq!(
            native_host_route(NativeHostWindowBackend::LinuxWayland),
            NativeHostRoute::LinuxBridge(NativeHostBackend::Wayland)
        );
        assert_eq!(
            native_host_route(NativeHostWindowBackend::Win32),
            NativeHostRoute::WindowsDirectComposition
        );
        assert_eq!(
            native_host_route(NativeHostWindowBackend::AppKit),
            NativeHostRoute::MacosCoreAnimation
        );
    }

    #[test]
    fn native_host_route_keeps_mobile_root_surface_separate() {
        assert_eq!(
            native_host_route(NativeHostWindowBackend::MobileRootView),
            NativeHostRoute::RootWindowSurface
        );
        assert_eq!(
            native_host_route(NativeHostWindowBackend::UnsupportedDesktop),
            NativeHostRoute::UnsupportedDesktop
        );
    }

    #[test]
    fn windows_directcomposition_plan_formats_unavailable_boundaries() {
        let error = WINDOWS_DIRECTCOMPOSITION_HOST_PLAN.unavailable_error();

        assert!(error.contains("planned but not implemented"));
        assert!(error.contains("native host root: HWND-owned DirectComposition visual tree"));
        assert!(error.contains("DXGI/DirectComposition surface"));
        assert!(error.contains("WebView2 CompositionController"));
        assert!(error.contains("Blocking work:"));
        assert!(
            !WINDOWS_DIRECTCOMPOSITION_HOST_PLAN
                .platform
                .support()
                .available
        );
    }

    #[test]
    fn macos_core_animation_plan_formats_unavailable_boundaries() {
        let error = MACOS_CORE_ANIMATION_HOST_PLAN.unavailable_error();

        assert!(error.contains("planned but not implemented"));
        assert!(
            error.contains("native host root: NSWindow/NSView-owned Core Animation layer tree")
        );
        assert!(error.contains("CAMetalLayer"));
        assert!(error.contains("WKWebView/AppKit panels"));
        assert!(error.contains("Blocking work:"));
        assert!(!MACOS_CORE_ANIMATION_HOST_PLAN.platform.support().available);
    }
}
