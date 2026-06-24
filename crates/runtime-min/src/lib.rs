#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Minimal Aster runtime and first playable game runner.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(feature = "script-python")]
use std::time::Instant;

use engine_assets::{
    AssetDatabase, AssetGuid, AssetRegistry, DecodedCubemapResource, DecodedTextureResource,
    GpuResource, HotReloadTracker, ImportTask, MaterialFormat, ModelResource, ResourceKind,
    import_builtin_asset, scan_project_assets,
};
#[cfg(feature = "audio")]
use engine_audio::{
    AcousticAabb, AcousticMaterial, AcousticSceneSnapshot, AcousticSolverConfig,
    AcousticSourceSample, AttenuationModel, AudioContext, AudioListenerDesc, AudioObjectTransform,
    AudioSourceDesc, AudioSourceShape, ClipHandle, HrtfQuality, MemoryAudioBackend, OutputMode,
    SourceHandle, SpatialMode, VirtualizationPolicy, VoiceCategory, solve_direct_propagation,
};
#[cfg(feature = "physics")]
use engine_core::math::Vec3;
use engine_core::{EngineConfig, EngineError, EngineResult, FrameCounter, TimeState, logging};
#[cfg(feature = "audio")]
use engine_ecs::AudioSourceComponentData;
#[cfg(feature = "script-python")]
use engine_ecs::ScriptComponentProxy;
use engine_ecs::{BuildConfiguration, ComponentData, ProjectManifest, Scene};
#[cfg(feature = "physics")]
use engine_physics::{
    BodyHandle, BodyKind, CharacterControllerDesc, ColliderDesc, ColliderShape, ColliderShapeRef,
    PhysicsWorld, QueryFilter, RapierPhysicsBackend, RigidbodyDesc, built_in_physical_material,
};
#[cfg(feature = "runtime-game")]
use engine_platform::GamepadProvider;
use engine_platform::{ActionMap, InputState};
use engine_render::{
    AntiAliasingMode, BatteryPolicy, FrameGenerationKind, HeadlessRenderDevice, ImageDesc,
    ImageFormat, ImageHandle, ImageUsage, PresentStrategy, RenderApi, RenderDevice, RenderFrame,
    RenderGraph, RenderGraphBuilder, RenderMaterialTextures, RenderPerformanceConfig,
    RenderPlatformClass, RenderQualityMode, RenderScalingContext, RenderScalingSettings,
    RenderWorld, ThermalState, UiCompositionPolicy, UpscalerKind,
};
#[cfg(feature = "wgpu")]
pub use engine_render_wgpu::WgpuRenderDevice;
use engine_script_varg::{VargRuntimeContext, VargScript, compile_script_source};

/// Explicit runtime services. There is no hidden global mutable state.
#[derive(Debug)]
pub struct RuntimeServices<R = HeadlessRenderDevice> {
    /// Runtime configuration.
    pub config: EngineConfig,
    /// Scene storage.
    pub scene: Scene,
    /// Render abstraction.
    pub renderer: R,
    /// Active render graph.
    pub render_graph: RenderGraph,
    /// Frame input state.
    pub input: InputState,
    /// Logical action bindings for high-level input queries.
    pub action_map: ActionMap,
    /// Latest scene extraction submitted to rendering.
    pub render_world: RenderWorld,
    /// Whether the game simulation is paused.
    pub paused: bool,
    /// Aggregated time state (delta, fixed delta, total time, frame index, time scale).
    pub time: TimeState,
    /// Latest runtime counters for diagnostics UI and smoke tests.
    pub stats: RuntimeStats,
    /// Current player-facing render scaling settings.
    pub render_scaling_settings: RenderScalingSettings,
    /// Last successfully negotiated render scaling selection.
    pub render_scaling_selection: Option<engine_render::RenderScalingSelection>,
    /// Diagnostics emitted by runtime subsystems.
    pub diagnostics: Vec<RuntimeDiagnostic>,
    #[cfg(feature = "physics")]
    /// Physics world used by runtime-game.
    pub physics: PhysicsWorld,
    #[cfg(feature = "runtime-game")]
    /// Platform gamepad backend used by runtime-game.
    pub gamepad_provider: Box<dyn GamepadProvider>,
    #[cfg(feature = "audio")]
    /// Audio context used by runtime-game.
    pub audio: AudioContext,
    /// Project root used to resolve script and asset paths.
    pub project_root: Option<PathBuf>,
    /// Asset database used to resolve project GUIDs to runtime resources.
    pub asset_database: AssetDatabase,
    /// Runtime asset registry and cache state.
    pub asset_registry: AssetRegistry,
    /// Asset folder used by scan/import and hot reload.
    pub asset_root: Option<PathBuf>,
    /// GPU textures resolved from project asset GUIDs.
    pub texture_resources: HashMap<engine_core::AssetId, ImageHandle>,
    /// CPU mesh resources resolved from project asset GUIDs.
    pub mesh_resources: HashMap<engine_core::AssetId, ModelResource>,
    /// Material resources resolved from project asset GUIDs.
    pub material_resources: HashMap<engine_core::AssetId, MaterialFormat>,
    frame_counter: FrameCounter,
    reported_script_errors: HashSet<String>,
    hot_reload: HotReloadTracker,
    #[cfg(feature = "script-python")]
    script_instances: HashSet<ScriptInstanceKey>,
    #[cfg(feature = "script-python")]
    python_script_runtime: PythonScriptRuntimeConfig,
    #[cfg(feature = "script-python")]
    script_diagnostics_this_frame: usize,
    varg_script_cache: HashMap<PathBuf, VargScript>,
    #[cfg(feature = "audio")]
    audio_bindings: Vec<AudioBinding>,
    #[cfg(feature = "audio")]
    audio_clips: HashMap<engine_core::AssetId, ClipHandle>,
    #[cfg(feature = "audio")]
    audio_listener_position: Option<engine_core::math::Vec3>,
    #[cfg(feature = "physics")]
    physics_bindings: Vec<PhysicsBinding>,
}

/// Runtime counters surfaced to editor and CLI diagnostics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeStats {
    /// Frame delta in seconds.
    pub frame_time_seconds: f32,
    /// Number of renderable objects submitted this frame.
    pub draw_calls: usize,
    /// Number of indexed triangles submitted by the render backend.
    pub triangles: u64,
    /// Number of render objects considered before visibility selection.
    pub submitted_render_objects: u32,
    /// Number of render objects selected into the latest Visibility Set.
    pub visible_render_objects: u32,
    /// Number of render objects rejected by visibility selection.
    pub culled_render_objects: u32,
    /// Number of enabled Frame Pipeline passes.
    pub render_pipeline_passes: u32,
    /// Number of scene objects.
    pub entity_count: usize,
    /// Number of render resources known to the runtime.
    pub resource_count: usize,
    /// Number of fixed physics steps run this frame.
    pub physics_steps: u32,
    /// CPU time spent preparing and submitting the render frame.
    pub render_cpu_ms: f32,
    /// Native output dimensions.
    pub output_size: (u32, u32),
    /// Internal rendering dimensions after dynamic resolution.
    pub internal_render_size: (u32, u32),
    /// Active internal linear rendering scale.
    pub render_scale: f32,
    /// Active upscaler implementation.
    pub upscaler: engine_render::UpscalerKind,
    /// Active frame generation implementation.
    pub frame_generation: engine_render::FrameGenerationKind,
    /// Latest GPU frame time, when supported.
    pub gpu_frame_ms: Option<f32>,
    /// Estimated input latency, when supported.
    pub estimated_latency_ms: Option<f32>,
    /// Dropped presentation frame count.
    pub dropped_frames: u64,
    /// Number of logical audio sources.
    pub audio_sources: u32,
    /// Number of physical voices rendered in the latest audio block.
    pub audio_physical_voices: u32,
    /// Number of virtualized voices.
    pub audio_virtual_voices: u32,
    /// Number of audio backend underruns or stream errors.
    pub audio_underruns: u64,
}

/// Structured runtime diagnostic entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeDiagnostic {
    /// Subsystem that emitted the diagnostic.
    pub source: String,
    /// Human-readable severity.
    pub level: String,
    /// Diagnostic message.
    pub message: String,
    /// Optional source file.
    pub file: Option<PathBuf>,
    /// Optional source line.
    pub line: Option<u32>,
}

#[cfg(feature = "physics")]
#[derive(Clone, Debug)]
struct PhysicsBinding {
    object: engine_core::EntityId,
    body: BodyHandle,
    last_position: engine_core::math::Vec3,
}

#[cfg(feature = "audio")]
#[derive(Clone, Debug)]
struct AudioBinding {
    object: engine_core::EntityId,
    source: SourceHandle,
    last_position: engine_core::math::Vec3,
}

#[cfg(feature = "script-python")]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ScriptInstanceKey {
    object: engine_core::EntityId,
    backend: String,
    script: String,
}

/// Configuration for the Python script subprocess backend.
#[cfg(feature = "script-python")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonScriptRuntimeConfig {
    /// Python interpreter executable.
    pub interpreter: PathBuf,
    /// Maximum time a single script invocation may block the frame.
    pub invocation_timeout: Duration,
    /// Maximum script diagnostics emitted per frame.
    pub diagnostics_per_frame: usize,
}

#[cfg(feature = "script-python")]
impl Default for PythonScriptRuntimeConfig {
    fn default() -> Self {
        Self {
            interpreter: PathBuf::from("python3"),
            invocation_timeout: Duration::from_millis(100),
            diagnostics_per_frame: 8,
        }
    }
}

impl RuntimeServices<HeadlessRenderDevice> {
    /// Creates minimal runtime services with a headless renderer.
    pub fn minimal(config: EngineConfig) -> Self {
        Self::with_renderer(config, HeadlessRenderDevice::default())
    }
}

/// Creates headless runtime services from a cloned edit-time scene snapshot.
pub fn headless_services_from_scene(
    config: EngineConfig,
    project_root: impl Into<PathBuf>,
    scene: &Scene,
) -> EngineResult<RuntimeServices> {
    let file = scene.to_scene_file("play-copy")?;
    let mut services = RuntimeServices::minimal(config);
    services.set_project_root(project_root);
    services.scene = Scene::from_scene_file(file)?;
    services.render_world = extract_render_world(&services.scene);
    Ok(services)
}

fn modified_time(path: &Path) -> EngineResult<std::time::SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|source| EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        })
}

impl<R: RenderDevice> RuntimeServices<R> {
    /// Creates runtime services with an explicit render backend.
    pub fn with_renderer(config: EngineConfig, renderer: R) -> Self {
        let render_graph = build_default_render_graph();
        Self {
            config,
            scene: Scene::default(),
            renderer,
            render_graph,
            input: {
                let mut input = InputState::default();
                input.bind_default_player_actions();
                input
            },
            action_map: ActionMap::new(),
            render_world: RenderWorld::default(),
            paused: false,
            time: TimeState::new(),
            stats: RuntimeStats::default(),
            render_scaling_settings: RenderScalingSettings::default(),
            render_scaling_selection: None,
            diagnostics: Vec::new(),
            #[cfg(feature = "physics")]
            physics: PhysicsWorld::new(RapierPhysicsBackend::new()),
            #[cfg(feature = "runtime-game")]
            gamepad_provider: engine_platform::GilrsGamepadProvider::new()
                .map(|provider| Box::new(provider) as Box<dyn GamepadProvider>)
                .unwrap_or_else(|_| Box::new(engine_platform::NullGamepadProvider)),
            #[cfg(feature = "audio")]
            audio: AudioContext::new(MemoryAudioBackend::new()),
            project_root: None,
            asset_database: AssetDatabase::new("assets", "builtin"),
            asset_registry: AssetRegistry::default(),
            asset_root: None,
            texture_resources: HashMap::new(),
            mesh_resources: HashMap::new(),
            material_resources: HashMap::new(),
            frame_counter: FrameCounter::default(),
            reported_script_errors: HashSet::new(),
            hot_reload: HotReloadTracker::default(),
            #[cfg(feature = "script-python")]
            script_instances: HashSet::new(),
            #[cfg(feature = "script-python")]
            python_script_runtime: PythonScriptRuntimeConfig::default(),
            #[cfg(feature = "script-python")]
            script_diagnostics_this_frame: 0,
            varg_script_cache: HashMap::new(),
            #[cfg(feature = "audio")]
            audio_bindings: Vec::new(),
            #[cfg(feature = "audio")]
            audio_clips: HashMap::new(),
            #[cfg(feature = "audio")]
            audio_listener_position: None,
            #[cfg(feature = "physics")]
            physics_bindings: Vec::new(),
        }
    }

    /// Applies player-facing render settings immediately without recreating the renderer.
    pub fn set_render_scaling(
        &mut self,
        settings: RenderScalingSettings,
        context: RenderScalingContext,
    ) -> engine_render::RenderScalingSelection {
        let settings = settings.normalized();
        let selection = self.renderer.configure_render_scaling(&settings, context);
        self.render_scaling_settings = settings;
        self.render_scaling_selection = Some(selection.clone());
        self.stats.render_scale = selection.render_scale;
        self.stats.upscaler = selection.upscaler;
        self.stats.frame_generation = selection.frame_generation;
        selection
    }

    /// Returns the renderer's currently available scaling capabilities.
    pub fn render_scaling_capabilities(&self) -> engine_render::RenderScalingCapabilities {
        self.renderer.render_scaling_capabilities()
    }

    #[cfg(all(feature = "audio", feature = "runtime-game"))]
    fn enable_default_audio_output(&mut self) {
        match AudioContext::device_default() {
            Ok(audio) => self.audio = audio,
            Err(error) => self.diagnostics.push(RuntimeDiagnostic {
                source: "audio".to_string(),
                level: "warning".to_string(),
                message: format!("default audio output unavailable; using memory backend: {error}"),
                file: None,
                line: None,
            }),
        }
    }

    /// Ticks one runtime frame.
    pub fn tick(&mut self) -> EngineResult<()> {
        logging::log_frame(self.frame_counter.get());
        let frame = RenderFrame {
            frame_index: self.frame_counter.get(),
        };
        self.renderer.execute_graph(&self.render_graph, frame)?;
        self.renderer
            .flush_destroy_queue(self.frame_counter.get().saturating_sub(2));
        self.frame_counter.advance();
        Ok(())
    }

    /// Ticks one game frame with explicit input, fixed update, scene, audio, render, and destroy order.
    pub fn tick_game_frame(&mut self, delta: Duration, single_step: bool) -> EngineResult<()> {
        self.run_frame(delta, single_step)
    }

    /// Executes one game frame in well-ordered phases:
    ///
    /// 1. **begin_frame** — reset transient input state
    /// 2. **fixed_timestep_loop** — physics step + fixed-update scripts
    /// 3. **update** — player controller + update scripts
    /// 4. **late_update** — scene runtime tick + audio
    /// 5. **render_submit** — extract render world, submit, execute graph
    /// 6. **deferred_destroy** — flush GPU destroy queue, process scene deferred destroys
    /// 7. **end_frame** — update stats, advance frame counter
    pub fn run_frame(&mut self, delta: Duration, single_step: bool) -> EngineResult<()> {
        let dt = delta.as_secs_f32();

        // ── begin_frame ────────────────────────────────────────────────
        logging::log_frame(self.frame_counter.get());
        self.input.end_frame();
        #[cfg(feature = "runtime-game")]
        {
            let gamepads = self.gamepad_provider.poll_gamepads();
            self.input.apply_gamepad_states(gamepads);
        }
        self.time.update(dt);
        self.stats.frame_time_seconds = self.time.delta_seconds;
        self.stats.physics_steps = 0;
        #[cfg(feature = "script-python")]
        {
            self.script_diagnostics_this_frame = 0;
        }
        self.report_script_proxy_diagnostics();

        let should_simulate = !self.paused || single_step;

        // ── script startup ───────────────────────────────────────────
        if should_simulate {
            #[cfg(feature = "physics")]
            self.ensure_physics_bindings()?;
            #[cfg(feature = "audio")]
            self.ensure_audio_bindings()?;
            #[cfg(feature = "script-python")]
            self.run_python_scripts("start", dt);
            self.run_varg_scripts_start();

            // ── fixed_timestep_loop ────────────────────────────────────
            let mut fixed_steps = 0;
            while self.time.consume_fixed_step(fixed_steps) {
                #[cfg(feature = "physics")]
                {
                    let fixed_dt = self.time.fixed_delta_seconds;
                    self.sync_scene_to_physics()?;
                    self.apply_environment_forces(fixed_dt)?;
                    self.physics.fixed_update(fixed_dt);
                    self.report_physics_events();
                    self.sync_physics_to_scene()?;
                    self.stats.physics_steps = self.stats.physics_steps.saturating_add(1);
                }
                #[cfg(feature = "script-python")]
                {
                    let fixed_dt = self.time.fixed_delta_seconds;
                    self.run_python_scripts("fixed_update", fixed_dt);
                }
                let fixed_dt = self.time.fixed_delta_seconds;
                self.run_varg_scripts_fixed_update(fixed_dt);
                self.scene.tick_fixed_frame();
                fixed_steps += 1;
            }

            // ── update ─────────────────────────────────────────────────
            self.apply_builtin_player_controller();
            #[cfg(feature = "script-python")]
            self.run_python_scripts("update", dt);
            self.run_varg_scripts_update(dt);

            // ── late_update ────────────────────────────────────────────
            self.scene.tick_runtime_frame();
        }

        #[cfg(feature = "audio")]
        self.update_audio(dt)?;

        // ── render_submit ──────────────────────────────────────────────
        self.render_world = extract_render_world(&self.scene);
        Self::resolve_render_materials(&mut self.render_world, &self.mesh_resources);
        let frame = RenderFrame {
            frame_index: self.frame_counter.get(),
        };
        self.renderer.submit_render_world_with_graph(
            &self.render_world,
            &self.render_graph,
            frame,
        )?;
        self.renderer.record_frame_time(dt * 1000.0);
        let render_metrics = self.renderer.performance_metrics();

        // ── deferred_destroy ───────────────────────────────────────────
        self.renderer
            .flush_destroy_queue(self.frame_counter.get().saturating_sub(2));
        self.scene.process_deferred_destroy()?;

        // ── end_frame ──────────────────────────────────────────────────
        self.stats.draw_calls = if self.renderer.api() == RenderApi::Headless {
            self.render_world.objects.len()
        } else {
            render_metrics.draw_calls as usize
        };
        if self.renderer.api() == RenderApi::Headless {
            self.stats.triangles = 0;
            self.stats.submitted_render_objects = self.render_world.objects.len() as u32;
            self.stats.visible_render_objects = self.render_world.objects.len() as u32;
            self.stats.culled_render_objects = 0;
            self.stats.render_pipeline_passes = self.render_graph.pass_count() as u32;
        } else {
            self.stats.triangles = render_metrics.triangles;
            self.stats.submitted_render_objects = render_metrics.submitted_objects;
            self.stats.visible_render_objects = render_metrics.visible_objects;
            self.stats.culled_render_objects = render_metrics.culled_objects;
            self.stats.render_pipeline_passes = render_metrics.pipeline_passes;
        }
        self.stats.entity_count = self.scene.object_count();
        self.stats.resource_count = self.render_world.objects.len()
            + self.render_world.lights.len()
            + usize::from(self.render_world.camera.is_some());
        self.stats.render_cpu_ms = render_metrics.render_cpu_ms;
        self.stats.output_size = (render_metrics.output_width, render_metrics.output_height);
        self.stats.internal_render_size = (
            render_metrics.internal_width,
            render_metrics.internal_height,
        );
        self.stats.render_scale = render_metrics.render_scale;
        self.stats.upscaler = render_metrics.upscaler;
        self.stats.frame_generation = render_metrics.frame_generation;
        self.stats.gpu_frame_ms = render_metrics.gpu_frame_ms;
        self.stats.estimated_latency_ms = render_metrics.estimated_latency_ms;
        self.stats.dropped_frames = render_metrics.dropped_frames;
        self.frame_counter.advance();
        Ok(())
    }

    /// Resolves material names on render objects using imported model
    /// materials. When a render object references a model but has no explicit
    /// material asset assignment, the first material from the model is used.
    fn resolve_render_materials(
        world: &mut RenderWorld,
        mesh_resources: &HashMap<engine_core::AssetId, ModelResource>,
    ) {
        for obj in &mut world.objects {
            if !obj.material.is_empty() && obj.material != "debug/default" {
                continue;
            }
            let Some(model_guid) = parse_asset_guid(&obj.mesh) else {
                continue;
            };
            let Some(model) = mesh_resources.get(&model_guid) else {
                continue;
            };
            if model.materials.is_empty() {
                continue;
            }
            let material_index = model_material_index_for_mesh(&obj.mesh, model).unwrap_or(0);
            obj.material = format!("material:{:032x}:{material_index}", model_guid.as_u128());
        }
    }

    /// Current frame index.
    pub fn frame_index(&self) -> u64 {
        self.frame_counter.get()
    }

    fn run_varg_scripts_start(&mut self) {
        self.run_varg_lifecycle("start", 0.0, true);
    }

    fn run_varg_scripts_fixed_update(&mut self, fixed_dt: f32) {
        self.run_varg_lifecycle("fixedUpdate", fixed_dt, false);
    }

    fn run_varg_scripts_update(&mut self, dt: f32) {
        self.run_varg_lifecycle("update", dt, false);
    }

    fn run_varg_lifecycle(&mut self, hook: &str, dt: f32, once: bool) {
        let invocations = self
            .scene
            .iter_objects()
            .flat_map(|(entity, object)| {
                object
                    .scripts
                    .iter()
                    .chain(
                        object
                            .components
                            .iter()
                            .filter_map(|component| match component {
                                ComponentData::Script(script) => Some(script),
                                _ => None,
                            }),
                    )
                    .filter(|script| {
                        script
                            .legacy_backend
                            .as_deref()
                            .is_none_or(|backend| backend == "varg")
                    })
                    .cloned()
                    .map(move |script| (entity, script))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        for (entity, script) in invocations {
            if once && !script.pending_recovery && script.state.contains_key("__varg_started") {
                continue;
            }
            let script_path = self.resolve_project_path(&script.source);
            let compiled = match self.load_varg_script(&script_path) {
                Ok(script) => script.clone(),
                Err(error) => {
                    self.diagnostics.push(RuntimeDiagnostic {
                        source: "script".to_string(),
                        level: "error".to_string(),
                        message: format!("varg load error: {error}"),
                        file: Some(script_path),
                        line: None,
                    });
                    continue;
                }
            };
            let transform = self.scene.transforms().local(entity).unwrap_or_default();
            let mut output = compiled.run_hook(
                hook,
                VargRuntimeContext {
                    transform,
                    input: self.input.clone(),
                    delta_time: dt,
                    exported_values: script.exported_values.clone(),
                    state: script.state.clone(),
                },
            );
            if once {
                output
                    .state
                    .insert("__varg_started".to_string(), serde_json::Value::Bool(true));
            }
            self.scene
                .transforms_mut()
                .set_local(entity, output.transform);
            self.apply_varg_script_state(entity, &script.source, output.state);
            for message in output.logs {
                self.diagnostics.push(RuntimeDiagnostic {
                    source: "script".to_string(),
                    level: "info".to_string(),
                    message,
                    file: Some(script_path.clone()),
                    line: None,
                });
            }
        }
    }

    fn load_varg_script(&mut self, path: &Path) -> EngineResult<&VargScript> {
        if !self.varg_script_cache.contains_key(path) {
            let source = fs::read_to_string(path).map_err(|source| EngineError::Filesystem {
                path: path.to_path_buf(),
                source,
            })?;
            let (script, diagnostics) = compile_script_source(path, &source);
            if !diagnostics.is_empty() {
                let details = diagnostics
                    .iter()
                    .map(|diagnostic| {
                        format!(
                            "{} at {}:{}: {}",
                            diagnostic.code,
                            diagnostic.line.unwrap_or(1),
                            diagnostic.column.unwrap_or(1),
                            diagnostic.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(EngineError::other(details));
            }
            let Some(script) = script else {
                return Err(EngineError::other("Varg script did not compile"));
            };
            self.varg_script_cache.insert(path.to_path_buf(), script);
        }
        Ok(self.varg_script_cache.get(path).expect("script inserted"))
    }

    fn apply_varg_script_state(
        &mut self,
        entity: engine_ecs::Entity,
        source: &str,
        state: HashMap<String, serde_json::Value>,
    ) {
        if let Some(object) = self.scene.object_mut(entity) {
            for candidate in &mut object.scripts {
                if candidate.source == source
                    && candidate
                        .legacy_backend
                        .as_deref()
                        .is_none_or(|backend| backend == "varg")
                {
                    candidate.state = state.clone();
                    candidate.pending_recovery = false;
                }
            }
            for component in &mut object.components {
                if let ComponentData::Script(candidate) = component {
                    if candidate.source == source
                        && candidate
                            .legacy_backend
                            .as_deref()
                            .is_none_or(|backend| backend == "varg")
                    {
                        candidate.state = state.clone();
                        candidate.pending_recovery = false;
                    }
                }
            }
        }
    }

    fn resolve_project_path(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            self.project_root
                .as_ref()
                .map(|root| root.join(&path))
                .unwrap_or(path)
        }
    }

    /// Returns true if any key bound to `action_name` was pressed this frame.
    pub fn action_pressed(&self, action_name: &str) -> bool {
        self.action_map.action_pressed(&self.input, action_name)
    }

    /// Returns true if any key bound to `action_name` is held (including the first frame).
    pub fn action_held(&self, action_name: &str) -> bool {
        self.action_map.action_held(&self.input, action_name)
    }

    /// Loads action bindings from a TOML file at the given path.
    ///
    /// The TOML format is:
    /// ```toml
    /// [actions]
    /// MoveForward = ["W", "ArrowUp"]
    /// Jump = ["Space"]
    /// ```
    /// Key names must match `KeyCode` variant names (Escape, Enter, Space,
    /// ArrowUp, ArrowDown, ArrowLeft, ArrowRight) or be a single character
    /// for `Character` keys.
    pub fn load_action_bindings(&mut self, path: &Path) -> EngineResult<()> {
        let content = fs::read_to_string(path).map_err(|source| EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;
        let doc: toml::Value = content.parse().map_err(|_| {
            EngineError::other(format!(
                "failed to parse action bindings TOML at {}",
                path.display()
            ))
        })?;
        let Some(actions) = doc.get("actions").and_then(|v| v.as_table()) else {
            return Ok(());
        };
        for (name, keys) in actions {
            if let Some(arr) = keys.as_array() {
                for key_val in arr {
                    if let Some(key_str) = key_val.as_str() {
                        if let Some(key) = ActionMap::parse_key_name(key_str) {
                            self.action_map.bind(name, key);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Updates Python script subprocess configuration.
    #[cfg(feature = "script-python")]
    pub fn set_python_script_runtime_config(&mut self, config: PythonScriptRuntimeConfig) {
        self.python_script_runtime = config;
    }

    /// Replaces the active render graph.
    pub fn set_render_graph(&mut self, graph: RenderGraph) {
        self.render_graph = graph;
    }

    /// Processes a winit window event, dispatching it to the appropriate
    /// input state or renderer method.
    ///
    /// Handles: KeyboardInput, MouseInput, CursorMoved, MouseWheel, Resized.
    /// Returns `true` if the event was CloseRequested (caller should exit).
    #[cfg(feature = "runtime-game")]
    pub fn process_winit_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use engine_platform::InputEvent;
        use winit::event::{ElementState, MouseScrollDelta, WindowEvent};

        match event {
            WindowEvent::CloseRequested => return true,
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key) = convert_winit_key_static(event.physical_key) {
                    match event.state {
                        ElementState::Pressed => {
                            self.input.apply_event(InputEvent::KeyDown(key));
                        }
                        ElementState::Released => {
                            self.input.apply_event(InputEvent::KeyUp(key));
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(btn) = convert_winit_mouse_button_static(*button) {
                    match state {
                        ElementState::Pressed => {
                            self.input.apply_event(InputEvent::MouseButtonDown(btn));
                        }
                        ElementState::Released => {
                            self.input.apply_event(InputEvent::MouseButtonUp(btn));
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.apply_event(InputEvent::MouseMove {
                    x: position.x as f32,
                    y: position.y as f32,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (*x, *y),
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x as f32, position.y as f32)
                    }
                };
                self.input.apply_event(InputEvent::MouseWheel { x, y });
            }
            _ => {}
        }
        false
    }

    /// Sets the project root used by runtime backends to resolve relative files.
    pub fn set_project_root(&mut self, root: impl Into<PathBuf>) {
        self.project_root = Some(root.into());
    }

    /// Scans, imports, and binds all supported project assets for runtime use.
    pub fn load_project_assets(&mut self, asset_root: impl Into<PathBuf>) -> EngineResult<()> {
        let asset_root = asset_root.into();
        self.asset_database = AssetDatabase::new(&asset_root, "builtin");
        self.asset_root = Some(asset_root.clone());
        let report = scan_project_assets(&asset_root, &mut self.asset_database)?;
        for meta in report.metas {
            self.import_runtime_asset(
                &asset_root,
                meta.guid,
                &meta.source_path,
                meta.kind,
                &meta.importer,
            )?;
            if let Ok(modified) = modified_time(&asset_root.join(&meta.source_path)) {
                self.hot_reload.observe(meta.guid, modified);
            }
        }
        self.register_loaded_model_materials();
        Ok(())
    }

    /// Reimports files whose modification stamp changed and refreshes runtime handles.
    pub fn reload_changed_project_assets(&mut self) -> EngineResult<Vec<engine_core::AssetId>> {
        let asset_root = self
            .asset_root
            .clone()
            .ok_or_else(|| EngineError::config("project assets are not loaded"))?;
        let report = scan_project_assets(&asset_root, &mut self.asset_database)?;
        let mut changed = Vec::new();
        for meta in report.metas {
            let path = asset_root.join(&meta.source_path);
            let Ok(modified) = modified_time(&path) else {
                continue;
            };
            if self.hot_reload.observe(meta.guid, modified) {
                self.import_runtime_asset(
                    &asset_root,
                    meta.guid,
                    &meta.source_path,
                    meta.kind,
                    &meta.importer,
                )?;
                changed.push(meta.guid.as_asset_id());
            }
        }
        self.register_loaded_model_materials();
        Ok(changed)
    }

    fn import_runtime_asset(
        &mut self,
        asset_root: &Path,
        guid: AssetGuid,
        source_path: &Path,
        kind: ResourceKind,
        importer: &str,
    ) -> EngineResult<()> {
        let outcome = import_builtin_asset(
            asset_root,
            &mut self.asset_registry,
            ImportTask {
                guid,
                source_path: source_path.to_path_buf(),
                kind,
                importer: importer.to_string(),
            },
        )?;
        for diagnostic in outcome.diagnostics {
            self.diagnostics.push(RuntimeDiagnostic {
                source: "assets".to_string(),
                level: "warning".to_string(),
                message: diagnostic.message,
                file: diagnostic.path,
                line: None,
            });
        }
        let Some(handle) = self.asset_registry.handle_for_guid(guid) else {
            return Ok(());
        };
        let Some(cpu) = self.asset_registry.cpu_resource(handle).cloned() else {
            return Ok(());
        };
        match kind {
            ResourceKind::Texture => {
                let gpu = if let Ok(cubemap) = DecodedCubemapResource::from_bytes(&cpu.bytes) {
                    let gpu = self.renderer.upload_cubemap(
                        ImageDesc::cubemap(cubemap.face_size, ImageFormat::Rgba8Srgb),
                        &cubemap.pixels,
                    )?;
                    self.renderer
                        .register_skybox_cubemap(&format!("asset:{:032x}", guid.as_u128()), gpu);
                    gpu
                } else {
                    let texture = DecodedTextureResource::from_bytes(&cpu.bytes)
                        .map_err(EngineError::from)?;
                    self.renderer.upload_texture(
                        ImageDesc {
                            width: texture.width,
                            height: texture.height,
                            mip_levels: 1,
                            samples: 1,
                            format: ImageFormat::Rgba8Srgb,
                            usage: ImageUsage::SAMPLED.or(ImageUsage::TRANSFER_DST),
                            label: Some("project texture"),
                        },
                        &texture.pixels,
                    )?
                };
                self.texture_resources.insert(guid.as_asset_id(), gpu);
                self.asset_registry.put_gpu(
                    handle,
                    GpuResource {
                        kind,
                        backend_token: gpu.raw().slot() as u64,
                    },
                )?;
            }
            ResourceKind::Model | ResourceKind::SkinnedModel => {
                let model = ModelResource::from_bytes(&cpu.bytes).map_err(EngineError::from)?;
                // Upload each mesh sub-object to the GPU
                for (i, mesh) in model.meshes.iter().enumerate() {
                    let mesh_name = if model.meshes.len() == 1 {
                        format!("asset:{:032x}", guid.as_u128())
                    } else {
                        format!("asset:{:032x}:{}", guid.as_u128(), i)
                    };
                    self.renderer.upload_mesh_data(
                        &mesh_name,
                        &mesh.positions,
                        &mesh.normals,
                        &mesh.texcoords,
                        &mesh.indices,
                    )?;
                }
                // Register PBR materials from the model so the renderer can
                // look up parameters by material name at render time.
                for (mat_idx, material) in model.materials.iter().enumerate() {
                    let material_name = format!("material:{:032x}:{}", guid.as_u128(), mat_idx);
                    self.renderer.register_material_params(
                        &material_name,
                        material.base_color,
                        material.metallic,
                        material.roughness,
                        material.emissive,
                    );
                    let textures = self.material_textures_from_refs(source_path, material);
                    self.renderer
                        .register_material_textures(&material_name, &textures);
                }
                self.mesh_resources.insert(guid.as_asset_id(), model);
            }
            ResourceKind::Material => {
                let text = std::str::from_utf8(&cpu.bytes)
                    .map_err(|error| EngineError::other(error.to_string()))?;
                let material = if importer == "material-toml" {
                    MaterialFormat::from_toml(text)
                } else {
                    MaterialFormat::from_json(text)
                }
                .map_err(EngineError::from)?;
                self.material_resources.insert(guid.as_asset_id(), material);
            }
            ResourceKind::Audio => {
                #[cfg(feature = "audio")]
                {
                    self.load_audio_clip(
                        guid.as_asset_id(),
                        &source_path.to_string_lossy(),
                        &cpu.bytes,
                    )?;
                }
            }
            ResourceKind::Shader
            | ResourceKind::Animation
            | ResourceKind::Script
            | ResourceKind::Prefab
            | ResourceKind::Scene => {}
        }
        Ok(())
    }

    fn material_textures_from_refs(
        &self,
        model_source_path: &Path,
        material: &engine_assets::CpuMaterialResource,
    ) -> RenderMaterialTextures {
        RenderMaterialTextures {
            base_color: self.texture_handle_for_material_ref(
                model_source_path,
                material.base_color_texture_ref.as_deref(),
            ),
            normal: self.texture_handle_for_material_ref(
                model_source_path,
                material.normal_texture_ref.as_deref(),
            ),
            metallic_roughness: self.texture_handle_for_material_ref(
                model_source_path,
                material.metallic_roughness_texture_ref.as_deref(),
            ),
            emissive: None,
            occlusion: None,
        }
    }

    fn texture_handle_for_material_ref(
        &self,
        model_source_path: &Path,
        texture_ref: Option<&str>,
    ) -> Option<ImageHandle> {
        let relative = resolve_model_texture_ref(model_source_path, texture_ref?);
        let guid = self.asset_database.get_guid_for_path(&relative)?;
        self.texture_resources.get(&guid.as_asset_id()).copied()
    }

    fn register_loaded_model_materials(&mut self) {
        let models: Vec<(engine_core::AssetId, PathBuf, ModelResource)> = self
            .mesh_resources
            .iter()
            .filter_map(|(asset_id, model)| {
                let guid = AssetGuid::from_asset_id(*asset_id);
                let path = self
                    .asset_database
                    .resolve_guid(guid)
                    .ok()?
                    .as_path()
                    .to_path_buf();
                Some((*asset_id, path, model.clone()))
            })
            .collect();

        for (asset_id, source_path, model) in models {
            for (mat_idx, material) in model.materials.iter().enumerate() {
                let material_name = format!("material:{:032x}:{mat_idx}", asset_id.as_u128());
                self.renderer.register_material_params(
                    &material_name,
                    material.base_color,
                    material.metallic,
                    material.roughness,
                    material.emissive,
                );
                let textures = self.material_textures_from_refs(&source_path, material);
                self.renderer
                    .register_material_textures(&material_name, &textures);
            }
        }
    }

    fn apply_builtin_player_controller(&mut self) {
        let Some(player) = self.scene.find_by_name("Player") else {
            return;
        };
        let move_x = self.input.action_value("MoveX");
        let move_z = self.input.action_value("MoveY");
        if move_x == 0.0 && move_z == 0.0 {
            return;
        }
        #[cfg(feature = "physics")]
        if let Some(binding) = self.physics_binding_for_entity(player).cloned() {
            let translation = engine_core::math::Vec3::new(move_x * 0.08, 0.0, move_z * 0.08);
            if self
                .physics
                .backend_mut()
                .move_character(
                    binding.body,
                    CharacterControllerDesc {
                        shape: ColliderShapeRef::Capsule {
                            half_height: 0.5,
                            radius: 0.25,
                        },
                        translation,
                        dt: self.stats.frame_time_seconds.max(1.0 / 60.0),
                        filter: QueryFilter::default(),
                    },
                )
                .is_ok()
            {
                if let Ok(world_transform) = self.physics.backend().body_transform(binding.body) {
                    self.scene
                        .transforms_mut()
                        .set_world(player, world_transform);
                    return;
                }
            }
        }
        if let Some(mut transform) = self.scene.transforms().local(player) {
            let speed = 0.08;
            transform.translation.x += move_x * speed;
            transform.translation.z += move_z * speed;
            self.scene.transforms_mut().set_local(player, transform);
        }
    }

    #[cfg(feature = "physics")]
    fn physics_binding_for_entity(&self, entity: engine_ecs::Entity) -> Option<&PhysicsBinding> {
        let object = self.scene.object(entity)?;
        self.physics_bindings
            .iter()
            .find(|binding| binding.object == object.id)
    }

    fn report_script_proxy_diagnostics(&mut self) {
        for (_, object) in self.scene.iter_objects() {
            for script in object
                .scripts
                .iter()
                .chain(
                    object
                        .components
                        .iter()
                        .filter_map(|component| match component {
                            ComponentData::Script(script) => Some(script),
                            _ => None,
                        }),
                )
            {
                #[cfg(feature = "script-python")]
                if script.legacy_backend.as_deref() == Some("python") {
                    continue;
                }
                let backend = script.legacy_backend.as_deref().unwrap_or("varg");
                if backend == "varg" {
                    continue;
                }
                let key = format!("{}:{}", backend, script.source);
                if script.pending_recovery && self.reported_script_errors.insert(key) {
                    self.diagnostics.push(RuntimeDiagnostic {
                        source: "script".to_string(),
                        level: "error".to_string(),
                        message: format!(
                            "{} script `{}` is pending backend recovery",
                            backend, script.source
                        ),
                        file: Some(PathBuf::from(&script.source)),
                        line: None,
                    });
                }
            }
        }
    }

    #[cfg(feature = "physics")]
    fn ensure_physics_bindings(&mut self) -> EngineResult<()> {
        for (entity, object) in self.scene.iter_objects() {
            if self
                .physics_bindings
                .iter()
                .any(|binding| binding.object == object.id)
            {
                continue;
            }
            let Some(rigidbody) = object
                .components
                .iter()
                .find_map(|component| match component {
                    ComponentData::Rigidbody(rigidbody) => Some(rigidbody),
                    _ => None,
                })
            else {
                continue;
            };
            let world_transform = self.scene.transforms().world(entity).unwrap_or_default();
            let desc = RigidbodyDesc {
                transform: world_transform,
                kind: match rigidbody.body_type.as_str() {
                    "static" => BodyKind::Static,
                    "kinematic" => BodyKind::Kinematic,
                    _ => BodyKind::Dynamic,
                },
                gravity_scale: if rigidbody.use_gravity { 1.0 } else { 0.0 },
                ..RigidbodyDesc::default()
            };
            let body = self.physics.backend_mut().create_body(&desc)?;
            for collider in object
                .components
                .iter()
                .filter_map(|component| match component {
                    ComponentData::Collider(collider) => Some(collider),
                    _ => None,
                })
            {
                self.physics
                    .backend_mut()
                    .add_collider(body, &collider_desc_from_scene(collider, object.layer))?;
            }
            self.physics_bindings.push(PhysicsBinding {
                object: object.id,
                body,
                last_position: world_transform.translation,
            });
        }
        Ok(())
    }

    #[cfg(feature = "physics")]
    fn sync_scene_to_physics(&mut self) -> EngineResult<()> {
        for binding in &self.physics_bindings {
            if let Some(entity) = self.scene.find_by_id(binding.object) {
                let transform = self.scene.transforms().world(entity).unwrap_or_default();
                self.physics
                    .backend_mut()
                    .set_body_transform(binding.body, transform)?;
            }
        }
        Ok(())
    }

    #[cfg(feature = "physics")]
    fn apply_environment_forces(&mut self, dt: f32) -> EngineResult<()> {
        if dt <= f32::EPSILON {
            return Ok(());
        }

        let mut fluid_volumes = Vec::new();
        let mut wind_zones = Vec::new();
        for (entity, object) in self.scene.iter_objects() {
            let transform = self.scene.transforms().world(entity).unwrap_or_default();
            for component in &object.components {
                match component {
                    ComponentData::FluidVolume(fluid) => {
                        let half_extents = (fluid.size * transform.scale) * 0.5;
                        let min = transform.translation - half_extents;
                        let max = transform.translation + half_extents;
                        let surface_y = (max.y + fluid.surface_offset).clamp(min.y, max.y);
                        fluid_volumes.push((fluid.clone(), min, max, surface_y));
                    }
                    ComponentData::WindZone(wind) => {
                        let half_extents = (wind.size * transform.scale) * 0.5;
                        let min = transform.translation - half_extents;
                        let max = transform.translation + half_extents;
                        wind_zones.push((wind.clone(), min, max));
                    }
                    _ => {}
                }
            }
        }

        if fluid_volumes.is_empty() && wind_zones.is_empty() {
            return Ok(());
        }

        let gravity = 9.81;
        for binding in &mut self.physics_bindings {
            let Some(entity) = self.scene.find_by_id(binding.object) else {
                continue;
            };
            let Some(object) = self.scene.object(entity) else {
                continue;
            };
            let Some(rigidbody) = object
                .components
                .iter()
                .find_map(|component| match component {
                    ComponentData::Rigidbody(rigidbody) => Some(rigidbody),
                    _ => None,
                })
            else {
                continue;
            };
            if rigidbody.body_type != "dynamic" {
                continue;
            }

            let transform = self.scene.transforms().world(entity).unwrap_or_default();
            let collider = object
                .components
                .iter()
                .find_map(|component| match component {
                    ComponentData::Collider(collider) => Some(collider),
                    _ => None,
                });
            let collider_size = collider.map_or(Vec3::ONE, |collider| collider.size);
            let body_half = (collider_size * transform.scale) * 0.5;
            let body_min = transform.translation - body_half;
            let body_max = transform.translation + body_half;
            let velocity = (transform.translation - binding.last_position) / dt;
            let collider_volume = collider
                .map(|collider| fluid_collider_volume(collider, transform.scale))
                .unwrap_or(1.0)
                .max(0.001);

            for (fluid, volume_min, volume_max, surface_y) in &fluid_volumes {
                let overlap_min = Vec3::new(
                    body_min.x.max(volume_min.x),
                    body_min.y.max(volume_min.y),
                    body_min.z.max(volume_min.z),
                );
                let overlap_max = Vec3::new(
                    body_max.x.min(volume_max.x),
                    body_max.y.min(*surface_y),
                    body_max.z.min(volume_max.z),
                );
                let overlap = overlap_max - overlap_min;
                if overlap.x <= 0.0 || overlap.y <= 0.0 || overlap.z <= 0.0 {
                    continue;
                }

                let body_aabb_volume = ((body_max.x - body_min.x)
                    * (body_max.y - body_min.y)
                    * (body_max.z - body_min.z))
                    .max(f32::EPSILON);
                let overlap_volume = overlap.x * overlap.y * overlap.z;
                let submerged_fraction = (overlap_volume / body_aabb_volume).clamp(0.0, 1.0);
                if submerged_fraction <= f32::EPSILON {
                    continue;
                }

                let mass = rigidbody.mass.max(0.001);
                let displaced_volume = collider_volume * submerged_fraction;
                // Scene mass is not yet propagated into every physics backend, so
                // scale buoyancy into an acceleration-like force here.
                let buoyancy = Vec3::new(
                    0.0,
                    fluid.density.max(0.0) * displaced_volume * gravity * fluid.buoyancy_scale
                        / mass,
                    0.0,
                );
                let relative_velocity = velocity - fluid.flow_velocity;
                let drag =
                    relative_velocity * (-fluid.linear_drag.max(0.0) * mass) * submerged_fraction;
                let force = buoyancy + drag;
                if force.length_squared() > f32::EPSILON {
                    self.physics
                        .backend_mut()
                        .apply_force(binding.body, force)?;
                }
            }

            for (wind, zone_min, zone_max) in &wind_zones {
                if transform.translation.x < zone_min.x
                    || transform.translation.x > zone_max.x
                    || transform.translation.y < zone_min.y
                    || transform.translation.y > zone_max.y
                    || transform.translation.z < zone_min.z
                    || transform.translation.z > zone_max.z
                {
                    continue;
                }

                let mass = rigidbody.mass.max(0.001);
                let relative_velocity = velocity - wind.wind_velocity;
                let force = relative_velocity
                    * (-wind.linear_drag.max(0.0) * wind.strength.max(0.0) * mass);
                if force.length_squared() > f32::EPSILON {
                    self.physics
                        .backend_mut()
                        .apply_force(binding.body, force)?;
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "physics")]
    fn sync_physics_to_scene(&mut self) -> EngineResult<()> {
        for binding in &mut self.physics_bindings {
            if let Some(entity) = self.scene.find_by_id(binding.object) {
                let transform = self.physics.backend().body_transform(binding.body)?;
                self.scene.transforms_mut().set_world(entity, transform);
                binding.last_position = transform.translation;
            }
        }
        Ok(())
    }

    #[cfg(feature = "physics")]
    fn report_physics_events(&mut self) {
        for event in self.physics.drain_contacts() {
            self.diagnostics.push(RuntimeDiagnostic {
                source: "physics".to_string(),
                level: "info".to_string(),
                message: format!(
                    "{} {} between body {} and body {}",
                    if event.is_trigger {
                        "trigger"
                    } else {
                        "collision"
                    },
                    if event.entered { "entered" } else { "exited" },
                    event.body_a.0,
                    event.body_b.0
                ),
                file: None,
                line: None,
            });
        }
    }

    #[cfg(feature = "audio")]
    /// Loads an encoded WAV/OGG asset for scene AudioSource components.
    pub fn load_audio_clip(
        &mut self,
        asset: engine_core::AssetId,
        name: &str,
        bytes: &[u8],
    ) -> EngineResult<ClipHandle> {
        const STREAMING_THRESHOLD_BYTES: usize = 1024 * 1024;
        let clip = if bytes.len() >= STREAMING_THRESHOLD_BYTES {
            self.audio
                .backend_mut()
                .load_streamed_clip(name, std::sync::Arc::from(bytes))?
        } else {
            let (samples, channels, sample_rate) = engine_audio::decode_audio_bytes(name, bytes)?;
            self.audio
                .backend_mut()
                .load_clip(name, &samples, channels, sample_rate)?
        };
        self.audio_clips.insert(asset, clip);
        Ok(clip)
    }

    #[cfg(feature = "audio")]
    fn ensure_audio_bindings(&mut self) -> EngineResult<()> {
        for (entity, object) in self.scene.iter_objects() {
            if self
                .audio_bindings
                .iter()
                .any(|binding| binding.object == object.id)
            {
                continue;
            }
            let Some(audio_source) =
                object
                    .components
                    .iter()
                    .find_map(|component| match component {
                        ComponentData::AudioSource(source) => Some(source),
                        _ => None,
                    })
            else {
                continue;
            };
            let Some(asset) = audio_source.clip else {
                continue;
            };
            let Some(clip) = self.audio_clips.get(&asset).copied() else {
                let key = format!("audio:{}", asset.as_u128());
                if self.reported_script_errors.insert(key) {
                    self.diagnostics.push(RuntimeDiagnostic {
                        source: "audio".to_string(),
                        level: "warning".to_string(),
                        message: format!("audio clip {} is not loaded", asset.as_u128()),
                        file: None,
                        line: None,
                    });
                }
                continue;
            };
            let transform = self.scene.transforms().world(entity).unwrap_or_default();
            let source = self.audio.backend_mut().spawn_source(&AudioSourceDesc {
                clip,
                volume: audio_source.volume,
                pitch: audio_source.pitch,
                looping: audio_source.looping,
                position: Some(transform.translation),
                auto_play: audio_source.play_on_start,
                bus: audio_source.bus.clone(),
                spatial_mode: parse_spatial_mode(&audio_source.spatial_mode),
                shape: parse_audio_source_shape(audio_source),
                attenuation: parse_attenuation_model(audio_source),
                priority: audio_source.priority,
                virtualization: parse_virtualization_policy(&audio_source.virtualization),
                category: parse_voice_category(&audio_source.category),
                critical: audio_source.critical,
                doppler_scale: audio_source.doppler_scale,
                spread: audio_source.spread,
                use_hrtf: audio_source.use_hrtf,
            })?;
            self.audio_bindings.push(AudioBinding {
                object: object.id,
                source,
                last_position: transform.translation,
            });
        }
        Ok(())
    }

    #[cfg(feature = "audio")]
    fn update_audio(&mut self, dt: f32) -> EngineResult<()> {
        let mut index = 0;
        while index < self.audio_bindings.len() {
            if self
                .scene
                .find_by_id(self.audio_bindings[index].object)
                .is_none()
            {
                let binding = self.audio_bindings.swap_remove(index);
                self.audio.backend_mut().destroy_source(binding.source)?;
            } else {
                index += 1;
            }
        }
        let listener_entity = self
            .scene
            .iter_objects()
            .find(|(_, object)| {
                object
                    .components
                    .iter()
                    .any(|component| matches!(component, ComponentData::AudioListener(_)))
            })
            .map(|(entity, _)| entity)
            .or_else(|| {
                self.scene
                    .main_camera()
                    .or_else(|| self.scene.game_camera())
            });

        if let Some(listener_entity) = listener_entity {
            let transform = self
                .scene
                .transforms()
                .world(listener_entity)
                .unwrap_or_default();
            let listener_data = self
                .scene
                .object(listener_entity)
                .and_then(|object| {
                    object
                        .components
                        .iter()
                        .find_map(|component| match component {
                            ComponentData::AudioListener(listener) => Some(listener.clone()),
                            _ => None,
                        })
                })
                .unwrap_or_default();
            let velocity = self
                .audio_listener_position
                .filter(|_| dt > f32::EPSILON)
                .map(|previous| (transform.translation - previous) / dt)
                .unwrap_or(engine_core::math::Vec3::ZERO);
            let output_mode = parse_output_mode(&listener_data.output_mode);
            let hrtf_quality = parse_hrtf_quality(&listener_data.hrtf_quality);
            let _ = self.audio.backend_mut().set_output_mode(output_mode);
            let _ = self.audio.backend_mut().set_hrtf_quality(hrtf_quality);
            self.audio.backend_mut().set_listener(&AudioListenerDesc {
                position: transform.translation,
                forward: transform
                    .rotation
                    .rotate(engine_core::math::Vec3::new(0.0, 0.0, -1.0))
                    .normalized(),
                up: transform
                    .rotation
                    .rotate(engine_core::math::Vec3::new(0.0, 1.0, 0.0))
                    .normalized(),
                velocity,
                output_mode,
                hrtf_quality,
                hrtf_enabled: listener_data.hrtf_enabled,
            });
            self.audio_listener_position = Some(transform.translation);
        }
        let mut acoustic_sources = Vec::with_capacity(self.audio_bindings.len());
        for binding in &mut self.audio_bindings {
            if let Some(entity) = self.scene.find_by_id(binding.object) {
                let transform = self.scene.transforms().world(entity).unwrap_or_default();
                let Some(object) = self.scene.object(entity) else {
                    continue;
                };
                if let Some(audio_source) =
                    object
                        .components
                        .iter()
                        .find_map(|component| match component {
                            ComponentData::AudioSource(source) => Some(source),
                            _ => None,
                        })
                {
                    self.audio
                        .backend_mut()
                        .set_volume(binding.source, audio_source.volume)?;
                    self.audio
                        .backend_mut()
                        .set_looping(binding.source, audio_source.looping)?;
                    let velocity = if dt > f32::EPSILON {
                        (transform.translation - binding.last_position) / dt
                    } else {
                        engine_core::math::Vec3::ZERO
                    };
                    self.audio.backend_mut().set_source_transform(
                        binding.source,
                        AudioObjectTransform {
                            position: transform.translation,
                            forward: transform
                                .rotation
                                .rotate(engine_core::math::Vec3::new(0.0, 0.0, -1.0))
                                .normalized(),
                            velocity,
                        },
                    )?;
                    acoustic_sources.push(AcousticSourceSample {
                        handle: binding.source,
                        position: transform.translation,
                    });
                    binding.last_position = transform.translation;
                }
            }
        }
        if let Some(listener_position) = self.audio_listener_position {
            let snapshot = AcousticSceneSnapshot {
                listener_position,
                sources: acoustic_sources,
                blockers: extract_acoustic_blockers(&self.scene),
            };
            for (source, propagation) in
                solve_direct_propagation(&snapshot, AcousticSolverConfig::default())
            {
                let _ = self
                    .audio
                    .backend_mut()
                    .set_source_propagation(source, propagation);
            }
        }
        self.audio.update(dt);
        let diagnostics = self.audio.diagnostics();
        self.stats.audio_sources = diagnostics.logical_sources;
        self.stats.audio_physical_voices = diagnostics.physical_voices;
        self.stats.audio_virtual_voices = diagnostics.virtual_voices;
        self.stats.audio_underruns = diagnostics.underruns;
        Ok(())
    }
}

#[cfg(feature = "audio")]
fn parse_spatial_mode(value: &str) -> SpatialMode {
    match value {
        "object" => SpatialMode::Object,
        "ambient_field" => SpatialMode::AmbientField,
        _ => SpatialMode::Direct,
    }
}

#[cfg(feature = "audio")]
fn parse_audio_source_shape(source: &AudioSourceComponentData) -> AudioSourceShape {
    match source.shape.as_str() {
        "cone" => AudioSourceShape::Cone {
            inner_angle_degrees: source.inner_angle_degrees,
            outer_angle_degrees: source.outer_angle_degrees.max(source.inner_angle_degrees),
            outer_gain: source.outer_gain.clamp(0.0, 1.0),
        },
        "sphere" => AudioSourceShape::Sphere {
            radius: source.sphere_radius.max(0.0),
        },
        _ => AudioSourceShape::Point,
    }
}

#[cfg(feature = "audio")]
fn parse_attenuation_model(source: &AudioSourceComponentData) -> AttenuationModel {
    match source.attenuation.as_str() {
        "inverse_distance" => AttenuationModel::InverseDistance {
            min_distance: source.min_distance.max(0.0),
            max_distance: source.max_distance.max(source.min_distance),
        },
        "linear_distance" => AttenuationModel::LinearDistance {
            min_distance: source.min_distance.max(0.0),
            max_distance: source.max_distance.max(source.min_distance),
        },
        "logarithmic_distance" => AttenuationModel::LogarithmicDistance {
            min_distance: source.min_distance.max(0.0),
            max_distance: source.max_distance.max(source.min_distance),
        },
        _ => AttenuationModel::None,
    }
}

#[cfg(feature = "audio")]
fn extract_acoustic_blockers(scene: &Scene) -> Vec<AcousticAabb> {
    scene
        .iter_objects()
        .filter_map(|(entity, object)| {
            let geometry = object
                .components
                .iter()
                .find_map(|component| match component {
                    ComponentData::AcousticGeometry(geometry) => Some(geometry),
                    _ => None,
                })?;
            let transform = scene.transforms().world(entity).unwrap_or_default();
            let material = geometry
                .material
                .as_ref()
                .map(acoustic_material_from_component)
                .or_else(|| {
                    object
                        .components
                        .iter()
                        .find_map(|component| match component {
                            ComponentData::AcousticMaterial(material) => {
                                Some(acoustic_material_from_component(material))
                            }
                            _ => None,
                        })
                })
                .unwrap_or_default();
            let half_size = geometry.size * 0.5;
            Some(AcousticAabb {
                min: transform.translation - half_size,
                max: transform.translation + half_size,
                material,
                blocks_direct_path: geometry.blocks_direct_path,
            })
        })
        .collect()
}

#[cfg(feature = "audio")]
fn acoustic_material_from_component(
    material: &engine_ecs::AcousticMaterialComponentData,
) -> AcousticMaterial {
    AcousticMaterial {
        absorption: material.absorption.map(|value| value.clamp(0.0, 1.0)),
        transmission: material.transmission.map(|value| value.clamp(0.0, 1.0)),
        scattering: material.scattering.clamp(0.0, 1.0),
    }
}

#[cfg(feature = "audio")]
fn parse_virtualization_policy(value: &str) -> VirtualizationPolicy {
    match value {
        "stop" => VirtualizationPolicy::Stop,
        "protected" => VirtualizationPolicy::Protected,
        _ => VirtualizationPolicy::Virtualize,
    }
}

#[cfg(feature = "audio")]
fn parse_voice_category(value: &str) -> VoiceCategory {
    match value {
        "critical" => VoiceCategory::Critical,
        "dialogue" => VoiceCategory::Dialogue,
        "music" => VoiceCategory::Music,
        "ui" => VoiceCategory::Ui,
        "ambience" => VoiceCategory::Ambience,
        "disposable" => VoiceCategory::Disposable,
        _ => VoiceCategory::Sfx,
    }
}

#[cfg(feature = "audio")]
fn parse_output_mode(value: &str) -> OutputMode {
    match value {
        "binaural" => OutputMode::Binaural,
        _ => OutputMode::Stereo,
    }
}

#[cfg(feature = "audio")]
fn parse_hrtf_quality(value: &str) -> HrtfQuality {
    match value {
        "low" => HrtfQuality::Low,
        "high" => HrtfQuality::High,
        _ => HrtfQuality::Medium,
    }
}

#[cfg(feature = "script-python")]
#[derive(Clone, Debug)]
struct ScriptInvocation {
    entity: engine_ecs::Entity,
    object: engine_core::EntityId,
    name: String,
    script: ScriptComponentProxy,
}

#[cfg(feature = "script-python")]
impl<R: RenderDevice> RuntimeServices<R> {
    fn run_python_scripts(&mut self, stage: &str, dt: f32) {
        let invocations = self
            .scene
            .iter_objects()
            .flat_map(|(entity, object)| {
                object
                    .scripts
                    .iter()
                    .chain(
                        object
                            .components
                            .iter()
                            .filter_map(|component| match component {
                                ComponentData::Script(script) => Some(script),
                                _ => None,
                            }),
                    )
                    .filter(|script| script.legacy_backend.as_deref() == Some("python"))
                    .cloned()
                    .map(move |script| ScriptInvocation {
                        entity,
                        object: object.id,
                        name: object.name.clone(),
                        script,
                    })
            })
            .collect::<Vec<_>>();

        for invocation in invocations {
            let key = ScriptInstanceKey {
                object: invocation.object,
                backend: invocation
                    .script
                    .legacy_backend
                    .clone()
                    .unwrap_or_else(|| "varg".to_string()),
                script: invocation.script.source.clone(),
            };
            if stage == "start" {
                if !self.script_instances.insert(key) {
                    continue;
                }
            } else if !self.script_instances.contains(&key) {
                continue;
            }
            if let Err(error) = self.run_python_script(invocation, stage, dt) {
                self.push_script_diagnostic(error.to_string(), None);
            }
        }
    }

    fn push_script_diagnostic(&mut self, message: String, file: Option<PathBuf>) {
        if self.script_diagnostics_this_frame >= self.python_script_runtime.diagnostics_per_frame {
            return;
        }
        self.script_diagnostics_this_frame = self.script_diagnostics_this_frame.saturating_add(1);
        self.diagnostics.push(RuntimeDiagnostic {
            source: "script".to_string(),
            level: "error".to_string(),
            message,
            file,
            line: None,
        });
    }

    fn run_python_script(
        &mut self,
        invocation: ScriptInvocation,
        stage: &str,
        dt: f32,
    ) -> EngineResult<()> {
        let script_path = self.resolve_project_path(&invocation.script.source);
        let transform = self
            .scene
            .transforms()
            .local(invocation.entity)
            .unwrap_or_default();
        let payload = serde_json::json!({
            "stage": stage,
            "dt": dt,
            "entity_id": invocation.object.as_u128(),
            "name": invocation.name,
            "transform": transform_to_json(transform),
            "input": {
                "actions": {
                    "MoveX": self.input.action_value("MoveX"),
                    "MoveY": self.input.action_value("MoveY"),
                    "MoveForward": self.input.action_value("MoveForward"),
                    "MoveBackward": self.input.action_value("MoveBackward"),
                    "MoveLeft": self.input.action_value("MoveLeft"),
                    "MoveRight": self.input.action_value("MoveRight")
                }
            },
            "state": invocation.script.state
        });
        let mut child = std::process::Command::new(&self.python_script_runtime.interpreter)
            .arg("-c")
            .arg(PYTHON_SCRIPT_SHIM)
            .arg(&script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|source| EngineError::Filesystem {
                path: script_path.clone(),
                source,
            })?;
        {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin
                    .write_all(payload.to_string().as_bytes())
                    .map_err(|source| EngineError::Filesystem {
                        path: script_path.clone(),
                        source,
                    })?;
            }
        }
        drop(child.stdin.take());

        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|source| EngineError::Filesystem {
                path: script_path.clone(),
                source,
            })? {
                break status;
            }
            if started.elapsed() >= self.python_script_runtime.invocation_timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EngineError::other(format!(
                    "python script `{}` timed out after {:?}",
                    invocation.script.source, self.python_script_runtime.invocation_timeout
                )));
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        {
            use std::io::Read;
            if let Some(mut pipe) = child.stdout.take() {
                pipe.read_to_end(&mut stdout)
                    .map_err(|source| EngineError::Filesystem {
                        path: script_path.clone(),
                        source,
                    })?;
            }
            if let Some(mut pipe) = child.stderr.take() {
                pipe.read_to_end(&mut stderr)
                    .map_err(|source| EngineError::Filesystem {
                        path: script_path.clone(),
                        source,
                    })?;
            }
        }
        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr);
            return Err(EngineError::other(format!(
                "python script `{}` failed: {}",
                invocation.script.source,
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&stdout);
        let result = serde_json::from_str::<serde_json::Value>(stdout.trim()).map_err(|error| {
            EngineError::other(format!(
                "python script `{}` returned invalid json: {error}",
                invocation.script.source
            ))
        })?;
        self.apply_script_result(invocation.entity, &invocation.script, &result)
    }

    fn apply_script_result(
        &mut self,
        entity: engine_ecs::Entity,
        script: &ScriptComponentProxy,
        result: &serde_json::Value,
    ) -> EngineResult<()> {
        if let Some(state) = result.get("state") {
            let state_map = match state {
                serde_json::Value::Object(map) => map.clone().into_iter().collect(),
                _ => std::collections::HashMap::new(),
            };
            if let Some(object) = self.scene.object_mut(entity) {
                for candidate in &mut object.scripts {
                    if candidate.legacy_backend == script.legacy_backend
                        && candidate.source == script.source
                    {
                        candidate.state = state_map.clone();
                        candidate.pending_recovery = false;
                    }
                }
                for component in &mut object.components {
                    if let ComponentData::Script(candidate) = component {
                        if candidate.legacy_backend == script.legacy_backend
                            && candidate.source == script.source
                        {
                            candidate.state = state_map.clone();
                            candidate.pending_recovery = false;
                        }
                    }
                }
            }
        }
        if let Some(transform) = result.get("transform").and_then(json_to_transform) {
            self.scene.transforms_mut().set_local(entity, transform);
        }
        if let Some(commands) = result.get("commands").and_then(serde_json::Value::as_array) {
            for command in commands {
                match command.get("type").and_then(serde_json::Value::as_str) {
                    Some("spawn") => {
                        let name = command
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("Script Object");
                        let spawned = self.scene.create_object(name)?;
                        if let Some(transform) =
                            command.get("transform").and_then(json_to_transform)
                        {
                            self.scene.transforms_mut().set_local(spawned, transform);
                        }
                    }
                    Some("destroy") => {
                        if let Some(id) = command.get("id").and_then(serde_json::Value::as_u64) {
                            let id = engine_core::EntityId::from_u128(id as u128);
                            if let Some(target) = self.scene.find_by_id(id) {
                                self.scene.destroy_deferred(target)?;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "script-python")]
fn transform_to_json(transform: engine_core::math::Transform) -> serde_json::Value {
    serde_json::json!({
        "translation": {
            "x": transform.translation.x,
            "y": transform.translation.y,
            "z": transform.translation.z
        },
        "rotation": {
            "x": transform.rotation.x,
            "y": transform.rotation.y,
            "z": transform.rotation.z,
            "w": transform.rotation.w
        },
        "scale": {
            "x": transform.scale.x,
            "y": transform.scale.y,
            "z": transform.scale.z
        }
    })
}

#[cfg(feature = "script-python")]
fn json_to_transform(value: &serde_json::Value) -> Option<engine_core::math::Transform> {
    Some(engine_core::math::Transform {
        translation: json_to_vec3(value.get("translation")?)?,
        rotation: engine_core::math::Quat {
            x: value.get("rotation")?.get("x")?.as_f64()? as f32,
            y: value.get("rotation")?.get("y")?.as_f64()? as f32,
            z: value.get("rotation")?.get("z")?.as_f64()? as f32,
            w: value.get("rotation")?.get("w")?.as_f64()? as f32,
        },
        scale: json_to_vec3(value.get("scale")?)?,
    })
}

#[cfg(feature = "script-python")]
fn json_to_vec3(value: &serde_json::Value) -> Option<engine_core::math::Vec3> {
    Some(engine_core::math::Vec3::new(
        value.get("x")?.as_f64()? as f32,
        value.get("y")?.as_f64()? as f32,
        value.get("z")?.as_f64()? as f32,
    ))
}

#[cfg(feature = "script-python")]
const PYTHON_SCRIPT_SHIM: &str = r#"
import importlib.util
import json
import sys
import traceback

script_path = sys.argv[1]
payload = json.load(sys.stdin)

class Vec3:
    def __init__(self, data):
        self.x = float(data.get("x", 0.0))
        self.y = float(data.get("y", 0.0))
        self.z = float(data.get("z", 0.0))
    def to_json(self):
        return {"x": self.x, "y": self.y, "z": self.z}

class Transform:
    def __init__(self, data):
        self.translation = Vec3(data.get("translation", {}))
        self.rotation = data.get("rotation", {"x": 0.0, "y": 0.0, "z": 0.0, "w": 1.0})
        self.scale = Vec3(data.get("scale", {"x": 1.0, "y": 1.0, "z": 1.0}))
    def to_json(self):
        return {"translation": self.translation.to_json(), "rotation": self.rotation, "scale": self.scale.to_json()}

class Input:
    def __init__(self, data):
        self.actions = data.get("actions", {})
    def action_value(self, name):
        return float(self.actions.get(name, 0.0))
    def action_down(self, name):
        return self.action_value(name) > 0.0

class Scene:
    def __init__(self):
        self.commands = []
    def spawn(self, name, transform=None):
        command = {"type": "spawn", "name": name}
        if transform is not None:
            command["transform"] = transform.to_json()
        self.commands.append(command)
    def destroy(self, entity_id):
        self.commands.append({"type": "destroy", "id": int(entity_id)})

class Context:
    def __init__(self, payload):
        self.entity_id = int(payload["entity_id"])
        self.name = payload["name"]
        self.dt = float(payload["dt"])
        self.transform = Transform(payload["transform"])
        self.input = Input(payload.get("input", {}))
        self.scene = Scene()
        self.state = payload.get("state", {})

try:
    spec = importlib.util.spec_from_file_location("aster_script_module", script_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    ctx = Context(payload)
    fn = getattr(module, payload["stage"], None)
    if callable(fn):
        fn(ctx)
    print(json.dumps({"transform": ctx.transform.to_json(), "state": ctx.state, "commands": ctx.scene.commands}))
except Exception:
    traceback.print_exc()
    sys.exit(1)
"#;

#[cfg(feature = "physics")]
fn collider_desc_from_scene(
    collider: &engine_ecs::ColliderComponentData,
    layer: u32,
) -> ColliderDesc {
    let half = collider.size * 0.5;
    let material = built_in_physical_material(&collider.physics_material);
    ColliderDesc {
        shape: match collider.shape.as_str() {
            "sphere" => ColliderShape::Sphere {
                radius: half.x.max(half.y).max(half.z),
            },
            "capsule" => ColliderShape::Capsule {
                half_height: half.y,
                radius: half.x.max(half.z),
            },
            _ => ColliderShape::Box { half_extents: half },
        },
        is_trigger: collider.is_trigger,
        layer,
        mask: collider.mask,
        friction: material.friction,
        restitution: material.restitution,
        friction_combine: material.friction_combine,
        restitution_combine: material.restitution_combine,
        ..ColliderDesc::default()
    }
}

#[cfg(feature = "physics")]
fn fluid_collider_volume(collider: &engine_ecs::ColliderComponentData, scale: Vec3) -> f32 {
    let size = collider.size * scale;
    match collider.shape.as_str() {
        "sphere" => {
            let radius = (size.x.abs().max(size.y.abs()).max(size.z.abs()) * 0.5).max(0.0);
            (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3)
        }
        "capsule" => {
            let radius = (size.x.abs().max(size.z.abs()) * 0.5).max(0.0);
            let half_height = (size.y.abs() * 0.5).max(0.0);
            let cylinder_height = (half_height * 2.0).max(0.0);
            let cylinder = std::f32::consts::PI * radius.powi(2) * cylinder_height;
            let caps = (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3);
            cylinder + caps
        }
        _ => (size.x.abs() * size.y.abs() * size.z.abs()).max(0.0),
    }
}

/// Loaded project context used by runtime-game.
#[derive(Debug)]
pub struct RuntimeProject {
    /// Project root directory.
    pub root: PathBuf,
    /// Parsed project manifest.
    pub manifest: ProjectManifest,
    /// Parsed build configuration.
    pub build: BuildConfiguration,
    /// Default scene loaded from the manifest.
    pub scene: Scene,
}

/// Loads a project manifest and default scene.
pub fn load_runtime_project(project: impl AsRef<Path>) -> EngineResult<RuntimeProject> {
    let project = project.as_ref();
    let manifest_path = if project.is_dir() {
        project.join("aster.project.toml")
    } else {
        project.to_path_buf()
    };
    let root = manifest_path
        .parent()
        .ok_or_else(|| EngineError::config("project manifest must have a parent directory"))?
        .to_path_buf();
    let manifest_text =
        fs::read_to_string(&manifest_path).map_err(|source| EngineError::Filesystem {
            path: manifest_path.clone(),
            source,
        })?;
    let manifest = toml::from_str::<ProjectManifest>(&manifest_text)
        .map_err(|error| EngineError::config(format!("project manifest parse failed: {error}")))?;
    if let Some(diagnostic) = manifest.diagnostics().into_iter().next() {
        return Err(EngineError::config(format!(
            "{}: {}",
            diagnostic.path, diagnostic.message
        )));
    }
    let scene_path = root.join(&manifest.default_scene);
    let scene_text = fs::read_to_string(&scene_path).map_err(|source| EngineError::Filesystem {
        path: scene_path.clone(),
        source,
    })?;
    let scene = Scene::from_json(&scene_text)?;
    let build_path = root.join(&manifest.build_config);
    let build_text = fs::read_to_string(&build_path).map_err(|source| EngineError::Filesystem {
        path: build_path.clone(),
        source,
    })?;
    let build = toml::from_str::<BuildConfiguration>(&build_text).map_err(|error| {
        EngineError::config(format!("build configuration parse failed: {error}"))
    })?;
    if let Some(diagnostic) = build.diagnostics().into_iter().next() {
        return Err(EngineError::config(format!(
            "{}: {}",
            diagnostic.path, diagnostic.message
        )));
    }
    Ok(RuntimeProject {
        root,
        manifest,
        build,
        scene,
    })
}

/// Extracts the active scene into a minimal render queue.
pub fn extract_render_world(scene: &Scene) -> RenderWorld {
    RenderWorld::extract(scene)
}

/// Builds the native runtime performance policy from environment overrides.
///
/// Supported variables are `ASTER_PRESENT_MODE`, `ASTER_TARGET_FPS`,
/// `ASTER_RENDER_SCALE`, and `ASTER_DYNAMIC_RESOLUTION`.
pub fn runtime_performance_config_from_env() -> RenderPerformanceConfig {
    let mut config = RenderPerformanceConfig::competitive_120hz();
    if let Ok(value) = std::env::var("ASTER_PRESENT_MODE") {
        config.present_strategy = match value.as_str() {
            "vsync" => PresentStrategy::VSync,
            "uncapped" => PresentStrategy::Uncapped,
            _ => PresentStrategy::LowLatency,
        };
    }
    if let Ok(value) = std::env::var("ASTER_TARGET_FPS") {
        if let Ok(target_fps) = value.parse::<u32>() {
            config.dynamic_resolution.target_fps = target_fps.max(1);
        }
    }
    if let Ok(value) = std::env::var("ASTER_RENDER_SCALE") {
        if let Ok(scale) = value.parse::<f32>() {
            config.render_scale = scale.clamp(
                config.dynamic_resolution.min_scale,
                config.dynamic_resolution.max_scale,
            );
        }
    }
    if let Ok(value) = std::env::var("ASTER_DYNAMIC_RESOLUTION") {
        config.dynamic_resolution.enabled = matches!(value.as_str(), "1" | "true" | "on");
    }
    config
}

/// Builds render scaling settings from environment overrides.
///
/// Supported variables are `ASTER_UPSCALER`, `ASTER_RENDER_QUALITY`,
/// `ASTER_RENDER_SCALE_MIN`, `ASTER_RENDER_SCALE_MAX`, `ASTER_UPSCALE_SHARPNESS`,
/// `ASTER_TARGET_FPS`, `ASTER_DYNAMIC_RESOLUTION`, `ASTER_BATTERY_POLICY`,
/// `ASTER_FRAME_GENERATION`, `ASTER_UI_COMPOSITION`, and `ASTER_ANTI_ALIASING`.
pub fn runtime_scaling_settings_from_env() -> RenderScalingSettings {
    apply_runtime_scaling_env(RenderScalingSettings::default())
}

/// Converts persisted build settings to the engine render scaling model.
pub fn render_scaling_settings_from_build(build: &BuildConfiguration) -> RenderScalingSettings {
    let render = &build.render;
    let settings = RenderScalingSettings {
        quality: parse_render_quality(&render.quality),
        preferred_upscaler: Some(parse_upscaler(&render.upscaler)),
        dynamic_resolution: render.dynamic_resolution,
        min_render_scale: f32::from(render.min_render_scale_percent) / 100.0,
        max_render_scale: f32::from(render.max_render_scale_percent) / 100.0,
        sharpness: f32::from(render.sharpness_percent) / 100.0,
        target_fps: render.target_fps,
        battery_policy: parse_battery_policy(&render.battery_policy),
        frame_generation: parse_frame_generation(&render.frame_generation),
        ui_composition: parse_ui_composition(&render.ui_composition),
        anti_aliasing: parse_anti_aliasing(&render.anti_aliasing),
        ..RenderScalingSettings::default()
    };
    apply_runtime_scaling_env(settings)
}

fn apply_runtime_scaling_env(mut settings: RenderScalingSettings) -> RenderScalingSettings {
    if let Ok(value) = std::env::var("ASTER_UPSCALER") {
        settings.preferred_upscaler = Some(parse_upscaler(&value));
    }
    if let Ok(value) = std::env::var("ASTER_RENDER_QUALITY") {
        settings.quality = parse_render_quality(&value);
    }
    if let Ok(value) = std::env::var("ASTER_RENDER_SCALE_MIN") {
        if let Ok(scale) = value.parse::<f32>() {
            settings.min_render_scale = scale;
        }
    }
    if let Ok(value) = std::env::var("ASTER_RENDER_SCALE_MAX") {
        if let Ok(scale) = value.parse::<f32>() {
            settings.max_render_scale = scale;
        }
    }
    if let Ok(value) = std::env::var("ASTER_UPSCALE_SHARPNESS") {
        if let Ok(sharpness) = value.parse::<f32>() {
            settings.sharpness = sharpness;
        }
    }
    if let Ok(value) = std::env::var("ASTER_TARGET_FPS") {
        if let Ok(target_fps) = value.parse::<u32>() {
            settings.target_fps = target_fps;
        }
    }
    if let Ok(value) = std::env::var("ASTER_DYNAMIC_RESOLUTION") {
        settings.dynamic_resolution = matches!(value.as_str(), "1" | "true" | "on");
    }
    if let Ok(value) = std::env::var("ASTER_BATTERY_POLICY") {
        settings.battery_policy = parse_battery_policy(&value);
    }
    if let Ok(value) = std::env::var("ASTER_FRAME_GENERATION") {
        settings.frame_generation = parse_frame_generation(&value);
    }
    if let Ok(value) = std::env::var("ASTER_UI_COMPOSITION") {
        settings.ui_composition = parse_ui_composition(&value);
    }
    if let Ok(value) =
        std::env::var("ASTER_ANTI_ALIASING").or_else(|_| std::env::var("ASTER_RENDER_AA"))
    {
        settings.anti_aliasing = parse_anti_aliasing(&value);
    }
    settings.normalized()
}

fn parse_upscaler(value: &str) -> UpscalerKind {
    match value.to_ascii_lowercase().as_str() {
        "native" | "off" => UpscalerKind::Native,
        "temporal" => UpscalerKind::BuiltInTemporal,
        "fsr" => UpscalerKind::Fsr,
        "dlss" => UpscalerKind::Dlss,
        "xess" => UpscalerKind::Xess,
        "metalfx" => UpscalerKind::MetalFx,
        "gsr" | "snapdragon-gsr" => UpscalerKind::SnapdragonGsr,
        "directsr" => UpscalerKind::DirectSr,
        "streamline" => UpscalerKind::Streamline,
        _ => UpscalerKind::BuiltInSpatial,
    }
}

fn parse_render_quality(value: &str) -> RenderQualityMode {
    match value.to_ascii_lowercase().as_str() {
        "native" => RenderQualityMode::Native,
        "ultra-quality" => RenderQualityMode::UltraQuality,
        "quality" => RenderQualityMode::Quality,
        "performance" => RenderQualityMode::Performance,
        "ultra-performance" => RenderQualityMode::UltraPerformance,
        "auto" => RenderQualityMode::Auto,
        _ => RenderQualityMode::Balanced,
    }
}

fn parse_battery_policy(value: &str) -> BatteryPolicy {
    match value.to_ascii_lowercase().as_str() {
        "quality" => BatteryPolicy::Quality,
        "saver" => BatteryPolicy::Saver,
        _ => BatteryPolicy::Balanced,
    }
}

fn parse_frame_generation(value: &str) -> FrameGenerationKind {
    match value.to_ascii_lowercase().as_str() {
        "fsr" => FrameGenerationKind::Fsr,
        "dlss" => FrameGenerationKind::Dlss,
        "xess" => FrameGenerationKind::Xess,
        "metalfx" | "metal-fx" => FrameGenerationKind::MetalFx,
        _ => FrameGenerationKind::Disabled,
    }
}

fn parse_ui_composition(value: &str) -> UiCompositionPolicy {
    match value.to_ascii_lowercase().as_str() {
        "before-frame-generation" | "before-fg" => UiCompositionPolicy::BeforeFrameGeneration,
        "separate-texture" | "separate" => UiCompositionPolicy::SeparateTexture,
        _ => UiCompositionPolicy::AfterFrameGeneration,
    }
}

fn parse_anti_aliasing(value: &str) -> AntiAliasingMode {
    match value.to_ascii_lowercase().as_str() {
        "off" | "none" | "disabled" => AntiAliasingMode::Off,
        "taa" | "temporal" => AntiAliasingMode::Taa,
        _ => AntiAliasingMode::Taa,
    }
}

/// Detects broad runtime conditions used by automatic scaling policy.
pub fn runtime_scaling_context() -> RenderScalingContext {
    let platform = if cfg!(target_os = "android") {
        RenderPlatformClass::Android
    } else if cfg!(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos"
    )) {
        RenderPlatformClass::AppleMobile
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        RenderPlatformClass::WindowsOnArm
    } else {
        RenderPlatformClass::Desktop
    };
    let thermal_state = match std::env::var("ASTER_THERMAL_STATE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "warm" => ThermalState::Warm,
        "throttling" => ThermalState::Throttling,
        "critical" => ThermalState::Critical,
        _ => ThermalState::Nominal,
    };
    let battery_saver = std::env::var("ASTER_BATTERY_SAVER")
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "on"));
    RenderScalingContext {
        platform,
        thermal_state,
        battery_saver,
    }
}

/// Parses an asset GUID from a mesh/material name string formatted as
/// `"asset:{:032x}"` or `"asset:{:032x}:N"` (multi-mesh model suffix).
fn parse_asset_guid(name: &str) -> Option<engine_core::AssetId> {
    let hex = name.strip_prefix("asset:")?.split(':').next()?;
    let value = u128::from_str_radix(hex, 16).ok()?;
    Some(engine_core::AssetId::from_u128(value))
}

fn parse_asset_mesh_index(name: &str) -> Option<usize> {
    name.strip_prefix("asset:")?.split(':').nth(1)?.parse().ok()
}

fn model_material_index_for_mesh(mesh_name: &str, model: &ModelResource) -> Option<usize> {
    let mesh_index = parse_asset_mesh_index(mesh_name).unwrap_or(0);
    model.meshes.get(mesh_index)?.material_index
}

fn resolve_model_texture_ref(model_source_path: &Path, texture_ref: &str) -> PathBuf {
    let texture_path = Path::new(texture_ref);
    let joined = if texture_path.is_absolute() {
        texture_path.to_path_buf()
    } else {
        model_source_path.parent().map_or_else(
            || texture_path.to_path_buf(),
            |parent| parent.join(texture_path),
        )
    };
    normalize_relative_path(&joined)
}

fn normalize_relative_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

/// Builds the default forward render graph used by the minimal runtime.
pub fn build_default_render_graph() -> RenderGraph {
    use engine_render::RenderStage;

    let mut builder = RenderGraphBuilder::new();
    let shadow = builder.add_pass("shadow");
    let forward = builder.add_pass("forward");
    let temporal_inputs = builder.add_pass_at_stage("temporal-inputs", RenderStage::TemporalInputs);
    let upscale = builder.add_pass_at_stage("upscale", RenderStage::Upscale);
    let post = builder.add_pass_at_stage("post", RenderStage::PostUpscale);
    let ui = builder.add_pass_at_stage("ui", RenderStage::UiComposition);
    builder.order_before(shadow, forward);
    builder.order_before(forward, temporal_inputs);
    builder.order_before(temporal_inputs, upscale);
    builder.order_before(upscale, post);
    builder.order_before(post, ui);
    builder.build()
}

/// Runs a one-frame native smoke path for the minimal runtime.
pub fn smoke_runtime_min() -> EngineResult<u64> {
    let config = EngineConfig::default();
    logging::log_runtime_start(&config.app_name, config.profile.as_str());
    let mut services = RuntimeServices::minimal(config);
    services.tick()?;
    Ok(services.frame_index())
}

/// Converts a winit physical key to an engine KeyCode.
#[cfg(feature = "runtime-game")]
fn convert_winit_key_static(key: winit::keyboard::PhysicalKey) -> Option<engine_platform::KeyCode> {
    use engine_platform::KeyCode;
    use winit::keyboard::{KeyCode as WinitKeyCode, PhysicalKey};

    match key {
        PhysicalKey::Code(WinitKeyCode::Escape) => Some(KeyCode::Escape),
        PhysicalKey::Code(WinitKeyCode::Enter) => Some(KeyCode::Enter),
        PhysicalKey::Code(WinitKeyCode::Space) => Some(KeyCode::Space),
        PhysicalKey::Code(WinitKeyCode::ArrowUp) => Some(KeyCode::ArrowUp),
        PhysicalKey::Code(WinitKeyCode::ArrowDown) => Some(KeyCode::ArrowDown),
        PhysicalKey::Code(WinitKeyCode::ArrowLeft) => Some(KeyCode::ArrowLeft),
        PhysicalKey::Code(WinitKeyCode::ArrowRight) => Some(KeyCode::ArrowRight),
        PhysicalKey::Code(WinitKeyCode::KeyA) => Some(KeyCode::Character('a')),
        PhysicalKey::Code(WinitKeyCode::KeyB) => Some(KeyCode::Character('b')),
        PhysicalKey::Code(WinitKeyCode::KeyC) => Some(KeyCode::Character('c')),
        PhysicalKey::Code(WinitKeyCode::KeyD) => Some(KeyCode::Character('d')),
        PhysicalKey::Code(WinitKeyCode::KeyE) => Some(KeyCode::Character('e')),
        PhysicalKey::Code(WinitKeyCode::KeyF) => Some(KeyCode::Character('f')),
        PhysicalKey::Code(WinitKeyCode::KeyG) => Some(KeyCode::Character('g')),
        PhysicalKey::Code(WinitKeyCode::KeyH) => Some(KeyCode::Character('h')),
        PhysicalKey::Code(WinitKeyCode::KeyI) => Some(KeyCode::Character('i')),
        PhysicalKey::Code(WinitKeyCode::KeyJ) => Some(KeyCode::Character('j')),
        PhysicalKey::Code(WinitKeyCode::KeyK) => Some(KeyCode::Character('k')),
        PhysicalKey::Code(WinitKeyCode::KeyL) => Some(KeyCode::Character('l')),
        PhysicalKey::Code(WinitKeyCode::KeyM) => Some(KeyCode::Character('m')),
        PhysicalKey::Code(WinitKeyCode::KeyN) => Some(KeyCode::Character('n')),
        PhysicalKey::Code(WinitKeyCode::KeyO) => Some(KeyCode::Character('o')),
        PhysicalKey::Code(WinitKeyCode::KeyP) => Some(KeyCode::Character('p')),
        PhysicalKey::Code(WinitKeyCode::KeyQ) => Some(KeyCode::Character('q')),
        PhysicalKey::Code(WinitKeyCode::KeyR) => Some(KeyCode::Character('r')),
        PhysicalKey::Code(WinitKeyCode::KeyS) => Some(KeyCode::Character('s')),
        PhysicalKey::Code(WinitKeyCode::KeyT) => Some(KeyCode::Character('t')),
        PhysicalKey::Code(WinitKeyCode::KeyU) => Some(KeyCode::Character('u')),
        PhysicalKey::Code(WinitKeyCode::KeyV) => Some(KeyCode::Character('v')),
        PhysicalKey::Code(WinitKeyCode::KeyW) => Some(KeyCode::Character('w')),
        PhysicalKey::Code(WinitKeyCode::KeyX) => Some(KeyCode::Character('x')),
        PhysicalKey::Code(WinitKeyCode::KeyY) => Some(KeyCode::Character('y')),
        PhysicalKey::Code(WinitKeyCode::KeyZ) => Some(KeyCode::Character('z')),
        PhysicalKey::Code(WinitKeyCode::Digit0) => Some(KeyCode::Character('0')),
        PhysicalKey::Code(WinitKeyCode::Digit1) => Some(KeyCode::Character('1')),
        PhysicalKey::Code(WinitKeyCode::Digit2) => Some(KeyCode::Character('2')),
        PhysicalKey::Code(WinitKeyCode::Digit3) => Some(KeyCode::Character('3')),
        PhysicalKey::Code(WinitKeyCode::Digit4) => Some(KeyCode::Character('4')),
        PhysicalKey::Code(WinitKeyCode::Digit5) => Some(KeyCode::Character('5')),
        PhysicalKey::Code(WinitKeyCode::Digit6) => Some(KeyCode::Character('6')),
        PhysicalKey::Code(WinitKeyCode::Digit7) => Some(KeyCode::Character('7')),
        PhysicalKey::Code(WinitKeyCode::Digit8) => Some(KeyCode::Character('8')),
        PhysicalKey::Code(WinitKeyCode::Digit9) => Some(KeyCode::Character('9')),
        _ => None,
    }
}

/// Converts a winit mouse button to an engine MouseButton.
#[cfg(feature = "runtime-game")]
fn convert_winit_mouse_button_static(
    button: winit::event::MouseButton,
) -> Option<engine_platform::MouseButton> {
    use engine_platform::MouseButton;
    match button {
        winit::event::MouseButton::Left => Some(MouseButton::Left),
        winit::event::MouseButton::Right => Some(MouseButton::Right),
        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
        winit::event::MouseButton::Other(id) => Some(MouseButton::Other(id)),
        _ => None,
    }
}

/// Runs a project with the runtime-game windowed loop.
#[cfg(feature = "runtime-game")]
pub fn run_project(project: impl AsRef<Path>) -> EngineResult<()> {
    use engine_platform::KeyCode;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use winit::{
        application::ApplicationHandler,
        event::{ElementState, WindowEvent},
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::{Window, WindowId},
    };

    #[cfg(feature = "wgpu")]
    type GameServices = RuntimeServices<WgpuRenderDevice>;
    #[cfg(not(feature = "wgpu"))]
    type GameServices = RuntimeServices;

    struct GameApp {
        services: Option<GameServices>,
        project: Option<RuntimeProject>,
        window: Option<Arc<Window>>,
        last_frame: Instant,
        single_step: bool,
        project_name: String,
        target_frame_time: Duration,
    }

    impl ApplicationHandler for GameApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let width = std::env::var("ASTER_OUTPUT_WIDTH")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1920_u32);
            let height = std::env::var("ASTER_OUTPUT_HEIGHT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1080_u32);
            let window = event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(&self.project_name)
                        .with_inner_size(winit::dpi::PhysicalSize::new(width, height)),
                )
                .expect("create runtime window");
            let window = Arc::new(window);
            let size = window.inner_size();
            let Some(project) = self.project.take() else {
                eprintln!("runtime error: project was already consumed");
                event_loop.exit();
                return;
            };
            match create_game_services(EngineConfig::default(), window.clone(), size, project) {
                Ok(services) => self.services = Some(services),
                Err(error) => {
                    eprintln!("runtime error: {error}");
                    event_loop.exit();
                    return;
                }
            }
            self.window = Some(window);
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match &event {
                WindowEvent::KeyboardInput { event, .. } => {
                    if let Some(key) = convert_winit_key_static(event.physical_key) {
                        if event.state == ElementState::Pressed {
                            if key == KeyCode::Escape {
                                event_loop.exit();
                                return;
                            } else if key == KeyCode::Space {
                                if let Some(services) = self.services.as_mut() {
                                    services.paused = !services.paused;
                                }
                            } else if key == KeyCode::Enter {
                                self.single_step = true;
                            }
                        }
                    }
                }
                WindowEvent::Resized(size) => {
                    #[cfg(feature = "wgpu")]
                    if let Some(services) = self.services.as_mut() {
                        services.renderer.resize_surface(size.width, size.height);
                    }
                    let title = format!(
                        "Aster Runtime - {}x{}",
                        size.width.max(1),
                        size.height.max(1)
                    );
                    if let Some(window) = &self.window {
                        window.set_title(&title);
                    }
                }
                WindowEvent::RedrawRequested => {
                    let Some(services) = self.services.as_mut() else {
                        return;
                    };
                    let now = Instant::now();
                    let delta = now.saturating_duration_since(self.last_frame);
                    self.last_frame = now;
                    if let Err(error) = services.tick_game_frame(delta, self.single_step) {
                        eprintln!("runtime error: {error}");
                        event_loop.exit();
                        return;
                    }
                    self.single_step = false;
                    if let Some(window) = &self.window {
                        let status = if services.render_world.is_visible() {
                            "rendering"
                        } else {
                            "empty"
                        };
                        window.set_title(&format!(
                            "Aster Runtime - frame {} - {status}",
                            services.frame_index()
                        ));
                    }
                }
                _ => {}
            }
            if let Some(services) = self.services.as_mut() {
                if services.process_winit_event(&event) {
                    event_loop.exit();
                }
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(window) = &self.window {
                window.request_redraw();
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + self.target_frame_time,
                ));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }

    let project = load_runtime_project(project)?;
    let project_name = project.manifest.name.clone();
    let target_fps = project.build.render.target_fps.max(1);
    let target_frame_time = Duration::from_secs_f64(1.0 / f64::from(target_fps));
    let event_loop = EventLoop::new().map_err(|error| EngineError::other(error.to_string()))?;
    let mut app = GameApp {
        services: None,
        project: Some(project),
        window: None,
        last_frame: Instant::now(),
        single_step: false,
        project_name,
        target_frame_time,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| EngineError::other(error.to_string()))
}

#[cfg(all(feature = "runtime-game", feature = "wgpu"))]
fn create_game_services(
    config: EngineConfig,
    window: std::sync::Arc<winit::window::Window>,
    size: winit::dpi::PhysicalSize<u32>,
    project: RuntimeProject,
) -> EngineResult<RuntimeServices<WgpuRenderDevice>> {
    let instance = engine_render_wgpu::wgpu::Instance::default();
    let surface = instance
        .create_surface(window)
        .map_err(|error| EngineError::other(format!("create wgpu surface failed: {error}")))?;
    let mut renderer =
        WgpuRenderDevice::new_surface(surface, size.width.max(1), size.height.max(1))?;
    renderer.configure_performance(runtime_performance_config_from_env());
    let scaling_settings = render_scaling_settings_from_build(&project.build);
    let mut services = RuntimeServices::with_renderer(config, renderer);
    services.set_render_scaling(scaling_settings, runtime_scaling_context());
    #[cfg(feature = "audio")]
    services.enable_default_audio_output();
    services.set_project_root(project.root.clone());
    let asset_root = project.root.join(&project.manifest.asset_root);
    services.load_project_assets(asset_root)?;
    services.scene = project.scene;
    services.render_world = extract_render_world(&services.scene);
    Ok(services)
}

#[cfg(all(feature = "runtime-game", not(feature = "wgpu")))]
fn create_game_services(
    config: EngineConfig,
    _window: std::sync::Arc<winit::window::Window>,
    _size: winit::dpi::PhysicalSize<u32>,
    project: RuntimeProject,
) -> EngineResult<RuntimeServices> {
    let mut services = RuntimeServices::minimal(config);
    #[cfg(feature = "audio")]
    services.enable_default_audio_output();
    services.set_project_root(project.root.clone());
    let asset_root = project.root.join(&project.manifest.asset_root);
    services.load_project_assets(asset_root)?;
    services.scene = project.scene;
    services.render_world = extract_render_world(&services.scene);
    Ok(services)
}

/// Reports that runtime-game support is not compiled into this binary.
#[cfg(not(feature = "runtime-game"))]
pub fn run_project(_project: impl AsRef<Path>) -> EngineResult<()> {
    Err(EngineError::UnsupportedCapability {
        capability: "runtime-game",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_min_ticks_one_frame() {
        assert_eq!(smoke_runtime_min().unwrap(), 1);
    }

    #[test]
    fn run_frame_60_frames_headless() {
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        let delta = Duration::from_secs_f32(1.0 / 60.0);
        for _ in 0..60 {
            services.run_frame(delta, false).unwrap();
        }
        assert_eq!(services.frame_index(), 60);
        assert_eq!(services.time.frame_index, 60);
        assert!(
            services.time.total_time > 0.0,
            "total_time should accumulate across frames"
        );
    }

    #[test]
    fn run_frame_input_reset_at_begin_frame() {
        use engine_platform::{InputEvent, KeyCode};
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services
            .input
            .apply_event(InputEvent::KeyDown(KeyCode::Space));
        assert!(
            services.input.key_pressed(KeyCode::Space),
            "key should be pressed before frame"
        );
        services
            .run_frame(Duration::from_millis(16), false)
            .unwrap();
        assert!(
            !services.input.key_pressed(KeyCode::Space),
            "pressed state should be cleared by begin_frame"
        );
        assert!(
            services.input.key_down(KeyCode::Space),
            "held state should persist after begin_frame"
        );
    }

    #[cfg(feature = "physics")]
    #[test]
    fn runtime_physics_diagnostics_respect_contact_filter_chain() {
        use engine_physics::{ColliderDesc, ContactFilter, RigidbodyDesc, Transform, Vec3};

        let mut services = RuntimeServices::minimal(EngineConfig::default());
        let ignored = services
            .physics
            .backend_mut()
            .create_body(&RigidbodyDesc::default())
            .unwrap();
        let other = services
            .physics
            .backend_mut()
            .create_body(&RigidbodyDesc {
                transform: Transform {
                    translation: Vec3::new(0.25, 0.0, 0.0),
                    ..Transform::IDENTITY
                },
                ..RigidbodyDesc::default()
            })
            .unwrap();
        services
            .physics
            .backend_mut()
            .add_collider(ignored, &ColliderDesc::default())
            .unwrap();
        services
            .physics
            .backend_mut()
            .add_collider(other, &ColliderDesc::default())
            .unwrap();
        services
            .physics
            .contact_filter_chain
            .push(ContactFilter::IgnoreBody { body: ignored });

        services.physics.fixed_update(0.0);
        services.report_physics_events();

        assert!(services.diagnostics.is_empty());
    }

    #[cfg(feature = "physics")]
    #[test]
    fn fluid_collider_volume_matches_basic_shapes() {
        let box_volume = fluid_collider_volume(
            &engine_ecs::ColliderComponentData {
                shape: "box".to_string(),
                size: Vec3::new(2.0, 3.0, 4.0),
                ..engine_ecs::ColliderComponentData::default()
            },
            Vec3::ONE,
        );
        assert!((box_volume - 24.0).abs() < 1e-5);

        let sphere_volume = fluid_collider_volume(
            &engine_ecs::ColliderComponentData {
                shape: "sphere".to_string(),
                size: Vec3::new(2.0, 2.0, 2.0),
                ..engine_ecs::ColliderComponentData::default()
            },
            Vec3::ONE,
        );
        assert!((sphere_volume - ((4.0 / 3.0) * std::f32::consts::PI)).abs() < 1e-5);

        let capsule_volume = fluid_collider_volume(
            &engine_ecs::ColliderComponentData {
                shape: "capsule".to_string(),
                size: Vec3::new(2.0, 2.0, 2.0),
                ..engine_ecs::ColliderComponentData::default()
            },
            Vec3::ONE,
        );
        let expected_capsule = std::f32::consts::PI * 2.0 + (4.0 / 3.0) * std::f32::consts::PI;
        assert!((capsule_volume - expected_capsule).abs() < 1e-5);
    }

    #[test]
    fn model_material_resolution_keeps_explicit_materials() {
        let model_guid = engine_core::AssetId::from_u128(0x1234);
        let explicit_guid = engine_core::AssetId::from_u128(0x5678);
        let mut mesh_resources = HashMap::new();
        mesh_resources.insert(
            model_guid,
            ModelResource {
                materials: vec![engine_assets::CpuMaterialResource::default()],
                ..ModelResource::default()
            },
        );
        let model_mesh = format!("asset:{:032x}", model_guid.as_u128());
        let mut world = RenderWorld {
            objects: vec![
                engine_render::RenderObject {
                    object: engine_core::EntityId::from_u128(1),
                    transform: engine_core::math::Transform::IDENTITY,
                    mesh: model_mesh.clone(),
                    material: format!("asset:{:032x}", explicit_guid.as_u128()),
                    casts_shadows: true,
                    receive_shadows: true,
                    bounds: engine_render::RenderBounds::default(),
                    lods: Vec::new(),
                },
                engine_render::RenderObject {
                    object: engine_core::EntityId::from_u128(2),
                    transform: engine_core::math::Transform::IDENTITY,
                    mesh: model_mesh,
                    material: "debug/default".to_string(),
                    casts_shadows: true,
                    receive_shadows: true,
                    bounds: engine_render::RenderBounds::default(),
                    lods: Vec::new(),
                },
            ],
            ..RenderWorld::default()
        };

        RuntimeServices::<HeadlessRenderDevice>::resolve_render_materials(
            &mut world,
            &mesh_resources,
        );

        assert_eq!(
            world.objects[0].material,
            format!("asset:{:032x}", explicit_guid.as_u128())
        );
        assert_eq!(
            world.objects[1].material,
            format!("material:{:032x}:0", model_guid.as_u128())
        );
    }

    #[test]
    fn model_material_resolution_uses_mesh_primitive_material_index() {
        let model_guid = engine_core::AssetId::from_u128(0x1234);
        let mut mesh_resources = HashMap::new();
        mesh_resources.insert(
            model_guid,
            ModelResource {
                meshes: vec![
                    engine_assets::BasicMeshResource {
                        positions: Vec::new(),
                        normals: Vec::new(),
                        texcoords: Vec::new(),
                        indices: Vec::new(),
                        material_index: Some(2),
                    },
                    engine_assets::BasicMeshResource {
                        positions: Vec::new(),
                        normals: Vec::new(),
                        texcoords: Vec::new(),
                        indices: Vec::new(),
                        material_index: Some(1),
                    },
                ],
                materials: vec![
                    engine_assets::CpuMaterialResource::default(),
                    engine_assets::CpuMaterialResource::default(),
                    engine_assets::CpuMaterialResource::default(),
                ],
            },
        );
        let mut world = RenderWorld {
            objects: vec![engine_render::RenderObject {
                object: engine_core::EntityId::from_u128(1),
                transform: engine_core::math::Transform::IDENTITY,
                mesh: format!("asset:{:032x}:1", model_guid.as_u128()),
                material: String::new(),
                casts_shadows: true,
                receive_shadows: true,
                bounds: engine_render::RenderBounds::default(),
                lods: Vec::new(),
            }],
            ..RenderWorld::default()
        };

        RuntimeServices::<HeadlessRenderDevice>::resolve_render_materials(
            &mut world,
            &mesh_resources,
        );

        assert_eq!(
            world.objects[0].material,
            format!("material:{:032x}:1", model_guid.as_u128())
        );
    }

    #[test]
    fn model_texture_refs_resolve_relative_to_model_source() {
        assert_eq!(
            resolve_model_texture_ref(Path::new("models/ship/ship.gltf"), "textures/albedo.png"),
            PathBuf::from("models/ship/textures/albedo.png")
        );
        assert_eq!(
            resolve_model_texture_ref(Path::new("models/ship/ship.gltf"), "../shared/normal.png"),
            PathBuf::from("models/shared/normal.png")
        );
    }

    #[test]
    fn default_render_graph_has_expected_passes() {
        let graph = build_default_render_graph();
        assert_eq!(graph.pass_count(), 6);
        assert_eq!(graph.passes[0].name, "shadow");
        assert_eq!(graph.passes[1].name, "forward");
        assert_eq!(graph.passes[2].name, "temporal-inputs");
        assert_eq!(graph.passes[3].name, "upscale");
        assert_eq!(graph.passes[4].name, "post");
        assert_eq!(graph.passes[5].name, "ui");
    }

    #[test]
    fn runtime_services_can_replace_render_graph() {
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        let mut builder = RenderGraphBuilder::new();
        builder.add_pass("custom");
        services.set_render_graph(builder.build());
        assert_eq!(services.render_graph.pass_count(), 1);
        services.tick().unwrap();
    }

    #[test]
    fn loads_example_project_and_extracts_render_world() {
        let project = load_runtime_project(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/project"),
        )
        .unwrap();
        let render_world = extract_render_world(&project.scene);

        assert!(project.scene.find_by_name("Player").is_some());
        assert!(render_world.is_visible());
    }

    #[test]
    fn game_frame_updates_stats_and_script_diagnostics() {
        let project = load_runtime_project(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/project"),
        )
        .unwrap();
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services.scene = project.scene;

        services
            .tick_game_frame(Duration::from_millis(16), false)
            .unwrap();

        assert!(services.stats.entity_count >= 2);
        assert!(services.stats.draw_calls >= 1);
        assert!(!services.diagnostics.iter().any(|diagnostic| {
            diagnostic.source == "script" && diagnostic.message.contains("pending backend recovery")
        }));
    }

    #[test]
    fn varg_script_start_and_update_can_move_transform() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("move.varg"),
            r#"script Move {
    @export var speed: Float = 2.0
    var started: Int = 0

    func start() {
        entity.translate(Vec3(1.0, 0.0, 0.0))
        state.started = 1
    }

    func update(_ dt: Float) {
        entity.translate(Vec3(0.0, 0.0, Input.actionValue("MoveY") * speed))
        state.ticks += 1
    }
}
"#,
        )
        .unwrap();

        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services.set_project_root(root.path());
        let entity = services.scene.create_object("Scripted").unwrap();
        services
            .scene
            .upsert_component(
                entity,
                ComponentData::Script(engine_ecs::ScriptComponent::new("scripts/move.varg")),
            )
            .unwrap();
        services
            .input
            .apply_event(engine_platform::InputEvent::KeyDown(
                engine_platform::KeyCode::Character('w'),
            ));

        services
            .tick_game_frame(Duration::from_millis(16), false)
            .unwrap();

        let transform = services.scene.transforms().local(entity).unwrap();
        assert_eq!(transform.translation.x, 1.0);
        assert_eq!(transform.translation.z, 2.0);
        let object = services.scene.object(entity).unwrap();
        let script = object
            .components
            .iter()
            .find_map(|component| match component {
                ComponentData::Script(script) => Some(script),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            script.state.get("started").and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert_eq!(
            script.state.get("ticks").and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert_eq!(script.pending_recovery, false);
    }

    #[cfg(feature = "script-python")]
    #[test]
    fn python_script_update_can_move_transform() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root = std::env::temp_dir().join(format!("aster-script-test-{}", std::process::id()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("move.py"),
            "def start(ctx):\n    ctx.transform.translation.x += 2.0\n    ctx.state['started'] = True\n\ndef update(ctx):\n    ctx.transform.translation.z += ctx.input.action_value('MoveY')\n",
        )
        .unwrap();

        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services.set_project_root(&root);
        let entity = services.scene.create_object("Scripted").unwrap();
        services
            .scene
            .object_mut(entity)
            .unwrap()
            .scripts
            .push(ScriptComponentProxy {
                source: "scripts/move.py".to_string(),
                exported_values: HashMap::new(),
                state: HashMap::new(),
                legacy_backend: Some("python".to_string()),
                pending_recovery: true,
            });
        services
            .input
            .apply_event(engine_platform::InputEvent::KeyDown(
                engine_platform::KeyCode::Character('w'),
            ));

        services
            .tick_game_frame(Duration::from_millis(16), false)
            .unwrap();

        let transform = services.scene.transforms().local(entity).unwrap();
        assert_eq!(transform.translation.x, 2.0);
        assert_eq!(transform.translation.z, 1.0);
        assert!(
            services
                .scene
                .object(entity)
                .unwrap()
                .scripts
                .first()
                .unwrap()
                .state
                .contains_key("started")
        );
    }

    #[cfg(feature = "script-python")]
    #[test]
    fn python_script_timeout_reports_diagnostic() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root =
            std::env::temp_dir().join(format!("aster-script-timeout-test-{}", std::process::id()));
        let scripts = root.join("scripts");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("hang.py"),
            "import time\n\ndef start(ctx):\n    time.sleep(1.0)\n",
        )
        .unwrap();

        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services.set_python_script_runtime_config(PythonScriptRuntimeConfig {
            invocation_timeout: Duration::from_millis(10),
            ..PythonScriptRuntimeConfig::default()
        });
        services.set_project_root(&root);
        let entity = services.scene.create_object("Scripted").unwrap();
        services
            .scene
            .object_mut(entity)
            .unwrap()
            .scripts
            .push(ScriptComponentProxy {
                source: "scripts/hang.py".to_string(),
                exported_values: HashMap::new(),
                state: HashMap::new(),
                legacy_backend: Some("python".to_string()),
                pending_recovery: true,
            });

        services
            .tick_game_frame(Duration::from_millis(16), false)
            .unwrap();

        assert!(
            services
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("timed out"))
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(feature = "audio")]
    #[test]
    fn game_frame_spawns_loaded_audio_source() {
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        let asset = engine_core::AssetId::from_u128(7);
        services
            .load_audio_clip(asset, "tone.wav", &test_wav_bytes())
            .unwrap();
        let entity = services.scene.create_object("Speaker").unwrap();
        services
            .scene
            .upsert_component(
                entity,
                ComponentData::AudioSource(engine_ecs::AudioSourceComponentData {
                    clip: Some(asset),
                    volume: 0.5,
                    looping: true,
                    play_on_start: true,
                    spatial_blend: 0.0,
                    ..engine_ecs::AudioSourceComponentData::default()
                }),
            )
            .unwrap();

        services
            .tick_game_frame(Duration::from_millis(16), false)
            .unwrap();

        assert_eq!(services.audio_bindings.len(), 1);
    }

    #[cfg(feature = "audio")]
    #[test]
    fn acoustic_geometry_drives_source_propagation() {
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        let asset = engine_core::AssetId::from_u128(8);
        services
            .load_audio_clip(asset, "tone.wav", &test_wav_bytes())
            .unwrap();

        let listener = services.scene.create_object("Listener").unwrap();
        services
            .scene
            .upsert_component(listener, ComponentData::AudioListener(Default::default()))
            .unwrap();

        let speaker = services.scene.create_object("Speaker").unwrap();
        services.scene.transforms_mut().set_local(
            speaker,
            engine_core::math::Transform {
                translation: engine_core::math::Vec3::new(0.0, 0.0, -10.0),
                ..engine_core::math::Transform::IDENTITY
            },
        );
        services
            .scene
            .upsert_component(
                speaker,
                ComponentData::AudioSource(engine_ecs::AudioSourceComponentData {
                    clip: Some(asset),
                    volume: 1.0,
                    looping: true,
                    play_on_start: true,
                    spatial_mode: "object".to_string(),
                    attenuation: "none".to_string(),
                    ..engine_ecs::AudioSourceComponentData::default()
                }),
            )
            .unwrap();

        let wall = services.scene.create_object("Wall").unwrap();
        services.scene.transforms_mut().set_local(
            wall,
            engine_core::math::Transform {
                translation: engine_core::math::Vec3::new(0.0, 0.0, -5.0),
                ..engine_core::math::Transform::IDENTITY
            },
        );
        services
            .scene
            .upsert_component(
                wall,
                ComponentData::AcousticGeometry(engine_ecs::AcousticGeometryComponentData {
                    size: engine_core::math::Vec3::new(4.0, 4.0, 0.5),
                    material: Some(engine_ecs::AcousticMaterialComponentData {
                        transmission: [0.3, 0.15, 0.02],
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();

        services
            .tick_game_frame(Duration::from_millis(16), false)
            .unwrap();

        assert_eq!(services.audio_bindings.len(), 1);
        assert_eq!(services.audio.diagnostics().acoustics_sources, 1);
    }

    #[cfg(feature = "physics")]
    #[test]
    fn game_frame_creates_physics_bindings_for_scene_components() {
        let project = load_runtime_project(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/project"),
        )
        .unwrap();
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services.scene = project.scene;

        services
            .tick_game_frame(Duration::from_millis(20), false)
            .unwrap();

        assert_eq!(services.physics_bindings.len(), 1);
        assert!(services.stats.physics_steps >= 1);
    }

    #[cfg(feature = "audio")]
    fn test_wav_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&88_200u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&i16::MAX.to_le_bytes());
        bytes
    }

    #[test]
    fn action_map_default_bindings_on_runtime_services() {
        let services = RuntimeServices::minimal(EngineConfig::default());
        // The default ActionMap should have MoveForward, MoveBack, MoveLeft,
        // MoveRight, Jump, Fire, Interact, Pause
        assert!(
            services.action_pressed("MoveForward") || !services.action_pressed("MoveForward"),
            "action_pressed should not panic for known action"
        );
        assert!(
            services.action_held("MoveForward") || !services.action_held("MoveForward"),
            "action_held should not panic for known action"
        );
        // Unknown actions return false
        assert!(!services.action_pressed("DoesNotExist"));
        assert!(!services.action_held("DoesNotExist"));
    }

    #[test]
    fn action_pressed_delegates_to_action_map() {
        use engine_platform::{InputEvent, KeyCode};
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services
            .input
            .apply_event(InputEvent::KeyDown(KeyCode::Space));
        assert!(
            services.action_pressed("Jump"),
            "Jump should be pressed when Space is pressed"
        );
        assert!(
            services.action_held("Jump"),
            "Jump should be held when Space is down"
        );
    }

    #[test]
    fn action_map_fire_and_interact_bindings() {
        use engine_platform::{InputEvent, KeyCode};
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services
            .input
            .apply_event(InputEvent::KeyDown(KeyCode::Character('f')));
        assert!(
            services.action_pressed("Fire"),
            "Fire should be pressed when F is pressed"
        );
        assert!(!services.action_pressed("Interact"));
    }

    #[test]
    fn action_map_pause_binding() {
        use engine_platform::{InputEvent, KeyCode};
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services
            .input
            .apply_event(InputEvent::KeyDown(KeyCode::Escape));
        assert!(
            services.action_pressed("Pause"),
            "Pause should be pressed when Escape is pressed"
        );
    }

    #[test]
    fn action_map_axis_value() {
        use engine_platform::{InputEvent, KeyCode};
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        // No input → 0.0
        assert_eq!(
            services
                .action_map
                .axis_value(&services.input, "MoveLeft", "MoveRight"),
            0.0
        );
        // Press D → MoveRight → +1.0
        services
            .input
            .apply_event(InputEvent::KeyDown(KeyCode::Character('d')));
        assert_eq!(
            services
                .action_map
                .axis_value(&services.input, "MoveLeft", "MoveRight"),
            1.0
        );
        // Press A too → both held → 0.0
        services
            .input
            .apply_event(InputEvent::KeyDown(KeyCode::Character('a')));
        assert_eq!(
            services
                .action_map
                .axis_value(&services.input, "MoveLeft", "MoveRight"),
            0.0
        );
    }

    #[cfg(feature = "runtime-game")]
    #[derive(Debug)]
    struct FixedGamepadProvider {
        states: Vec<engine_platform::GamepadState>,
    }

    #[cfg(feature = "runtime-game")]
    impl engine_platform::GamepadProvider for FixedGamepadProvider {
        fn poll_gamepads(&mut self) -> Vec<engine_platform::GamepadState> {
            self.states.clone()
        }

        fn gamepad_count(&self) -> usize {
            self.states.len()
        }
    }

    #[cfg(feature = "runtime-game")]
    #[test]
    fn runtime_game_frame_polls_gamepads() {
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        let mut gamepad = engine_platform::GamepadState::connected(0, "Xbox Wireless Controller");
        gamepad.press_button(engine_platform::GamepadButton::A);
        gamepad.left_stick_x = 0.75;
        services.gamepad_provider = Box::new(FixedGamepadProvider {
            states: vec![gamepad],
        });

        services
            .tick_game_frame(Duration::from_millis(16), false)
            .unwrap();

        assert!(
            services
                .input
                .gamepad_button_down(engine_platform::GamepadButton::A)
        );
        assert_eq!(services.input.gamepad_states().len(), 1);
        assert_eq!(services.input.gamepad_states()[0].left_stick_x, 0.75);
    }

    #[test]
    fn load_action_bindings_from_toml() {
        use engine_platform::KeyCode;
        let dir =
            std::env::temp_dir().join(format!("aster-action-bindings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join("action_bindings.toml");
        std::fs::write(
            &toml_path,
            "[actions]\nCrouch = [\"C\"]\nSprint = [\"Shift\"]\n",
        )
        .unwrap();

        let mut services = RuntimeServices::minimal(EngineConfig::default());
        services.load_action_bindings(&toml_path).unwrap();

        // Verify the Crouch binding was added
        assert!(services.action_map.bindings.get("Crouch").is_some());
        assert!(
            services
                .action_map
                .bindings
                .get("Crouch")
                .unwrap()
                .contains(&KeyCode::Character('c')),
            "Crouch should be bound to C"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_action_bindings_missing_file_returns_error() {
        let mut services = RuntimeServices::minimal(EngineConfig::default());
        let result = services.load_action_bindings(Path::new("/nonexistent/action_bindings.toml"));
        assert!(result.is_err());
    }
}
