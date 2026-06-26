//! Native editor Scene View with direct GPU surface presentation.

use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use engine_core::math::{Transform, Vec3};
use engine_core::{EngineConfig, EntityId};
use engine_render::{
    AntiAliasingMode, GuiDrawCmd, GuiDrawList, GuiTextureId, GuiVertex, PresentStrategy,
    RenderCamera, RenderDevice, RenderFrame, RenderPerformanceConfig, RenderProjection,
    RenderQualityMode, RenderScalingContext, RenderScalingSettings, UpscalerKind,
};
use engine_render_wgpu::WgpuRenderDevice;
use runtime_min::RuntimeServices;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

const EDITOR_TARGET_FRAME_DT: Duration = Duration::from_micros(13_333);
const TEMPORAL_ACCUMULATION_FRAMES: u8 = 16;
const NATIVE_ORIENTATION_GIZMO_SIZE: f32 = 80.0;
const NATIVE_ORIENTATION_GIZMO_MARGIN: f32 = 8.0;

#[derive(Clone, Copy, Debug)]
pub struct SceneCameraState {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for SceneCameraState {
    fn default() -> Self {
        Self {
            yaw: -0.5,
            pitch: 0.3,
            distance: 6.0,
            target: Vec3::new(0.0, 1.0, 0.0),
        }
    }
}

pub enum SceneCommand {
    Restart(SceneRuntimeSnapshot, SceneCameraState),
    Show,
    Hide,
    SetCamera(SceneCameraState),
    SetViewport(SceneViewportRect),
    Shutdown,
}

#[derive(Clone, Copy, Debug)]
pub struct SceneViewportRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl SceneViewportRect {
    pub fn sanitized(self) -> Self {
        Self {
            x: self.x,
            y: self.y,
            width: self.width.max(1),
            height: self.height.max(1),
        }
    }
}

#[derive(Clone, Debug)]
pub enum SceneWindowMode {
    Floating,
    WaylandEmbedded {
        socket_name: String,
        viewport: SceneViewportRect,
    },
    CompositorRaw {
        surface: SceneRawSurface,
        surface_width: u32,
        surface_height: u32,
        viewport: SceneViewportRect,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum SceneRawSurface {
    Xlib {
        display: usize,
        window: u64,
    },
    Wayland {
        display: usize,
        surface: usize,
    },
    Win32 {
        hwnd: isize,
        hinstance: Option<isize>,
    },
    AppKit {
        ns_view: usize,
    },
    UiKit {
        ui_view: usize,
        ui_view_controller: Option<usize>,
    },
    AndroidNdk {
        a_native_window: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneWindowKind {
    Floating,
    Embedded,
}

pub struct SceneRuntimeSnapshot {
    config: EngineConfig,
    project_root: PathBuf,
    script_roots: Vec<PathBuf>,
    asset_root: PathBuf,
    scene_file: engine_ecs::SceneFile,
}

impl SceneRuntimeSnapshot {
    pub fn new(
        config: EngineConfig,
        project_root: PathBuf,
        script_roots: Vec<PathBuf>,
        asset_root: PathBuf,
        scene_file: engine_ecs::SceneFile,
    ) -> Self {
        Self {
            config,
            project_root,
            script_roots,
            asset_root,
            scene_file,
        }
    }

    fn into_runtime(
        self,
        renderer: WgpuRenderDevice,
    ) -> engine_core::EngineResult<RuntimeServices<WgpuRenderDevice>> {
        let scene = engine_ecs::Scene::from_scene_file(self.scene_file)?;
        let mut runtime = RuntimeServices::with_renderer(self.config, renderer);
        runtime.set_project_root(self.project_root);
        runtime.set_script_roots(self.script_roots);
        runtime.load_project_assets(self.asset_root)?;
        runtime.scene = scene;
        runtime.render_world = runtime_min::extract_render_world(&runtime.scene);
        Ok(runtime)
    }
}

pub enum SceneEvent {
    Closed,
    Error(String),
}

pub struct SceneWindowHandle {
    cmd_tx: mpsc::Sender<SceneCommand>,
    event_rx: mpsc::Receiver<SceneEvent>,
    thread: Option<thread::JoinHandle<()>>,
    kind: SceneWindowKind,
}

impl SceneWindowHandle {
    pub fn kind(&self) -> SceneWindowKind {
        self.kind
    }

    pub fn restart(
        &self,
        snapshot: SceneRuntimeSnapshot,
        camera: SceneCameraState,
    ) -> Result<(), String> {
        self.cmd_tx
            .send(SceneCommand::Restart(snapshot, camera))
            .map_err(|_| "scene window thread is not running".to_owned())
    }

    pub fn show(&self) -> Result<(), String> {
        self.cmd_tx
            .send(SceneCommand::Show)
            .map_err(|_| "scene window thread is not running".to_owned())
    }

    pub fn hide(&self) -> Result<(), String> {
        self.cmd_tx
            .send(SceneCommand::Hide)
            .map_err(|_| "scene window thread is not running".to_owned())
    }

    pub fn set_viewport(&self, viewport: SceneViewportRect) -> Result<(), String> {
        self.cmd_tx
            .send(SceneCommand::SetViewport(viewport.sanitized()))
            .map_err(|_| "scene window thread is not running".to_owned())
    }

    pub fn set_camera(&self, camera: SceneCameraState) -> Result<(), String> {
        self.cmd_tx
            .send(SceneCommand::SetCamera(camera))
            .map_err(|_| "scene window thread is not running".to_owned())
    }

    pub fn poll_events(&self) -> Vec<SceneEvent> {
        self.event_rx.try_iter().collect()
    }

    #[cfg(test)]
    pub fn shutdown(mut self) {
        let _ = self.cmd_tx.send(SceneCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for SceneWindowHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(SceneCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn spawn_scene_window(
    title: String,
    width: u32,
    height: u32,
    snapshot: SceneRuntimeSnapshot,
    camera: SceneCameraState,
) -> SceneWindowHandle {
    spawn_scene_window_with_mode(
        title,
        width,
        height,
        snapshot,
        camera,
        SceneWindowMode::Floating,
    )
}

pub fn spawn_scene_window_with_mode(
    title: String,
    width: u32,
    height: u32,
    snapshot: SceneRuntimeSnapshot,
    camera: SceneCameraState,
    mode: SceneWindowMode,
) -> SceneWindowHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<SceneCommand>();
    let (event_tx, event_rx) = mpsc::channel::<SceneEvent>();
    let kind = match &mode {
        SceneWindowMode::Floating => SceneWindowKind::Floating,
        SceneWindowMode::WaylandEmbedded { .. } | SceneWindowMode::CompositorRaw { .. } => {
            SceneWindowKind::Embedded
        }
    };
    let thread = thread::spawn(move || match mode {
        SceneWindowMode::CompositorRaw {
            surface,
            surface_width,
            surface_height,
            viewport,
        } => {
            run_raw_scene_surface(
                surface,
                SceneViewportRect {
                    x: 0,
                    y: 0,
                    width: surface_width,
                    height: surface_height,
                },
                Some(raw_surface_local_viewport(viewport)),
                snapshot,
                camera,
                cmd_rx,
                event_tx,
            );
        }
        SceneWindowMode::Floating | SceneWindowMode::WaylandEmbedded { .. } => {
            run_scene_window(
                title, width, height, snapshot, camera, mode, cmd_rx, event_tx,
            );
        }
    });
    SceneWindowHandle {
        cmd_tx,
        event_rx,
        thread: Some(thread),
        kind,
    }
}

fn run_scene_window(
    title: String,
    width: u32,
    height: u32,
    snapshot: SceneRuntimeSnapshot,
    camera: SceneCameraState,
    mode: SceneWindowMode,
    cmd_rx: mpsc::Receiver<SceneCommand>,
    event_tx: mpsc::Sender<SceneEvent>,
) {
    let mut builder = EventLoop::builder();
    configure_event_loop_builder_for_mode(&mut builder, &mode);

    let event_loop = match build_event_loop_for_mode(&mut builder, &mode) {
        Ok(event_loop) => event_loop,
        Err(error) => {
            let _ = event_tx.send(SceneEvent::Error(format!("event loop: {error}")));
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);

    let initial_viewport = match mode {
        SceneWindowMode::WaylandEmbedded { viewport, .. } => Some(viewport.sanitized()),
        _ => None,
    };

    let mut app = SceneApp {
        title,
        width,
        height,
        runtime: None,
        window: None,
        cmd_rx,
        event_tx,
        last_frame: Instant::now(),
        pending_snapshot: Some(snapshot),
        camera,
        mode,
        visible: true,
        dirty: true,
        temporal_frames_remaining: TEMPORAL_ACCUMULATION_FRAMES,
        render_frame_index: 0,
        dragging: None,
        last_cursor: None,
        initial_viewport,
    };
    let run_result = event_loop.run_app(&mut app);
    if let Err(error) = run_result {
        let _ = app
            .event_tx
            .send(SceneEvent::Error(format!("run: {error}")));
    }
}

fn configure_event_loop_builder_for_mode(
    builder: &mut winit::event_loop::EventLoopBuilder<()>,
    mode: &SceneWindowMode,
) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        match mode {
            SceneWindowMode::WaylandEmbedded { .. } => {
                use winit::platform::wayland::EventLoopBuilderExtWayland;
                EventLoopBuilderExtWayland::with_any_thread(builder, true);
                builder.with_wayland();
            }
            _ => {
                use winit::platform::x11::EventLoopBuilderExtX11;
                EventLoopBuilderExtX11::with_any_thread(builder, true);
            }
        }
    }
    let _ = mode;
}

fn build_event_loop_for_mode(
    builder: &mut winit::event_loop::EventLoopBuilder<()>,
    mode: &SceneWindowMode,
) -> Result<EventLoop<()>, winit::error::EventLoopError> {
    match mode {
        SceneWindowMode::WaylandEmbedded { socket_name, .. } => {
            with_wayland_display(socket_name, || builder.build())
        }
        _ => builder.build(),
    }
}

fn wayland_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_wayland_display<T>(socket_name: &str, f: impl FnOnce() -> T) -> T {
    let _guard = wayland_env_lock()
        .lock()
        .expect("Wayland environment lock poisoned");
    let previous_display = std::env::var_os("WAYLAND_DISPLAY");
    let previous_socket = std::env::var_os("WAYLAND_SOCKET");

    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", socket_name);
        std::env::remove_var("WAYLAND_SOCKET");
    }
    let result = f();

    unsafe {
        match previous_display {
            Some(value) => std::env::set_var("WAYLAND_DISPLAY", value),
            None => std::env::remove_var("WAYLAND_DISPLAY"),
        }
        match previous_socket {
            Some(value) => std::env::set_var("WAYLAND_SOCKET", value),
            None => std::env::remove_var("WAYLAND_SOCKET"),
        }
    }

    result
}

struct SceneApp {
    title: String,
    width: u32,
    height: u32,
    runtime: Option<RuntimeServices<WgpuRenderDevice>>,
    window: Option<Window>,
    cmd_rx: mpsc::Receiver<SceneCommand>,
    event_tx: mpsc::Sender<SceneEvent>,
    last_frame: Instant,
    pending_snapshot: Option<SceneRuntimeSnapshot>,
    camera: SceneCameraState,
    mode: SceneWindowMode,
    visible: bool,
    dirty: bool,
    temporal_frames_remaining: u8,
    render_frame_index: u64,
    dragging: Option<MouseButton>,
    last_cursor: Option<(f64, f64)>,
    initial_viewport: Option<SceneViewportRect>,
}

impl ApplicationHandler for SceneApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window_attrs = self.window_attributes();
        let window = match event_loop.create_window(window_attrs) {
            Ok(window) => window,
            Err(error) => {
                let _ = self
                    .event_tx
                    .send(SceneEvent::Error(format!("create window: {error}")));
                event_loop.exit();
                return;
            }
        };

        if let Some(viewport) = self.initial_viewport.take() {
            let _ = window.request_inner_size(LogicalSize::new(viewport.width, viewport.height));
        }
        self.window = Some(window);
        let Some(snapshot) = self.pending_snapshot.take() else {
            let _ = self.event_tx.send(SceneEvent::Error(
                "missing initial scene snapshot".to_owned(),
            ));
            event_loop.exit();
            return;
        };
        if let Err(error) = self.install_runtime(snapshot) {
            let _ = self.event_tx.send(SceneEvent::Error(error));
            event_loop.exit();
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.visible = false;
                if let Some(window) = self.window.as_ref() {
                    window.set_visible(false);
                }
                let _ = self.event_tx.send(SceneEvent::Closed);
            }
            WindowEvent::Resized(size) => {
                self.width = size.width.max(1);
                self.height = size.height.max(1);
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.renderer.resize_surface(self.width, self.height);
                }
                self.request_temporal_accumulation();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.dragging = if state == ElementState::Pressed {
                    Some(button)
                } else {
                    None
                };
                self.last_cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.update_drag(position.x, position.y);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y) * 32.0,
                    MouseScrollDelta::PixelDelta(pos) => pos.y,
                };
                self.camera.distance =
                    (self.camera.distance - scroll as f32 * 0.01).clamp(0.5, 100.0);
                self.request_temporal_accumulation();
            }
            WindowEvent::RedrawRequested => {
                self.render_frame();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let commands: Vec<_> = self.cmd_rx.try_iter().collect();
        for command in commands {
            match command {
                SceneCommand::Restart(snapshot, camera) => {
                    self.camera = camera;
                    if let Err(error) = self.install_runtime(snapshot) {
                        let _ = self.event_tx.send(SceneEvent::Error(error));
                    }
                }
                SceneCommand::Show => {
                    self.visible = true;
                    if let Some(window) = self.window.as_ref() {
                        window.set_visible(true);
                        if matches!(self.mode, SceneWindowMode::Floating) {
                            window.focus_window();
                        }
                        window.request_redraw();
                    }
                    self.request_temporal_accumulation();
                }
                SceneCommand::Hide => {
                    self.visible = false;
                    self.dragging = None;
                    self.last_cursor = None;
                    if let Some(window) = self.window.as_ref() {
                        window.set_visible(false);
                    }
                    let _ = self.event_tx.send(SceneEvent::Closed);
                }
                SceneCommand::SetCamera(camera) => {
                    self.camera = camera;
                    self.request_temporal_accumulation();
                }
                SceneCommand::SetViewport(viewport) => {
                    self.apply_viewport(viewport);
                }
                SceneCommand::Shutdown => {
                    event_loop.exit();
                    return;
                }
            }
        }

        if self.visible
            && (self.dirty || self.dragging.is_some() || self.temporal_frames_remaining > 0)
        {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + EDITOR_TARGET_FRAME_DT,
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {}

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        _event: DeviceEvent,
    ) {
    }
}

impl SceneApp {
    fn window_attributes(&self) -> WindowAttributes {
        let attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(self.width, self.height));
        match self.mode {
            SceneWindowMode::Floating => attrs,
            SceneWindowMode::WaylandEmbedded { .. } | SceneWindowMode::CompositorRaw { .. } => {
                attrs
            }
        }
    }

    fn apply_viewport(&mut self, viewport: SceneViewportRect) {
        let viewport = viewport.sanitized();
        self.width = viewport.width;
        self.height = viewport.height;
        if let Some(window) = self.window.as_ref() {
            if !matches!(self.mode, SceneWindowMode::WaylandEmbedded { .. }) {
                window.set_outer_position(LogicalPosition::new(viewport.x, viewport.y));
            }
            let _ = window.request_inner_size(LogicalSize::new(viewport.width, viewport.height));
            window.set_visible(true);
        }
        if let Some(runtime) = self.runtime.as_mut() {
            runtime
                .renderer
                .resize_surface(viewport.width, viewport.height);
        }
        self.request_temporal_accumulation();
    }

    fn install_runtime(&mut self, snapshot: SceneRuntimeSnapshot) -> Result<(), String> {
        self.runtime = None;
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| "scene window is not initialized".to_owned())?;
        let renderer = WgpuRenderDevice::new_with_performance(
            window,
            scene_surface_performance_config(&self.mode),
        )
        .map_err(|error| format!("wgpu device: {error}"))?;
        let mut runtime = snapshot
            .into_runtime(renderer)
            .map_err(|error| format!("runtime: {error}"))?;
        runtime.set_render_scaling(
            scene_view_render_scaling_settings(),
            RenderScalingContext::default(),
        );
        self.runtime = Some(runtime);
        self.last_frame = Instant::now();
        self.render_frame_index = 0;
        self.request_temporal_accumulation();
        Ok(())
    }

    fn update_drag(&mut self, x: f64, y: f64) {
        let Some(button) = self.dragging else {
            self.last_cursor = Some((x, y));
            return;
        };
        let Some((last_x, last_y)) = self.last_cursor else {
            self.last_cursor = Some((x, y));
            return;
        };
        let dx = x - last_x;
        let dy = y - last_y;
        match button {
            MouseButton::Right => {
                self.camera.yaw -= dx as f32 * 0.005;
                self.camera.pitch = (self.camera.pitch + dy as f32 * 0.005).clamp(-1.5, 1.5);
            }
            MouseButton::Middle => {
                let d = self.camera.distance * 0.002;
                let yaw = self.camera.yaw;
                self.camera.target.x += (-dx as f32 * yaw.cos() - dy as f32 * yaw.sin() * 0.5) * d;
                self.camera.target.y += dy as f32 * d * 0.5;
                self.camera.target.z += (dx as f32 * yaw.sin() - dy as f32 * yaw.cos() * 0.5) * d;
            }
            _ => {}
        }
        self.last_cursor = Some((x, y));
        self.request_temporal_accumulation();
    }

    fn request_temporal_accumulation(&mut self) {
        self.dirty = true;
        self.temporal_frames_remaining = TEMPORAL_ACCUMULATION_FRAMES;
    }

    fn render_frame(&mut self) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_frame);
        if dt < EDITOR_TARGET_FRAME_DT {
            return;
        }
        self.last_frame = now;

        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        let mut world = runtime_min::extract_render_world(&runtime.scene);
        let target = self.camera.target;
        let translation = Vec3::new(
            target.x + self.camera.distance * self.camera.pitch.cos() * self.camera.yaw.sin(),
            target.y + self.camera.distance * self.camera.pitch.sin(),
            target.z + self.camera.distance * self.camera.pitch.cos() * self.camera.yaw.cos(),
        );
        let object = world
            .camera
            .as_ref()
            .map(|camera| camera.object)
            .unwrap_or_else(|| EntityId::from_u128(0));
        world.camera = Some(RenderCamera {
            object,
            transform: Transform {
                translation,
                ..Transform::IDENTITY
            },
            projection: RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.01,
            far: 1000.0,
            look_at_target: Some(target),
        });

        let frame_index = self.render_frame_index;
        self.render_frame_index = self.render_frame_index.wrapping_add(1);
        runtime
            .renderer
            .set_next_surface_gui(native_orientation_gizmo_draw_list(
                self.width,
                self.height,
                self.camera,
            ));
        if let Err(error) = runtime
            .renderer
            .submit_render_world(&world, RenderFrame { frame_index })
        {
            tracing::error!(target: "scene", error = %error, "scene view render failed");
        }
        runtime.render_world = world;
        if self.temporal_frames_remaining > 0 {
            self.temporal_frames_remaining -= 1;
        }
        self.dirty = false;
    }
}

struct RawSceneApp {
    surface: SceneRawSurface,
    width: u32,
    height: u32,
    viewport: Option<SceneViewportRect>,
    runtime: Option<RuntimeServices<WgpuRenderDevice>>,
    cmd_rx: mpsc::Receiver<SceneCommand>,
    event_tx: mpsc::Sender<SceneEvent>,
    last_frame: Instant,
    pending_snapshot: Option<SceneRuntimeSnapshot>,
    camera: SceneCameraState,
    visible: bool,
    dirty: bool,
    temporal_frames_remaining: u8,
    render_frame_index: u64,
    shutdown: bool,
}

fn run_raw_scene_surface(
    surface: SceneRawSurface,
    surface_rect: SceneViewportRect,
    viewport: Option<SceneViewportRect>,
    snapshot: SceneRuntimeSnapshot,
    camera: SceneCameraState,
    cmd_rx: mpsc::Receiver<SceneCommand>,
    event_tx: mpsc::Sender<SceneEvent>,
) {
    let surface_rect = surface_rect.sanitized();
    let mut app = RawSceneApp {
        surface,
        width: surface_rect.width,
        height: surface_rect.height,
        viewport: viewport.map(SceneViewportRect::sanitized),
        runtime: None,
        cmd_rx,
        event_tx,
        last_frame: Instant::now(),
        pending_snapshot: Some(snapshot),
        camera,
        visible: true,
        dirty: true,
        temporal_frames_remaining: TEMPORAL_ACCUMULATION_FRAMES,
        render_frame_index: 0,
        shutdown: false,
    };
    if let Some(snapshot) = app.pending_snapshot.take() {
        if let Err(error) = app.install_runtime(snapshot) {
            let _ = app.event_tx.send(SceneEvent::Error(error));
            return;
        }
    }

    loop {
        app.process_pending_commands();
        if app.shutdown {
            break;
        }
        if !app.visible {
            match app.cmd_rx.recv() {
                Ok(SceneCommand::Shutdown) | Err(_) => break,
                Ok(command) => {
                    app.process_command(command);
                    if app.shutdown {
                        break;
                    }
                }
            }
            continue;
        }
        if app.dirty || app.temporal_frames_remaining > 0 {
            app.render_frame();
        }
        match app.cmd_rx.recv_timeout(EDITOR_TARGET_FRAME_DT) {
            Ok(SceneCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(command) => app.process_command(command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if app.shutdown {
            break;
        }
    }
}

impl RawSceneApp {
    fn process_pending_commands(&mut self) {
        let commands: Vec<_> = self.cmd_rx.try_iter().collect();
        for command in commands {
            self.process_command(command);
        }
    }

    fn process_command(&mut self, command: SceneCommand) {
        match command {
            SceneCommand::Restart(snapshot, camera) => {
                self.camera = camera;
                if let Err(error) = self.install_runtime(snapshot) {
                    let _ = self.event_tx.send(SceneEvent::Error(error));
                }
            }
            SceneCommand::Show => {
                self.visible = true;
                self.restore_surface_after_show();
            }
            SceneCommand::Hide => {
                self.visible = false;
                let _ = self.event_tx.send(SceneEvent::Closed);
            }
            SceneCommand::SetCamera(camera) => {
                self.camera = camera;
                self.request_temporal_accumulation();
            }
            SceneCommand::SetViewport(viewport) => self.apply_viewport(viewport),
            SceneCommand::Shutdown => {
                self.shutdown = true;
            }
        }
    }

    fn apply_viewport(&mut self, viewport: SceneViewportRect) {
        let viewport = raw_surface_local_viewport(viewport);
        if let Some(runtime) = self.runtime.as_mut() {
            if self.viewport.is_some() {
                self.viewport = Some(viewport);
                self.width = viewport.width;
                self.height = viewport.height;
                runtime
                    .renderer
                    .resize_surface(viewport.width, viewport.height);
                runtime.renderer.set_surface_viewport(Some(
                    engine_render_wgpu::SurfaceViewportRect::new(
                        viewport.x.max(0) as u32,
                        viewport.y.max(0) as u32,
                        viewport.width,
                        viewport.height,
                    ),
                ));
            } else {
                self.width = viewport.width;
                self.height = viewport.height;
                runtime
                    .renderer
                    .resize_surface(viewport.width, viewport.height);
            }
        }
        self.request_temporal_accumulation();
    }

    fn restore_surface_after_show(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.renderer.resize_surface(self.width, self.height);
            if let Some(viewport) = self.viewport {
                runtime.renderer.set_surface_viewport(Some(
                    engine_render_wgpu::SurfaceViewportRect::new(
                        viewport.x.max(0) as u32,
                        viewport.y.max(0) as u32,
                        viewport.width,
                        viewport.height,
                    ),
                ));
            }
        }
        self.last_frame = Instant::now() - EDITOR_TARGET_FRAME_DT;
        self.request_temporal_accumulation();
    }

    fn install_runtime(&mut self, snapshot: SceneRuntimeSnapshot) -> Result<(), String> {
        self.runtime = None;
        let (raw_display, raw_window) = match self.surface {
            SceneRawSurface::Xlib { display, window } => {
                let display = NonNull::new(display as *mut std::ffi::c_void)
                    .ok_or_else(|| "Xlib display handle is null".to_owned())?;
                (
                    Some(raw_window_handle::RawDisplayHandle::Xlib(
                        raw_window_handle::XlibDisplayHandle::new(Some(display), 0),
                    )),
                    raw_window_handle::RawWindowHandle::Xlib(
                        raw_window_handle::XlibWindowHandle::new(window),
                    ),
                )
            }
            SceneRawSurface::Wayland { display, surface } => {
                let display = NonNull::new(display as *mut std::ffi::c_void)
                    .ok_or_else(|| "Wayland display handle is null".to_owned())?;
                let surface = NonNull::new(surface as *mut std::ffi::c_void)
                    .ok_or_else(|| "Wayland surface handle is null".to_owned())?;
                (
                    Some(raw_window_handle::RawDisplayHandle::Wayland(
                        raw_window_handle::WaylandDisplayHandle::new(display),
                    )),
                    raw_window_handle::RawWindowHandle::Wayland(
                        raw_window_handle::WaylandWindowHandle::new(surface),
                    ),
                )
            }
            SceneRawSurface::Win32 { hwnd, hinstance } => {
                let hwnd = std::num::NonZeroIsize::new(hwnd)
                    .ok_or_else(|| "Win32 HWND handle is null".to_owned())?;
                let mut handle = raw_window_handle::Win32WindowHandle::new(hwnd);
                handle.hinstance = hinstance.and_then(std::num::NonZeroIsize::new);
                (
                    Some(raw_window_handle::RawDisplayHandle::Windows(
                        raw_window_handle::WindowsDisplayHandle::new(),
                    )),
                    raw_window_handle::RawWindowHandle::Win32(handle),
                )
            }
            SceneRawSurface::AppKit { ns_view } => {
                let ns_view = NonNull::new(ns_view as *mut std::ffi::c_void)
                    .ok_or_else(|| "AppKit NSView handle is null".to_owned())?;
                (
                    Some(raw_window_handle::RawDisplayHandle::AppKit(
                        raw_window_handle::AppKitDisplayHandle::new(),
                    )),
                    raw_window_handle::RawWindowHandle::AppKit(
                        raw_window_handle::AppKitWindowHandle::new(ns_view),
                    ),
                )
            }
            SceneRawSurface::UiKit {
                ui_view,
                ui_view_controller,
            } => {
                let ui_view = NonNull::new(ui_view as *mut std::ffi::c_void)
                    .ok_or_else(|| "UIKit UIView handle is null".to_owned())?;
                let mut handle = raw_window_handle::UiKitWindowHandle::new(ui_view);
                handle.ui_view_controller =
                    ui_view_controller.and_then(|ptr| NonNull::new(ptr as *mut std::ffi::c_void));
                (
                    Some(raw_window_handle::RawDisplayHandle::UiKit(
                        raw_window_handle::UiKitDisplayHandle::new(),
                    )),
                    raw_window_handle::RawWindowHandle::UiKit(handle),
                )
            }
            SceneRawSurface::AndroidNdk { a_native_window } => {
                let a_native_window = NonNull::new(a_native_window as *mut std::ffi::c_void)
                    .ok_or_else(|| "Android native window handle is null".to_owned())?;
                (
                    Some(raw_window_handle::RawDisplayHandle::Android(
                        raw_window_handle::AndroidDisplayHandle::new(),
                    )),
                    raw_window_handle::RawWindowHandle::AndroidNdk(
                        raw_window_handle::AndroidNdkWindowHandle::new(a_native_window),
                    ),
                )
            }
        };
        let mut renderer = unsafe {
            WgpuRenderDevice::new_raw_surface_with_performance(
                raw_display,
                raw_window,
                self.width,
                self.height,
                scene_surface_performance_config(&SceneWindowMode::CompositorRaw {
                    surface: self.surface,
                    surface_width: self.width,
                    surface_height: self.height,
                    viewport: self.viewport.unwrap_or(SceneViewportRect {
                        x: 0,
                        y: 0,
                        width: self.width,
                        height: self.height,
                    }),
                }),
            )
        }
        .map_err(|error| format!("wgpu device: {error}"))?;
        if let Some(viewport) = self.viewport {
            renderer.set_surface_viewport(Some(engine_render_wgpu::SurfaceViewportRect::new(
                viewport.x.max(0) as u32,
                viewport.y.max(0) as u32,
                viewport.width,
                viewport.height,
            )));
        }
        let mut runtime = snapshot
            .into_runtime(renderer)
            .map_err(|error| format!("runtime: {error}"))?;
        runtime.set_render_scaling(
            scene_view_render_scaling_settings(),
            RenderScalingContext::default(),
        );
        self.runtime = Some(runtime);
        self.last_frame = Instant::now();
        self.render_frame_index = 0;
        self.request_temporal_accumulation();
        Ok(())
    }

    fn request_temporal_accumulation(&mut self) {
        self.dirty = true;
        self.temporal_frames_remaining = TEMPORAL_ACCUMULATION_FRAMES;
    }

    fn render_frame(&mut self) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_frame);
        if dt < EDITOR_TARGET_FRAME_DT {
            return;
        }
        self.last_frame = now;

        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        let mut world = runtime_min::extract_render_world(&runtime.scene);
        let target = self.camera.target;
        let translation = Vec3::new(
            target.x + self.camera.distance * self.camera.pitch.cos() * self.camera.yaw.sin(),
            target.y + self.camera.distance * self.camera.pitch.sin(),
            target.z + self.camera.distance * self.camera.pitch.cos() * self.camera.yaw.cos(),
        );
        let object = world
            .camera
            .as_ref()
            .map(|camera| camera.object)
            .unwrap_or_else(|| EntityId::from_u128(0));
        world.camera = Some(RenderCamera {
            object,
            transform: Transform {
                translation,
                ..Transform::IDENTITY
            },
            projection: RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.01,
            far: 1000.0,
            look_at_target: Some(target),
        });
        let frame_index = self.render_frame_index;
        self.render_frame_index = self.render_frame_index.wrapping_add(1);
        runtime
            .renderer
            .set_next_surface_gui(native_orientation_gizmo_draw_list(
                self.width,
                self.height,
                self.camera,
            ));
        if let Err(error) = runtime
            .renderer
            .submit_render_world(&world, RenderFrame { frame_index })
        {
            let _ = self
                .event_tx
                .send(SceneEvent::Error(format!("render: {error}")));
        }
        runtime.render_world = world;
        if self.temporal_frames_remaining > 0 {
            self.temporal_frames_remaining -= 1;
        }
        self.dirty = false;
    }
}

fn raw_surface_local_viewport(viewport: SceneViewportRect) -> SceneViewportRect {
    let viewport = viewport.sanitized();
    SceneViewportRect {
        x: 0,
        y: 0,
        width: viewport.width,
        height: viewport.height,
    }
}

fn scene_surface_performance_config(mode: &SceneWindowMode) -> RenderPerformanceConfig {
    let mut config = RenderPerformanceConfig::editor_1080p75();
    if matches!(
        mode,
        SceneWindowMode::WaylandEmbedded { .. } | SceneWindowMode::CompositorRaw { .. }
    ) {
        config.present_strategy = PresentStrategy::LowLatency;
        config.maximum_frame_latency = 1;
    }
    config
}

fn scene_view_render_scaling_settings() -> RenderScalingSettings {
    RenderScalingSettings {
        quality: RenderQualityMode::Native,
        preferred_upscaler: Some(UpscalerKind::Native),
        dynamic_resolution: false,
        min_render_scale: 1.0,
        max_render_scale: 1.0,
        anti_aliasing: AntiAliasingMode::Off,
        ..RenderScalingSettings::default()
    }
}

fn native_orientation_gizmo_draw_list(
    width: u32,
    height: u32,
    camera: SceneCameraState,
) -> GuiDrawList {
    if width == 0 || height == 0 {
        return GuiDrawList::default();
    }
    let size = NATIVE_ORIENTATION_GIZMO_SIZE
        .min(width as f32)
        .min(height as f32);
    if size < 24.0 {
        return GuiDrawList::default();
    }
    let origin = [
        width as f32 - NATIVE_ORIENTATION_GIZMO_MARGIN - size * 0.5,
        NATIVE_ORIENTATION_GIZMO_MARGIN + size * 0.5,
    ];
    let radius = size * 0.34;
    let mut builder = NativeGizmoDrawBuilder::new(width, height);

    let mut axes = native_orientation_axes(camera);
    axes.sort_by(|left, right| left.depth.total_cmp(&right.depth));
    for axis in axes {
        let end = [origin[0] + axis.x * radius, origin[1] + axis.y * radius];
        let facing = axis.depth < 0.0;
        let alpha = if facing { 255 } else { 143 };
        let line_color = css_rgba(axis.color[0], axis.color[1], axis.color[2], alpha);
        let shadow_end = [
            origin[0] + axis.x * radius + 1.0,
            origin[1] + axis.y * radius + 1.0,
        ];
        let dot_radius = if facing { size * 0.09 } else { size * 0.07 };
        builder.line(
            [origin[0] + 0.9, origin[1] + 0.9],
            shadow_end,
            if facing { size * 0.085 } else { size * 0.06 },
            css_rgba(0, 0, 0, if facing { 92 } else { 58 }),
        );
        builder.line(
            origin,
            end,
            if facing { size * 0.08 } else { size * 0.055 },
            line_color,
        );
        builder.circle(
            [end[0] + 1.0, end[1] + 1.0],
            dot_radius + 0.85,
            css_rgba(0, 0, 0, if facing { 112 } else { 72 }),
            20,
        );
        builder.circle(end, dot_radius, line_color, 18);
        builder.axis_label(axis.label, end, dot_radius * 0.58, axis.negative, facing);
    }
    builder.circle(
        [origin[0] + 0.8, origin[1] + 0.8],
        size * 0.08,
        css_rgba(0, 0, 0, 105),
        18,
    );
    builder.circle(origin, size * 0.075, css_rgba(85, 85, 85, 232), 16);
    builder.ring(
        origin,
        size * 0.075,
        size * 0.01,
        css_rgba(136, 136, 136, 220),
        16,
    );
    builder.circle(
        [origin[0] - size * 0.018, origin[1] - size * 0.018],
        size * 0.022,
        css_rgba(255, 255, 255, 38),
        12,
    );
    builder.finish()
}

#[derive(Clone, Copy)]
struct NativeOrientationAxis {
    label: char,
    color: [u8; 3],
    negative: bool,
    x: f32,
    y: f32,
    depth: f32,
}

fn native_orientation_axes(camera: SceneCameraState) -> Vec<NativeOrientationAxis> {
    let forward = Vec3::new(
        -camera.pitch.cos() * camera.yaw.sin(),
        -camera.pitch.sin(),
        -camera.pitch.cos() * camera.yaw.cos(),
    )
    .normalized();
    let preferred_up = Vec3::new(0.0, 1.0, 0.0);
    let fallback_up = if forward.y.abs() > 0.99 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        preferred_up
    };
    let up_seed = if forward.cross(preferred_up).length_squared() > 1e-8 {
        preferred_up
    } else {
        fallback_up
    };
    let right = forward.cross(up_seed).normalized();
    let up = right.cross(forward).normalized();
    let axes = [
        ('X', [255, 77, 77], false, Vec3::new(1.0, 0.0, 0.0)),
        ('X', [185, 28, 28], true, Vec3::new(-1.0, 0.0, 0.0)),
        ('Y', [74, 222, 92], false, Vec3::new(0.0, 1.0, 0.0)),
        ('Y', [21, 128, 61], true, Vec3::new(0.0, -1.0, 0.0)),
        ('Z', [77, 141, 255], false, Vec3::new(0.0, 0.0, 1.0)),
        ('Z', [29, 78, 216], true, Vec3::new(0.0, 0.0, -1.0)),
    ];
    axes.into_iter()
        .map(|(label, color, negative, dir)| NativeOrientationAxis {
            label,
            color,
            negative,
            x: dir.dot(right),
            y: -dir.dot(up),
            depth: dir.dot(forward),
        })
        .collect()
}

struct NativeGizmoDrawBuilder {
    width: u32,
    height: u32,
    vertices: Vec<GuiVertex>,
    indices: Vec<u32>,
}

impl NativeGizmoDrawBuilder {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn finish(self) -> GuiDrawList {
        let index_count = self.indices.len() as u32;
        GuiDrawList {
            vertices: self.vertices,
            indices: self.indices,
            commands: vec![GuiDrawCmd {
                texture: GuiTextureId(0),
                scissor: [0, 0, self.width, self.height],
                index_offset: 0,
                index_count,
            }],
        }
    }

    fn push_quad(&mut self, points: [[f32; 2]; 4], color: u32) {
        let base = self.vertices.len() as u32;
        for point in points {
            self.vertices.push(GuiVertex {
                pos: point,
                uv: [0.5, 0.5],
                color,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
    }

    fn rect(&mut self, min: [f32; 2], max: [f32; 2], color: u32) {
        self.push_quad(
            [
                [min[0], min[1]],
                [max[0], min[1]],
                [max[0], max[1]],
                [min[0], max[1]],
            ],
            color,
        );
    }

    fn line(&mut self, start: [f32; 2], end: [f32; 2], thickness: f32, color: u32) {
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.001 {
            return;
        }
        let nx = -dy / len * thickness * 0.5;
        let ny = dx / len * thickness * 0.5;
        self.push_quad(
            [
                [start[0] + nx, start[1] + ny],
                [end[0] + nx, end[1] + ny],
                [end[0] - nx, end[1] - ny],
                [start[0] - nx, start[1] - ny],
            ],
            color,
        );
    }

    fn circle(&mut self, center: [f32; 2], radius: f32, color: u32, segments: u32) {
        let base = self.vertices.len() as u32;
        self.vertices.push(GuiVertex {
            pos: center,
            uv: [0.5, 0.5],
            color,
        });
        for i in 0..segments {
            let angle = i as f32 / segments as f32 * std::f32::consts::TAU;
            self.vertices.push(GuiVertex {
                pos: [
                    center[0] + angle.cos() * radius,
                    center[1] + angle.sin() * radius,
                ],
                uv: [0.5, 0.5],
                color,
            });
        }
        for i in 0..segments {
            self.indices
                .extend_from_slice(&[base, base + 1 + i, base + 1 + ((i + 1) % segments)]);
        }
    }

    fn ring(&mut self, center: [f32; 2], radius: f32, thickness: f32, color: u32, segments: u32) {
        for i in 0..segments {
            let a0 = i as f32 / segments as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let outer0 = [center[0] + a0.cos() * radius, center[1] + a0.sin() * radius];
            let outer1 = [center[0] + a1.cos() * radius, center[1] + a1.sin() * radius];
            let inner_radius = (radius - thickness).max(0.0);
            let inner1 = [
                center[0] + a1.cos() * inner_radius,
                center[1] + a1.sin() * inner_radius,
            ];
            let inner0 = [
                center[0] + a0.cos() * inner_radius,
                center[1] + a0.sin() * inner_radius,
            ];
            self.push_quad([outer0, outer1, inner1, inner0], color);
        }
    }

    fn axis_label(
        &mut self,
        label: char,
        center: [f32; 2],
        scale: f32,
        negative: bool,
        facing: bool,
    ) {
        let color = css_rgba(255, 255, 255, if facing { 246 } else { 156 });
        let shadow = css_rgba(0, 0, 0, if facing { 72 } else { 42 });
        let pixel = scale * if negative { 0.245 } else { 0.265 };
        let gap = pixel * 0.22;
        let total_cols = if negative { 9.0 } else { 5.0 };
        let total_width = total_cols * pixel + (total_cols - 1.0) * gap;
        let total_height = 7.0 * pixel + 6.0 * gap;
        let origin = [
            center[0] - total_width * 0.5,
            center[1] - total_height * 0.5 + pixel * 0.08,
        ];

        self.axis_label_pixels(label, origin, pixel, gap, negative, shadow, [0.55, 0.65]);
        self.axis_label_pixels(label, origin, pixel, gap, negative, color, [0.0, 0.0]);
    }

    fn axis_label_pixels(
        &mut self,
        label: char,
        origin: [f32; 2],
        pixel: f32,
        gap: f32,
        negative: bool,
        color: u32,
        offset: [f32; 2],
    ) {
        let mut x_offset = 0.0;
        if negative {
            self.draw_pixel_glyph(&MINUS_GLYPH, origin, pixel, gap, color, offset);
            x_offset = 4.0 * (pixel + gap);
        }
        match label {
            'X' => self.draw_pixel_glyph(
                &X_GLYPH,
                [origin[0] + x_offset, origin[1]],
                pixel,
                gap,
                color,
                offset,
            ),
            'Y' => self.draw_pixel_glyph(
                &Y_GLYPH,
                [origin[0] + x_offset, origin[1]],
                pixel,
                gap,
                color,
                offset,
            ),
            'Z' => self.draw_pixel_glyph(
                &Z_GLYPH,
                [origin[0] + x_offset, origin[1]],
                pixel,
                gap,
                color,
                offset,
            ),
            _ => {}
        }
    }

    fn draw_pixel_glyph(
        &mut self,
        glyph: &[u8; 7],
        origin: [f32; 2],
        pixel: f32,
        gap: f32,
        color: u32,
        offset: [f32; 2],
    ) {
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                let x = origin[0] + offset[0] + col as f32 * (pixel + gap);
                let y = origin[1] + offset[1] + row as f32 * (pixel + gap);
                self.rect([x, y], [x + pixel, y + pixel], color);
            }
        }
    }
}

const MINUS_GLYPH: [u8; 7] = [
    0b00000, 0b00000, 0b00000, 0b11100, 0b00000, 0b00000, 0b00000,
];
const X_GLYPH: [u8; 7] = [
    0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b01010, 0b10001,
];
const Y_GLYPH: [u8; 7] = [
    0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
];
const Z_GLYPH: [u8; 7] = [
    0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
];

fn css_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    let alpha = f32::from(a) / 255.0;
    let r = (srgb_to_linear(r) * alpha * 255.0).round() as u8;
    let g = (srgb_to_linear(g) * alpha * 255.0).round() as u8;
    let b = (srgb_to_linear(b) * alpha * 255.0).round() as u8;
    rgba(r, g, b, a)
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16) | (u32::from(a) << 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    fn has_linux_display() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var_os("WAYLAND_SOCKET").is_some()
            || std::env::var_os("DISPLAY").is_some()
    }

    #[test]
    fn embedded_scene_surfaces_use_low_latency_policy() {
        let config = scene_surface_performance_config(&SceneWindowMode::WaylandEmbedded {
            socket_name: "varg-editor-test".to_owned(),
            viewport: SceneViewportRect {
                x: 0,
                y: 0,
                width: 320,
                height: 180,
            },
        });

        assert_eq!(config.present_strategy, PresentStrategy::LowLatency);
        assert_eq!(config.maximum_frame_latency, 1);

        let config = scene_surface_performance_config(&SceneWindowMode::CompositorRaw {
            surface: SceneRawSurface::Xlib {
                display: 1,
                window: 1,
            },
            surface_width: 320,
            surface_height: 180,
            viewport: SceneViewportRect {
                x: 0,
                y: 0,
                width: 320,
                height: 180,
            },
        });

        assert_eq!(config.present_strategy, PresentStrategy::LowLatency);
        assert_eq!(config.maximum_frame_latency, 1);
    }

    #[test]
    fn native_orientation_gizmo_builds_visible_overlay_geometry() {
        let draw_list = native_orientation_gizmo_draw_list(640, 360, SceneCameraState::default());

        assert!(!draw_list.vertices.is_empty());
        assert!(!draw_list.indices.is_empty());
        assert_eq!(draw_list.commands.len(), 1);
        assert_eq!(draw_list.commands[0].scissor, [0, 0, 640, 360]);
    }

    #[test]
    fn native_orientation_gizmo_tracks_camera_orientation() {
        let front = native_orientation_axes(SceneCameraState {
            yaw: 0.0,
            pitch: 0.0,
            ..SceneCameraState::default()
        });
        let top = native_orientation_axes(SceneCameraState {
            yaw: 0.0,
            pitch: 1.5,
            ..SceneCameraState::default()
        });
        let front_y = front.iter().find(|axis| axis.label == 'Y').unwrap();
        let top_y = top.iter().find(|axis| axis.label == 'Y').unwrap();

        assert_ne!(front_y.y, top_y.y);
    }

    #[test]
    fn native_orientation_gizmo_skips_tiny_viewports() {
        let draw_list = native_orientation_gizmo_draw_list(12, 12, SceneCameraState::default());

        assert!(draw_list.vertices.is_empty());
        assert!(draw_list.indices.is_empty());
        assert!(draw_list.commands.is_empty());
    }

    #[test]
    fn floating_scene_surface_keeps_editor_vsync_policy() {
        let config = scene_surface_performance_config(&SceneWindowMode::Floating);

        assert_eq!(config.present_strategy, PresentStrategy::VSync);
        assert_eq!(config.maximum_frame_latency, 2);
    }

    #[test]
    fn scene_view_scaling_prefers_crisp_native_preview() {
        let settings = scene_view_render_scaling_settings();

        assert_eq!(settings.quality, RenderQualityMode::Native);
        assert_eq!(settings.preferred_upscaler, Some(UpscalerKind::Native));
        assert!(!settings.dynamic_resolution);
        assert_eq!(settings.min_render_scale, 1.0);
        assert_eq!(settings.max_render_scale, 1.0);
        assert_eq!(settings.anti_aliasing, AntiAliasingMode::Off);
    }

    #[test]
    fn raw_surface_viewport_uses_surface_local_coordinates() {
        let viewport = raw_surface_local_viewport(SceneViewportRect {
            x: 320,
            y: 96,
            width: 1273,
            height: 752,
        });

        assert_eq!(viewport.x, 0);
        assert_eq!(viewport.y, 0);
        assert_eq!(viewport.width, 1273);
        assert_eq!(viewport.height, 752);
    }

    #[test]
    fn repeated_hide_show_reuses_the_scene_window_thread() {
        #[cfg(target_os = "linux")]
        if !has_linux_display() {
            eprintln!("skipping scene window test: no Linux display is available");
            return;
        }

        if !crate::claim_native_event_loop_test_slot(
            "scene_window::tests::repeated_hide_show_reuses_the_scene_window_thread",
        ) {
            return;
        }

        let scene = engine_ecs::Scene::default();
        let handle = spawn_scene_window(
            "Scene View Test".to_owned(),
            320,
            180,
            SceneRuntimeSnapshot::new(
                EngineConfig::default(),
                PathBuf::from("."),
                vec![PathBuf::from("scripts")],
                PathBuf::from("assets"),
                scene.to_scene_file("test").unwrap(),
            ),
            SceneCameraState::default(),
        );

        for _ in 0..20 {
            handle
                .hide()
                .expect("scene window thread should stay alive");
            handle
                .show()
                .expect("scene window thread should stay alive");
        }

        std::thread::sleep(Duration::from_millis(250));
        let errors = handle
            .poll_events()
            .into_iter()
            .filter_map(|event| match event {
                SceneEvent::Error(message) => Some(message),
                SceneEvent::Closed => None,
            })
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "scene window errors: {errors:?}");

        handle.shutdown();
    }
}
