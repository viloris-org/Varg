#![forbid(unsafe_code)]

use std::process::ExitCode;

use engine_core::{EngineError, EngineResult, RuntimeProfile};

#[cfg(feature = "editor")]
use egui;
#[cfg(feature = "editor")]
use egui_wgpu;
#[cfg(feature = "editor")]
use egui_winit;
#[cfg(feature = "editor")]
use engine_editor_ui;
#[cfg(feature = "editor")]
use engine_render::ImageFormat;
#[cfg(feature = "editor")]
use engine_render_wgpu::WgpuRenderDevice;
#[cfg(feature = "editor")]
use winit;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("engine-cli error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> EngineResult<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("smoke") => smoke(args.next())?,
        Some("run") => run_project(args.next())?,
        Some("profiles") => print_profiles(),
        Some("--help") | Some("-h") | Some("help") => print_help(),
        #[cfg(feature = "editor")]
        Some("open") => open_editor()?,
        Some(command) => {
            return Err(EngineError::config(format!(
                "unknown engine-cli command `{command}`"
            )));
        }
    }

    Ok(())
}

fn smoke(profile_arg: Option<String>) -> EngineResult<()> {
    let profile = match profile_arg.as_deref() {
        None => RuntimeProfile::RuntimeMin,
        Some("runtime-min") => RuntimeProfile::RuntimeMin,
        Some("runtime-game") => RuntimeProfile::RuntimeGame,
        Some("editor") => RuntimeProfile::Editor,
        Some("agent-tools") => RuntimeProfile::AgentTools,
        Some("script-python") => RuntimeProfile::ScriptPython,
        Some("dev-full") => RuntimeProfile::DevFull,
        Some(profile) => {
            return Err(EngineError::config(format!(
                "unsupported profile `{profile}`"
            )));
        }
    };

    let frame = runtime_min::smoke_runtime_min()?;
    println!(
        "Aster {} smoke completed at frame {frame}",
        profile.as_str()
    );
    Ok(())
}

fn run_project(project_arg: Option<String>) -> EngineResult<()> {
    let project = project_arg.unwrap_or_else(|| "examples/project".to_string());
    runtime_min::run_project(project)
}

fn print_profiles() {
    for profile in [
        RuntimeProfile::RuntimeMin,
        RuntimeProfile::RuntimeGame,
        RuntimeProfile::Editor,
        RuntimeProfile::AgentTools,
        RuntimeProfile::ScriptPython,
        RuntimeProfile::DevFull,
    ] {
        println!("{}", profile.as_str());
    }
}

fn print_help() {
    println!("Aster native CLI");
    println!();
    println!("Usage:");
    println!("  cargo run -p engine-cli -- [smoke] [profile]");
    println!("  cargo run -p engine-cli -- run <project>");
    println!("  cargo run -p engine-cli -- profiles");
    #[cfg(feature = "editor")]
    println!("  cargo run -p engine-cli --features editor -- open");
}

#[cfg(feature = "editor")]
fn open_editor() -> EngineResult<()> {
    use egui_wgpu::wgpu;
    use engine_core::EngineConfig;
    use engine_editor::{
        ConsoleEntry, ConsoleLevel, ConsoleSource, EditorPreferences, ThemePreference,
    };
    use engine_editor_ui::{
        draw_hub, draw_shell, EditorShell, HubState, PlayModeRequest, ShellUiState,
    };
    use runtime_min::RuntimeServices;
    use std::{sync::Arc, time::Instant};
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        window::{Window, WindowId},
    };

    struct RenderState {
        surface: wgpu::Surface<'static>,
        device: std::sync::Arc<wgpu::Device>,
        queue: std::sync::Arc<wgpu::Queue>,
        config: wgpu::SurfaceConfiguration,
        renderer: egui_wgpu::Renderer,
    }

    /// Which top-level screen is active.
    enum Screen {
        Hub,
        Editor,
    }

    struct App {
        window: Option<Arc<Window>>,
        egui_ctx: egui::Context,
        egui_state: Option<egui_winit::State>,
        render_state: Option<RenderState>,
        wgpu_render_device: Option<WgpuRenderDevice>,
        scene_view_texture_id: Option<egui::TextureId>,
        screen: Screen,
        hub: HubState,
        shell: EditorShell,
        shell_ui: ShellUiState,
        play_runtime: Option<RuntimeServices>,
        last_editor_frame: Instant,
        runtime_diagnostic_cursor: usize,
    }

    impl App {
        /// Renders the editor scene view to an offscreen texture and registers it
        /// for display in the next egui frame.
        fn render_scene_view(&mut self) {
            let Some(wgpu_dev) = self.wgpu_render_device.as_mut() else {
                return;
            };
            let Some(render_state) = self.render_state.as_mut() else {
                return;
            };

            let world = engine_editor_ui::build_editor_render_world(&self.shell, &self.shell_ui);
            if !world.is_visible() {
                self.shell_ui.scene_view_texture = None;
                return;
            }

            if let Err(e) = wgpu_dev.render_world_offscreen(&world) {
                self.push_editor_error(format!("scene render failed: {e}"));
                self.shell_ui.scene_view_texture = None;
                return;
            }

            let (width, height) = wgpu_dev.default_target_size();
            let egui_texture_id =
                if let Some(texture_id) = self.scene_view_texture_id {
                    render_state
                        .renderer
                        .update_egui_texture_from_wgpu_texture(
                            &render_state.device,
                            wgpu_dev.default_target_view(),
                            wgpu::FilterMode::Linear,
                            texture_id,
                        );
                    texture_id
                } else {
                    let texture_id = render_state.renderer.register_native_texture(
                        &render_state.device,
                        wgpu_dev.default_target_view(),
                        wgpu::FilterMode::Linear,
                    );
                    self.scene_view_texture_id = Some(texture_id);
                    texture_id
                };

            let egui::TextureId::User(texture_id) = egui_texture_id else {
                return;
            };

            self.shell_ui.scene_view_texture =
                Some(engine_editor_ui::ViewportTexture {
                    id: texture_id,
                    width,
                    height,
                });
        }

        fn handle_play_mode_request(&mut self) {
            let Some(request) = self.shell_ui.play_mode_request.take() else {
                return;
            };
            match request {
                PlayModeRequest::Enter => self.enter_play_mode(),
                PlayModeRequest::Pause(paused) => {
                    if let Some(runtime) = self.play_runtime.as_mut() {
                        runtime.paused = paused;
                    }
                }
                PlayModeRequest::Step => {
                    self.shell_ui.paused = true;
                    if let Some(runtime) = self.play_runtime.as_mut() {
                        runtime.paused = true;
                    }
                    self.tick_play_runtime_once(true);
                }
                PlayModeRequest::Stop => self.stop_play_mode(),
            }
        }

        fn enter_play_mode(&mut self) {
            let Some(project) = self.shell.project_mut() else {
                self.shell_ui.playing = false;
                self.push_editor_error("Cannot enter Play Mode without an open project".to_owned());
                return;
            };
            let root = project.root.clone();
            if let Err(error) = project.scene.enter_play_mode() {
                self.shell_ui.playing = false;
                self.push_editor_error(error.to_string());
                return;
            }
            match runtime_min::headless_services_from_scene(
                EngineConfig::default(),
                root,
                &project.scene,
            ) {
                Ok(mut runtime) => {
                    runtime.paused = self.shell_ui.paused;
                    self.shell_ui.runtime_game_world = Some(runtime.render_world.clone());
                    self.play_runtime = Some(runtime);
                    self.runtime_diagnostic_cursor = 0;
                    self.last_editor_frame = Instant::now();
                }
                Err(error) => {
                    project.scene.exit_play_mode();
                    self.shell_ui.playing = false;
                    self.shell_ui.paused = false;
                    self.shell_ui.runtime_game_world = None;
                    self.push_editor_error(error.to_string());
                }
            }
        }

        fn stop_play_mode(&mut self) {
            self.play_runtime = None;
            self.runtime_diagnostic_cursor = 0;
            self.shell_ui.playing = false;
            self.shell_ui.paused = false;
            self.shell_ui.runtime_game_world = None;
            if let Some(project) = self.shell.project_mut() {
                project.scene.exit_play_mode();
            }
        }

        fn tick_play_runtime(&mut self) {
            if !self.shell_ui.playing {
                return;
            }
            let now = Instant::now();
            let delta = now.saturating_duration_since(self.last_editor_frame);
            self.last_editor_frame = now;
            self.tick_play_runtime_delta(delta, false);
        }

        fn tick_play_runtime_once(&mut self, single_step: bool) {
            self.last_editor_frame = Instant::now();
            self.tick_play_runtime_delta(
                std::time::Duration::from_secs_f32(1.0 / 60.0),
                single_step,
            );
        }

        fn tick_play_runtime_delta(&mut self, delta: std::time::Duration, single_step: bool) {
            let Some(runtime) = self.play_runtime.as_mut() else {
                return;
            };
            runtime.paused = self.shell_ui.paused;
            if let Err(error) = runtime.tick_game_frame(delta, single_step) {
                self.push_editor_error(error.to_string());
                self.stop_play_mode();
                return;
            }
            self.shell_ui.runtime_game_world = Some(runtime.render_world.clone());
            self.sync_runtime_diagnostics();
        }

        fn sync_runtime_diagnostics(&mut self) {
            let Some(runtime) = self.play_runtime.as_ref() else {
                return;
            };
            let diagnostics = runtime
                .diagnostics
                .iter()
                .skip(self.runtime_diagnostic_cursor)
                .cloned()
                .collect::<Vec<_>>();
            self.runtime_diagnostic_cursor = runtime.diagnostics.len();
            for diagnostic in diagnostics {
                self.shell.console_mut().push(ConsoleEntry {
                    timestamp: format!("frame {}", runtime.frame_index()),
                    level: match diagnostic.level.as_str() {
                        "error" => ConsoleLevel::Error,
                        "warning" | "warn" => ConsoleLevel::Warn,
                        "debug" => ConsoleLevel::Debug,
                        "trace" => ConsoleLevel::Trace,
                        _ => ConsoleLevel::Info,
                    },
                    source: ConsoleSource {
                        subsystem: diagnostic.source,
                        file: diagnostic.file,
                        line: diagnostic.line,
                    },
                    message: diagnostic.message,
                });
            }
        }

        fn push_editor_error(&mut self, message: String) {
            self.shell.console_mut().push(ConsoleEntry {
                timestamp: "now".to_string(),
                level: ConsoleLevel::Error,
                source: ConsoleSource {
                    subsystem: "editor".to_string(),
                    file: None,
                    line: None,
                },
                message,
            });
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let attrs = Window::default_attributes()
                .with_title("Aster Hub")
                .with_inner_size(winit::dpi::LogicalSize::new(1080u32, 720u32));
            let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

            let state = egui_winit::State::new(
                self.egui_ctx.clone(),
                egui::ViewportId::ROOT,
                &window,
                None,
                None,
                None,
            );
            self.egui_state = Some(state);

            let instance = wgpu::Instance::default();
            let surface = instance.create_surface(window.clone()).unwrap();
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                }))
                .expect("request wgpu adapter");

            let (raw_device, raw_queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                    .expect("request wgpu device");

            let device = std::sync::Arc::new(raw_device);
            let queue = std::sync::Arc::new(raw_queue);

            let size = window.inner_size();
            let surface_caps = surface.get_capabilities(&adapter);
            let surface_format = surface_caps
                .formats
                .iter()
                .copied()
                .find(|f| f.is_srgb())
                .unwrap_or(surface_caps.formats[0]);

            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface_format,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: surface_caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &config);

            let renderer = egui_wgpu::Renderer::new(
                &device,
                config.format,
                egui_wgpu::RendererOptions::default(),
            );

            self.render_state = Some(RenderState {
                surface,
                device: device.clone(),
                queue: queue.clone(),
                config,
                renderer,
            });
            self.scene_view_texture_id = None;

            let wgpu_render_device = WgpuRenderDevice::from_arc_device(
                instance,
                adapter,
                device,
                queue,
                size.width.max(1),
                size.height.max(1),
                ImageFormat::Rgba8Srgb,
                None,
            )
            .expect("create wgpu render device for offscreen rendering");
            self.wgpu_render_device = Some(wgpu_render_device);

            self.window = Some(window);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            let Some(window) = self.window.clone() else {
                return;
            };
            {
                let Some(state) = self.egui_state.as_mut() else {
                    return;
                };
                let response = state.on_window_event(&window, &event);
                if response.consumed {
                    return;
                }
            }

            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    if let Some(rs) = self.render_state.as_mut() {
                        rs.config.width = size.width.max(1);
                        rs.config.height = size.height.max(1);
                        rs.surface.configure(&rs.device, &rs.config);
                    }
                }
                WindowEvent::RedrawRequested => {
                    let raw_input = {
                        let Some(state) = self.egui_state.as_mut() else {
                            return;
                        };
                        state.take_egui_input(&window)
                    };
                    let mut should_close = false;
                    let egui_ctx = self.egui_ctx.clone();

                    // Render 3D scene to offscreen texture before the egui frame.
                    if matches!(self.screen, Screen::Editor) {
                        self.render_scene_view();
                    }

                    let full_output = egui_ctx.run_ui(raw_input, |ctx| match self.screen {
                        Screen::Hub => {
                            should_close = draw_hub(ctx, &mut self.hub);
                            if let Some(action) = self.hub.pending_action.take() {
                                match action {
                                    engine_editor_ui::HubAction::LaunchEditor {
                                        project_path,
                                        ..
                                    } => {
                                        self.play_runtime = None;
                                        self.shell_ui.playing = false;
                                        self.shell_ui.paused = false;
                                        self.shell_ui.runtime_game_world = None;
                                        if let Err(error) = self.shell.open_project(&project_path) {
                                            self.shell.console_mut().push(ConsoleEntry {
                                                timestamp: "now".to_string(),
                                                level: ConsoleLevel::Error,
                                                source: ConsoleSource {
                                                    subsystem: "editor".to_string(),
                                                    file: None,
                                                    line: None,
                                                },
                                                message: error.to_string(),
                                            });
                                        } else {
                                            self.screen = Screen::Editor;
                                            window.set_title("Aster Editor");
                                        }
                                    }
                                    engine_editor_ui::HubAction::OpenFolder(path) => {
                                        self.shell.console_mut().push(ConsoleEntry {
                                            timestamp: "now".to_string(),
                                            level: ConsoleLevel::Info,
                                            source: ConsoleSource {
                                                subsystem: "hub".to_string(),
                                                file: None,
                                                line: None,
                                            },
                                            message: format!(
                                                "open folder requested: {}",
                                                path.display()
                                            ),
                                        });
                                    }
                                    engine_editor_ui::HubAction::SelectProjectLocation => {
                                        if let Some(folder) = rfd::FileDialog::new()
                                            .set_title("Choose project location")
                                            .pick_folder()
                                        {
                                            if let Some(dialog) =
                                                self.hub.new_project_dialog.as_mut()
                                            {
                                                dialog.location =
                                                    folder.to_string_lossy().into_owned();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Screen::Editor => {
                            self.tick_play_runtime();
                            should_close = draw_shell(ctx, &mut self.shell, &mut self.shell_ui);
                            self.handle_play_mode_request();
                        }
                    });
                    if let Some(state) = self.egui_state.as_mut() {
                        state.handle_platform_output(&window, full_output.platform_output);
                    }

                    if let Some(rs) = self.render_state.as_mut() {
                        let clipped_primitives = self
                            .egui_ctx
                            .tessellate(full_output.shapes, full_output.pixels_per_point);
                        let screen_descriptor = egui_wgpu::ScreenDescriptor {
                            size_in_pixels: [rs.config.width, rs.config.height],
                            pixels_per_point: full_output.pixels_per_point,
                        };

                        for (id, image_delta) in full_output.textures_delta.set {
                            rs.renderer
                                .update_texture(&rs.device, &rs.queue, id, &image_delta);
                        }

                        let mut encoder =
                            rs.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: None,
                                });

                        rs.renderer.update_buffers(
                            &rs.device,
                            &rs.queue,
                            &mut encoder,
                            &clipped_primitives,
                            &screen_descriptor,
                        );

                        let frame = match rs.surface.get_current_texture() {
                            wgpu::CurrentSurfaceTexture::Success(frame)
                            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                            wgpu::CurrentSurfaceTexture::Outdated
                            | wgpu::CurrentSurfaceTexture::Lost => {
                                rs.surface.configure(&rs.device, &rs.config);
                                return;
                            }
                            wgpu::CurrentSurfaceTexture::Timeout
                            | wgpu::CurrentSurfaceTexture::Occluded
                            | wgpu::CurrentSurfaceTexture::Validation => return,
                        };
                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        {
                            let render_pass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: None,
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &view,
                                        resolve_target: None,
                                        ops: wgpu::Operations {
                                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                                r: 0.1,
                                                g: 0.1,
                                                b: 0.1,
                                                a: 1.0,
                                            }),
                                            store: wgpu::StoreOp::Store,
                                        },
                                        depth_slice: None,
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                    multiview_mask: None,
                                });

                            rs.renderer.render(
                                &mut render_pass.forget_lifetime(),
                                &clipped_primitives,
                                &screen_descriptor,
                            );
                        }

                        rs.queue.submit(std::iter::once(encoder.finish()));
                        frame.present();

                        for id in full_output.textures_delta.free {
                            rs.renderer.free_texture(&id);
                        }
                    }

                    if should_close {
                        event_loop.exit();
                    }
                    window.request_redraw();
                }
                _ => {}
            }
        }
    }

    let prefs = EditorPreferences {
        theme: ThemePreference::Dark,
        ..EditorPreferences::default()
    };

    let event_loop = EventLoop::new().map_err(|e| EngineError::other(e.to_string()))?;
    let mut hub = HubState::new(prefs.clone());
    let example_project = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("examples/project");
    hub.add_install(engine_editor::ToolchainInstall::new("0.1.0", "."));
    hub.upsert_project(engine_editor::ProjectMetadata::new(
        "Aster Example",
        example_project,
        "2026-05-19",
        "0.1.0",
    ));

    let egui_ctx = egui::Context::default();
    engine_editor_ui::setup_egui_fonts(&egui_ctx);

    let mut app = App {
        window: None,
        egui_ctx,
        egui_state: None,
        render_state: None,
        wgpu_render_device: None,
        scene_view_texture_id: None,
        screen: Screen::Hub,
        hub,
        shell: EditorShell::with_core_services(prefs),
        shell_ui: ShellUiState::all_open(),
        play_runtime: None,
        last_editor_frame: Instant::now(),
        runtime_diagnostic_cursor: 0,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| EngineError::other(e.to_string()))
}
