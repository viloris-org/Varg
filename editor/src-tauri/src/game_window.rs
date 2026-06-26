//! Native game window with direct GPU surface presentation.
//!
//! Runs a winit event loop on a dedicated thread with a wgpu surface,
//! bypassing the readback → IPC → canvas pipeline for no-CPU-readback rendering.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use engine_core::EngineConfig;
use engine_render_wgpu::WgpuRenderDevice;
use runtime_min::RuntimeServices;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

/// Minimum frame interval for ~60fps cap.
const MIN_FRAME_DT: Duration = Duration::from_millis(16);

/// Commands sent from the editor to the game window thread.
pub enum GameCommand {
    /// Restart the game using a fresh edit-time scene snapshot.
    Restart(GameRuntimeSnapshot),
    /// Show the existing game window.
    Show,
    /// Applies player-facing render settings immediately.
    SetRenderScaling(engine_render::RenderScalingSettings),
    /// Shut down the game window.
    Shutdown,
}

/// Sendable project data used to construct the thread-affine runtime.
pub struct GameRuntimeSnapshot {
    config: EngineConfig,
    project_root: PathBuf,
    script_roots: Vec<PathBuf>,
    asset_root: PathBuf,
    scene_file: engine_ecs::SceneFile,
}

impl GameRuntimeSnapshot {
    /// Creates a game runtime snapshot from edit-time project data.
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

/// Events sent from the game window thread back to the editor.
pub enum GameEvent {
    /// Window was closed by the user.
    Closed,
    /// An error occurred.
    Error(String),
}

/// Handle to a running game window.
pub struct GameWindowHandle {
    cmd_tx: Option<EventLoopProxy<GameCommand>>,
    event_rx: mpsc::Receiver<GameEvent>,
    thread: Option<thread::JoinHandle<()>>,
}

impl GameWindowHandle {
    /// Restarts the game with a fresh runtime snapshot.
    pub fn restart(&self, snapshot: GameRuntimeSnapshot) -> Result<(), String> {
        self.cmd_tx
            .as_ref()
            .ok_or_else(|| "game window thread is not running".to_owned())?
            .send_event(GameCommand::Restart(snapshot))
            .map_err(|_| "game window thread is not running".to_owned())
    }

    /// Shows the game window without recreating its event loop.
    pub fn show(&self) -> Result<(), String> {
        self.cmd_tx
            .as_ref()
            .ok_or_else(|| "game window thread is not running".to_owned())?
            .send_event(GameCommand::Show)
            .map_err(|_| "game window thread is not running".to_owned())
    }

    /// Applies render quality settings to the running game.
    pub fn set_render_scaling(
        &self,
        settings: engine_render::RenderScalingSettings,
    ) -> Result<(), String> {
        self.cmd_tx
            .as_ref()
            .ok_or_else(|| "game window thread is not running".to_owned())?
            .send_event(GameCommand::SetRenderScaling(settings))
            .map_err(|_| "game window thread is not running".to_owned())
    }

    /// Polls for events from the game window (non-blocking).
    pub fn poll_events(&self) -> Vec<GameEvent> {
        self.event_rx.try_iter().collect()
    }

    /// Shuts down the game window and waits for the thread to finish.
    #[cfg(test)]
    pub fn shutdown(mut self) {
        if let Some(cmd_tx) = self.cmd_tx.as_ref() {
            let _ = cmd_tx.send_event(GameCommand::Shutdown);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for GameWindowHandle {
    fn drop(&mut self) {
        if let Some(cmd_tx) = self.cmd_tx.as_ref() {
            let _ = cmd_tx.send_event(GameCommand::Shutdown);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Spawns a native game window on a background thread.
///
/// Returns a handle for sending render worlds and receiving events.
/// The window uses wgpu surface direct presentation — no readback or IPC.
pub fn spawn_game_window(
    title: String,
    width: u32,
    height: u32,
    snapshot: GameRuntimeSnapshot,
) -> GameWindowHandle {
    let (event_tx, event_rx) = mpsc::channel::<GameEvent>();
    let (proxy_tx, proxy_rx) = mpsc::sync_channel(1);
    let thread_event_tx = event_tx.clone();

    let thread = thread::spawn(move || {
        run_game_window(title, width, height, snapshot, thread_event_tx, proxy_tx);
    });
    let cmd_tx = match proxy_rx.recv() {
        Ok(Ok(proxy)) => Some(proxy),
        Ok(Err(error)) => {
            let _ = event_tx.send(GameEvent::Error(error));
            None
        }
        Err(_) => None,
    };

    GameWindowHandle {
        cmd_tx,
        event_rx,
        thread: Some(thread),
    }
}

fn run_game_window(
    title: String,
    width: u32,
    height: u32,
    snapshot: GameRuntimeSnapshot,
    event_tx: mpsc::Sender<GameEvent>,
    proxy_tx: mpsc::SyncSender<Result<EventLoopProxy<GameCommand>, String>>,
) {
    let mut builder = EventLoop::<GameCommand>::with_user_event();
    // Allow event loop on non-main thread (required since Tauri owns the main thread)
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_any_thread(true);
    }
    let event_loop = match builder.build() {
        Ok(el) => el,
        Err(e) => {
            let _ = proxy_tx.send(Err(format!("event loop: {e}")));
            return;
        }
    };
    let proxy = event_loop.create_proxy();
    if proxy_tx.send(Ok(proxy)).is_err() {
        return;
    }
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = GameApp {
        title,
        width,
        height,
        runtime: None,
        window: None,
        event_tx,
        last_frame: Instant::now(),
        pending_snapshot: Some(snapshot),
        visible: true,
    };

    let run_result = event_loop.run_app(&mut app);
    if let Err(e) = run_result {
        let _ = app.event_tx.send(GameEvent::Error(format!("run: {e}")));
    }
}

struct GameApp {
    title: String,
    width: u32,
    height: u32,
    // The surface in `runtime.renderer` borrows the window with an extended
    // lifetime. Fields drop in declaration order, so runtime precedes window.
    runtime: Option<RuntimeServices<WgpuRenderDevice>>,
    window: Option<Window>,
    event_tx: mpsc::Sender<GameEvent>,
    last_frame: Instant,
    pending_snapshot: Option<GameRuntimeSnapshot>,
    visible: bool,
}

impl ApplicationHandler<GameCommand> for GameApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.create_window_and_install_runtime(event_loop) {
            let _ = self.event_tx.send(GameEvent::Error(error));
            event_loop.exit();
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if !matches!(&event, WindowEvent::CloseRequested) {
            if let Some(runtime) = self.runtime.as_mut() {
                runtime.process_winit_event(&event);
            }
        }
        match event {
            WindowEvent::CloseRequested => {
                self.close_window();
                let _ = self.event_tx.send(GameEvent::Closed);
            }
            WindowEvent::Resized(size) => {
                self.width = size.width.max(1);
                self.height = size.height.max(1);
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.renderer.resize_surface(self.width, self.height);
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_frame();
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: GameCommand) {
        self.handle_command(event_loop, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Request a redraw for continuous rendering
        if self.visible {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + MIN_FRAME_DT));
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

impl GameApp {
    fn handle_command(&mut self, event_loop: &ActiveEventLoop, cmd: GameCommand) {
        match cmd {
            GameCommand::Restart(snapshot) => {
                if self.window.is_some() {
                    if let Err(error) = self.install_runtime(snapshot) {
                        let _ = self.event_tx.send(GameEvent::Error(error));
                    }
                } else {
                    self.pending_snapshot = Some(snapshot);
                }
            }
            GameCommand::Show => {
                self.visible = true;
                if self.window.is_none() {
                    if let Err(error) = self.create_window_and_install_runtime(event_loop) {
                        let _ = self.event_tx.send(GameEvent::Error(error));
                    }
                }
                if let Some(window) = self.window.as_ref() {
                    window.set_visible(true);
                    window.focus_window();
                    window.request_redraw();
                }
            }
            GameCommand::SetRenderScaling(settings) => {
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.set_render_scaling(settings, runtime_min::runtime_scaling_context());
                }
            }
            GameCommand::Shutdown => {
                event_loop.exit();
            }
        }
    }

    fn window_attributes(&self) -> WindowAttributes {
        WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(self.width, self.height))
    }

    fn create_window_and_install_runtime(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), String> {
        if self.window.is_some() {
            return Ok(());
        }

        let window = event_loop
            .create_window(self.window_attributes())
            .map_err(|error| format!("create window: {error}"))?;
        self.window = Some(window);

        let Some(snapshot) = self.pending_snapshot.take() else {
            self.close_window();
            return Err("missing runtime snapshot".to_owned());
        };

        if let Err(error) = self.install_runtime(snapshot) {
            self.close_window();
            return Err(error);
        }

        Ok(())
    }

    fn close_window(&mut self) {
        self.visible = false;
        self.runtime = None;
        self.window = None;
    }

    fn install_runtime(&mut self, snapshot: GameRuntimeSnapshot) -> Result<(), String> {
        // Drop the old surface before creating a replacement for the same window.
        self.runtime = None;
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| "game window is not initialized".to_owned())?;
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
        Ok(())
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
        if let Err(e) = runtime.tick_game_frame(dt, false) {
            tracing::error!(target: "game", error = %e, "runtime tick/render failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_editor::FileEditorStore;

    #[cfg(target_os = "linux")]
    fn has_linux_display() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var_os("WAYLAND_SOCKET").is_some()
            || std::env::var_os("DISPLAY").is_some()
    }

    #[test]
    fn repeated_show_reuses_the_game_window_thread() {
        #[cfg(target_os = "linux")]
        if !has_linux_display() {
            eprintln!("skipping game window test: no Linux display is available");
            return;
        }

        if !crate::claim_native_event_loop_test_slot(
            "game_window::tests::repeated_show_reuses_the_game_window_thread",
        ) {
            return;
        }

        let scene = engine_ecs::Scene::default();
        let handle = spawn_game_window(
            "Game View Test".to_owned(),
            320,
            180,
            GameRuntimeSnapshot::new(
                EngineConfig::default(),
                PathBuf::from("."),
                vec![PathBuf::from("scripts")],
                PathBuf::from("assets"),
                scene.to_scene_file("test").unwrap(),
            ),
        );
        for _ in 0..20 {
            handle.show().expect("game window thread should stay alive");
        }

        std::thread::sleep(Duration::from_millis(250));
        let errors = handle
            .poll_events()
            .into_iter()
            .filter_map(|event| match event {
                GameEvent::Error(message) => Some(message),
                GameEvent::Closed => None,
            })
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "game window errors: {errors:?}");

        handle.shutdown();
    }

    #[test]
    fn polling_closed_game_window_keeps_event_loop_handle() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let store = FileEditorStore::new(&temp_dir.path().join("editor-state.toml"));
        let mut host = crate::EditorHost::new(store).expect("create editor host");
        let (event_tx, event_rx) = mpsc::channel();

        host.game_window = Some(GameWindowHandle {
            cmd_tx: None,
            event_rx,
            thread: None,
        });
        event_tx.send(GameEvent::Closed).expect("send close event");

        host.poll_game_window();

        assert!(
            host.game_window.is_some(),
            "closed game windows should reuse their event loop on the next Play"
        );
    }
}
