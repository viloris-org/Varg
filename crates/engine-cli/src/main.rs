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
use std::path::PathBuf;
#[cfg(feature = "editor")]
use winit;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("aster error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> EngineResult<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        #[cfg(feature = "editor")]
        None | Some("open") => open_editor()?,
        #[cfg(not(feature = "editor"))]
        None => smoke(args.next())?,
        Some("smoke") => smoke(args.next())?,
        Some("run") => run_project(args.next())?,
        Some("build") => build_project(args.collect())?,
        Some("profiles") => print_profiles(),
        Some("--help") | Some("-h") | Some("help") => print_help(),
        Some(command) => {
            return Err(EngineError::config(format!(
                "unknown aster command `{command}`"
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

fn build_project(args: Vec<String>) -> EngineResult<()> {
    use engine_ecs::{BuildConfiguration, ProjectManifest};
    use std::path::PathBuf;
    use std::process::Command;

    // Parse arguments
    let mut project_path: Option<String> = None;
    let mut config_path: Option<String> = None;
    let mut output_path: Option<String> = None;
    let mut args_iter = args.into_iter();

    while let Some(arg) = args_iter.next() {
        match arg.as_str() {
            "--config" => {
                config_path = args_iter.next();
            }
            "--output" => {
                output_path = args_iter.next();
            }
            _ if !arg.starts_with("--") && project_path.is_none() => {
                project_path = Some(arg);
            }
            _ => {
                return Err(EngineError::config(format!(
                    "unknown build argument: {}",
                    arg
                )));
            }
        }
    }

    let project_path = project_path.unwrap_or_else(|| "examples/project".to_string());
    let project_root = PathBuf::from(&project_path);

    // Load project manifest
    let manifest = if project_root.is_dir() {
        ProjectManifest::load(&project_root)?
    } else {
        return Err(EngineError::config(format!(
            "project path does not exist or is not a directory: {}",
            project_root.display()
        )));
    };

    // Load build configuration
    let config_file = if let Some(config) = config_path {
        PathBuf::from(config)
    } else {
        project_root.join(&manifest.build_config)
    };

    let config_content = std::fs::read_to_string(&config_file).map_err(|e| {
        EngineError::config(format!(
            "failed to read build config {}: {}",
            config_file.display(),
            e
        ))
    })?;

    let build_config: BuildConfiguration = toml::from_str(&config_content).map_err(|e| {
        EngineError::config(format!(
            "failed to parse build config {}: {}",
            config_file.display(),
            e
        ))
    })?;

    // Validate build configuration
    let diagnostics = build_config.diagnostics();
    if !diagnostics.is_empty() {
        eprintln!("Build configuration validation errors:");
        for diag in &diagnostics {
            eprintln!("  {}: {}", diag.path, diag.message);
        }
        return Err(EngineError::config(
            "build configuration validation failed".to_string(),
        ));
    }

    // Determine output directory
    let output_dir = if let Some(output) = output_path {
        PathBuf::from(output)
    } else {
        project_root.join("build")
    };

    println!("Building project: {}", manifest.name);
    println!("  Target: {}", build_config.target);
    println!("  Release: {}", build_config.release);
    println!("  Features: {}", build_config.features.join(", "));
    println!("  Output: {}", output_dir.display());

    // Build the runtime binary with cargo
    let workspace_root = std::env::current_dir()
        .map_err(|e| EngineError::config(format!("failed to get current directory: {}", e)))?;
    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd.arg("build").arg("-p").arg("aster");

    if build_config.release {
        cargo_cmd.arg("--release");
    }

    // Map build config features to aster CLI features
    // The build config may specify runtime features like "runtime-game", "physics", "audio"
    // which need to be translated to aster's feature set
    let mut aster_features = Vec::new();
    for feature in &build_config.features {
        match feature.as_str() {
            "runtime-game" => aster_features.push("runtime-game"),
            "runtime-min" => aster_features.push("runtime-min"),
            "wgpu" => aster_features.push("wgpu"),
            // physics and audio are runtime-min features, not aster features
            // They will be included when runtime-game is enabled
            "physics" | "audio" => {
                // Skip - these are runtime-min features
            }
            _ => {
                // Pass through other features
                aster_features.push(feature.as_str());
            }
        }
    }

    if !aster_features.is_empty() {
        cargo_cmd.arg("--features").arg(aster_features.join(","));
    }

    if build_config.target != "native" {
        cargo_cmd.arg("--target").arg(&build_config.target);
    }

    println!("\nRunning cargo build...");
    let cargo_output = cargo_cmd
        .output()
        .map_err(|e| EngineError::config(format!("failed to execute cargo build: {}", e)))?;

    if !cargo_output.status.success() {
        eprintln!("Cargo build failed:");
        eprintln!("{}", String::from_utf8_lossy(&cargo_output.stderr));
        return Err(EngineError::config("cargo build failed".to_string()));
    }

    println!("Cargo build succeeded");

    // Create output directory structure
    std::fs::create_dir_all(&output_dir).map_err(|e| {
        EngineError::config(format!(
            "failed to create output directory {}: {}",
            output_dir.display(),
            e
        ))
    })?;

    let bin_dir = output_dir.join("bin");
    let scenes_dir = output_dir.join("scenes");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| EngineError::config(format!("failed to create bin directory: {}", e)))?;
    std::fs::create_dir_all(&scenes_dir)
        .map_err(|e| EngineError::config(format!("failed to create scenes directory: {}", e)))?;

    // Copy runtime binary
    let profile = if build_config.release {
        "release"
    } else {
        "debug"
    };
    let binary_name = if cfg!(target_os = "windows") {
        "aster.exe"
    } else {
        "aster"
    };

    let source_binary = if build_config.target == "native" {
        workspace_root
            .join("target")
            .join(profile)
            .join(binary_name)
    } else {
        workspace_root
            .join("target")
            .join(&build_config.target)
            .join(profile)
            .join(binary_name)
    };

    let dest_binary = bin_dir.join(binary_name);
    std::fs::copy(&source_binary, &dest_binary).map_err(|e| {
        EngineError::config(format!(
            "failed to copy binary from {} to {}: {}",
            source_binary.display(),
            dest_binary.display(),
            e
        ))
    })?;

    println!("Copied binary to {}", dest_binary.display());

    // Copy default scene
    let scene_path = project_root.join(&manifest.default_scene);
    if scene_path.exists() {
        let scene_name = scene_path
            .file_name()
            .ok_or_else(|| EngineError::config("invalid scene path".to_string()))?;
        let dest_scene = scenes_dir.join(scene_name);
        std::fs::copy(&scene_path, &dest_scene).map_err(|e| {
            EngineError::config(format!(
                "failed to copy scene from {} to {}: {}",
                scene_path.display(),
                dest_scene.display(),
                e
            ))
        })?;
        println!("Copied scene to {}", dest_scene.display());
    }

    // Generate assets_manifest.json (placeholder for now)
    let assets_manifest = output_dir.join("assets_manifest.json");
    let manifest_content = serde_json::json!({
        "version": 1,
        "assets": []
    });
    let manifest_json = serde_json::to_string_pretty(&manifest_content)
        .map_err(|e| EngineError::config(format!("failed to serialize assets manifest: {}", e)))?;
    std::fs::write(&assets_manifest, manifest_json)
        .map_err(|e| EngineError::config(format!("failed to write assets_manifest.json: {}", e)))?;
    println!("Generated {}", assets_manifest.display());

    // Copy import_cache.json if it exists
    let import_cache = project_root.join("import_cache.json");
    if import_cache.exists() {
        let dest_cache = output_dir.join("import_cache.json");
        std::fs::copy(&import_cache, &dest_cache)
            .map_err(|e| EngineError::config(format!("failed to copy import_cache.json: {}", e)))?;
        println!("Copied import_cache.json");
    }

    // Write build_info.json
    let build_info = output_dir.join("build_info.json");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let build_info_content = serde_json::json!({
        "timestamp": timestamp,
        "target": build_config.target,
        "release": build_config.release,
        "engine_version": env!("CARGO_PKG_VERSION")
    });
    let build_info_json = serde_json::to_string_pretty(&build_info_content)
        .map_err(|e| EngineError::config(format!("failed to serialize build info: {}", e)))?;
    std::fs::write(&build_info, build_info_json)
        .map_err(|e| EngineError::config(format!("failed to write build_info.json: {}", e)))?;
    println!("Generated {}", build_info.display());

    println!("\nBuild completed successfully!");
    println!("Output directory: {}", output_dir.display());

    Ok(())
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
    println!("Aster");
    println!();
    println!("Usage:");
    #[cfg(feature = "editor")]
    println!("  cargo run -p aster");
    #[cfg(feature = "editor")]
    println!("  cargo run -p aster -- open");
    println!("  cargo run -p aster -- smoke [profile]");
    println!("  cargo run -p aster -- run <project>");
    println!("  cargo run -p aster -- build <project> [--config <path>] [--output <path>]");
    println!("  cargo run -p aster -- profiles");
}

#[cfg(feature = "editor")]
fn open_editor() -> EngineResult<()> {
    use egui_wgpu::wgpu;
    use engine_core::EngineConfig;
    use engine_editor::{
        ConsoleEntry, ConsoleLevel, ConsoleSource, DurableEditorState, EditorPreferences,
        FileEditorStore, ProjectMetadata, ThemePreference,
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
    #[derive(PartialEq)]
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
        game_view_texture_id: Option<egui::TextureId>,
        camera_preview_texture_id: Option<egui::TextureId>,
        screen: Screen,
        hub: HubState,
        shell: EditorShell,
        shell_ui: ShellUiState,
        play_runtime: Option<RuntimeServices>,
        last_editor_frame: Instant,
        runtime_diagnostic_cursor: usize,
        store: FileEditorStore,
    }

    fn register_viewport_texture(
        renderer: &mut egui_wgpu::Renderer,
        device: &wgpu::Device,
        cached_id: &mut Option<egui::TextureId>,
        output: &mut Option<engine_editor_ui::ViewportTexture>,
        texture_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        let egui_texture_id = if let Some(texture_id) = *cached_id {
            renderer.update_egui_texture_from_wgpu_texture(
                device,
                texture_view,
                wgpu::FilterMode::Linear,
                texture_id,
            );
            texture_id
        } else {
            let texture_id =
                renderer.register_native_texture(device, texture_view, wgpu::FilterMode::Linear);
            *cached_id = Some(texture_id);
            texture_id
        };
        let egui::TextureId::User(texture_id) = egui_texture_id else {
            return;
        };
        *output = Some(engine_editor_ui::ViewportTexture {
            id: texture_id,
            width,
            height,
        });
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

            // Resize GPU target to match the viewport rect from the previous frame.
            if let Some(target) = &self.shell_ui.scene_view_target {
                let (cw, ch) = wgpu_dev.default_target_size();
                if cw != target.desc.width || ch != target.desc.height {
                    let _ = wgpu_dev.resize_default_target(target.desc.width, target.desc.height);
                }
            }

            if let Err(e) = wgpu_dev.render_world_offscreen(&world) {
                self.push_editor_error(format!("scene render failed: {e}"));
                self.shell_ui.scene_view_texture = None;
                return;
            }

            let (width, height) = wgpu_dev.default_target_size();
            register_viewport_texture(
                &mut render_state.renderer,
                &render_state.device,
                &mut self.scene_view_texture_id,
                &mut self.shell_ui.scene_view_texture,
                wgpu_dev.default_target_view(),
                width,
                height,
            );
        }

        /// Renders the game view to an offscreen texture and registers it
        /// for display in the next egui frame.
        fn render_game_view(&mut self) {
            let Some(wgpu_dev) = self.wgpu_render_device.as_mut() else {
                return;
            };
            let Some(render_state) = self.render_state.as_mut() else {
                return;
            };

            // Only render when in Play Mode (or paused).
            if !self.shell_ui.playing && !self.shell_ui.paused {
                self.shell_ui.game_view_texture = None;
                return;
            }

            let Some(world) = self.shell_ui.runtime_game_world.as_ref() else {
                self.shell_ui.game_view_texture = None;
                return;
            };
            if !world.is_visible() {
                self.shell_ui.game_view_texture = None;
                return;
            }

            // Resize GPU target to match the viewport rect from the previous frame.
            if let Some(target) = &self.shell_ui.game_view_target {
                let (cw, ch) = wgpu_dev.game_target_size();
                if cw != target.desc.width || ch != target.desc.height {
                    let _ = wgpu_dev.resize_game_target(target.desc.width, target.desc.height);
                }
            }

            if let Err(e) = wgpu_dev.render_world_offscreen_game(world) {
                self.push_editor_error(format!("game render failed: {e}"));
                self.shell_ui.game_view_texture = None;
                return;
            }

            let (width, height) = wgpu_dev.game_target_size();
            register_viewport_texture(
                &mut render_state.renderer,
                &render_state.device,
                &mut self.game_view_texture_id,
                &mut self.shell_ui.game_view_texture,
                wgpu_dev.game_target_view(),
                width,
                height,
            );
        }

        /// Renders the selected-camera preview for the Inspector.
        fn render_camera_preview(&mut self) {
            let Some(wgpu_dev) = self.wgpu_render_device.as_mut() else {
                return;
            };
            let Some(render_state) = self.render_state.as_mut() else {
                return;
            };
            let Some(target) = self.shell_ui.camera_preview_target.as_ref() else {
                self.shell_ui.camera_preview_texture = None;
                return;
            };
            if !target.world.is_visible() {
                self.shell_ui.camera_preview_texture = None;
                return;
            }

            if let Err(e) = wgpu_dev.render_world_offscreen_preview(&target.world) {
                self.push_editor_error(format!("camera preview render failed: {e}"));
                self.shell_ui.camera_preview_texture = None;
                return;
            }

            let (width, height) = wgpu_dev.preview_target_size();
            register_viewport_texture(
                &mut render_state.renderer,
                &render_state.device,
                &mut self.camera_preview_texture_id,
                &mut self.shell_ui.camera_preview_texture,
                wgpu_dev.preview_target_view(),
                width,
                height,
            );
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
            self.shell_ui.game_view_texture = None;
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
            self.game_view_texture_id = None;
            self.camera_preview_texture_id = None;

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
                WindowEvent::CloseRequested => {
                    if self.screen == Screen::Editor && self.shell.is_scene_dirty() {
                        self.shell_ui.show_close_dialog = true;
                        self.shell_ui.close_dialog_exit_app = true;
                    } else {
                        event_loop.exit();
                    }
                }
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

                    // Render 3D scene and game views to offscreen textures before the egui frame.
                    if matches!(self.screen, Screen::Editor) {
                        self.render_scene_view();
                        self.render_game_view();
                        self.render_camera_preview();
                    }

                    let full_output = egui_ctx.run_ui(raw_input, |ctx| match self.screen {
                        Screen::Hub => {
                            should_close = draw_hub(ctx, &mut self.hub);
                            if let Some(action) = self.hub.pending_action.take() {
                                match action {
                                    engine_editor_ui::HubAction::LaunchEditor {
                                        project_path,
                                        toolchain_version,
                                    } => {
                                        self.play_runtime = None;
                                        self.shell_ui.playing = false;
                                        self.shell_ui.paused = false;
                                        self.shell_ui.runtime_game_world = None;
                                        self.shell_ui.game_view_texture = None;
                                        self.shell_ui.camera_preview_texture = None;
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
                                            let name = project_path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("Project");
                                            self.hub.upsert_project(ProjectMetadata::new(
                                                name,
                                                &project_path,
                                                "now",
                                                &toolchain_version,
                                            ));
                                            let state = self.hub.durable_state();
                                            let _ = self.store.save(&state);
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
                            if let Some(action) = self.shell_ui.pending_action.take() {
                                match action {
                                    engine_editor_ui::EditorAction::OpenScene => {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .set_title("Open Scene")
                                            .add_filter("Scene JSON", &["json", "scene"])
                                            .set_directory(
                                                self.shell
                                                    .project()
                                                    .map(|p| p.root.clone())
                                                    .unwrap_or_else(|| {
                                                        std::path::PathBuf::from(".")
                                                    }),
                                            )
                                            .pick_file()
                                        {
                                            match self.shell.load_scene(&path) {
                                                Ok(display_path) => {
                                                    self.shell_ui.status_toast = Some(format!(
                                                        "Scene loaded from {display_path}"
                                                    ));
                                                    self.shell_ui.status_toast_frames = 180;
                                                }
                                                Err(error) => {
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
                                                }
                                            }
                                        }
                                    }
                                    engine_editor_ui::EditorAction::SaveAs => {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .set_title("Save Scene As")
                                            .add_filter("Scene JSON", &["json", "scene"])
                                            .save_file()
                                        {
                                            match self.shell.save_scene_as(&path) {
                                                Ok(display_path) => {
                                                    self.shell_ui.status_toast = Some(format!(
                                                        "Scene saved to {display_path}"
                                                    ));
                                                    self.shell_ui.status_toast_frames = 180;
                                                }
                                                Err(error) => {
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
                                                }
                                            }
                                        }
                                    }
                                    engine_editor_ui::EditorAction::CloseWindow => {
                                        should_close = true;
                                    }
                                    engine_editor_ui::EditorAction::ReturnToHub => {
                                        self.screen = Screen::Hub;
                                        self.play_runtime = None;
                                        self.shell_ui.playing = false;
                                        self.shell_ui.paused = false;
                                        self.shell_ui.runtime_game_world = None;
                                        self.shell_ui.game_view_texture = None;
                                        self.shell_ui.scene_view_texture = None;
                                        self.shell_ui.camera_preview_texture = None;
                                        window.set_title("Aster Hub");
                                    }
                                }
                            }
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
                        let state = self.hub.durable_state();
                        let _ = self.store.save(&state);
                        event_loop.exit();
                    }
                    window.request_redraw();
                }
                _ => {}
            }
        }
    }

    let config_dir = std::env::var("HOME")
        .map(|home| PathBuf::from(home).join(".config").join("aster"))
        .unwrap_or_else(|_| PathBuf::from(".aster-config"));
    let store = FileEditorStore::new(config_dir.join("editor-state.toml"));
    let durable_state = store
        .load()
        .unwrap_or_else(|_| DurableEditorState::default());

    let prefs = durable_state.preferences.clone();
    let prefs = EditorPreferences {
        theme: ThemePreference::Dark,
        ..prefs
    };

    let event_loop = EventLoop::new().map_err(|e| EngineError::other(e.to_string()))?;
    let mut hub = HubState::from_durable_state(durable_state);
    hub.add_install(engine_editor::ToolchainInstall::new("0.1.0", "."));

    // Seed the example project if no projects exist.
    if hub.filtered_projects().is_empty() {
        let example_project = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("examples/project");
        if example_project.join("aster.project.toml").is_file() {
            hub.upsert_project(ProjectMetadata::new(
                "Aster Example",
                example_project,
                "2026-05-19",
                "0.1.0",
            ));
        }
    }

    let egui_ctx = egui::Context::default();
    engine_editor_ui::setup_egui_fonts(&egui_ctx);

    let mut app = App {
        window: None,
        egui_ctx,
        egui_state: None,
        render_state: None,
        wgpu_render_device: None,
        scene_view_texture_id: None,
        game_view_texture_id: None,
        camera_preview_texture_id: None,
        screen: Screen::Hub,
        hub,
        shell: EditorShell::with_core_services(prefs),
        shell_ui: ShellUiState::all_open(),
        play_runtime: None,
        last_editor_frame: Instant::now(),
        runtime_diagnostic_cursor: 0,
        store,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| EngineError::other(e.to_string()))
}
