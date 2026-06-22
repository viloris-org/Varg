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
use std::time::Duration;

use serde::Serialize;

use crate::scene_window;

pub const BACKEND_ID: &str = "wayland-embedded-compositor";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum WaylandEmbeddedCompositorStatus {
    Available,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WaylandEmbeddedCompositorRuntimeStatus {
    pub socket_name: Option<String>,
    pub viewport: Option<WaylandEmbeddedViewport>,
    pub dmabuf_available: bool,
    pub dmabuf_reason: &'static str,
}

impl WaylandEmbeddedCompositorRuntimeStatus {
    #[cfg(feature = "wayland-embedded-compositor")]
    fn stopped(viewport: Option<WaylandEmbeddedViewport>) -> Self {
        Self {
            socket_name: None,
            viewport,
            dmabuf_available: false,
            dmabuf_reason: "DMA-BUF global is stopped with the embedded compositor runtime.",
        }
    }

    #[cfg(feature = "wayland-embedded-compositor")]
    fn running(socket_name: String, viewport: Option<WaylandEmbeddedViewport>) -> Self {
        Self {
            socket_name: Some(socket_name),
            viewport,
            dmabuf_available: true,
            dmabuf_reason: "zwp_linux_dmabuf_v1 is backed by Smithay EGL/GLES DMA-BUF import.",
        }
    }
}

#[derive(Debug, Default)]
pub struct WaylandEmbeddedCompositor {
    viewport: Option<WaylandEmbeddedViewport>,
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

    #[cfg(feature = "wayland-embedded-compositor")]
    pub fn status(&self) -> WaylandEmbeddedCompositorRuntimeStatus {
        if let Some(handle) = self.handle.as_ref() {
            return WaylandEmbeddedCompositorRuntimeStatus::running(
                handle.socket_name().to_owned(),
                self.viewport,
            );
        }

        WaylandEmbeddedCompositorRuntimeStatus::stopped(self.viewport)
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

    use smithay::backend::allocator::{Buffer, Format, dmabuf::Dmabuf};
    use smithay::backend::egl::{EGLContext, EGLDisplay, native::EGLSurfacelessDisplay};
    use smithay::backend::renderer::{
        ImportDma, Texture,
        gles::{GlesRenderer, GlesTexture},
    };
    use smithay::delegate_compositor;
    use smithay::delegate_dmabuf;
    use smithay::delegate_seat;
    use smithay::delegate_shm;
    use smithay::delegate_xdg_shell;
    use smithay::input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus};
    use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
    use smithay::reexports::wayland_server::protocol::{
        wl_buffer::WlBuffer, wl_seat, wl_shm, wl_surface::WlSurface,
    };
    use smithay::reexports::wayland_server::{Client, Display, ListeningSocket, Resource};
    use smithay::utils::{Serial, Size};
    use smithay::wayland::buffer::BufferHandler;
    use smithay::wayland::compositor::{
        BufferAssignment, CompositorClientState, CompositorHandler, CompositorState,
        SurfaceAttributes, with_states,
    };
    use smithay::wayland::dmabuf::{
        DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier, get_dmabuf,
    };
    use smithay::wayland::shell::xdg::{
        PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    };
    use smithay::wayland::shm::{ShmHandler, ShmState};

    use super::*;

    #[derive(Debug)]
    pub enum RuntimeCommand {
        SetViewport(WaylandEmbeddedViewport),
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
        committed_surfaces: u64,
    }

    impl EmbeddedCompositorState {
        fn set_viewport(&mut self, viewport: WaylandEmbeddedViewport) {
            self.viewport = Some(viewport);
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
    }

    struct DmabufImporter {
        renderer: GlesRenderer,
        formats: Vec<Format>,
        imported_buffers: u64,
    }

    #[derive(Clone, Debug)]
    struct ImportedDmabufFrame {
        texture: GlesTexture,
        size: Size<i32, smithay::utils::Buffer>,
        format: Format,
        sequence: u64,
    }

    #[derive(Clone, Debug)]
    struct CommittedDmabufFrame {
        buffer_id: smithay::reexports::wayland_server::backend::ObjectId,
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
                texture,
                size: dmabuf.size(),
                format: dmabuf.format(),
                sequence: self.imported_buffers,
            })
        }
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
                        texture: frame.texture.clone(),
                        size: frame.size,
                        format: frame.format,
                        import_sequence: frame.sequence,
                        commit_sequence: self.committed_surfaces,
                    });
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
    delegate_seat!(EmbeddedCompositorState);
    delegate_shm!(EmbeddedCompositorState);
    delegate_xdg_shell!(EmbeddedCompositorState);

    pub fn start_runtime(
        viewport: Option<WaylandEmbeddedViewport>,
    ) -> Result<WaylandEmbeddedCompositorHandle, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let thread = thread::Builder::new()
            .name("aster-wayland-embedded-compositor".to_owned())
            .spawn(move || run_runtime(viewport, command_rx, ready_tx))
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
        let mut dmabuf_state = DmabufState::new();
        let dmabuf_global = dmabuf_state
            .create_global::<EmbeddedCompositorState>(&display_handle, dmabuf_importer.formats());
        let mut seat_state = SeatState::new();
        let _ = seat_state.new_wl_seat(&display_handle, "aster-editor");
        let xdg_shell_state = XdgShellState::new::<EmbeddedCompositorState>(&display_handle);
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
            committed_surfaces: 0,
        };

        loop {
            let mut should_stop = false;
            loop {
                match command_rx.try_recv() {
                    Ok(RuntimeCommand::SetViewport(viewport)) => state.set_viewport(viewport),
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

            thread::sleep(Duration::from_millis(8));
        }

        tracing::info!(
            target: "editor",
            "Wayland embedded compositor stopped"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
