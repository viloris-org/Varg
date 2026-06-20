//! Native editor Scene View with direct GPU surface presentation.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use engine_core::math::{Transform, Vec3};
use engine_core::{EngineConfig, EntityId};
use engine_render::{RenderCamera, RenderDevice, RenderFrame, RenderProjection};
use engine_render_wgpu::WgpuRenderDevice;
use runtime_min::RuntimeServices;
use winit::application::ApplicationHandler;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

const MIN_FRAME_DT: Duration = Duration::from_millis(16);

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
    Shutdown,
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
}

impl SceneWindowHandle {
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
    let (cmd_tx, cmd_rx) = mpsc::channel::<SceneCommand>();
    let (event_tx, event_rx) = mpsc::channel::<SceneEvent>();
    let thread = thread::spawn(move || {
        run_scene_window(title, width, height, snapshot, camera, cmd_rx, event_tx);
    });
    SceneWindowHandle {
        cmd_tx,
        event_rx,
        thread: Some(thread),
    }
}

fn run_scene_window(
    title: String,
    width: u32,
    height: u32,
    snapshot: SceneRuntimeSnapshot,
    camera: SceneCameraState,
    cmd_rx: mpsc::Receiver<SceneCommand>,
    event_tx: mpsc::Sender<SceneEvent>,
) {
    let mut builder = EventLoop::builder();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_any_thread(true);
    }
    let event_loop = match builder.build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            let _ = event_tx.send(SceneEvent::Error(format!("event loop: {error}")));
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);

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
        visible: true,
        dirty: true,
        dragging: None,
        last_cursor: None,
    };
    if let Err(error) = event_loop.run_app(&mut app) {
        let _ = app
            .event_tx
            .send(SceneEvent::Error(format!("run: {error}")));
    }
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
    visible: bool,
    dirty: bool,
    dragging: Option<MouseButton>,
    last_cursor: Option<(f64, f64)>,
}

impl ApplicationHandler for SceneApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            WindowAttributes::default()
                .with_title(&self.title)
                .with_inner_size(winit::dpi::LogicalSize::new(self.width, self.height)),
        ) {
            Ok(window) => window,
            Err(error) => {
                let _ = self
                    .event_tx
                    .send(SceneEvent::Error(format!("create window: {error}")));
                event_loop.exit();
                return;
            }
        };

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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.visible = false;
                let _ = self.event_tx.send(SceneEvent::Closed);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.width = size.width.max(1);
                self.height = size.height.max(1);
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.renderer.resize_surface(self.width, self.height);
                }
                self.dirty = true;
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
                self.dirty = true;
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
                        window.focus_window();
                    }
                    self.dirty = true;
                }
                SceneCommand::Shutdown => {
                    event_loop.exit();
                    return;
                }
            }
        }

        if self.visible && (self.dirty || self.dragging.is_some()) {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
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
    fn install_runtime(&mut self, snapshot: SceneRuntimeSnapshot) -> Result<(), String> {
        self.runtime = None;
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| "scene window is not initialized".to_owned())?;
        let renderer = WgpuRenderDevice::new_with_performance(
            window,
            engine_render::RenderPerformanceConfig::competitive_120hz(),
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
        self.dirty = true;
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
        self.dirty = true;
    }

    fn render_frame(&mut self) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_frame);
        if dt < MIN_FRAME_DT {
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

        let frame_index = runtime.frame_index();
        if let Err(error) = runtime
            .renderer
            .submit_render_world(&world, RenderFrame { frame_index })
        {
            tracing::error!(target: "scene", error = %error, "scene view render failed");
        }
        runtime.render_world = world;
        self.dirty = false;
    }
}
