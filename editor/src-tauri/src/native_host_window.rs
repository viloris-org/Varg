//! Platform adapters for native host-window presentation.
//!
//! The target model is that native code owns the editor root window and embeds
//! Web UI as panels/overlays.

use std::num::NonZeroIsize;
use std::sync::mpsc;
use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::Manager;

use crate::scene_window;

pub struct NativeHostSceneTarget {
    pub surface: scene_window::SceneRawSurface,
    pub surface_width: u32,
    pub surface_height: u32,
    pub layout_mode: NativeHostLayoutMode,
}

pub enum ExperimentalChildSceneTarget {
    Raw(scene_window::SceneRawSurface),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum NativeHostLayoutMode {
    /// Legacy Linux bridge: retrofit a host surface around Tauri's main WebView.
    BridgeTauriWebView,
    /// Target model: native host window owns root and embeds Web UI views.
    HostOwnedRoot,
}

#[derive(Clone, Copy)]
enum NativeHostBackend {
    X11,
    Wayland,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GtkSceneSurfaceOrigin {
    x: i32,
    y: i32,
}

fn gtk_scene_surface_rect(
    viewport: scene_window::SceneViewportRect,
    origin: GtkSceneSurfaceOrigin,
) -> scene_window::SceneViewportRect {
    scene_window::SceneViewportRect {
        x: viewport.x + origin.x,
        y: viewport.y + origin.y,
        width: viewport.width,
        height: viewport.height,
    }
    .sanitized()
}

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
    match handle {
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {
            ensure_linux_host_surface_on_main_thread(app, NativeHostBackend::X11)
        }
        RawWindowHandle::Wayland(_) => {
            ensure_linux_host_surface_on_main_thread(app, NativeHostBackend::Wayland)
        }
        RawWindowHandle::Win32(_)
        | RawWindowHandle::AppKit(_)
        | RawWindowHandle::UiKit(_)
        | RawWindowHandle::AndroidNdk(_) => Ok(NativeHostLayoutMode::HostOwnedRoot),
        other => Err(format!(
            "native host window Scene View does not support this desktop backend yet: {other:?}"
        )),
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
    match handle {
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {
            create_linux_host_surface(app.clone(), NativeHostBackend::X11)
        }
        RawWindowHandle::Wayland(_) => {
            create_linux_host_surface(app.clone(), NativeHostBackend::Wayland)
        }
        RawWindowHandle::Win32(_)
        | RawWindowHandle::AppKit(_)
        | RawWindowHandle::UiKit(_)
        | RawWindowHandle::AndroidNdk(_) => create_root_window_scene_surface(app),
        other => Err(format!(
            "native host window Scene View does not support this desktop backend yet: {other:?}"
        )),
    }
}

pub fn experimental_child_scene_target(
    app: &tauri::AppHandle,
    viewport: scene_window::SceneViewportRect,
) -> Result<ExperimentalChildSceneTarget, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let handle = window
        .window_handle()
        .map_err(|error| format!("main window handle: {error}"))?
        .as_raw();
    match handle {
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {
            tracing::info!(
                target: "editor",
                "opening experimental embedded Scene View on GTK/X11 child surface"
            );
            create_gtk_scene_surface(app.clone(), viewport, NativeHostBackend::X11)
                .map(ExperimentalChildSceneTarget::Raw)
        }
        RawWindowHandle::Wayland(_) => {
            tracing::info!(
                target: "editor",
                "opening experimental embedded Scene View on GTK/Wayland child surface"
            );
            create_gtk_scene_surface(app.clone(), viewport, NativeHostBackend::Wayland)
                .map(ExperimentalChildSceneTarget::Raw)
        }
        other => Err(format!(
            "embedded native Scene View does not support this desktop backend yet: {other:?}"
        )),
    }
}

fn create_root_window_scene_surface(
    app: &tauri::AppHandle,
) -> Result<NativeHostSceneTarget, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let size = window
        .inner_size()
        .map_err(|error| format!("main window inner size: {error}"))?;
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
        surface_width: size.width.max(1),
        surface_height: size.height.max(1),
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
    drawing.set_size_request(-1, -1);
    drawing.show_all();
    drawing.realize();
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }

    let allocation = drawing.allocation();
    let surface_width = allocation.width().max(1) as u32;
    let surface_height = allocation.height().max(1) as u32;
    let surface = gtk_drawing_area_raw_surface(&drawing, backend)?;
    Ok(NativeHostSceneTarget {
        surface,
        surface_width,
        surface_height,
        layout_mode: NativeHostLayoutMode::HostOwnedRoot,
    })
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

#[cfg(target_os = "linux")]
fn create_gtk_scene_surface(
    app: tauri::AppHandle,
    viewport: scene_window::SceneViewportRect,
    backend: NativeHostBackend,
) -> Result<scene_window::SceneRawSurface, String> {
    let (tx, rx) = mpsc::channel();
    app.clone()
        .run_on_main_thread(move || {
            let result = create_gtk_scene_surface_on_main_thread(&app, viewport, backend);
            let _ = tx.send(result);
        })
        .map_err(|error| format!("schedule GTK surface creation: {error}"))?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|error| format!("GTK surface creation timed out: {error}"))?
}

#[cfg(not(target_os = "linux"))]
fn create_gtk_scene_surface(
    _app: tauri::AppHandle,
    _viewport: scene_window::SceneViewportRect,
    _backend: NativeHostBackend,
) -> Result<scene_window::SceneRawSurface, String> {
    Err("GTK native embedding is only available on Linux".to_owned())
}

#[cfg(target_os = "linux")]
fn create_gtk_scene_surface_on_main_thread(
    app: &tauri::AppHandle,
    viewport: scene_window::SceneViewportRect,
    backend: NativeHostBackend,
) -> Result<scene_window::SceneRawSurface, String> {
    use gtk::prelude::*;

    const OVERLAY_NAME: &str = "aster-scene-native-overlay";
    const FIXED_NAME: &str = "aster-scene-native-fixed";
    const DRAWING_NAME: &str = "aster-scene-native-drawing-area";
    const WEBVIEW_NAME: &str = "aster-scene-native-webview";

    let window = app
        .get_window("main")
        .ok_or_else(|| "main editor window is not available".to_owned())?;
    let vbox = window
        .default_vbox()
        .map_err(|error| format!("main GTK vbox: {error}"))?;
    let vbox_widget: gtk::Widget = vbox.clone().upcast();

    let fixed = if let Some(widget) = find_named_widget(&vbox_widget, FIXED_NAME) {
        widget
            .downcast::<gtk::Fixed>()
            .map_err(|_| "native scene fixed widget has unexpected GTK type".to_owned())?
    } else {
        let children = vbox.children();
        let webview_widget = children
            .last()
            .cloned()
            .ok_or_else(|| "main GTK vbox has no webview child".to_owned())?;
        webview_widget.set_widget_name(WEBVIEW_NAME);
        vbox.remove(&webview_widget);
        webview_widget.set_hexpand(true);
        webview_widget.set_vexpand(true);
        webview_widget.set_halign(gtk::Align::Fill);
        webview_widget.set_valign(gtk::Align::Fill);

        let fixed = gtk::Fixed::new();
        fixed.set_widget_name(FIXED_NAME);
        fixed.set_hexpand(true);
        fixed.set_vexpand(true);
        fixed.set_halign(gtk::Align::Fill);
        fixed.set_valign(gtk::Align::Fill);

        let overlay = gtk::Overlay::new();
        overlay.set_widget_name(OVERLAY_NAME);
        overlay.add(&webview_widget);
        overlay.add_overlay(&fixed);
        overlay.set_overlay_pass_through(&fixed, true);

        vbox.pack_start(&overlay, true, true, 0);
        overlay.show_all();
        fixed
    };
    let webview_origin = find_named_widget(&vbox_widget, WEBVIEW_NAME)
        .map(|webview| {
            let allocation = webview.allocation();
            GtkSceneSurfaceOrigin {
                x: allocation.x(),
                y: allocation.y(),
            }
        })
        .unwrap_or(GtkSceneSurfaceOrigin { x: 0, y: 0 });
    let surface_viewport = gtk_scene_surface_rect(viewport, webview_origin);

    let drawing = if let Some(widget) = find_named_widget(&vbox_widget, DRAWING_NAME) {
        widget
            .downcast::<gtk::DrawingArea>()
            .map_err(|_| "native scene drawing widget has unexpected GTK type".to_owned())?
    } else {
        let drawing = gtk::DrawingArea::new();
        drawing.set_widget_name(DRAWING_NAME);
        drawing.set_has_window(true);
        drawing.set_can_focus(true);
        drawing.add_events(
            gdk::EventMask::BUTTON_PRESS_MASK
                | gdk::EventMask::BUTTON_RELEASE_MASK
                | gdk::EventMask::POINTER_MOTION_MASK
                | gdk::EventMask::SCROLL_MASK,
        );
        fixed.put(&drawing, surface_viewport.x, surface_viewport.y);
        drawing
    };

    drawing.set_size_request(
        surface_viewport.width as i32,
        surface_viewport.height as i32,
    );
    fixed.move_(&drawing, surface_viewport.x, surface_viewport.y);
    drawing.show_all();
    drawing.realize();
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }

    gtk_drawing_area_raw_surface(&drawing, backend)
}

#[cfg(target_os = "linux")]
pub fn sync_experimental_child_scene_surface(
    app: tauri::AppHandle,
    viewport: scene_window::SceneViewportRect,
) {
    let _ = app.clone().run_on_main_thread(move || {
        use gtk::prelude::*;

        const FIXED_NAME: &str = "aster-scene-native-fixed";
        const DRAWING_NAME: &str = "aster-scene-native-drawing-area";
        const WEBVIEW_NAME: &str = "aster-scene-native-webview";

        let Some(window) = app.get_window("main") else {
            return;
        };
        let Ok(vbox) = window.default_vbox() else {
            return;
        };
        let vbox_widget: gtk::Widget = vbox.upcast();
        let Some(fixed) = find_named_widget(&vbox_widget, FIXED_NAME)
            .and_then(|widget| widget.downcast::<gtk::Fixed>().ok())
        else {
            return;
        };
        let Some(drawing) = find_named_widget(&vbox_widget, DRAWING_NAME)
            .and_then(|widget| widget.downcast::<gtk::DrawingArea>().ok())
        else {
            return;
        };
        let webview_origin = find_named_widget(&vbox_widget, WEBVIEW_NAME)
            .map(|webview| {
                let allocation = webview.allocation();
                GtkSceneSurfaceOrigin {
                    x: allocation.x(),
                    y: allocation.y(),
                }
            })
            .unwrap_or(GtkSceneSurfaceOrigin { x: 0, y: 0 });
        let surface_viewport = gtk_scene_surface_rect(viewport, webview_origin);
        drawing.set_size_request(
            surface_viewport.width as i32,
            surface_viewport.height as i32,
        );
        fixed.move_(&drawing, surface_viewport.x, surface_viewport.y);
        drawing.show_all();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn sync_experimental_child_scene_surface(
    _app: tauri::AppHandle,
    _viewport: scene_window::SceneViewportRect,
) {
}

#[cfg(target_os = "linux")]
pub fn hide_experimental_child_scene_surface(app: tauri::AppHandle) {
    let _ = app.clone().run_on_main_thread(move || {
        use gtk::prelude::*;

        let Some(window) = app.get_window("main") else {
            return;
        };
        let Ok(vbox) = window.default_vbox() else {
            return;
        };
        let vbox_widget: gtk::Widget = vbox.upcast();
        if let Some(widget) = find_named_widget(&vbox_widget, "aster-scene-native-drawing-area") {
            widget.hide();
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn hide_experimental_child_scene_surface(_app: tauri::AppHandle) {}
