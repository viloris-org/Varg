//! Native editor Scene View with direct GPU surface presentation.

use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use engine_core::math::{Transform, Vec3};
use engine_core::{EngineConfig, EntityId};
use engine_render::{
    PresentStrategy, RenderCamera, RenderDevice, RenderFrame, RenderPerformanceConfig,
    RenderProjection,
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
    asset_root: PathBuf,
    scene_json: String,
}

impl SceneRuntimeSnapshot {
    pub fn new(
        config: EngineConfig,
        project_root: PathBuf,
        asset_root: PathBuf,
        scene_json: String,
    ) -> Self {
        Self {
            config,
            project_root,
            asset_root,
            scene_json,
        }
    }

    fn into_runtime(
        self,
        renderer: WgpuRenderDevice,
    ) -> engine_core::EngineResult<RuntimeServices<WgpuRenderDevice>> {
        let scene = engine_ecs::Scene::from_json(&self.scene_json)?;
        let mut runtime = RuntimeServices::with_renderer(self.config, renderer);
        runtime.set_project_root(self.project_root);
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
            runtime_min::runtime_scaling_settings_from_env(),
            runtime_min::runtime_scaling_context(),
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
                self.request_temporal_accumulation();
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
            runtime_min::runtime_scaling_settings_from_env(),
            runtime_min::runtime_scaling_context(),
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
    fn floating_scene_surface_keeps_editor_vsync_policy() {
        let config = scene_surface_performance_config(&SceneWindowMode::Floating);

        assert_eq!(config.present_strategy, PresentStrategy::VSync);
        assert_eq!(config.maximum_frame_latency, 2);
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
                PathBuf::from("assets"),
                scene.to_json("test").unwrap(),
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
