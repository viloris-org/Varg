//! Wayland embedded compositor presentation boundary.
//!
//! Wayland does not provide an X11-style foreign child-window embedding model.
//! The production zero-copy path for Wayland is therefore an application-owned
//! embedded compositor that imports render buffers through DMA-BUF and composites
//! them with Web UI views inside the host window.
//!
//! This module owns that boundary. The default build exposes the capability and
//! refusal semantics without pretending the old GTK child-surface bridge is a
//! real Wayland solution. The `wayland-embedded-compositor` feature is reserved
//! for the platform backend that wires a compositor implementation, DMA-BUF
//! import, explicit synchronization, input routing, and WebView panel surfaces.

use std::thread;
#[cfg(feature = "wayland-embedded-compositor")]
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::scene_window;

pub const BACKEND_ID: &str = "wayland-embedded-compositor";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum WaylandEmbeddedCompositorStatus {
    Available,
    Incomplete,
    FeatureDisabled,
    NotWayland,
    MissingBackend,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct WaylandEmbeddedCompositorSupport {
    pub status: WaylandEmbeddedCompositorStatus,
    pub available: bool,
    pub reason: &'static str,
}

impl WaylandEmbeddedCompositorSupport {
    pub fn unavailable_error(self) -> String {
        format!("{} is unavailable: {}", BACKEND_ID, self.reason)
    }
}

pub fn support() -> WaylandEmbeddedCompositorSupport {
    support_for_runtime(is_wayland_session())
}

pub fn support_for_runtime(is_wayland: bool) -> WaylandEmbeddedCompositorSupport {
    if !is_wayland {
        return WaylandEmbeddedCompositorSupport {
            status: WaylandEmbeddedCompositorStatus::NotWayland,
            available: false,
            reason: "the current desktop session is not Wayland",
        };
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    {
        if !experimental_backend_enabled() {
            return WaylandEmbeddedCompositorSupport {
                status: WaylandEmbeddedCompositorStatus::Incomplete,
                available: false,
                reason: "Wayland embedded compositor backend can import DMA-BUFs but does not yet composite frames into the editor host; set ASTER_WAYLAND_EMBEDDED_EXPERIMENTAL=1 to test it.",
            };
        }

        backend_support()
    }

    #[cfg(not(feature = "wayland-embedded-compositor"))]
    {
        WaylandEmbeddedCompositorSupport {
            status: WaylandEmbeddedCompositorStatus::FeatureDisabled,
            available: false,
            reason: "built without the wayland-embedded-compositor backend feature",
        }
    }
}

#[cfg(feature = "wayland-embedded-compositor")]
fn experimental_backend_enabled() -> bool {
    std::env::var("ASTER_WAYLAND_EMBEDDED_EXPERIMENTAL").is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

pub fn is_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct WaylandEmbeddedViewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl WaylandEmbeddedViewport {
    pub fn from_scene_rect(rect: scene_window::SceneViewportRect) -> Self {
        let rect = rect.sanitized();
        Self {
            x: rect.x.max(0) as u32,
            y: rect.y.max(0) as u32,
            width: rect.width.max(1),
            height: rect.height.max(1),
        }
    }

    pub fn into_scene_rect(self) -> scene_window::SceneViewportRect {
        scene_window::SceneViewportRect {
            x: self.x.min(i32::MAX as u32) as i32,
            y: self.y.min(i32::MAX as u32) as i32,
            width: self.width.max(1),
            height: self.height.max(1),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WaylandEmbeddedHostOutputTarget {
    surface: scene_window::SceneRawSurface,
    viewport: WaylandEmbeddedViewport,
}

impl WaylandEmbeddedHostOutputTarget {
    pub fn new(surface: scene_window::SceneRawSurface, viewport: WaylandEmbeddedViewport) -> Self {
        Self { surface, viewport }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WaylandEmbeddedCompositorRuntimeStatus {
    pub socket_name: Option<String>,
    pub viewport: Option<WaylandEmbeddedViewport>,
    pub dmabuf_available: bool,
    pub dmabuf_reason: &'static str,
    pub imported_frames: u64,
    pub committed_frames: u64,
    pub frame_callbacks_sent: u64,
    pub pending_frame_callbacks: u64,
    pub latest_frame: Option<WaylandEmbeddedFrameStatus>,
    pub output_composition: WaylandEmbeddedOutputCompositionStatus,
}

impl WaylandEmbeddedCompositorRuntimeStatus {
    #[cfg(feature = "wayland-embedded-compositor")]
    fn stopped(viewport: Option<WaylandEmbeddedViewport>) -> Self {
        Self {
            socket_name: None,
            viewport,
            dmabuf_available: false,
            dmabuf_reason: "DMA-BUF global is stopped with the embedded compositor runtime.",
            imported_frames: 0,
            committed_frames: 0,
            frame_callbacks_sent: 0,
            pending_frame_callbacks: 0,
            latest_frame: None,
            output_composition: WaylandEmbeddedOutputCompositionStatus::not_started(),
        }
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    fn running(
        socket_name: String,
        viewport: Option<WaylandEmbeddedViewport>,
        telemetry: WaylandEmbeddedCompositorTelemetry,
    ) -> Self {
        Self {
            socket_name: Some(socket_name),
            viewport,
            dmabuf_available: true,
            dmabuf_reason: "zwp_linux_dmabuf_v1 is backed by Smithay EGL/GLES DMA-BUF import.",
            imported_frames: telemetry.imported_frames,
            committed_frames: telemetry.committed_frames,
            frame_callbacks_sent: telemetry.frame_callbacks_sent,
            pending_frame_callbacks: telemetry.pending_frame_callbacks,
            latest_frame: telemetry.latest_frame,
            output_composition: telemetry.output_composition,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct WaylandEmbeddedFrameStatus {
    pub width: i32,
    pub height: i32,
    pub fourcc: u32,
    pub import_sequence: u64,
    pub commit_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WaylandEmbeddedOutputCompositionStatus {
    pub attached: bool,
    pub composed_frames: u64,
    pub reason: String,
}

impl WaylandEmbeddedOutputCompositionStatus {
    fn not_started() -> Self {
        Self {
            attached: false,
            composed_frames: 0,
            reason: "host output composition target is not attached yet.".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WaylandEmbeddedCompositorTelemetry {
    pub imported_frames: u64,
    pub committed_frames: u64,
    pub frame_callbacks_sent: u64,
    pub pending_frame_callbacks: u64,
    pub latest_frame: Option<WaylandEmbeddedFrameStatus>,
    pub output_composition: WaylandEmbeddedOutputCompositionStatus,
}

impl Default for WaylandEmbeddedCompositorTelemetry {
    fn default() -> Self {
        Self {
            imported_frames: 0,
            committed_frames: 0,
            frame_callbacks_sent: 0,
            pending_frame_callbacks: 0,
            latest_frame: None,
            output_composition: WaylandEmbeddedOutputCompositionStatus::not_started(),
        }
    }
}

#[derive(Debug, Default)]
pub struct WaylandEmbeddedCompositor {
    viewport: Option<WaylandEmbeddedViewport>,
    host_output_target: Option<WaylandEmbeddedHostOutputTarget>,
    handle: Option<WaylandEmbeddedCompositorHandle>,
}

impl WaylandEmbeddedCompositor {
    pub fn set_viewport(&mut self, viewport: WaylandEmbeddedViewport) {
        self.viewport = Some(viewport);
        #[cfg(feature = "wayland-embedded-compositor")]
        if let Some(handle) = self.handle.as_ref() {
            handle.set_viewport(viewport);
        }
    }

    pub fn set_host_output_target(&mut self, target: WaylandEmbeddedHostOutputTarget) {
        self.viewport = Some(target.viewport);
        self.host_output_target = Some(target);
        #[cfg(feature = "wayland-embedded-compositor")]
        if let Some(handle) = self.handle.as_ref() {
            handle.set_host_output_target(target);
        }
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    pub fn status(&self) -> WaylandEmbeddedCompositorRuntimeStatus {
        if let Some(handle) = self.handle.as_ref() {
            return WaylandEmbeddedCompositorRuntimeStatus::running(
                handle.socket_name().to_owned(),
                self.viewport,
                handle.telemetry(),
            );
        }

        WaylandEmbeddedCompositorRuntimeStatus::stopped(self.viewport)
    }

    #[cfg(not(feature = "wayland-embedded-compositor"))]
    pub fn status(&self) -> WaylandEmbeddedCompositorRuntimeStatus {
        WaylandEmbeddedCompositorRuntimeStatus {
            socket_name: None,
            viewport: self.viewport,
            dmabuf_available: false,
            dmabuf_reason: "built without the wayland-embedded-compositor backend feature",
            imported_frames: 0,
            committed_frames: 0,
            frame_callbacks_sent: 0,
            pending_frame_callbacks: 0,
            latest_frame: None,
            output_composition: WaylandEmbeddedOutputCompositionStatus::not_started(),
        }
    }

    pub fn open_scene_view(&mut self) -> Result<WaylandEmbeddedCompositorRuntimeStatus, String> {
        let support = support();
        if !support.available {
            return Err(support.unavailable_error());
        }

        #[cfg(feature = "wayland-embedded-compositor")]
        {
            if self.handle.is_none() {
                self.handle = Some(start_backend(self.viewport)?);
            }
            if let Some(handle) = self.handle.as_ref() {
                if let Some(target) = self.host_output_target {
                    handle.set_host_output_target(target);
                }
                tracing::info!(
                    target: "editor",
                    socket = handle.socket_name(),
                    "Wayland embedded compositor scene view is ready for DMA-BUF clients"
                );
            }
            return Ok(self.status());
        }

        #[cfg(not(feature = "wayland-embedded-compositor"))]
        {
            Err(support.unavailable_error())
        }
    }
}

impl Drop for WaylandEmbeddedCompositor {
    fn drop(&mut self) {
        self.handle.take();
    }
}

#[derive(Debug)]
pub struct WaylandEmbeddedCompositorHandle {
    #[cfg(feature = "wayland-embedded-compositor")]
    socket_name: String,
    #[cfg(feature = "wayland-embedded-compositor")]
    command_tx: std::sync::mpsc::Sender<backend::RuntimeCommand>,
    #[cfg(feature = "wayland-embedded-compositor")]
    telemetry: std::sync::Arc<std::sync::Mutex<WaylandEmbeddedCompositorTelemetry>>,
    thread: Option<thread::JoinHandle<()>>,
}

#[cfg(feature = "wayland-embedded-compositor")]
impl WaylandEmbeddedCompositorHandle {
    pub fn socket_name(&self) -> &str {
        &self.socket_name
    }

    pub fn set_viewport(&self, viewport: WaylandEmbeddedViewport) {
        if let Err(error) = self
            .command_tx
            .send(backend::RuntimeCommand::SetViewport(viewport))
        {
            tracing::warn!(
                target: "editor",
                "failed to update Wayland embedded compositor viewport: {error}"
            );
        }
    }

    pub fn set_host_output_target(&self, target: WaylandEmbeddedHostOutputTarget) {
        if let Err(error) = self
            .command_tx
            .send(backend::RuntimeCommand::SetHostOutputTarget(target))
        {
            tracing::warn!(
                target: "editor",
                "failed to update Wayland embedded compositor host output target: {error}"
            );
        }
    }

    pub fn telemetry(&self) -> WaylandEmbeddedCompositorTelemetry {
        self.telemetry
            .lock()
            .map(|telemetry| telemetry.clone())
            .unwrap_or_default()
    }
}

impl Drop for WaylandEmbeddedCompositorHandle {
    fn drop(&mut self) {
        #[cfg(feature = "wayland-embedded-compositor")]
        let _ = self.command_tx.send(backend::RuntimeCommand::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(feature = "wayland-embedded-compositor")]
fn backend_support() -> WaylandEmbeddedCompositorSupport {
    match backend::dmabuf_import_support() {
        Ok(format_count) => {
            tracing::debug!(
                target: "editor",
                format_count,
                "Wayland embedded compositor DMA-BUF importer probe succeeded"
            );
            WaylandEmbeddedCompositorSupport {
                status: WaylandEmbeddedCompositorStatus::Available,
                available: true,
                reason: "Wayland embedded compositor backend is available with EGL/GLES DMA-BUF import.",
            }
        }
        Err(error) => {
            tracing::warn!(
                target: "editor",
                "Wayland embedded compositor DMA-BUF importer probe failed: {error}"
            );
            WaylandEmbeddedCompositorSupport {
                status: WaylandEmbeddedCompositorStatus::MissingBackend,
                available: false,
                reason: "EGL/GLES DMA-BUF import is unavailable for the embedded compositor.",
            }
        }
    }
}

#[cfg(feature = "wayland-embedded-compositor")]
fn start_backend(
    viewport: Option<WaylandEmbeddedViewport>,
) -> Result<WaylandEmbeddedCompositorHandle, String> {
    backend::start_runtime(viewport)
}

#[cfg(feature = "wayland-embedded-compositor")]
mod backend {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::OnceLock;

    use smithay::backend::allocator::{Buffer, Format, Fourcc, dmabuf::Dmabuf};
    use smithay::backend::egl::{
        EGLContext, EGLDisplay, EGLSurface,
        context::{GlAttributes, PixelFormatRequirements},
        ffi,
        native::{EGLNativeDisplay, EGLPlatform, EGLSurfacelessDisplay},
    };
    use smithay::backend::renderer::{
        Bind, Color32F, Frame, ImportDma, Offscreen, Renderer, Texture, TextureFilter,
        gles::{GlesRenderer, GlesTexture},
        utils::on_commit_buffer_handler,
    };
    use smithay::delegate_compositor;
    use smithay::delegate_dmabuf;
    use smithay::delegate_output;
    use smithay::delegate_seat;
    use smithay::delegate_shm;
    use smithay::delegate_xdg_shell;
    use smithay::egl_platform;
    use smithay::input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus};
    use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
    use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
    use smithay::reexports::wayland_server::protocol::{
        wl_buffer::WlBuffer, wl_seat, wl_shm, wl_surface::WlSurface,
    };
    use smithay::reexports::wayland_server::{Client, Display, ListeningSocket, Resource};
    use smithay::utils::{Physical, Rectangle, Serial, Size, Transform};
    use smithay::wayland::buffer::BufferHandler;
    use smithay::wayland::compositor::{
        BufferAssignment, CompositorClientState, CompositorHandler, CompositorState,
        SurfaceAttributes, TraversalAction, with_states, with_surface_tree_downward,
    };
    use smithay::wayland::dmabuf::{
        DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier, get_dmabuf,
    };
    use smithay::wayland::output::OutputHandler;
    use smithay::wayland::shell::xdg::{
        PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    };
    use smithay::wayland::shm::{ShmHandler, ShmState};
    use wayland_egl::WlEglSurface;

    use super::*;

    #[derive(Debug)]
    pub enum RuntimeCommand {
        SetViewport(WaylandEmbeddedViewport),
        SetHostOutputTarget(WaylandEmbeddedHostOutputTarget),
        Stop,
    }

    struct EmbeddedCompositorState {
        viewport: Option<WaylandEmbeddedViewport>,
        compositor_state: CompositorState,
        shm_state: ShmState,
        dmabuf_state: DmabufState,
        _dmabuf_global: DmabufGlobal,
        dmabuf_importer: DmabufImporter,
        seat_state: SeatState<Self>,
        xdg_shell_state: XdgShellState,
        toplevel_surfaces: Vec<ToplevelSurface>,
        imported_buffers:
            HashMap<smithay::reexports::wayland_server::backend::ObjectId, ImportedDmabufFrame>,
        latest_frame: Option<CommittedDmabufFrame>,
        host_output: EmbeddedHostOutput,
        committed_surfaces: u64,
        frame_callbacks_sent: u64,
        pending_frame_callbacks: u64,
        pending_frame_delivery: bool,
        telemetry: std::sync::Arc<std::sync::Mutex<WaylandEmbeddedCompositorTelemetry>>,
        start_time: Instant,
    }

    impl EmbeddedCompositorState {
        fn set_viewport(&mut self, viewport: WaylandEmbeddedViewport) {
            self.viewport = Some(viewport);
            self.host_output.set_viewport(viewport);
            self.configure_toplevels_for_viewport();
        }

        fn set_host_output_target(&mut self, target: WaylandEmbeddedHostOutputTarget) {
            self.viewport = Some(target.viewport);
            self.host_output.set_target(target);
            self.configure_toplevels_for_viewport();
        }

        fn configure_toplevels_for_viewport(&mut self) {
            let Some(viewport) = self.viewport else {
                return;
            };
            self.toplevel_surfaces.retain(ToplevelSurface::alive);
            for surface in &self.toplevel_surfaces {
                configure_toplevel_surface(surface, viewport);
                let serial = surface.send_configure();
                tracing::debug!(
                    target: "editor",
                    ?serial,
                    ?viewport,
                    "Wayland embedded compositor xdg toplevel resized"
                );
            }
        }

        fn record_telemetry(&self) {
            let latest_frame = self
                .latest_frame
                .as_ref()
                .map(|frame| WaylandEmbeddedFrameStatus {
                    width: frame.size.w,
                    height: frame.size.h,
                    fourcc: frame.format.code as u32,
                    import_sequence: frame.import_sequence,
                    commit_sequence: frame.commit_sequence,
                });
            if let Ok(mut telemetry) = self.telemetry.lock() {
                *telemetry = WaylandEmbeddedCompositorTelemetry {
                    imported_frames: self.dmabuf_importer.imported_buffers,
                    committed_frames: self.committed_surfaces,
                    frame_callbacks_sent: self.frame_callbacks_sent,
                    pending_frame_callbacks: self.pending_frame_callbacks,
                    latest_frame,
                    output_composition: self.host_output.status(),
                };
            }
        }

        fn compose_pending_frame(&mut self) {
            if !self.pending_frame_delivery {
                return;
            }
            let Some(frame) = self.latest_frame.as_ref() else {
                self.pending_frame_delivery = false;
                self.record_telemetry();
                return;
            };
            if let Err(error) = self.host_output.compose(frame, self.viewport) {
                tracing::warn!(
                    target: "editor",
                    error,
                    import_sequence = frame.import_sequence,
                    commit_sequence = frame.commit_sequence,
                    "Wayland embedded compositor failed to composite latest DMA-BUF frame into host output"
                );
                self.record_telemetry();
                return;
            }
            let time = self.start_time.elapsed();
            let mut delivered = 0;
            self.toplevel_surfaces.retain(ToplevelSurface::alive);
            for surface in &self.toplevel_surfaces {
                delivered += send_frame_callbacks_surface_tree(surface.wl_surface(), time);
            }
            self.pending_frame_delivery = false;
            self.frame_callbacks_sent = self.frame_callbacks_sent.saturating_add(delivered);
            self.pending_frame_callbacks = self.pending_frame_callbacks.saturating_sub(delivered);
            self.record_telemetry();
            tracing::trace!(
                target: "editor",
                delivered,
                total_frame_callbacks = self.frame_callbacks_sent,
                composed_frames = self.host_output.composed_frames(),
                "Wayland embedded compositor composited committed DMA-BUF frame and delivered frame callbacks"
            );
        }
    }

    fn send_frame_callbacks_surface_tree(surface: &WlSurface, time: Duration) -> u64 {
        let mut delivered = 0_u64;
        with_surface_tree_downward(
            surface,
            (),
            |_, _, &()| TraversalAction::DoChildren(()),
            |_, states, &()| {
                let mut current = states.cached_state.get::<SurfaceAttributes>();
                let callbacks = std::mem::take(&mut current.current().frame_callbacks);
                delivered = delivered.saturating_add(callbacks.len() as u64);
                for callback in callbacks {
                    callback.done(time.as_millis() as u32);
                }
            },
            |_, _, &()| true,
        );
        delivered
    }

    struct DmabufImporter {
        renderer: GlesRenderer,
        formats: Vec<Format>,
        imported_buffers: u64,
    }

    struct EmbeddedHostOutput {
        renderer: GlesRenderer,
        target: EmbeddedHostOutputTarget,
        composed_frames: u64,
        last_error: Option<String>,
    }

    enum EmbeddedHostOutputTarget {
        Pending,
        Offscreen(GlesHostOffscreenTarget),
        Wayland(GlesHostWaylandTarget),
    }

    struct GlesHostOffscreenTarget {
        renderbuffer: smithay::backend::renderer::gles::GlesRenderbuffer,
        size: Size<i32, smithay::utils::Buffer>,
    }

    struct GlesHostWaylandTarget {
        egl_surface: EGLSurface,
        size: Size<i32, smithay::utils::Buffer>,
    }

    #[derive(Clone, Copy, Debug)]
    struct RawWaylandEglDisplay {
        display: usize,
    }

    impl EGLNativeDisplay for RawWaylandEglDisplay {
        fn supported_platforms(&self) -> Vec<EGLPlatform<'_>> {
            vec![
                egl_platform!(
                    PLATFORM_WAYLAND_KHR,
                    self.display,
                    &["EGL_KHR_platform_wayland"]
                ),
                egl_platform!(
                    PLATFORM_WAYLAND_EXT,
                    self.display,
                    &["EGL_EXT_platform_wayland"]
                ),
            ]
        }

        fn identifier(&self) -> Option<String> {
            Some("Aster/GTK/Wayland".to_owned())
        }
    }

    #[derive(Clone, Debug)]
    struct ImportedDmabufFrame {
        dmabuf: Dmabuf,
        texture: GlesTexture,
        size: Size<i32, smithay::utils::Buffer>,
        format: Format,
        sequence: u64,
    }

    #[derive(Clone, Debug)]
    struct CommittedDmabufFrame {
        buffer_id: smithay::reexports::wayland_server::backend::ObjectId,
        dmabuf: Dmabuf,
        texture: GlesTexture,
        size: Size<i32, smithay::utils::Buffer>,
        format: Format,
        import_sequence: u64,
        commit_sequence: u64,
    }

    impl DmabufImporter {
        fn new() -> Result<Self, String> {
            // SAFETY: The embedded compositor owns this surfaceless EGL display/context
            // and creates/uses the renderer on the compositor thread only.
            let display = unsafe { EGLDisplay::new(EGLSurfacelessDisplay) }
                .map_err(|error| format!("create surfaceless EGL display: {error}"))?;
            let context = EGLContext::new(&display)
                .map_err(|error| format!("create surfaceless EGL context: {error}"))?;
            // SAFETY: The context was created above and has not been made current on
            // any other thread. The renderer remains thread-local in this runtime.
            let renderer = unsafe { GlesRenderer::new(context) }
                .map_err(|error| format!("create GLES DMA-BUF renderer: {error}"))?;
            let formats = renderer
                .dmabuf_formats()
                .iter()
                .copied()
                .collect::<Vec<_>>();
            if formats.is_empty() {
                return Err("EGL/GLES renderer reported no DMA-BUF texture formats".to_owned());
            }

            tracing::info!(
                target: "editor",
                format_count = formats.len(),
                "Wayland embedded compositor initialized EGL/GLES DMA-BUF importer"
            );

            Ok(Self {
                renderer,
                formats,
                imported_buffers: 0,
            })
        }

        fn formats(&self) -> Vec<Format> {
            self.formats.clone()
        }

        fn import(&mut self, dmabuf: &Dmabuf) -> Result<ImportedDmabufFrame, String> {
            let texture = self
                .renderer
                .import_dmabuf(dmabuf, None)
                .map_err(|error| format!("{error}"))?;
            self.imported_buffers = self.imported_buffers.saturating_add(1);
            Ok(ImportedDmabufFrame {
                dmabuf: dmabuf.clone(),
                texture,
                size: dmabuf.size(),
                format: dmabuf.format(),
                sequence: self.imported_buffers,
            })
        }
    }

    impl EmbeddedHostOutput {
        fn new(viewport: Option<WaylandEmbeddedViewport>) -> Result<Self, String> {
            // SAFETY: The host output renderer is owned by the compositor runtime
            // thread and is never made current from any other thread.
            let display = unsafe { EGLDisplay::new(EGLSurfacelessDisplay) }
                .map_err(|error| format!("create host output EGL display: {error}"))?;
            let context = EGLContext::new(&display)
                .map_err(|error| format!("create host output EGL context: {error}"))?;
            // SAFETY: The EGL context above is thread-local to this compositor
            // runtime and has not been used by any other renderer.
            let mut renderer = unsafe { GlesRenderer::new(context) }
                .map_err(|error| format!("create host output GLES renderer: {error}"))?;
            renderer
                .downscale_filter(TextureFilter::Linear)
                .map_err(|error| format!("configure host output downscale filter: {error}"))?;
            renderer
                .upscale_filter(TextureFilter::Linear)
                .map_err(|error| format!("configure host output upscale filter: {error}"))?;

            let target = match viewport {
                Some(viewport) => Self::create_offscreen_target(&mut renderer, viewport)
                    .map(EmbeddedHostOutputTarget::Offscreen)?,
                None => EmbeddedHostOutputTarget::Pending,
            };

            Ok(Self {
                renderer,
                target,
                composed_frames: 0,
                last_error: None,
            })
        }

        fn set_viewport(&mut self, viewport: WaylandEmbeddedViewport) {
            let needs_resize = match &self.target {
                EmbeddedHostOutputTarget::Pending => true,
                EmbeddedHostOutputTarget::Offscreen(target) => {
                    target.size.w != viewport.width as i32
                        || target.size.h != viewport.height as i32
                }
                EmbeddedHostOutputTarget::Wayland(target) => {
                    target.size.w != viewport.width as i32
                        || target.size.h != viewport.height as i32
                }
            };
            if !needs_resize {
                return;
            }

            if let EmbeddedHostOutputTarget::Wayland(target) = &mut self.target {
                let width = viewport.width.max(1).min(i32::MAX as u32) as i32;
                let height = viewport.height.max(1).min(i32::MAX as u32) as i32;
                target.egl_surface.resize(width, height, 0, 0);
                target.size = (width, height).into();
                self.last_error = None;
                return;
            }

            match Self::create_offscreen_target(&mut self.renderer, viewport) {
                Ok(target) => {
                    self.target = EmbeddedHostOutputTarget::Offscreen(target);
                    self.last_error = None;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "editor",
                        error,
                        ?viewport,
                        "Wayland embedded compositor failed to resize host output target"
                    );
                    self.target = EmbeddedHostOutputTarget::Pending;
                    self.last_error =
                        Some("host output render target allocation failed.".to_owned());
                }
            }
        }

        fn set_target(&mut self, target: WaylandEmbeddedHostOutputTarget) {
            match Self::create_wayland_target(target) {
                Ok((renderer, target)) => {
                    let size = target.size;
                    self.renderer = renderer;
                    self.target = EmbeddedHostOutputTarget::Wayland(target);
                    self.last_error = None;
                    tracing::info!(
                        target: "editor",
                        width = size.w,
                        height = size.h,
                        "Wayland embedded compositor attached host Wayland EGL output surface"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        target: "editor",
                        error = %error,
                        "Wayland embedded compositor failed to attach host output surface"
                    );
                    self.last_error = Some(format!(
                        "host output Wayland surface attachment failed: {error}"
                    ));
                    self.set_viewport(target.viewport);
                }
            }
        }

        fn compose(
            &mut self,
            frame: &CommittedDmabufFrame,
            viewport: Option<WaylandEmbeddedViewport>,
        ) -> Result<(), &'static str> {
            if let Some(viewport) = viewport {
                self.set_viewport(viewport);
            }

            let output_size: Size<i32, Physical> = match &self.target {
                EmbeddedHostOutputTarget::Pending => {
                    let error = "host output render target is waiting for a Scene View viewport.";
                    self.last_error = Some(error.to_owned());
                    return Err(error);
                }
                EmbeddedHostOutputTarget::Offscreen(target) => {
                    (target.size.w, target.size.h).into()
                }
                EmbeddedHostOutputTarget::Wayland(target) => (target.size.w, target.size.h).into(),
            };
            let damage = [Rectangle::from_size(output_size)];
            let host_texture = match self.renderer.import_dmabuf(&frame.dmabuf, None) {
                Ok(texture) => texture,
                Err(error) => {
                    tracing::warn!(
                        target: "editor",
                        error = %error,
                        import_sequence = frame.import_sequence,
                        commit_sequence = frame.commit_sequence,
                        "Wayland embedded compositor host renderer failed to import committed DMA-BUF"
                    );
                    let error = "host output DMA-BUF texture import failed.";
                    self.last_error = Some(error.to_owned());
                    return Err(error);
                }
            };
            let src = Rectangle::from_size(host_texture.size()).to_f64();
            let dst = Rectangle::from_size(output_size);

            let render_result = match &mut self.target {
                EmbeddedHostOutputTarget::Pending => Err("host output render target disappeared."),
                EmbeddedHostOutputTarget::Offscreen(target) => render_host_frame(
                    &mut self.renderer,
                    &mut target.renderbuffer,
                    output_size,
                    src,
                    dst,
                    &damage,
                    &host_texture,
                    "host output render target bind failed.",
                ),
                EmbeddedHostOutputTarget::Wayland(target) => {
                    let result = render_host_frame(
                        &mut self.renderer,
                        &mut target.egl_surface,
                        output_size,
                        src,
                        dst,
                        &damage,
                        &host_texture,
                        "host output Wayland surface bind failed.",
                    );
                    if result.is_ok() {
                        target
                            .egl_surface
                            .swap_buffers(None)
                            .map_err(|_| "host output Wayland surface swap failed.")?;
                    }
                    result
                }
            };

            match render_result {
                Ok(()) => {
                    self.composed_frames = self.composed_frames.saturating_add(1);
                    self.last_error = None;
                    if self.composed_frames <= 3 || self.composed_frames % 60 == 0 {
                        tracing::info!(
                            target: "editor",
                            composed_frames = self.composed_frames,
                            target = self.target.kind(),
                            output_width = output_size.w,
                            output_height = output_size.h,
                            texture_width = host_texture.width(),
                            texture_height = host_texture.height(),
                            import_sequence = frame.import_sequence,
                            commit_sequence = frame.commit_sequence,
                            "Wayland embedded compositor rendered DMA-BUF frame into host output"
                        );
                    }
                    Ok(())
                }
                Err(error) => {
                    self.last_error = Some(error.to_owned());
                    Err(error)
                }
            }
        }

        fn status(&self) -> WaylandEmbeddedOutputCompositionStatus {
            let attached = matches!(
                self.target,
                EmbeddedHostOutputTarget::Offscreen(_) | EmbeddedHostOutputTarget::Wayland(_)
            );
            WaylandEmbeddedOutputCompositionStatus {
                attached,
                composed_frames: self.composed_frames,
                reason: self.status_reason(),
            }
        }

        fn composed_frames(&self) -> u64 {
            self.composed_frames
        }

        fn status_reason(&self) -> String {
            if let Some(error) = self.last_error.as_ref() {
                return error.clone();
            }
            match self.target {
                EmbeddedHostOutputTarget::Wayland(_) => {
                    "host Wayland EGL surface is attached; latest DMA-BUF texture is composited into the editor host output before frame callbacks are sent.".to_owned()
                }
                EmbeddedHostOutputTarget::Offscreen(_) => {
                    "offscreen GLES render target is attached; latest DMA-BUF texture is composited before frame callbacks are sent.".to_owned()
                }
                EmbeddedHostOutputTarget::Pending => {
                    "host output render target is waiting for a Scene View viewport.".to_owned()
                }
            }
        }

        fn create_offscreen_target(
            renderer: &mut GlesRenderer,
            viewport: WaylandEmbeddedViewport,
        ) -> Result<GlesHostOffscreenTarget, String> {
            let size: Size<i32, smithay::utils::Buffer> = (
                viewport.width.max(1).min(i32::MAX as u32) as i32,
                viewport.height.max(1).min(i32::MAX as u32) as i32,
            )
                .into();
            let renderbuffer = renderer
                .create_buffer(Fourcc::Abgr8888, size)
                .map_err(|error| format!("create host output renderbuffer: {error}"))?;
            Ok(GlesHostOffscreenTarget { renderbuffer, size })
        }

        fn create_wayland_target(
            target: WaylandEmbeddedHostOutputTarget,
        ) -> Result<(GlesRenderer, GlesHostWaylandTarget), String> {
            let scene_window::SceneRawSurface::Wayland { display, surface } = target.surface else {
                return Err(
                    "embedded compositor host output only supports Wayland surfaces".to_owned(),
                );
            };
            if display == 0 || surface == 0 {
                return Err("host Wayland display or surface handle is null".to_owned());
            }
            let width = target.viewport.width.max(1).min(i32::MAX as u32) as i32;
            let height = target.viewport.height.max(1).min(i32::MAX as u32) as i32;
            let display = unsafe { EGLDisplay::new(RawWaylandEglDisplay { display }) }
                .map_err(|error| format!("create host Wayland EGL display: {error}"))?;
            let gl_attributes = GlAttributes {
                version: (3, 0),
                profile: None,
                debug: false,
                vsync: false,
            };
            let context = EGLContext::new_with_config(
                &display,
                gl_attributes,
                PixelFormatRequirements::_10_bit(),
            )
            .or_else(|_| {
                EGLContext::new_with_config(
                    &display,
                    gl_attributes,
                    PixelFormatRequirements::_8_bit(),
                )
            })
            .map_err(|error| format!("create host Wayland EGL context: {error}"))?;
            let wl_egl_surface =
                unsafe { WlEglSurface::new_from_raw(surface as *mut _, width, height) }
                    .map_err(|error| format!("create wl_egl_window: {error}"))?;
            let egl_surface = unsafe {
                EGLSurface::new(
                    &display,
                    context
                        .pixel_format()
                        .ok_or_else(|| "host Wayland EGL context has no pixel format".to_owned())?,
                    context.config_id(),
                    wl_egl_surface,
                )
            }
            .map_err(|error| format!("create host Wayland EGL surface: {error}"))?;
            let mut renderer = unsafe { GlesRenderer::new(context) }
                .map_err(|error| format!("create host Wayland GLES renderer: {error}"))?;
            renderer
                .downscale_filter(TextureFilter::Linear)
                .map_err(|error| format!("configure host Wayland downscale filter: {error}"))?;
            renderer
                .upscale_filter(TextureFilter::Linear)
                .map_err(|error| format!("configure host Wayland upscale filter: {error}"))?;
            Ok((
                renderer,
                GlesHostWaylandTarget {
                    egl_surface,
                    size: (width, height).into(),
                },
            ))
        }
    }

    impl EmbeddedHostOutputTarget {
        fn kind(&self) -> &'static str {
            match self {
                EmbeddedHostOutputTarget::Pending => "pending",
                EmbeddedHostOutputTarget::Offscreen(_) => "offscreen",
                EmbeddedHostOutputTarget::Wayland(_) => "wayland",
            }
        }
    }

    fn render_host_frame<'target, Target>(
        renderer: &mut GlesRenderer,
        target: &'target mut Target,
        output_size: Size<i32, Physical>,
        src: Rectangle<f64, smithay::utils::Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        texture: &GlesTexture,
        bind_error: &'static str,
    ) -> Result<(), &'static str>
    where
        GlesRenderer: Bind<Target>,
    {
        let mut framebuffer = renderer.bind(target).map_err(|_| bind_error)?;
        let mut render_frame = renderer
            .render(&mut framebuffer, output_size, Transform::Normal)
            .map_err(|_| "host output render begin failed.")?;
        render_frame
            .clear(Color32F::TRANSPARENT, damage)
            .map_err(|_| "host output render clear failed.")?;
        Frame::render_texture_from_to(
            &mut render_frame,
            texture,
            src,
            dst,
            damage,
            &[],
            if texture.is_y_inverted() {
                Transform::Flipped180
            } else {
                Transform::Normal
            },
            1.0,
        )
        .map_err(|_| "host output texture blit failed.")?;
        let _sync = render_frame
            .finish()
            .map_err(|_| "host output render finish failed.")?;
        Ok(())
    }

    fn configure_toplevel_surface(surface: &ToplevelSurface, viewport: WaylandEmbeddedViewport) {
        surface.with_pending_state(|state| {
            state.size = Some(Size::from((
                viewport.width.min(i32::MAX as u32) as i32,
                viewport.height.min(i32::MAX as u32) as i32,
            )));
        });
    }

    #[derive(Debug)]
    struct EmbeddedClientData {
        compositor_state: CompositorClientState,
    }

    impl ClientData for EmbeddedClientData {
        fn initialized(&self, client_id: ClientId) {
            tracing::debug!(
                target: "editor",
                ?client_id,
                "Wayland embedded compositor client initialized"
            );
        }

        fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
            tracing::debug!(
                target: "editor",
                ?client_id,
                ?reason,
                "Wayland embedded compositor client disconnected"
            );
        }
    }

    impl EmbeddedClientData {
        fn new() -> Self {
            Self {
                compositor_state: CompositorClientState::default(),
            }
        }
    }

    impl CompositorHandler for EmbeddedCompositorState {
        fn compositor_state(&mut self) -> &mut CompositorState {
            &mut self.compositor_state
        }

        fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
            &client
                .get_data::<EmbeddedClientData>()
                .expect("embedded Wayland client data is installed")
                .compositor_state
        }

        fn commit(&mut self, surface: &WlSurface) {
            on_commit_buffer_handler::<Self>(surface);
            self.committed_surfaces = self.committed_surfaces.saturating_add(1);
            let buffer = with_states(surface, |states| {
                let mut guard = states.cached_state.get::<SurfaceAttributes>();
                match guard.current().buffer.as_ref() {
                    Some(BufferAssignment::NewBuffer(buffer)) => Some(buffer.clone()),
                    _ => None,
                }
            });
            if let Some(buffer) = buffer {
                let buffer_id = buffer.id();
                if let Some(frame) = self.imported_buffers.get(&buffer_id).cloned() {
                    self.latest_frame = Some(CommittedDmabufFrame {
                        buffer_id,
                        dmabuf: frame.dmabuf.clone(),
                        texture: frame.texture.clone(),
                        size: frame.size,
                        format: frame.format,
                        import_sequence: frame.sequence,
                        commit_sequence: self.committed_surfaces,
                    });
                    self.pending_frame_delivery = true;
                    self.pending_frame_callbacks = self.pending_frame_callbacks.saturating_add(1);
                    if let Some(latest) = self.latest_frame.as_ref() {
                        tracing::trace!(
                            target: "editor",
                            ?surface,
                            texture = latest.texture.tex_id(),
                            width = latest.size.w,
                            height = latest.size.h,
                            format = ?latest.format,
                            import_sequence = latest.import_sequence,
                            commit_sequence = latest.commit_sequence,
                            y_inverted = latest.texture.is_y_inverted(),
                            "Wayland embedded compositor latest DMA-BUF frame is ready for GPU composition"
                        );
                    }
                    tracing::trace!(
                        target: "editor",
                        ?surface,
                        texture = frame.texture.tex_id(),
                        width = frame.texture.width(),
                        height = frame.texture.height(),
                        y_inverted = frame.texture.is_y_inverted(),
                        import_sequence = frame.sequence,
                        commit_sequence = self.committed_surfaces,
                        "Wayland embedded compositor selected committed DMA-BUF frame for GPU composition"
                    );
                } else if get_dmabuf(&buffer).is_ok() {
                    tracing::warn!(
                        target: "editor",
                        ?surface,
                        ?buffer,
                        "Wayland embedded compositor saw DMA-BUF commit without imported texture cache entry"
                    );
                }
            }
            self.record_telemetry();
            tracing::trace!(
                target: "editor",
                ?surface,
                commits = self.committed_surfaces,
                ?self.viewport,
                "Wayland embedded compositor surface committed"
            );
        }
    }

    impl BufferHandler for EmbeddedCompositorState {
        fn buffer_destroyed(&mut self, buffer: &WlBuffer) {
            let buffer_id = buffer.id();
            self.imported_buffers.remove(&buffer_id);
            if self
                .latest_frame
                .as_ref()
                .is_some_and(|frame| frame.buffer_id == buffer_id)
            {
                self.latest_frame = None;
            }
            tracing::trace!(
                target: "editor",
                ?buffer,
                "Wayland embedded compositor buffer destroyed"
            );
        }
    }

    impl ShmHandler for EmbeddedCompositorState {
        fn shm_state(&self) -> &ShmState {
            &self.shm_state
        }
    }

    impl OutputHandler for EmbeddedCompositorState {}

    impl DmabufHandler for EmbeddedCompositorState {
        fn dmabuf_state(&mut self) -> &mut DmabufState {
            &mut self.dmabuf_state
        }

        fn dmabuf_imported(
            &mut self,
            _global: &DmabufGlobal,
            dmabuf: Dmabuf,
            notifier: ImportNotifier,
        ) {
            let size = dmabuf.size();
            let format = dmabuf.format();
            match self.dmabuf_importer.import(&dmabuf) {
                Ok(frame) => match notifier.successful::<EmbeddedCompositorState>() {
                    Ok(buffer) => {
                        let buffer_id = buffer.id();
                        self.imported_buffers.insert(buffer_id, frame.clone());
                        tracing::trace!(
                            target: "editor",
                            ?buffer,
                            ?size,
                            ?format,
                            texture = frame.texture.tex_id(),
                            width = frame.texture.width(),
                            height = frame.texture.height(),
                            y_inverted = frame.texture.is_y_inverted(),
                            imported_buffers = self.dmabuf_importer.imported_buffers,
                            "Wayland embedded compositor imported DMA-BUF"
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "editor",
                            "Wayland embedded compositor imported DMA-BUF but failed to create wl_buffer: {error}"
                        );
                    }
                },
                Err(error) => {
                    tracing::warn!(
                        target: "editor",
                        ?size,
                        ?format,
                        "Wayland embedded compositor rejected DMA-BUF import: {error}"
                    );
                    notifier.failed();
                }
            }
        }
    }

    impl SeatHandler for EmbeddedCompositorState {
        type KeyboardFocus = WlSurface;
        type PointerFocus = WlSurface;
        type TouchFocus = WlSurface;

        fn seat_state(&mut self) -> &mut SeatState<Self> {
            &mut self.seat_state
        }

        fn focus_changed(&mut self, _seat: &Seat<Self>, focused: Option<&WlSurface>) {
            tracing::trace!(
                target: "editor",
                ?focused,
                "Wayland embedded compositor seat focus changed"
            );
        }

        fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {
            tracing::trace!(
                target: "editor",
                "Wayland embedded compositor client requested cursor image"
            );
        }
    }

    impl XdgShellHandler for EmbeddedCompositorState {
        fn xdg_shell_state(&mut self) -> &mut XdgShellState {
            &mut self.xdg_shell_state
        }

        fn new_toplevel(&mut self, surface: ToplevelSurface) {
            if let Some(viewport) = self.viewport {
                configure_toplevel_surface(&surface, viewport);
            }
            let serial = surface.send_configure();
            self.toplevel_surfaces.push(surface);
            tracing::debug!(
                target: "editor",
                ?serial,
                ?self.viewport,
                "Wayland embedded compositor xdg toplevel configured"
            );
        }

        fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
            match surface.send_configure() {
                Ok(serial) => tracing::debug!(
                    target: "editor",
                    ?serial,
                    "Wayland embedded compositor xdg popup configured"
                ),
                Err(error) => tracing::warn!(
                    target: "editor",
                    "failed to configure Wayland embedded compositor xdg popup: {error}"
                ),
            }
        }

        fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, serial: Serial) {
            tracing::trace!(
                target: "editor",
                ?serial,
                "Wayland embedded compositor xdg popup grab requested"
            );
        }

        fn reposition_request(
            &mut self,
            surface: PopupSurface,
            _positioner: PositionerState,
            token: u32,
        ) {
            let serial = surface.send_repositioned(token);
            tracing::trace!(
                target: "editor",
                ?serial,
                token,
                "Wayland embedded compositor xdg popup repositioned"
            );
        }

        fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
            self.toplevel_surfaces.retain(|known| known != &surface);
        }
    }

    delegate_compositor!(EmbeddedCompositorState);
    delegate_dmabuf!(EmbeddedCompositorState);
    delegate_output!(EmbeddedCompositorState);
    delegate_seat!(EmbeddedCompositorState);
    delegate_shm!(EmbeddedCompositorState);
    delegate_xdg_shell!(EmbeddedCompositorState);

    pub fn start_runtime(
        viewport: Option<WaylandEmbeddedViewport>,
    ) -> Result<WaylandEmbeddedCompositorHandle, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let telemetry = std::sync::Arc::new(std::sync::Mutex::new(
            WaylandEmbeddedCompositorTelemetry::default(),
        ));
        let runtime_telemetry = telemetry.clone();
        let thread = thread::Builder::new()
            .name("aster-wayland-embedded-compositor".to_owned())
            .spawn(move || run_runtime(viewport, command_rx, ready_tx, runtime_telemetry))
            .map_err(|error| format!("spawn Wayland embedded compositor: {error}"))?;

        let socket_name = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| {
                let _ = command_tx.send(RuntimeCommand::Stop);
                format!("Wayland embedded compositor did not become ready: {error}")
            })?
            .map_err(|error| {
                let _ = command_tx.send(RuntimeCommand::Stop);
                error
            })?;

        Ok(WaylandEmbeddedCompositorHandle {
            socket_name,
            command_tx,
            telemetry,
            thread: Some(thread),
        })
    }

    pub fn dmabuf_import_support() -> Result<usize, String> {
        static SUPPORT: OnceLock<Result<usize, String>> = OnceLock::new();

        SUPPORT
            .get_or_init(|| DmabufImporter::new().map(|importer| importer.formats.len()))
            .clone()
    }

    fn run_runtime(
        viewport: Option<WaylandEmbeddedViewport>,
        command_rx: std::sync::mpsc::Receiver<RuntimeCommand>,
        ready_tx: std::sync::mpsc::Sender<Result<String, String>>,
        telemetry: std::sync::Arc<std::sync::Mutex<WaylandEmbeddedCompositorTelemetry>>,
    ) {
        let mut display = match Display::<EmbeddedCompositorState>::new() {
            Ok(display) => display,
            Err(error) => {
                let _ = ready_tx.send(Err(format!("create Wayland display: {error}")));
                return;
            }
        };
        let socket = match ListeningSocket::bind_auto("aster-editor", 0..128) {
            Ok(socket) => socket,
            Err(error) => {
                let _ = ready_tx.send(Err(format!("bind Wayland socket: {error}")));
                return;
            }
        };
        let Some(socket_name) = socket
            .socket_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            let _ = ready_tx.send(Err("Wayland socket name is unavailable".to_owned()));
            return;
        };
        let display_handle = display.handle();
        let compositor_state = CompositorState::new::<EmbeddedCompositorState>(&display_handle);
        let shm_state = ShmState::new::<EmbeddedCompositorState>(
            &display_handle,
            [wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888],
        );
        let dmabuf_importer = match DmabufImporter::new() {
            Ok(importer) => importer,
            Err(error) => {
                let _ = ready_tx.send(Err(format!("initialize DMA-BUF importer: {error}")));
                return;
            }
        };
        let host_output = match EmbeddedHostOutput::new(viewport) {
            Ok(target) => target,
            Err(error) => {
                let _ = ready_tx.send(Err(format!("initialize host output target: {error}")));
                return;
            }
        };
        let mut dmabuf_state = DmabufState::new();
        let dmabuf_global = dmabuf_state
            .create_global::<EmbeddedCompositorState>(&display_handle, dmabuf_importer.formats());
        let mut seat_state = SeatState::new();
        let _ = seat_state.new_wl_seat(&display_handle, "aster-editor");
        let xdg_shell_state = XdgShellState::new::<EmbeddedCompositorState>(&display_handle);
        let output = create_embedded_output(viewport);
        let _output_global = output.create_global::<EmbeddedCompositorState>(&display_handle);
        let client_data: Arc<dyn ClientData> = Arc::new(EmbeddedClientData::new());
        let _ = ready_tx.send(Ok(socket_name.clone()));
        tracing::info!(
            target: "editor",
            socket = socket_name,
            ?viewport,
            "Wayland embedded compositor listening with DMA-BUF import"
        );
        let mut state = EmbeddedCompositorState {
            viewport,
            compositor_state,
            shm_state,
            dmabuf_state,
            _dmabuf_global: dmabuf_global,
            dmabuf_importer,
            seat_state,
            xdg_shell_state,
            toplevel_surfaces: Vec::new(),
            imported_buffers: HashMap::new(),
            latest_frame: None,
            host_output,
            committed_surfaces: 0,
            frame_callbacks_sent: 0,
            pending_frame_callbacks: 0,
            pending_frame_delivery: false,
            telemetry,
            start_time: Instant::now(),
        };
        state.record_telemetry();

        loop {
            let mut should_stop = false;
            loop {
                match command_rx.try_recv() {
                    Ok(RuntimeCommand::SetViewport(viewport)) => state.set_viewport(viewport),
                    Ok(RuntimeCommand::SetHostOutputTarget(target)) => {
                        state.set_host_output_target(target)
                    }
                    Ok(RuntimeCommand::Stop) => {
                        should_stop = true;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        should_stop = true;
                        break;
                    }
                }
            }
            if should_stop {
                break;
            }

            loop {
                match socket.accept() {
                    Ok(Some(stream)) => {
                        let mut handle = display.handle();
                        if let Err(error) = handle.insert_client(stream, client_data.clone()) {
                            tracing::warn!(
                                target: "editor",
                                "failed to insert Wayland embedded compositor client: {error}"
                            );
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(
                            target: "editor",
                            "failed to accept Wayland embedded compositor client: {error}"
                        );
                        break;
                    }
                }
            }

            if let Err(error) = display.dispatch_clients(&mut state) {
                tracing::warn!(
                    target: "editor",
                    "Wayland embedded compositor dispatch failed: {error}"
                );
            }
            if let Err(error) = display.flush_clients() {
                tracing::warn!(
                    target: "editor",
                    "Wayland embedded compositor flush failed: {error}"
                );
            }
            state.compose_pending_frame();
            if let Err(error) = display.flush_clients() {
                tracing::warn!(
                    target: "editor",
                    "Wayland embedded compositor post-frame flush failed: {error}"
                );
            }

            thread::sleep(Duration::from_millis(1));
        }

        tracing::info!(
            target: "editor",
            "Wayland embedded compositor stopped"
        );
    }

    fn create_embedded_output(viewport: Option<WaylandEmbeddedViewport>) -> Output {
        let size = viewport
            .map(|viewport| (viewport.width.max(1) as i32, viewport.height.max(1) as i32))
            .unwrap_or((640, 480));
        let output = Output::new(
            "aster-editor-embedded".to_owned(),
            PhysicalProperties {
                size: (size.0.max(1), size.1.max(1)).into(),
                subpixel: Subpixel::Unknown,
                make: "Aster".to_owned(),
                model: "Embedded Scene View".to_owned(),
            },
        );
        let mode = Mode {
            size: size.into(),
            refresh: 60_000,
        };
        output.change_current_state(
            Some(mode),
            Some(smithay::utils::Transform::Normal),
            Some(Scale::Integer(1)),
            Some((0, 0).into()),
        );
        output.set_preferred(mode);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "wayland-embedded-compositor")]
    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    impl EnvVarGuard {
        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match self.previous.as_ref() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn support_reports_not_wayland_before_backend_state() {
        let support = support_for_runtime(false);

        assert_eq!(support.status, WaylandEmbeddedCompositorStatus::NotWayland);
        assert!(!support.available);
    }

    #[test]
    fn default_build_exposes_feature_disabled_on_wayland() {
        #[cfg(not(feature = "wayland-embedded-compositor"))]
        {
            let support = support_for_runtime(true);
            assert_eq!(
                support.status,
                WaylandEmbeddedCompositorStatus::FeatureDisabled
            );
            assert!(!support.available);
        }
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    #[test]
    fn feature_build_keeps_incomplete_wayland_backend_off_by_default() {
        let _guard = EnvVarGuard::unset("ASTER_WAYLAND_EMBEDDED_EXPERIMENTAL");

        let support = support_for_runtime(true);

        assert_eq!(support.status, WaylandEmbeddedCompositorStatus::Incomplete);
        assert!(!support.available);
    }

    #[test]
    fn viewport_sanitizes_dom_rects() {
        let viewport = WaylandEmbeddedViewport::from_scene_rect(scene_window::SceneViewportRect {
            x: -4,
            y: 8,
            width: 0,
            height: 0,
        });

        assert_eq!(
            viewport,
            WaylandEmbeddedViewport {
                x: 0,
                y: 8,
                width: 1,
                height: 1,
            }
        );
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    #[test]
    fn feature_backend_starts_runtime_socket() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            eprintln!("skipping Wayland embedded compositor socket test: XDG_RUNTIME_DIR is unset");
            return;
        }
        if let Err(error) = backend::dmabuf_import_support() {
            eprintln!(
                "skipping Wayland embedded compositor socket test: DMA-BUF import unavailable: {error}"
            );
            return;
        }

        let handle = backend::start_runtime(Some(WaylandEmbeddedViewport {
            x: 0,
            y: 0,
            width: 640,
            height: 360,
        }))
        .expect("embedded compositor runtime starts");

        assert!(!handle.socket_name().is_empty());
    }
}
