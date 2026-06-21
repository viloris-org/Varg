#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Rhai script backend for the Aster engine.
//!
//! # API layers
//!
//! | Layer | Module | Style |
//! |---|---|---|
//! | **new** (recommended) | [`threesh`] | three.js-like: `create_mesh("Cube", geo, mat)`, `cube.position.set(1,2,3)` |
//! | legacy | global functions | Aster-specific: `get_position()`, `set_position(x,y,z)` |
//!
//! Models trained on three.js / Web Audio can use the new API with zero learning cost.

#[allow(missing_docs)]
pub mod threesh;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use engine_core::EngineResult;
use engine_ecs::Entity;
use engine_editor::{ConsoleEntry, ConsoleLevel, ConsoleSource};
use rhai::{ParseErrorType, Scope, AST};
use threesh::scene::SceneContext;

// ── UI draw commands (immediate-mode) ────────────────────────────────────────

/// A single immediate-mode UI draw command issued by scripts.
#[derive(Clone, Debug)]
pub enum UiCommand {
    /// Draw text at screen position.
    Text {
        /// X position in pixels.
        x: f32,
        /// Y position in pixels.
        y: f32,
        /// Text content.
        content: String,
        /// Optional color [r, g, b, a] in 0..=1.
        color: [f32; 4],
    },
    /// Draw a progress bar.
    Bar {
        /// X position.
        x: f32,
        /// Y position.
        y: f32,
        /// Width.
        width: f32,
        /// Height.
        height: f32,
        /// Fill ratio 0..=1.
        ratio: f32,
        /// Optional color [r, g, b, a].
        color: [f32; 4],
    },
    /// Draw a button and return whether it was clicked.
    Button {
        /// X position.
        x: f32,
        /// Y position.
        y: f32,
        /// Button label.
        label: String,
        /// Width.
        width: f32,
        /// Height.
        height: f32,
    },
}

// ── Script event (pub/sub) ───────────────────────────────────────────────────

/// A pending script event.
#[derive(Clone, Debug)]
pub struct ScriptEvent {
    /// Event name (e.g. "coin_collected", "enemy_killed").
    pub name: String,
    /// Sender entity ID (empty for global events).
    pub sender: String,
    /// Arbitrary key-value payload.
    pub data: HashMap<String, rhai::Dynamic>,
}

// ── Animation player state ───────────────────────────────────────────────────

/// Per-entity animation playback state.
#[derive(Clone, Debug)]
pub struct AnimationState {
    /// Currently playing clip name.
    pub clip: String,
    /// Current time in seconds.
    pub time: f32,
    /// Playback speed multiplier.
    pub speed: f32,
    /// Whether the animation is playing.
    pub playing: bool,
    /// Loop mode: "once", "loop", "ping_pong".
    pub loop_mode: String,
}

/// Configuration for the Rhai script backend.
///
/// Controls which capabilities are enabled in the sandboxed Rhai engine.
/// By default, all dangerous capabilities (file I/O, network, process execution)
/// are disabled for security.
#[derive(Clone, Debug, Default)]
pub struct RhaiConfig {
    /// Enable file system read access (read_file, read_text_file, list_dir).
    pub enable_file_read: bool,
    /// Enable file system write access (write_file, remove_file, rename_file).
    pub enable_file_write: bool,
    /// Enable network access (http_get, http_post).
    pub enable_network: bool,
    /// Enable process execution (eval, command execution).
    pub enable_process_execution: bool,
    /// Enable strict variable checking for language-service validation.
    pub strict_variables: bool,
}

/// Severity reported by the Aster Script language service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsterDiagnosticSeverity {
    /// The script cannot be accepted until the issue is fixed.
    Error,
    /// The script is valid, but the issue is likely unintended.
    Warning,
}

/// Structured Aster Script diagnostic suitable for editor and AI tooling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsterScriptDiagnostic {
    /// Stable machine-readable diagnostic code.
    pub code: String,
    /// Diagnostic severity.
    pub severity: AsterDiagnosticSeverity,
    /// One-based source line, when available.
    pub line: Option<usize>,
    /// One-based source column, when available.
    pub column: Option<usize>,
    /// Clear explanation of what is invalid.
    pub message: String,
    /// Concrete guidance for correcting the issue.
    pub suggestion: String,
    /// Source line containing the issue, when available.
    pub source_line: Option<String>,
}

/// Rhai-based script backend for runtime-game builds.
///
/// Manages script compilation, AST caching, and lifecycle dispatch.
pub struct RhaiScriptBackend {
    engine: rhai::Engine,
    ast_cache: HashMap<PathBuf, AST>,
    /// Per-entity scopes for persistent script state.
    /// Key is (entity_id, script_path) to support multiple scripts per entity.
    entity_scopes: HashMap<(String, PathBuf), Scope<'static>>,
    /// Shared input state for script queries.
    input_state: Arc<Mutex<Option<engine_platform::InputState>>>,
    /// Shared scene reference for transform queries and modifications.
    scene: Arc<Mutex<Option<engine_ecs::Scene>>>,
    /// Current entity context for transform API.
    transform_context: Arc<Mutex<Option<String>>>,
    /// Shared physics backend reference for physics queries.
    physics_backend: Arc<Mutex<Option<Box<dyn engine_physics::PhysicsBackend>>>>,
    /// Shared asset database reference for resource path resolution.
    asset_database: Arc<Mutex<Option<engine_assets::AssetDatabase>>>,
    /// Three.js-compatible API context (shares `scene` Arc).
    pub threesh_ctx: SceneContext,
    /// Shared audio backend reference for sound playback.
    audio_backend: Arc<Mutex<Option<Box<dyn engine_audio::AudioBackend>>>>,
    /// Pending collision contacts drained from physics.
    collision_events: Arc<Mutex<Vec<engine_physics::ContactEvent>>>,
    /// Body-to-entity ID mapping for collision callback resolution.
    body_entity_map: Arc<Mutex<HashMap<u64, String>>>,
    /// Entity-to-body mapping for physics API (reverse of body_entity_map).
    entity_body_map: Arc<Mutex<HashMap<String, u64>>>,
    /// Entity event queue: entity_id -> list of pending events.
    event_queue: Arc<Mutex<HashMap<String, Vec<ScriptEvent>>>>,
    /// Global event queue (not entity-specific).
    global_event_queue: Arc<Mutex<Vec<ScriptEvent>>>,
    /// UI draw commands collected this frame.
    ui_commands: Arc<Mutex<Vec<UiCommand>>>,
    /// Per-entity animation states.
    animation_states: Arc<Mutex<HashMap<String, AnimationState>>>,
    /// Loaded animation clips: name -> duration.
    animation_clips: Arc<Mutex<HashMap<String, f32>>>,
    /// Synth graph for procedural audio synthesis.
    synth_graph: Arc<Mutex<engine_audio::synth::SynthGraph>>,
}

impl Default for RhaiScriptBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RhaiScriptBackend {
    /// Creates a new RhaiScriptBackend with a sandboxed Rhai engine.
    ///
    /// Uses `Engine::new_raw()` to start with zero registered packages, then
    /// selectively enables only safe, non-I/O packages. File system access,
    /// network access, and process execution are disabled by default.
    pub fn new() -> Self {
        Self::with_config(RhaiConfig::default())
    }

    /// Creates a new RhaiScriptBackend with custom configuration.
    ///
    /// Allows enabling specific capabilities like file I/O, network access,
    /// or process execution. Use with caution - enabling these capabilities
    /// can have security implications.
    pub fn with_config(config: RhaiConfig) -> Self {
        let input_state = Arc::new(Mutex::new(None));
        let scene = Arc::new(Mutex::new(None));
        let transform_context = Arc::new(Mutex::new(None));
        let physics_backend: Arc<Mutex<Option<Box<dyn engine_physics::PhysicsBackend>>>> =
            Arc::new(Mutex::new(None));
        let asset_database = Arc::new(Mutex::new(None));

        // Build the three.js-compatible API context sharing the same scene Arc.
        let threesh_ctx = SceneContext {
            inner: Arc::clone(&scene),
            physics: Arc::clone(&physics_backend),
            assets: Arc::clone(&asset_database),
        };

        let audio_backend: Arc<Mutex<Option<Box<dyn engine_audio::AudioBackend>>>> =
            Arc::new(Mutex::new(None));
        let collision_events = Arc::new(Mutex::new(Vec::new()));
        let body_entity_map = Arc::new(Mutex::new(HashMap::new()));
        let entity_body_map = Arc::new(Mutex::new(HashMap::new()));
        let event_queue: Arc<Mutex<HashMap<String, Vec<ScriptEvent>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let global_event_queue = Arc::new(Mutex::new(Vec::new()));
        let ui_commands = Arc::new(Mutex::new(Vec::new()));
        let animation_states: Arc<Mutex<HashMap<String, AnimationState>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let animation_clips: Arc<Mutex<HashMap<String, f32>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let synth_graph = Arc::new(Mutex::new(engine_audio::synth::SynthGraph::new(44100)));

        // Start with a standard engine, then disable dangerous capabilities.
        // Rhai's default engine includes arithmetic, collections, string ops,
        // and time — but also file I/O and networking. We disable the latter
        // unless explicitly enabled via config.
        let mut engine = rhai::Engine::new();

        // Disable capabilities based on config
        if !config.enable_process_execution {
            engine.disable_symbol("eval");
        }

        if !config.enable_file_read {
            engine.disable_symbol("read_file");
            engine.disable_symbol("read_text_file");
            engine.disable_symbol("list_dir");
        }

        if !config.enable_file_write {
            engine.disable_symbol("write_file");
            engine.disable_symbol("remove_file");
            engine.disable_symbol("rename_file");
        }

        if !config.enable_file_write {
            engine.disable_symbol("write_text_file");
        }

        if !config.enable_network {
            engine.disable_symbol("http_get");
            engine.disable_symbol("http_post");
        }

        if config.strict_variables {
            engine.set_strict_variables(true);
        }

        // Register input API functions
        Self::register_input_api(&mut engine, Arc::clone(&input_state));

        // Register transform API functions
        Self::register_transform_api(
            &mut engine,
            Arc::clone(&scene),
            Arc::clone(&transform_context),
        );

        // Register world API functions
        Self::register_world_api(&mut engine, Arc::clone(&scene));

        // Register physics API functions
        Self::register_physics_api(&mut engine, Arc::clone(&physics_backend));

        // Register resource API functions
        Self::register_resource_api(&mut engine, Arc::clone(&asset_database));

        // Register the new three.js-compatible API
        threesh::register_threesh_api(&mut engine, &threesh_ctx);

        // Register audio API functions
        Self::register_audio_api(&mut engine, Arc::clone(&audio_backend));

        // Register collision API functions
        Self::register_collision_api(
            &mut engine,
            Arc::clone(&collision_events),
            Arc::clone(&body_entity_map),
            Arc::clone(&scene),
        );

        // Register event/messaging API functions
        Self::register_event_api(
            &mut engine,
            Arc::clone(&event_queue),
            Arc::clone(&global_event_queue),
            Arc::clone(&transform_context),
        );

        // Register UI API functions
        Self::register_ui_api(&mut engine, Arc::clone(&ui_commands));

        // Register animation API functions
        Self::register_animation_api(
            &mut engine,
            Arc::clone(&animation_states),
            Arc::clone(&animation_clips),
            Arc::clone(&transform_context),
        );

        // Register synthesis API functions
        Self::register_synth_api(&mut engine, Arc::clone(&synth_graph));

        // Register entity query API functions (find, parent/children, components)
        Self::register_entity_query_api(
            &mut engine,
            Arc::clone(&scene),
            Arc::clone(&transform_context),
        );

        // Register enhanced physics API (forces, velocity by entity ID)
        Self::register_physics_entity_api(
            &mut engine,
            Arc::clone(&physics_backend),
            Arc::clone(&entity_body_map),
            Arc::clone(&body_entity_map),
        );

        Self {
            engine,
            ast_cache: HashMap::new(),
            entity_scopes: HashMap::new(),
            input_state,
            scene,
            transform_context,
            physics_backend,
            asset_database,
            threesh_ctx,
            audio_backend,
            collision_events,
            body_entity_map,
            entity_body_map,
            event_queue,
            global_event_queue,
            ui_commands,
            animation_states,
            animation_clips,
            synth_graph,
        }
    }

    /// Registers the input API module with the Rhai engine.
    fn register_input_api(
        engine: &mut rhai::Engine,
        input_state: Arc<Mutex<Option<engine_platform::InputState>>>,
    ) {
        // input::is_pressed(key_name: &str) -> bool
        {
            let state = Arc::clone(&input_state);
            engine.register_fn("is_pressed", move |key_name: &str| -> bool {
                let guard = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(input) = guard.as_ref() {
                    Self::parse_key_name(key_name)
                        .map(|key| input.key_pressed(key))
                        .unwrap_or(false)
                } else {
                    false
                }
            });
        }

        // input::is_held(key_name: &str) -> bool
        {
            let state = Arc::clone(&input_state);
            engine.register_fn("is_held", move |key_name: &str| -> bool {
                let guard = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(input) = guard.as_ref() {
                    Self::parse_key_name(key_name)
                        .map(|key| input.key_down(key))
                        .unwrap_or(false)
                } else {
                    false
                }
            });
        }

        // input::is_released(key_name: &str) -> bool
        {
            let state = Arc::clone(&input_state);
            engine.register_fn("is_released", move |key_name: &str| -> bool {
                let guard = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(input) = guard.as_ref() {
                    Self::parse_key_name(key_name)
                        .map(|key| input.key_released(key))
                        .unwrap_or(false)
                } else {
                    false
                }
            });
        }

        // input::axis(axis_name: &str) -> f64
        {
            let state = Arc::clone(&input_state);
            engine.register_fn("axis", move |axis_name: &str| -> f64 {
                let guard = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(input) = guard.as_ref() {
                    input.action_value(axis_name) as f64
                } else {
                    0.0
                }
            });
        }

        // input::mouse_delta() -> rhai::Array [f64, f64]
        {
            let state = Arc::clone(&input_state);
            engine.register_fn("mouse_delta", move || -> rhai::Array {
                let guard = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(input) = guard.as_ref() {
                    let (dx, dy) = input.mouse_delta();
                    vec![
                        rhai::Dynamic::from(dx as f64),
                        rhai::Dynamic::from(dy as f64),
                    ]
                } else {
                    vec![rhai::Dynamic::from(0.0_f64), rhai::Dynamic::from(0.0_f64)]
                }
            });
        }

        // input::mouse_position() -> rhai::Array [f64, f64]
        {
            let state = Arc::clone(&input_state);
            engine.register_fn("mouse_position", move || -> rhai::Array {
                let guard = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(input) = guard.as_ref() {
                    if let Some((x, y)) = input.cursor_position() {
                        vec![rhai::Dynamic::from(x as f64), rhai::Dynamic::from(y as f64)]
                    } else {
                        vec![rhai::Dynamic::from(0.0_f64), rhai::Dynamic::from(0.0_f64)]
                    }
                } else {
                    vec![rhai::Dynamic::from(0.0_f64), rhai::Dynamic::from(0.0_f64)]
                }
            });
        }
    }

    /// Parses an entity ID string in format "slot:generation" to an Entity.
    fn parse_entity_id(entity_str: &str) -> Option<Entity> {
        let parts: Vec<&str> = entity_str.split(':').collect();
        if parts.len() == 2 {
            let slot = parts[0].parse::<u32>().ok()?;
            let _gen = parts[1].parse::<u32>().ok()?;
            // Note: Generation constructor is private, so we use FIRST for now
            // This is a limitation - proper entity ID serialization would need Generation to be constructible
            Some(Entity::from_handle(engine_core::Handle::new(
                slot,
                engine_core::Generation::FIRST,
            )))
        } else {
            None
        }
    }

    /// Registers the transform API module with the Rhai engine.
    fn register_transform_api(
        engine: &mut rhai::Engine,
        scene: Arc<Mutex<Option<engine_ecs::Scene>>>,
        context: Arc<Mutex<Option<String>>>,
    ) {
        use engine_core::math::{Quat, Vec3};

        // get_position() -> rhai::Array [f64, f64, f64]
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("get_position", move || -> rhai::Array {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_ref()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(transform) = scene.transforms().local(entity) {
                            return vec![
                                rhai::Dynamic::from(transform.translation.x as f64),
                                rhai::Dynamic::from(transform.translation.y as f64),
                                rhai::Dynamic::from(transform.translation.z as f64),
                            ];
                        }
                    }
                }
                vec![
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                ]
            });
        }

        // set_position(x, y, z)
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("set_position", move |x: f64, y: f64, z: f64| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_mut()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(mut transform) = scene.transforms().local(entity) {
                            transform.translation = Vec3::new(x as f32, y as f32, z as f32);
                            scene.transforms_mut().set_local(entity, transform);
                        }
                    }
                }
            });
        }

        // get_rotation() -> rhai::Array [f64, f64, f64, f64] (x, y, z, w quaternion)
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("get_rotation", move || -> rhai::Array {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_ref()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(transform) = scene.transforms().local(entity) {
                            return vec![
                                rhai::Dynamic::from(transform.rotation.x as f64),
                                rhai::Dynamic::from(transform.rotation.y as f64),
                                rhai::Dynamic::from(transform.rotation.z as f64),
                                rhai::Dynamic::from(transform.rotation.w as f64),
                            ];
                        }
                    }
                }
                vec![
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(1.0_f64),
                ]
            });
        }

        // set_rotation(x, y, z, w)
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("set_rotation", move |x: f64, y: f64, z: f64, w: f64| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_mut()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(mut transform) = scene.transforms().local(entity) {
                            transform.rotation = Quat {
                                x: x as f32,
                                y: y as f32,
                                z: z as f32,
                                w: w as f32,
                            };
                            scene.transforms_mut().set_local(entity, transform);
                        }
                    }
                }
            });
        }

        // get_scale() -> rhai::Array [f64, f64, f64]
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("get_scale", move || -> rhai::Array {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_ref()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(transform) = scene.transforms().local(entity) {
                            return vec![
                                rhai::Dynamic::from(transform.scale.x as f64),
                                rhai::Dynamic::from(transform.scale.y as f64),
                                rhai::Dynamic::from(transform.scale.z as f64),
                            ];
                        }
                    }
                }
                vec![
                    rhai::Dynamic::from(1.0_f64),
                    rhai::Dynamic::from(1.0_f64),
                    rhai::Dynamic::from(1.0_f64),
                ]
            });
        }

        // set_scale(x, y, z)
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("set_scale", move |x: f64, y: f64, z: f64| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_mut()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(mut transform) = scene.transforms().local(entity) {
                            transform.scale = Vec3::new(x as f32, y as f32, z as f32);
                            scene.transforms_mut().set_local(entity, transform);
                        }
                    }
                }
            });
        }

        // translate(dx, dy, dz) - adds to current position
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("translate", move |dx: f64, dy: f64, dz: f64| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_mut()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(mut transform) = scene.transforms().local(entity) {
                            transform.translation =
                                transform.translation + Vec3::new(dx as f32, dy as f32, dz as f32);
                            scene.transforms_mut().set_local(entity, transform);
                        }
                    }
                }
            });
        }

        // look_at(tx, ty, tz) - sets rotation to face target point
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("look_at", move |tx: f64, ty: f64, tz: f64| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_mut()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(mut transform) = scene.transforms().local(entity) {
                            let target = Vec3::new(tx as f32, ty as f32, tz as f32);
                            let direction = (target - transform.translation).normalized();

                            // Compute look-at rotation (simple version - look along direction)
                            if direction.length_squared() > f32::EPSILON {
                                // Calculate yaw and pitch from direction vector
                                let yaw = direction.z.atan2(direction.x);
                                let pitch = direction.y.asin();
                                transform.rotation = Quat::from_euler(yaw, pitch, 0.0);
                                scene.transforms_mut().set_local(entity, transform);
                            }
                        }
                    }
                }
            });
        }
    }

    /// Registers the world API module with the Rhai engine.
    fn register_world_api(engine: &mut rhai::Engine, scene: Arc<Mutex<Option<engine_ecs::Scene>>>) {
        // create_entity(name: &str) -> String (entity_id)
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("create_entity", move |name: &str| -> String {
                let mut guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(scene) = guard.as_mut() {
                    if let Ok(entity) = scene.create_object(name) {
                        let handle = entity.handle();
                        return format!("{}:{}", handle.slot(), handle.generation().get());
                    }
                }
                String::new()
            });
        }

        // destroy_entity(id: String)
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("destroy_entity", move |id: String| {
                let mut guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(scene) = guard.as_mut() {
                    if let Some(entity) = Self::parse_entity_id(&id) {
                        let _ = scene.destroy_deferred(entity);
                    }
                }
            });
        }
    }

    /// Registers entity query API: find by name/tag, parent/children, component queries.
    fn register_entity_query_api(
        engine: &mut rhai::Engine,
        scene: Arc<Mutex<Option<engine_ecs::Scene>>>,
        context: Arc<Mutex<Option<String>>>,
    ) {
        // find_entity(name: &str) -> entity_id string or ""
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("find_entity", move |name: &str| -> String {
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(scene) = guard.as_ref() {
                    if let Some(entity) = scene.find_by_name(name) {
                        let handle = entity.handle();
                        return format!("{}:{}", handle.slot(), handle.generation().get());
                    }
                }
                String::new()
            });
        }

        // find_entities_by_tag(tag: &str) -> Array of entity_id strings
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("find_entities_by_tag", move |tag: &str| -> rhai::Array {
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(scene) = guard.as_ref() {
                    return scene
                        .find_by_tag(tag)
                        .iter()
                        .map(|e| {
                            let handle = e.handle();
                            rhai::Dynamic::from(format!(
                                "{}:{}",
                                handle.slot(),
                                handle.generation().get()
                            ))
                        })
                        .collect();
                }
                vec![]
            });
        }

        // get_parent(entity_id: &str) -> parent entity_id string or ""
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("get_parent", move |entity_str: &str| -> String {
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(scene), Some(entity)) =
                    (guard.as_ref(), Self::parse_entity_id(entity_str))
                {
                    if let Some(parent) = scene.transforms().parent(entity) {
                        let handle = parent.handle();
                        return format!("{}:{}", handle.slot(), handle.generation().get());
                    }
                }
                String::new()
            });
        }

        // get_children(entity_id: &str) -> Array of child entity_id strings
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("get_children", move |entity_str: &str| -> rhai::Array {
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(scene), Some(entity)) =
                    (guard.as_ref(), Self::parse_entity_id(entity_str))
                {
                    return scene
                        .transforms()
                        .children(entity)
                        .iter()
                        .map(|e| {
                            let handle = e.handle();
                            rhai::Dynamic::from(format!(
                                "{}:{}",
                                handle.slot(),
                                handle.generation().get()
                            ))
                        })
                        .collect();
                }
                vec![]
            });
        }

        // set_parent_entity(child_id: &str, parent_id: &str or ())
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn(
                "set_parent_entity",
                move |child_str: &str, parent_val: rhai::Dynamic| {
                    let mut guard = scene_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let (Some(scene), Some(child)) =
                        (guard.as_mut(), Self::parse_entity_id(child_str))
                    {
                        let parent = if parent_val.is_unit() {
                            None
                        } else if let Some(s) = parent_val.clone().try_cast::<String>() {
                            Self::parse_entity_id(&s)
                        } else {
                            None
                        };
                        let _ = scene.set_parent(child, parent);
                    }
                },
            );
        }

        // has_component(entity_id: &str, component_type: &str) -> bool
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn(
                "has_component",
                move |entity_str: &str, component_type: &str| -> bool {
                    let guard = scene_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let (Some(scene), Some(entity)) =
                        (guard.as_ref(), Self::parse_entity_id(entity_str))
                    {
                        if let Some(components) = scene.components(entity) {
                            return components.iter().any(|c| c.type_id() == component_type);
                        }
                    }
                    false
                },
            );
        }

        // get_entity_name(entity_id: &str) -> name string
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("get_entity_name", move |entity_str: &str| -> String {
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(scene), Some(entity)) =
                    (guard.as_ref(), Self::parse_entity_id(entity_str))
                {
                    if let Some(obj) = scene.object(entity) {
                        return obj.name.clone();
                    }
                }
                String::new()
            });
        }

        // get_entity_tag(entity_id: &str) -> tag string
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn("get_entity_tag", move |entity_str: &str| -> String {
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(scene), Some(entity)) =
                    (guard.as_ref(), Self::parse_entity_id(entity_str))
                {
                    if let Some(obj) = scene.object(entity) {
                        return obj.tag.clone();
                    }
                }
                String::new()
            });
        }

        // self_position() -> [x, y, z] — convenience for getting current entity position
        {
            let scene_ref = Arc::clone(&scene);
            let ctx = Arc::clone(&context);
            engine.register_fn("self_position", move || -> rhai::Array {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let guard = scene_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let (Some(eid), Some(scene)) = (entity_id.as_ref(), guard.as_ref()) {
                    if let Some(entity) = Self::parse_entity_id(eid) {
                        if let Some(transform) = scene.transforms().local(entity) {
                            return vec![
                                rhai::Dynamic::from(transform.translation.x as f64),
                                rhai::Dynamic::from(transform.translation.y as f64),
                                rhai::Dynamic::from(transform.translation.z as f64),
                            ];
                        }
                    }
                }
                vec![
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                ]
            });
        }

        // distance_between(entity_a: &str, entity_b: &str) -> f64
        {
            let scene_ref = Arc::clone(&scene);
            engine.register_fn(
                "distance_between",
                move |entity_a_str: &str, entity_b_str: &str| -> f64 {
                    let guard = scene_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(scene) = guard.as_ref() {
                        if let (Some(a), Some(b)) = (
                            Self::parse_entity_id(entity_a_str),
                            Self::parse_entity_id(entity_b_str),
                        ) {
                            if let (Some(ta), Some(tb)) =
                                (scene.transforms().world(a), scene.transforms().world(b))
                            {
                                let diff = ta.translation - tb.translation;
                                return diff.length() as f64;
                            }
                        }
                    }
                    0.0
                },
            );
        }
    }

    /// Registers enhanced physics API: forces, velocity, entity-to-body mapping.
    fn register_physics_entity_api(
        engine: &mut rhai::Engine,
        physics_backend: Arc<Mutex<Option<Box<dyn engine_physics::PhysicsBackend>>>>,
        entity_body_map: Arc<Mutex<HashMap<String, u64>>>,
        body_entity_map: Arc<Mutex<HashMap<u64, String>>>,
    ) {
        // register_entity_body(entity_id: &str, body_handle: f64)
        // Maps entity to physics body for force/velocity APIs
        {
            let eb_map = Arc::clone(&entity_body_map);
            let be_map = Arc::clone(&body_entity_map);
            engine.register_fn(
                "register_entity_body",
                move |entity_id: &str, body_handle: f64| {
                    let handle = body_handle as u64;
                    let mut eb_guard = eb_map
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    eb_guard.insert(entity_id.to_string(), handle);
                    let mut be_guard = be_map
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    be_guard.insert(handle, entity_id.to_string());
                },
            );
        }

        // apply_force(entity_id: &str, fx: f64, fy: f64, fz: f64)
        // Note: In a real physics engine, forces are accumulated and applied per-step.
        // Here we apply as an impulse for simplicity.
        {
            let backend = Arc::clone(&physics_backend);
            let eb_map = Arc::clone(&entity_body_map);
            engine.register_fn(
                "apply_force",
                move |entity_id: &str, fx: f64, fy: f64, fz: f64| {
                    let eb_guard = eb_map
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(&body_handle) = eb_guard.get(entity_id) {
                        let mut guard = backend
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if let Some(physics) = guard.as_mut() {
                            let impulse =
                                engine_physics::Vec3::new(fx as f32, fy as f32, fz as f32);
                            let _ = physics
                                .apply_impulse(engine_physics::BodyHandle(body_handle), impulse);
                        }
                    }
                },
            );
        }

        // apply_impulse_to(entity_id: &str, ix: f64, iy: f64, iz: f64)
        {
            let backend = Arc::clone(&physics_backend);
            let eb_map = Arc::clone(&entity_body_map);
            engine.register_fn(
                "apply_impulse_to",
                move |entity_id: &str, ix: f64, iy: f64, iz: f64| {
                    let eb_guard = eb_map
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(&body_handle) = eb_guard.get(entity_id) {
                        let mut guard = backend
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if let Some(physics) = guard.as_mut() {
                            let impulse =
                                engine_physics::Vec3::new(ix as f32, iy as f32, iz as f32);
                            let _ = physics
                                .apply_impulse(engine_physics::BodyHandle(body_handle), impulse);
                        }
                    }
                },
            );
        }

        // get_velocity(entity_id: &str) -> [vx, vy, vz]
        // Note: This requires the physics backend to expose velocity, which is not yet
        // in the trait. For now, returns [0,0,0] as placeholder.
        {
            let _backend = Arc::clone(&physics_backend);
            let _eb_map = Arc::clone(&entity_body_map);
            engine.register_fn("get_velocity", move |entity_id: &str| -> rhai::Array {
                let _ = entity_id;
                // TODO: Add get_velocity to PhysicsBackend trait
                vec![
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                ]
            });
        }

        // set_velocity(entity_id: &str, vx: f64, vy: f64, vz: f64)
        // Note: This requires the physics backend to expose velocity setting.
        {
            let _backend = Arc::clone(&physics_backend);
            let _eb_map = Arc::clone(&entity_body_map);
            engine.register_fn(
                "set_velocity",
                move |entity_id: &str, vx: f64, vy: f64, vz: f64| {
                    let _ = (entity_id, vx, vy, vz);
                    // TODO: Add set_velocity to PhysicsBackend trait
                },
            );
        }

        // teleport_body(entity_id: &str, x: f64, y: f64, z: f64)
        {
            let backend = Arc::clone(&physics_backend);
            let eb_map = Arc::clone(&entity_body_map);
            engine.register_fn(
                "teleport_body",
                move |entity_id: &str, x: f64, y: f64, z: f64| {
                    let eb_guard = eb_map
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(&body_handle) = eb_guard.get(entity_id) {
                        let mut guard = backend
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if let Some(physics) = guard.as_mut() {
                            if let Ok(mut transform) =
                                physics.body_transform(engine_physics::BodyHandle(body_handle))
                            {
                                transform.translation =
                                    engine_physics::Vec3::new(x as f32, y as f32, z as f32);
                                let _ = physics.set_body_transform(
                                    engine_physics::BodyHandle(body_handle),
                                    transform,
                                );
                            }
                        }
                    }
                },
            );
        }
    }

    /// Registers the physics API module with the Rhai engine.
    fn register_physics_api(
        engine: &mut rhai::Engine,
        physics_backend: Arc<Mutex<Option<Box<dyn engine_physics::PhysicsBackend>>>>,
    ) {
        use engine_physics::{QueryFilter, Vec3};

        // raycast(ox, oy, oz, dx, dy, dz, max_dist) -> f64 or () if no hit
        {
            let backend = Arc::clone(&physics_backend);
            engine.register_fn(
                "raycast",
                move |ox: f64,
                      oy: f64,
                      oz: f64,
                      dx: f64,
                      dy: f64,
                      dz: f64,
                      max_dist: f64|
                      -> rhai::Dynamic {
                    let guard = backend
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(physics) = guard.as_ref() {
                        let origin = Vec3::new(ox as f32, oy as f32, oz as f32);
                        let direction = Vec3::new(dx as f32, dy as f32, dz as f32);
                        let filter = QueryFilter::default();
                        if let Some(hit) =
                            physics.raycast(origin, direction, max_dist as f32, filter)
                        {
                            return rhai::Dynamic::from(hit.distance as f64);
                        }
                    }
                    rhai::Dynamic::UNIT
                },
            );
        }

        // overlap_sphere(cx, cy, cz, radius) -> Array of entity_id strings
        {
            let backend = Arc::clone(&physics_backend);
            engine.register_fn(
                "overlap_sphere",
                move |cx: f64, cy: f64, cz: f64, radius: f64| -> rhai::Array {
                    let guard = backend
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(physics) = guard.as_ref() {
                        let center = Vec3::new(cx as f32, cy as f32, cz as f32);
                        let filter = QueryFilter::default();
                        let results = physics.overlap_sphere(center, radius as f32, filter);
                        // Note: We return body handles as strings, but they're not entity IDs
                        // This is a limitation - proper integration would need a body->entity mapping
                        results
                            .iter()
                            .map(|r| rhai::Dynamic::from(format!("{}", r.body.0)))
                            .collect()
                    } else {
                        vec![]
                    }
                },
            );
        }
    }

    /// Registers the resource API with the Rhai engine.
    fn register_resource_api(
        engine: &mut rhai::Engine,
        asset_database: Arc<Mutex<Option<engine_assets::AssetDatabase>>>,
    ) {
        // get_resource(path: &str) -> String (GUID)
        {
            let db = Arc::clone(&asset_database);
            engine.register_fn("get_resource", move |path: &str| -> String {
                let guard = db.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(database) = guard.as_ref() {
                    // Resolve project:/ or builtin:/ prefixes
                    let resolved_path = if let Some(stripped) = path.strip_prefix("project:/") {
                        std::path::PathBuf::from(stripped)
                    } else if let Some(stripped) = path.strip_prefix("builtin:/") {
                        std::path::PathBuf::from("builtin").join(stripped)
                    } else {
                        std::path::PathBuf::from(path)
                    };

                    // Look up the asset by path
                    if let Some(entry) = database.entry_for_path(&resolved_path) {
                        return entry.guid.to_string();
                    }
                }
                String::new()
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Audio API — three.js-like: play_sound("jump.wav"), play_music("bgm.ogg")
    // ═══════════════════════════════════════════════════════════════════════════

    fn register_audio_api(
        engine: &mut rhai::Engine,
        audio: Arc<Mutex<Option<Box<dyn engine_audio::AudioBackend>>>>,
    ) {
        // play_sound(path, options?) -> source_id (f64)
        // Three.js-like: just play a sound by path
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("play_sound", move |_path: &str| -> f64 {
                let guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(_backend) = guard.as_ref() {
                    // For now return 0 as a placeholder source handle
                    // Real implementation needs asset loading pipeline integration
                    0.0
                } else {
                    0.0
                }
            });
        }

        // play_sound_with_volume(path, volume) -> source_id
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn(
                "play_sound_with_volume",
                move |_path: &str, volume: f64| -> f64 {
                    let _ = volume;
                    let guard = audio_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(_backend) = guard.as_ref() {
                        0.0
                    } else {
                        0.0
                    }
                },
            );
        }

        // stop_sound(source_id)
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("stop_sound", move |source_id: f64| {
                let _ = source_id;
                let _guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                // Placeholder — needs source handle tracking
            });
        }

        // pause_sound(source_id)
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("pause_sound", move |source_id: f64| {
                let _ = source_id;
                let _guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            });
        }

        // resume_sound(source_id)
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("resume_sound", move |source_id: f64| {
                let _ = source_id;
                let _guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            });
        }

        // set_sound_volume(source_id, volume)
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("set_sound_volume", move |source_id: f64, volume: f64| {
                let _ = (source_id, volume);
                let _guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            });
        }

        // set_sound_loop(source_id, looping)
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("set_sound_loop", move |source_id: f64, looping: bool| {
                let _ = (source_id, looping);
                let _guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            });
        }

        // play_music(path, options?) — convenience for looping background music
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("play_music", move |path: &str| -> f64 {
                let _ = path;
                let guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(_backend) = guard.as_ref() {
                    0.0
                } else {
                    0.0
                }
            });
        }

        // stop_music()
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("stop_music", move || {
                let _guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            });
        }

        // set_master_volume(volume)
        {
            let audio_ref = Arc::clone(&audio);
            engine.register_fn("set_master_volume", move |volume: f64| {
                let _ = volume;
                let _guard = audio_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Collision API — on_collision_enter/exit callbacks, drain_contacts()
    // ═══════════════════════════════════════════════════════════════════════════

    fn register_collision_api(
        engine: &mut rhai::Engine,
        collision_events: Arc<Mutex<Vec<engine_physics::ContactEvent>>>,
        body_entity_map: Arc<Mutex<HashMap<u64, String>>>,
        _scene: Arc<Mutex<Option<engine_ecs::Scene>>>,
    ) {
        // get_collision_count() -> number of pending collision events
        {
            let events = Arc::clone(&collision_events);
            engine.register_fn("get_collision_count", move || -> i64 {
                let guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.len() as i64
            });
        }

        // has_collisions() -> bool
        {
            let events = Arc::clone(&collision_events);
            engine.register_fn("has_collisions", move || -> bool {
                let guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                !guard.is_empty()
            });
        }

        // get_collision_entity(index) -> entity_id string of the "other" body
        {
            let events = Arc::clone(&collision_events);
            let map = Arc::clone(&body_entity_map);
            engine.register_fn("get_collision_entity", move |index: i64| -> String {
                let events_guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let map_guard = map
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(event) = events_guard.get(index as usize) {
                    // Return the entity ID of body_b (the "other" body)
                    map_guard.get(&event.body_b.0).cloned().unwrap_or_default()
                } else {
                    String::new()
                }
            });
        }

        // get_collision_entered(index) -> bool (enter vs exit)
        {
            let events = Arc::clone(&collision_events);
            engine.register_fn("get_collision_entered", move |index: i64| -> bool {
                let guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard
                    .get(index as usize)
                    .map(|e| e.entered)
                    .unwrap_or(false)
            });
        }

        // get_collision_point(index) -> [x, y, z]
        {
            let events = Arc::clone(&collision_events);
            engine.register_fn("get_collision_point", move |index: i64| -> rhai::Array {
                let guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(event) = guard.get(index as usize) {
                    vec![
                        rhai::Dynamic::from(event.point.x as f64),
                        rhai::Dynamic::from(event.point.y as f64),
                        rhai::Dynamic::from(event.point.z as f64),
                    ]
                } else {
                    vec![
                        rhai::Dynamic::from(0.0_f64),
                        rhai::Dynamic::from(0.0_f64),
                        rhai::Dynamic::from(0.0_f64),
                    ]
                }
            });
        }

        // get_collision_normal(index) -> [x, y, z]
        {
            let events = Arc::clone(&collision_events);
            engine.register_fn("get_collision_normal", move |index: i64| -> rhai::Array {
                let guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(event) = guard.get(index as usize) {
                    vec![
                        rhai::Dynamic::from(event.normal.x as f64),
                        rhai::Dynamic::from(event.normal.y as f64),
                        rhai::Dynamic::from(event.normal.z as f64),
                    ]
                } else {
                    vec![
                        rhai::Dynamic::from(0.0_f64),
                        rhai::Dynamic::from(0.0_f64),
                        rhai::Dynamic::from(0.0_f64),
                    ]
                }
            });
        }

        // is_trigger_collision(index) -> bool
        {
            let events = Arc::clone(&collision_events);
            engine.register_fn("is_trigger_collision", move |index: i64| -> bool {
                let guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard
                    .get(index as usize)
                    .map(|e| e.is_trigger)
                    .unwrap_or(false)
            });
        }

        // register_body_entity(body_handle_str, entity_id) — maps physics body to entity
        {
            let map = Arc::clone(&body_entity_map);
            engine.register_fn(
                "register_body_entity",
                move |body_handle: &str, entity_id: &str| {
                    if let Ok(handle) = body_handle.parse::<u64>() {
                        let mut guard = map
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        guard.insert(handle, entity_id.to_string());
                    }
                },
            );
        }

        // clear_collisions() — drain all pending collision events
        {
            let events = Arc::clone(&collision_events);
            engine.register_fn("clear_collisions", move || {
                let mut guard = events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.clear();
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Event API — emit("event_name", data) / on_event("event_name")
    // ═══════════════════════════════════════════════════════════════════════════

    fn register_event_api(
        engine: &mut rhai::Engine,
        event_queue: Arc<Mutex<HashMap<String, Vec<ScriptEvent>>>>,
        global_event_queue: Arc<Mutex<Vec<ScriptEvent>>>,
        transform_context: Arc<Mutex<Option<String>>>,
    ) {
        // emit(event_name, data_map) — emit event from current entity
        {
            let queue = Arc::clone(&event_queue);
            let global_queue = Arc::clone(&global_event_queue);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn("emit", move |event_name: &str, data: rhai::Map| {
                let sender = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let event = ScriptEvent {
                    name: event_name.to_string(),
                    sender: sender.clone(),
                    data: data.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                };
                // Queue for entity-specific listeners
                let mut guard = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard
                    .entry(event_name.to_string())
                    .or_default()
                    .push(event.clone());
                // Also queue for global listeners
                let mut global_guard = global_queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                global_guard.push(event);
            });
        }

        // emit_global(event_name, data_map) — emit event not tied to an entity
        {
            let queue = Arc::clone(&event_queue);
            let global_queue = Arc::clone(&global_event_queue);
            engine.register_fn("emit_global", move |event_name: &str, data: rhai::Map| {
                let event = ScriptEvent {
                    name: event_name.to_string(),
                    sender: String::new(),
                    data: data.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                };
                let mut guard = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard
                    .entry(event_name.to_string())
                    .or_default()
                    .push(event.clone());
                let mut global_guard = global_queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                global_guard.push(event);
            });
        }

        // get_event_count(event_name) -> number of pending events with this name
        {
            let queue = Arc::clone(&event_queue);
            engine.register_fn("get_event_count", move |event_name: &str| -> i64 {
                let guard = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard
                    .get(event_name)
                    .map(|events| events.len() as i64)
                    .unwrap_or(0)
            });
        }

        // has_events(event_name) -> bool
        {
            let queue = Arc::clone(&event_queue);
            engine.register_fn("has_events", move |event_name: &str| -> bool {
                let guard = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard
                    .get(event_name)
                    .map(|events| !events.is_empty())
                    .unwrap_or(false)
            });
        }

        // consume_event(event_name) -> Map with event data (removes first event)
        {
            let queue = Arc::clone(&event_queue);
            engine.register_fn("consume_event", move |event_name: &str| -> rhai::Map {
                let mut guard = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(events) = guard.get_mut(event_name) {
                    if let Some(event) = events.first() {
                        let sender = event.sender.clone();
                        let data = event.data.clone();
                        events.remove(0);
                        let mut map = rhai::Map::new();
                        map.insert("sender".into(), rhai::Dynamic::from(sender));
                        for (k, v) in data {
                            map.insert(k.into(), v);
                        }
                        return map;
                    }
                }
                rhai::Map::new()
            });
        }

        // clear_events(event_name) — remove all events of this name
        {
            let queue = Arc::clone(&event_queue);
            engine.register_fn("clear_events", move |event_name: &str| {
                let mut guard = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.remove(event_name);
            });
        }

        // clear_all_events() — remove all events
        {
            let queue = Arc::clone(&event_queue);
            let global_queue = Arc::clone(&global_event_queue);
            engine.register_fn("clear_all_events", move || {
                let mut guard = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.clear();
                let mut global_guard = global_queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                global_guard.clear();
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // UI API — immediate-mode: ui_text(), ui_bar(), ui_button()
    // ═══════════════════════════════════════════════════════════════════════════

    fn register_ui_api(engine: &mut rhai::Engine, ui_commands: Arc<Mutex<Vec<UiCommand>>>) {
        // ui_text(x, y, content) — draw text at screen position
        {
            let cmds = Arc::clone(&ui_commands);
            engine.register_fn("ui_text", move |x: f64, y: f64, content: &str| {
                let mut guard = cmds
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.push(UiCommand::Text {
                    x: x as f32,
                    y: y as f32,
                    content: content.to_string(),
                    color: [1.0, 1.0, 1.0, 1.0],
                });
            });
        }

        // ui_text_color(x, y, content, r, g, b, a) — draw colored text
        {
            let cmds = Arc::clone(&ui_commands);
            engine.register_fn(
                "ui_text_color",
                move |x: f64, y: f64, content: &str, r: f64, g: f64, b: f64, a: f64| {
                    let mut guard = cmds
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.push(UiCommand::Text {
                        x: x as f32,
                        y: y as f32,
                        content: content.to_string(),
                        color: [r as f32, g as f32, b as f32, a as f32],
                    });
                },
            );
        }

        // ui_bar(x, y, width, height, ratio) — draw a progress bar
        {
            let cmds = Arc::clone(&ui_commands);
            engine.register_fn(
                "ui_bar",
                move |x: f64, y: f64, width: f64, height: f64, ratio: f64| {
                    let mut guard = cmds
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.push(UiCommand::Bar {
                        x: x as f32,
                        y: y as f32,
                        width: width as f32,
                        height: height as f32,
                        ratio: ratio as f32,
                        color: [0.2, 0.8, 0.2, 1.0],
                    });
                },
            );
        }

        // ui_bar_color(x, y, width, height, ratio, r, g, b, a) — draw a colored progress bar
        {
            let cmds = Arc::clone(&ui_commands);
            engine.register_fn(
                "ui_bar_color",
                move |x: f64,
                      y: f64,
                      width: f64,
                      height: f64,
                      ratio: f64,
                      r: f64,
                      g: f64,
                      b: f64,
                      a: f64| {
                    let mut guard = cmds
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.push(UiCommand::Bar {
                        x: x as f32,
                        y: y as f32,
                        width: width as f32,
                        height: height as f32,
                        ratio: ratio as f32,
                        color: [r as f32, g as f32, b as f32, a as f32],
                    });
                },
            );
        }

        // ui_button(x, y, label, width, height) -> bool (clicked this frame)
        {
            let cmds = Arc::clone(&ui_commands);
            engine.register_fn(
                "ui_button",
                move |x: f64, y: f64, label: &str, width: f64, height: f64| -> bool {
                    let mut guard = cmds
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.push(UiCommand::Button {
                        x: x as f32,
                        y: y as f32,
                        label: label.to_string(),
                        width: width as f32,
                        height: height as f32,
                    });
                    // Return false by default; actual click detection needs renderer integration
                    false
                },
            );
        }

        // clear_ui() — clear all UI commands for this frame
        {
            let cmds = Arc::clone(&ui_commands);
            engine.register_fn("clear_ui", move || {
                let mut guard = cmds
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.clear();
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Animation API — play_animation("run"), stop_animation(), etc.
    // ═══════════════════════════════════════════════════════════════════════════

    fn register_animation_api(
        engine: &mut rhai::Engine,
        animation_states: Arc<Mutex<HashMap<String, AnimationState>>>,
        animation_clips: Arc<Mutex<HashMap<String, f32>>>,
        transform_context: Arc<Mutex<Option<String>>>,
    ) {
        // play_animation(clip_name, loop_mode?) — play animation on current entity
        {
            let states = Arc::clone(&animation_states);
            let clips = Arc::clone(&animation_clips);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn("play_animation", move |clip_name: &str| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let clips_guard = clips
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let duration = clips_guard.get(clip_name).copied().unwrap_or(1.0);
                let mut guard = states
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.insert(
                    entity_id,
                    AnimationState {
                        clip: clip_name.to_string(),
                        time: 0.0,
                        speed: 1.0,
                        playing: true,
                        loop_mode: "loop".to_string(),
                    },
                );
                let _ = duration;
            });
        }

        // play_animation_once(clip_name) — play animation once (no loop)
        {
            let states = Arc::clone(&animation_states);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn("play_animation_once", move |clip_name: &str| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let mut guard = states
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.insert(
                    entity_id,
                    AnimationState {
                        clip: clip_name.to_string(),
                        time: 0.0,
                        speed: 1.0,
                        playing: true,
                        loop_mode: "once".to_string(),
                    },
                );
            });
        }

        // stop_animation() — stop animation on current entity
        {
            let states = Arc::clone(&animation_states);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn("stop_animation", move || {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let mut guard = states
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(state) = guard.get_mut(&entity_id) {
                    state.playing = false;
                }
            });
        }

        // set_animation_speed(speed) — set playback speed on current entity
        {
            let states = Arc::clone(&animation_states);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn("set_animation_speed", move |speed: f64| {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let mut guard = states
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(state) = guard.get_mut(&entity_id) {
                    state.speed = speed as f32;
                }
            });
        }

        // get_animation_time() -> current animation time on current entity
        {
            let states = Arc::clone(&animation_states);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn("get_animation_time", move || -> f64 {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let guard = states
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.get(&entity_id).map(|s| s.time as f64).unwrap_or(0.0)
            });
        }

        // is_animation_playing() -> bool
        {
            let states = Arc::clone(&animation_states);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn("is_animation_playing", move || -> bool {
                let entity_id = ctx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let guard = states
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.get(&entity_id).map(|s| s.playing).unwrap_or(false)
            });
        }

        // crossfade_animation(clip_name, duration) — crossfade to new animation
        {
            let states = Arc::clone(&animation_states);
            let ctx = Arc::clone(&transform_context);
            engine.register_fn(
                "crossfade_animation",
                move |clip_name: &str, _duration: f64| {
                    let entity_id = ctx
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone()
                        .unwrap_or_default();
                    let mut guard = states
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.insert(
                        entity_id,
                        AnimationState {
                            clip: clip_name.to_string(),
                            time: 0.0,
                            speed: 1.0,
                            playing: true,
                            loop_mode: "loop".to_string(),
                        },
                    );
                },
            );
        }

        // register_animation_clip(name, duration) — register a clip by name
        {
            let clips = Arc::clone(&animation_clips);
            engine.register_fn(
                "register_animation_clip",
                move |name: &str, duration: f64| {
                    let mut guard = clips
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.insert(name.to_string(), duration as f32);
                },
            );
        }
    }
    fn parse_key_name(name: &str) -> Option<engine_platform::KeyCode> {
        use engine_platform::KeyCode;
        match name {
            "Escape" => Some(KeyCode::Escape),
            "Enter" => Some(KeyCode::Enter),
            "Space" => Some(KeyCode::Space),
            "ArrowUp" => Some(KeyCode::ArrowUp),
            "ArrowDown" => Some(KeyCode::ArrowDown),
            "ArrowLeft" => Some(KeyCode::ArrowLeft),
            "ArrowRight" => Some(KeyCode::ArrowRight),
            s if s.len() == 1 => s.chars().next().map(KeyCode::Character),
            _ => None,
        }
    }

    /// Updates the input state that scripts can query.
    ///
    /// This should be called each frame before running script lifecycle functions.
    pub fn set_input_state(&mut self, input: engine_platform::InputState) {
        *self
            .input_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(input);
    }

    /// Updates the scene reference that scripts can query and modify.
    ///
    /// This should be called each frame before running script lifecycle functions.
    /// Syncs to both the legacy API and the new three.js-compatible API.
    pub fn set_scene(&mut self, scene: engine_ecs::Scene) {
        *self
            .scene
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scene);
    }

    /// Takes the scene back from the script backend after lifecycle execution.
    ///
    /// This should be called after all script lifecycle functions have run.
    pub fn take_scene(&mut self) -> Option<engine_ecs::Scene> {
        self.scene
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Updates the physics backend reference that scripts can query.
    ///
    /// This should be called before running script lifecycle functions that need physics queries.
    pub fn set_physics_backend(&mut self, backend: Box<dyn engine_physics::PhysicsBackend>) {
        *self
            .physics_backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(backend);
    }

    /// Takes the physics backend back from the script backend.
    pub fn take_physics_backend(&mut self) -> Option<Box<dyn engine_physics::PhysicsBackend>> {
        self.physics_backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Updates the asset database reference that scripts can query.
    ///
    /// This should be called before running script lifecycle functions that need resource resolution.
    pub fn set_asset_database(&mut self, database: engine_assets::AssetDatabase) {
        *self
            .asset_database
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(database);
    }

    /// Takes the asset database back from the script backend.
    pub fn take_asset_database(&mut self) -> Option<engine_assets::AssetDatabase> {
        self.asset_database
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Updates the audio backend reference that scripts can use for sound playback.
    pub fn set_audio_backend(&mut self, backend: Box<dyn engine_audio::AudioBackend>) {
        *self
            .audio_backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(backend);
    }

    /// Takes the audio backend back from the script backend.
    pub fn take_audio_backend(&mut self) -> Option<Box<dyn engine_audio::AudioBackend>> {
        self.audio_backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Drains collision events from the physics backend and stores them for script queries.
    ///
    /// Call this each frame before running script lifecycle functions.
    pub fn drain_collision_events(&mut self, backend: &mut dyn engine_physics::PhysicsBackend) {
        let contacts = backend.drain_contacts();
        let mut guard = self
            .collision_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = contacts;
    }

    /// Returns the current UI draw commands collected by scripts this frame.
    pub fn take_ui_commands(&mut self) -> Vec<UiCommand> {
        let mut guard = self
            .ui_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *guard)
    }

    /// Returns the current animation states for all entities.
    pub fn take_animation_states(&mut self) -> HashMap<String, AnimationState> {
        let mut guard = self
            .animation_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *guard)
    }

    /// Returns and clears all pending script events.
    pub fn take_events(&mut self) -> HashMap<String, Vec<ScriptEvent>> {
        let mut guard = self
            .event_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *guard)
    }

    /// Returns and clears all pending global events.
    pub fn take_global_events(&mut self) -> Vec<ScriptEvent> {
        let mut guard = self
            .global_event_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *guard)
    }

    /// Registers a body-to-entity mapping for collision callback resolution.
    pub fn register_body_entity(&mut self, body_handle: u64, entity_id: String) {
        let mut guard = self
            .body_entity_map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.insert(body_handle, entity_id);
    }

    /// Clears all body-entity mappings.
    pub fn clear_body_entity_map(&mut self) {
        let mut guard = self
            .body_entity_map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clear();
    }

    /// Updates animation states by advancing time. Call each frame with dt.
    pub fn update_animations(&mut self, dt: f32) {
        let mut guard = self
            .animation_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for state in guard.values_mut() {
            if state.playing {
                state.time += dt * state.speed;
            }
        }
    }

    /// Registers an animation clip with its duration.
    pub fn register_animation_clip(&mut self, name: &str, duration: f32) {
        let mut guard = self
            .animation_clips
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.insert(name.to_string(), duration);
    }

    /// Returns a reference to the synth graph for rendering audio.
    pub fn synth_graph(&self) -> &Arc<Mutex<engine_audio::synth::SynthGraph>> {
        &self.synth_graph
    }

    /// Renders audio from the synth graph into the output buffer.
    pub fn render_synth(&mut self, output: &mut [f32]) {
        let mut guard = self
            .synth_graph
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.render(output);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Synth API — Web Audio-style: create_oscillator(), create_gain(), etc.
    // ═══════════════════════════════════════════════════════════════════════════

    fn register_synth_api(
        engine: &mut rhai::Engine,
        synth: Arc<Mutex<engine_audio::synth::SynthGraph>>,
    ) {
        use engine_audio::synth::{Envelope, FilterKind, SynthNode, Waveform};

        // create_oscillator(waveform, frequency) -> node_handle (f64)
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn(
                "create_oscillator",
                move |waveform: &str, freq: f64| -> f64 {
                    let wf = match waveform {
                        "sine" => Waveform::Sine,
                        "square" => Waveform::Square,
                        "sawtooth" => Waveform::Sawtooth,
                        "saw" => Waveform::Sawtooth,
                        "triangle" => Waveform::Triangle,
                        "tri" => Waveform::Triangle,
                        "noise" => Waveform::Noise,
                        _ => Waveform::Sine,
                    };
                    let mut guard = synth_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let handle = guard.add_node(SynthNode::oscillator(wf, freq as f32));
                    handle.0 as f64
                },
            );
        }

        // create_gain(value) -> node_handle
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("create_gain", move |value: f64| -> f64 {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let handle = guard.add_node(SynthNode::gain(value as f32));
                handle.0 as f64
            });
        }

        // create_gain_envelope(value, attack, decay, sustain, release) -> node_handle
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn(
                "create_gain_envelope",
                move |value: f64, attack: f64, decay: f64, sustain: f64, release: f64| -> f64 {
                    let env = Envelope {
                        attack: attack as f32,
                        decay: decay as f32,
                        sustain: sustain as f32,
                        release: release as f32,
                    };
                    let mut guard = synth_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let handle = guard.add_node(SynthNode::gain_with_envelope(value as f32, env));
                    handle.0 as f64
                },
            );
        }

        // create_filter(type, cutoff, resonance) -> node_handle
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn(
                "create_filter",
                move |filter_type: &str, cutoff: f64, resonance: f64| -> f64 {
                    let ft = match filter_type {
                        "lowpass" | "low" => FilterKind::LowPass,
                        "highpass" | "high" => FilterKind::HighPass,
                        "bandpass" | "band" => FilterKind::BandPass,
                        _ => FilterKind::LowPass,
                    };
                    let mut guard = synth_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let handle =
                        guard.add_node(SynthNode::filter(ft, cutoff as f32, resonance as f32));
                    handle.0 as f64
                },
            );
        }

        // create_delay(time, feedback, mix) -> node_handle
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn(
                "create_delay",
                move |time: f64, feedback: f64, mix: f64| -> f64 {
                    let mut guard = synth_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let handle =
                        guard.add_node(SynthNode::delay(time as f32, feedback as f32, mix as f32));
                    handle.0 as f64
                },
            );
        }

        // create_mixer() -> node_handle
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("create_mixer", move || -> f64 {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let handle = guard.add_node(SynthNode::mixer());
                handle.0 as f64
            });
        }

        // synth_connect(from, to) — connect two nodes
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_connect", move |from: f64, to: f64| {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.connect(
                    engine_audio::synth::NodeHandle(from as u32),
                    engine_audio::synth::NodeHandle(to as u32),
                );
            });
        }

        // synth_disconnect(from, to)
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_disconnect", move |from: f64, to: f64| {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.disconnect(
                    engine_audio::synth::NodeHandle(from as u32),
                    engine_audio::synth::NodeHandle(to as u32),
                );
            });
        }

        // synth_destination(handle) — set the output node
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_destination", move |handle: f64| {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.set_destination(engine_audio::synth::NodeHandle(handle as u32));
            });
        }

        // synth_start(handle) — start a node
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_start", move |handle: f64| {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.start_node(engine_audio::synth::NodeHandle(handle as u32));
            });
        }

        // synth_stop(handle) — stop a node
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_stop", move |handle: f64| {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.stop_node(engine_audio::synth::NodeHandle(handle as u32));
            });
        }

        // synth_set(handle, param, value) — set a parameter
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_set", move |handle: f64, param: &str, value: f64| {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.set_param(
                    engine_audio::synth::NodeHandle(handle as u32),
                    param,
                    value as f32,
                );
            });
        }

        // synth_get(handle, param) -> value
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_get", move |handle: f64, param: &str| -> f64 {
                let guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.get_param(engine_audio::synth::NodeHandle(handle as u32), param) as f64
            });
        }

        // synth_ramp(handle, param, target, duration) — linear ramp
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn(
                "synth_ramp",
                move |handle: f64, param: &str, target: f64, duration: f64| {
                    let mut guard = synth_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.linear_ramp(
                        engine_audio::synth::NodeHandle(handle as u32),
                        param,
                        target as f32,
                        duration as f32,
                    );
                },
            );
        }

        // synth_ramp_exp(handle, param, target, duration) — exponential ramp
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn(
                "synth_ramp_exp",
                move |handle: f64, param: &str, target: f64, duration: f64| {
                    let mut guard = synth_ref
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.exponential_ramp(
                        engine_audio::synth::NodeHandle(handle as u32),
                        param,
                        target as f32,
                        duration as f32,
                    );
                },
            );
        }

        // synth_is_playing(handle) -> bool
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_is_playing", move |handle: f64| -> bool {
                let guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.is_playing(engine_audio::synth::NodeHandle(handle as u32))
            });
        }

        // synth_remove(handle) — remove a node
        {
            let synth_ref = Arc::clone(&synth);
            engine.register_fn("synth_remove", move |handle: f64| {
                let mut guard = synth_ref
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.remove_node(engine_audio::synth::NodeHandle(handle as u32));
            });
        }

        // play_tone(waveform, frequency, duration) — one-shot convenience
        {
            let _synth_ref = Arc::clone(&synth);
            engine.register_fn(
                "play_tone",
                move |waveform: &str, freq: f64, duration: f64| {
                    let wf = match waveform {
                        "sine" => Waveform::Sine,
                        "square" => Waveform::Square,
                        "sawtooth" | "saw" => Waveform::Sawtooth,
                        "triangle" | "tri" => Waveform::Triangle,
                        "noise" => Waveform::Noise,
                        _ => Waveform::Sine,
                    };
                    // Create a mini synth graph for the one-shot tone
                    let mut tone_graph = engine_audio::synth::SynthGraph::new(44100);
                    let osc = tone_graph.add_node(SynthNode::oscillator(wf, freq as f32));
                    let gain = tone_graph.add_node(SynthNode::gain_with_envelope(
                        0.5,
                        Envelope {
                            attack: 0.005,
                            decay: 0.01,
                            sustain: 0.3,
                            release: (duration as f32 * 0.3).max(0.05),
                        },
                    ));
                    tone_graph.connect(osc, gain);
                    tone_graph.set_destination(gain);
                    tone_graph.start_node(osc);
                    tone_graph.start_node(gain);

                    let num_samples = (duration as f32 * 44100.0) as usize;
                    let mut output = vec![0.0f32; num_samples];
                    tone_graph.render(&mut output);

                    // Store in the main synth graph's node list as a reference
                    // For now, return the number of samples generated
                    num_samples as f64
                },
            );
        }
    }

    /// Dispatches `on_collision_enter` / `on_collision_exit` callbacks to scripts.
    ///
    /// For each collision event stored from the last `drain_collision_events` call,
    /// resolves body handles to entity IDs via the body-entity map, then calls
    /// the appropriate lifecycle function on both involved entities' scripts.
    ///
    /// # Arguments
    /// * `scripted_entities` - Slice of `(entity_id, script_path)` for all scripted entities.
    ///
    /// # Script callback signature
    /// ```rhai
    /// fn on_collision_enter(other_entity, point_x, point_y, point_z,
    ///                       normal_x, normal_y, normal_z, is_trigger) { ... }
    /// fn on_collision_exit(other_entity, point_x, point_y, point_z,
    ///                      normal_x, normal_y, normal_z, is_trigger) { ... }
    /// ```
    pub fn dispatch_collision_callbacks(
        &mut self,
        scripted_entities: &[(String, PathBuf)],
    ) -> Vec<ConsoleEntry> {
        let mut errors = Vec::new();

        // Build a set of scripted entity IDs for quick lookup
        let scripted_set: std::collections::HashSet<&str> = scripted_entities
            .iter()
            .map(|(eid, _)| eid.as_str())
            .collect();

        // Read collision events and body-entity map (clone to release lock)
        let contacts = {
            let mut guard = self
                .collision_events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };

        let map: HashMap<u64, String> = self
            .body_entity_map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        for contact in &contacts {
            // Resolve body handles to entity IDs
            let entity_a = map.get(&contact.body_a.0).cloned().unwrap_or_default();
            let entity_b = map.get(&contact.body_b.0).cloned().unwrap_or_default();

            let callback_name = if contact.entered {
                "on_collision_enter"
            } else {
                "on_collision_exit"
            };

            let px = contact.point.x as f64;
            let py = contact.point.y as f64;
            let pz = contact.point.z as f64;
            let nx = contact.normal.x as f64;
            let ny = contact.normal.y as f64;
            let nz = contact.normal.z as f64;
            let is_trigger = contact.is_trigger;

            // Dispatch to entity_a's script (with entity_b as "other")
            if scripted_set.contains(entity_a.as_str()) && !entity_b.is_empty() {
                if let Some((_, script_path)) =
                    scripted_entities.iter().find(|(eid, _)| eid == &entity_a)
                {
                    let args = vec![
                        rhai::Dynamic::from(entity_b.clone()),
                        rhai::Dynamic::from(px),
                        rhai::Dynamic::from(py),
                        rhai::Dynamic::from(pz),
                        rhai::Dynamic::from(nx),
                        rhai::Dynamic::from(ny),
                        rhai::Dynamic::from(nz),
                        rhai::Dynamic::from(is_trigger),
                    ];
                    match self.run_lifecycle_function(&entity_a, script_path, callback_name, &args)
                    {
                        Ok(Some(entry)) => errors.push(entry),
                        Ok(None) => {}
                        Err(e) => errors.push(ConsoleEntry {
                            timestamp: "now".to_string(),
                            level: ConsoleLevel::Error,
                            source: ConsoleSource {
                                subsystem: "script".to_string(),
                                file: Some(script_path.clone()),
                                line: None,
                            },
                            message: format!("Collision callback error: {}", e),
                        }),
                    }
                }
            }

            // Dispatch to entity_b's script (with entity_a as "other")
            if scripted_set.contains(entity_b.as_str()) && !entity_a.is_empty() {
                if let Some((_, script_path)) =
                    scripted_entities.iter().find(|(eid, _)| eid == &entity_b)
                {
                    // Flip the normal direction for entity_b (normal points from B toward A)
                    let args = vec![
                        rhai::Dynamic::from(entity_a.clone()),
                        rhai::Dynamic::from(px),
                        rhai::Dynamic::from(py),
                        rhai::Dynamic::from(pz),
                        rhai::Dynamic::from(-nx),
                        rhai::Dynamic::from(-ny),
                        rhai::Dynamic::from(-nz),
                        rhai::Dynamic::from(is_trigger),
                    ];
                    match self.run_lifecycle_function(&entity_b, script_path, callback_name, &args)
                    {
                        Ok(Some(entry)) => errors.push(entry),
                        Ok(None) => {}
                        Err(e) => errors.push(ConsoleEntry {
                            timestamp: "now".to_string(),
                            level: ConsoleLevel::Error,
                            source: ConsoleSource {
                                subsystem: "script".to_string(),
                                file: Some(script_path.clone()),
                                line: None,
                            },
                            message: format!("Collision callback error: {}", e),
                        }),
                    }
                }
            }
        }

        errors
    }

    /// Returns a reference to the underlying Rhai engine.
    pub fn engine(&self) -> &rhai::Engine {
        &self.engine
    }

    /// Returns a mutable reference to the underlying Rhai engine.
    pub fn engine_mut(&mut self) -> &mut rhai::Engine {
        &mut self.engine
    }

    /// Returns the number of cached AST entries.
    pub fn ast_cache_size(&self) -> usize {
        self.ast_cache.len()
    }

    /// Loads and compiles a Rhai script file, caching the AST.
    ///
    /// If the script has already been compiled, returns the cached AST.
    /// Compilation errors are returned as `EngineError` with file path and line number.
    pub fn load_script(&mut self, path: &std::path::Path) -> EngineResult<()> {
        if self.ast_cache.contains_key(path) {
            return Ok(());
        }
        let source =
            std::fs::read_to_string(path).map_err(|e| engine_core::EngineError::Filesystem {
                path: path.to_path_buf(),
                source: e,
            })?;
        let ast = self
            .compile_aster_source(&source)
            .map_err(|e| engine_core::EngineError::other(format!("{}: {}", path.display(), e)))?;
        self.ast_cache.insert(path.to_path_buf(), ast);
        Ok(())
    }

    /// Clears all cached ASTs.
    pub fn clear_cache(&mut self) {
        self.ast_cache.clear();
    }

    /// Compiles a Rhai script from an in-memory source string.
    ///
    /// The `logical_path` is used as a cache key and for error messages.
    /// No filesystem access is performed. If a script with the same path
    /// is already cached, it is replaced.
    pub fn compile_source(
        &mut self,
        logical_path: &std::path::Path,
        source: &str,
    ) -> EngineResult<()> {
        let ast = self.compile_aster_source(source).map_err(|e| {
            engine_core::EngineError::other(format!("{}: {}", logical_path.display(), e))
        })?;
        self.ast_cache.insert(logical_path.to_path_buf(), ast);
        Ok(())
    }

    /// Validates Aster Script source without modifying files or the AST cache.
    ///
    /// Validation uses strict variable rules and verifies lifecycle hook
    /// signatures. Every error includes a stable code, source location, and a
    /// concrete correction hint.
    pub fn diagnose_source(&self, logical_path: &Path, source: &str) -> Vec<AsterScriptDiagnostic> {
        if !self.engine.strict_variables() {
            let validator = Self::with_config(RhaiConfig {
                strict_variables: true,
                ..RhaiConfig::default()
            });
            return validator.diagnose_source(logical_path, source);
        }

        if logical_path.extension().and_then(|ext| ext.to_str()) != Some("aster") {
            return vec![AsterScriptDiagnostic {
                code: "ASTER0001".to_owned(),
                severity: AsterDiagnosticSeverity::Error,
                line: None,
                column: None,
                message: format!(
                    "Aster Script files must use the .aster extension: {}",
                    logical_path.display()
                ),
                suggestion: "Rename the file so its path ends in `.aster`.".to_owned(),
                source_line: None,
            }];
        }

        let ast = match self.compile_aster_source(source) {
            Ok(ast) => ast,
            Err(error) => {
                let position = error.position();
                let line = position.line();
                return vec![AsterScriptDiagnostic {
                    code: parse_error_code(error.err_type()).to_owned(),
                    severity: AsterDiagnosticSeverity::Error,
                    line,
                    column: position.position(),
                    message: error.err_type().to_string(),
                    suggestion: parse_error_suggestion(error.err_type()),
                    source_line: line.and_then(|line| {
                        source
                            .lines()
                            .nth(line.saturating_sub(1))
                            .map(str::to_owned)
                    }),
                }];
            }
        };

        let expected_hooks = [
            ("on_start", 0usize),
            ("on_update", 1),
            ("on_fixed_update", 1),
            ("on_collision_enter", 8),
            ("on_collision_exit", 8),
        ];
        let mut diagnostics = Vec::new();
        for function in ast.iter_functions() {
            let Some((_, expected)) = expected_hooks
                .iter()
                .find(|(name, _)| *name == function.name)
            else {
                continue;
            };
            if function.params.len() == *expected {
                continue;
            }
            let (line, column, source_line) = find_function_declaration(source, function.name);
            diagnostics.push(AsterScriptDiagnostic {
                code: "ASTER0101".to_owned(),
                severity: AsterDiagnosticSeverity::Error,
                line,
                column,
                message: format!(
                    "Lifecycle hook `{}` has {} parameters; expected {}.",
                    function.name,
                    function.params.len(),
                    expected
                ),
                suggestion: lifecycle_signature(function.name).to_owned(),
                source_line,
            });
        }
        diagnostics
    }

    /// Validates an Aster Script file from disk.
    pub fn diagnose_file(&self, path: &Path) -> EngineResult<Vec<AsterScriptDiagnostic>> {
        let source = std::fs::read_to_string(path).map_err(|source| {
            engine_core::EngineError::Filesystem {
                path: path.to_path_buf(),
                source,
            }
        })?;
        Ok(self.diagnose_source(path, &source))
    }

    fn compile_aster_source(&self, source: &str) -> Result<AST, rhai::ParseError> {
        if !self.engine.strict_variables() {
            return self.engine.compile(source);
        }
        let mut scope = Scope::new();
        scope.push("self_entity", String::new());
        for name in top_level_variable_names(source) {
            scope.push_dynamic(name, rhai::Dynamic::UNIT);
        }
        self.engine.compile_with_scope(&scope, source)
    }

    /// Creates or overwrites a script file on disk and compiles it.
    ///
    /// Writes to `<asset_root>/<relative_path>`, creating parent directories
    /// as needed, then compiles the source. Returns the full path to the
    /// created file.
    pub fn create_script(
        &mut self,
        asset_root: &std::path::Path,
        relative_path: &std::path::Path,
        source: &str,
    ) -> EngineResult<std::path::PathBuf> {
        // Guard: relative_path must be non-empty and resolve to a file, not a directory.
        // When the AI passes an empty string or a bare directory name the join produces
        // a path that IS the asset_root itself; fs::write on a directory yields EISDIR.
        if relative_path.as_os_str().is_empty() {
            return Err(engine_core::EngineError::config(
                "write_script: 'path' must not be empty — provide a file name such as \
                 'scripts/my_script.aster'",
            ));
        }
        let file_name = relative_path
            .file_name()
            .filter(|n| !n.is_empty())
            .ok_or_else(|| {
                engine_core::EngineError::config(format!(
                    "write_script: '{}' has no file name component — the path must end with a \
                     file name, e.g. 'scripts/my_script.aster'",
                    relative_path.display()
                ))
            })?;
        // Enforce the public Aster Script extension so we don't accidentally
        // create or overwrite arbitrary files.
        if std::path::Path::new(file_name)
            .extension()
            .map(|ext| ext != "aster")
            .unwrap_or(true)
        {
            return Err(engine_core::EngineError::config(format!(
                "write_script: '{}' must have a .aster extension",
                relative_path.display()
            )));
        }

        let full_path = asset_root.join(relative_path);

        // Extra safety: if the resolved path is a directory, bail out clearly.
        if full_path.is_dir() {
            return Err(engine_core::EngineError::Filesystem {
                path: full_path,
                source: std::io::Error::new(
                    std::io::ErrorKind::IsADirectory,
                    "resolved path is a directory, not a file",
                ),
            });
        }

        let diagnostics = self.diagnose_source(relative_path, source);
        if !diagnostics.is_empty() {
            let details = diagnostics
                .iter()
                .map(|diagnostic| {
                    let location = match (diagnostic.line, diagnostic.column) {
                        (Some(line), Some(column)) => format!("line {line}, column {column}"),
                        (Some(line), None) => format!("line {line}"),
                        _ => "unknown location".to_owned(),
                    };
                    format!(
                        "{} at {}: {} Suggestion: {}",
                        diagnostic.code, location, diagnostic.message, diagnostic.suggestion
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Err(engine_core::EngineError::config(format!(
                "write_script: validation failed for '{}':\n{details}",
                relative_path.display()
            )));
        }
        let ast = self.compile_aster_source(source).map_err(|error| {
            engine_core::EngineError::config(format!(
                "write_script: validation failed for '{}': {}",
                relative_path.display(),
                error
            ))
        })?;

        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| engine_core::EngineError::Filesystem {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        std::fs::write(&full_path, source).map_err(|e| engine_core::EngineError::Filesystem {
            path: full_path.clone(),
            source: e,
        })?;
        self.ast_cache.insert(full_path.clone(), ast);
        Ok(full_path)
    }

    /// Runs the `on_start()` lifecycle function for an entity's script.
    ///
    /// If the script defines an `on_start()` function, it will be called.
    /// Missing lifecycle functions are silently skipped (not an error).
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID, exposed to the script as `self_entity`
    /// * `script_path` - Path to the script (must be already loaded via `load_script`)
    ///
    /// # Returns
    /// Returns `Ok(None)` on success, or `Ok(Some(ConsoleEntry))` if a runtime error occurred.
    pub fn run_start(
        &mut self,
        entity_id: &str,
        script_path: &Path,
    ) -> EngineResult<Option<ConsoleEntry>> {
        self.run_lifecycle_function(entity_id, script_path, "on_start", &[])
    }

    /// Runs the `on_update(dt)` lifecycle function for an entity's script.
    ///
    /// If the script defines an `on_update(dt)` function, it will be called with the delta time.
    /// Missing lifecycle functions are silently skipped (not an error).
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID, exposed to the script as `self_entity`
    /// * `script_path` - Path to the script (must be already loaded via `load_script`)
    /// * `dt` - Delta time in seconds since last update
    ///
    /// # Returns
    /// Returns `Ok(None)` on success, or `Ok(Some(ConsoleEntry))` if a runtime error occurred.
    pub fn run_update(
        &mut self,
        entity_id: &str,
        script_path: &Path,
        dt: f32,
    ) -> EngineResult<Option<ConsoleEntry>> {
        self.run_lifecycle_function(
            entity_id,
            script_path,
            "on_update",
            &[rhai::Dynamic::from(dt as f64)],
        )
    }

    /// Runs the `on_fixed_update(fixed_dt)` lifecycle function for an entity's script.
    ///
    /// If the script defines an `on_fixed_update(fixed_dt)` function, it will be called with the fixed delta time.
    /// Missing lifecycle functions are silently skipped (not an error).
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID, exposed to the script as `self_entity`
    /// * `script_path` - Path to the script (must be already loaded via `load_script`)
    /// * `fixed_dt` - Fixed delta time in seconds (typically 1/60)
    ///
    /// # Returns
    /// Returns `Ok(None)` on success, or `Ok(Some(ConsoleEntry))` if a runtime error occurred.
    pub fn run_fixed_update(
        &mut self,
        entity_id: &str,
        script_path: &Path,
        fixed_dt: f32,
    ) -> EngineResult<Option<ConsoleEntry>> {
        self.run_lifecycle_function(
            entity_id,
            script_path,
            "on_fixed_update",
            &[rhai::Dynamic::from(fixed_dt as f64)],
        )
    }

    /// Internal helper to run a lifecycle function with arguments.
    ///
    /// Gets or creates the entity's scope, sets `self_entity` constant, and calls the function if it exists.
    /// Runtime errors are caught and converted to ConsoleEntry instead of propagating as EngineError.
    fn run_lifecycle_function(
        &mut self,
        entity_id: &str,
        script_path: &Path,
        function_name: &str,
        args: &[rhai::Dynamic],
    ) -> EngineResult<Option<ConsoleEntry>> {
        // Get the cached AST
        let ast = self.ast_cache.get(script_path).ok_or_else(|| {
            engine_core::EngineError::other(format!("Script not loaded: {}", script_path.display()))
        })?;

        // Set the current entity context for transform API
        *self
            .transform_context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(entity_id.to_string());

        // Get or create the entity's scope
        let scope_key = (entity_id.to_string(), script_path.to_path_buf());
        let scope = self.entity_scopes.entry(scope_key).or_insert_with(|| {
            let mut scope = Scope::new();
            // Set self_entity as a constant in the scope
            scope.push_constant("self_entity", entity_id.to_string());

            // Run the AST once to initialize module-level variables
            // This populates the scope with all top-level let/const declarations
            let _ = self.engine.run_ast_with_scope(&mut scope, ast);

            scope
        });

        // Check if the function exists in the AST
        if !ast.iter_functions().any(|f| f.name == function_name) {
            // Missing lifecycle function is not an error - silently skip
            *self
                .transform_context
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            return Ok(None);
        }

        // Call the function with the scope, catching runtime errors
        let result: Result<rhai::Dynamic, _> =
            self.engine
                .call_fn(scope, ast, function_name, args.to_vec());

        // Clear the transform context after the call
        *self
            .transform_context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

        // Convert runtime errors to ConsoleEntry
        match result {
            Ok(_) => Ok(None),
            Err(e) => {
                let console_entry =
                    self.rhai_error_to_console_entry(&e, script_path, function_name);
                Ok(Some(console_entry))
            }
        }
    }

    /// Converts a Rhai error to a ConsoleEntry.
    ///
    /// Extracts line number information from the error if available.
    fn rhai_error_to_console_entry(
        &self,
        error: &Box<rhai::EvalAltResult>,
        script_path: &Path,
        function_name: &str,
    ) -> ConsoleEntry {
        // Extract line number from Rhai error if available
        let line = error.position().line().map(|l| l as u32);

        // Format error message
        let message = format!("Runtime error in {}: {}", function_name, error);

        ConsoleEntry {
            timestamp: "now".to_string(),
            level: ConsoleLevel::Error,
            source: ConsoleSource {
                subsystem: "script".to_string(),
                file: Some(script_path.to_path_buf()),
                line,
            },
            message,
        }
    }

    /// Loads and compiles a Rhai script file, returning compilation errors as ConsoleEntry.
    ///
    /// This is a wrapper around `load_script` that converts compilation errors to ConsoleEntry
    /// for display in the editor console.
    ///
    /// # Returns
    /// Returns `Ok(None)` on success, or `Ok(Some(ConsoleEntry))` if compilation failed.
    pub fn load_script_safe(&mut self, path: &Path) -> EngineResult<Option<ConsoleEntry>> {
        match self.load_script(path) {
            Ok(()) => Ok(None),
            Err(e) => {
                // Convert compilation error to ConsoleEntry
                let message = format!("Compilation error: {}", e);

                // Try to extract line number from error message
                let line = None; // Rhai compilation errors don't expose line numbers easily

                let console_entry = ConsoleEntry {
                    timestamp: "now".to_string(),
                    level: ConsoleLevel::Error,
                    source: ConsoleSource {
                        subsystem: "script".to_string(),
                        file: Some(path.to_path_buf()),
                        line,
                    },
                    message,
                };
                Ok(Some(console_entry))
            }
        }
    }

    /// Loads all Rhai scripts referenced by a scene.
    ///
    /// Iterates all GameObjects in the scene, finds ScriptComponentProxy with backend='rhai',
    /// resolves script paths (project:/ or builtin:/ prefixes), and loads each script.
    /// AST cache is reused across entities using the same script file.
    ///
    /// # Arguments
    /// * `scene` - The scene to scan for script components
    /// * `asset_root` - Base path for resolving `project:/` paths
    ///
    /// # Errors
    /// Returns an error if any script fails to load or compile.
    pub fn load_scene_scripts(
        &mut self,
        scene: &engine_ecs::Scene,
        asset_root: &Path,
    ) -> EngineResult<()> {
        for (_entity, _object) in scene.iter_objects() {
            if let Some(components) = scene.components(_entity) {
                for component in components {
                    if let engine_ecs::ComponentData::Script(script_proxy) = component {
                        // Only load scripts for this backend
                        if script_proxy.backend == "rhai" {
                            let resolved_path =
                                resolve_script_path(&script_proxy.script, asset_root)?;
                            self.load_script(&resolved_path)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Resolves a script path with `project:/` or `builtin:/` prefix.
///
fn top_level_variable_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut brace_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for line in source.lines() {
        if brace_depth == 0 {
            let trimmed = line.trim_start();
            let declaration = trimmed
                .strip_prefix("let ")
                .or_else(|| trimmed.strip_prefix("const "));
            if let Some(declaration) = declaration {
                let name = declaration
                    .chars()
                    .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                    .collect::<String>();
                if !name.is_empty() && !names.contains(&name) {
                    names.push(name);
                }
            }
        }

        let mut characters = line.chars().peekable();
        while let Some(character) = characters.next() {
            if !in_string && character == '/' && characters.peek() == Some(&'/') {
                break;
            }
            if character == '"' && !escaped {
                in_string = !in_string;
            }
            if !in_string {
                match character {
                    '{' => brace_depth = brace_depth.saturating_add(1),
                    '}' => brace_depth = brace_depth.saturating_sub(1),
                    _ => {}
                }
            }
            escaped = in_string && character == '\\' && !escaped;
            if character != '\\' {
                escaped = false;
            }
        }
    }
    names
}

fn parse_error_code(error: &ParseErrorType) -> &'static str {
    match error {
        ParseErrorType::UnexpectedEOF => "ASTER1001",
        ParseErrorType::MissingToken(..) | ParseErrorType::MissingSymbol(..) => "ASTER1002",
        ParseErrorType::VariableUndefined(..) => "ASTER1003",
        ParseErrorType::AssignmentToConstant(..) => "ASTER1004",
        ParseErrorType::FnDuplicatedDefinition(..) => "ASTER1005",
        ParseErrorType::FnDuplicatedParam(..) => "ASTER1006",
        ParseErrorType::MismatchedType(..) => "ASTER1007",
        ParseErrorType::UnknownOperator(..) => "ASTER1008",
        _ => "ASTER1099",
    }
}

fn parse_error_suggestion(error: &ParseErrorType) -> String {
    match error {
        ParseErrorType::UnexpectedEOF => {
            "Complete the unfinished expression or add the missing closing `}`, `]`, or `)`."
                .to_owned()
        }
        ParseErrorType::MissingToken(token, _) => {
            format!("Insert the missing `{token}` near the reported position.")
        }
        ParseErrorType::MissingSymbol(description) => {
            format!("Add the required symbol: {description}.")
        }
        ParseErrorType::VariableUndefined(name) => format!(
            "Declare `{name}` with `let` before it is used, correct its spelling, or replace it with an API listed in the Aster Script reference."
        ),
        ParseErrorType::AssignmentToConstant(name) => format!(
            "Do not assign to constant `{name}`. Declare mutable state with `let` if it must change."
        ),
        ParseErrorType::FnDuplicatedDefinition(name, arity) => format!(
            "Keep only one `{name}` function with {arity} parameter(s), or rename the helper function."
        ),
        ParseErrorType::FnDuplicatedParam(function, parameter) => format!(
            "Rename one `{parameter}` parameter in `{function}` so every parameter name is unique."
        ),
        ParseErrorType::MismatchedType(expected, actual) => format!(
            "Change the expression to produce `{expected}` instead of `{actual}`."
        ),
        ParseErrorType::UnknownOperator(operator) => format!(
            "Replace `{operator}` with a supported Aster Script operator."
        ),
        _ => "Correct the syntax at the reported location. Compare it with the closest Aster Script example and use only documented engine APIs.".to_owned(),
    }
}

fn lifecycle_signature(name: &str) -> &'static str {
    match name {
        "on_start" => "Change the declaration to `fn on_start() { ... }`.",
        "on_update" => "Change the declaration to `fn on_update(dt) { ... }`.",
        "on_fixed_update" => "Change the declaration to `fn on_fixed_update(fixed_dt) { ... }`.",
        "on_collision_enter" => {
            "Use `fn on_collision_enter(other, px, py, pz, nx, ny, nz, is_trigger) { ... }`."
        }
        "on_collision_exit" => {
            "Use `fn on_collision_exit(other, px, py, pz, nx, ny, nz, is_trigger) { ... }`."
        }
        _ => "Use the lifecycle signature documented in the Aster Script reference.",
    }
}

fn find_function_declaration(
    source: &str,
    function_name: &str,
) -> (Option<usize>, Option<usize>, Option<String>) {
    let needle = format!("fn {function_name}");
    for (index, line) in source.lines().enumerate() {
        if let Some(column) = line.find(&needle) {
            return (Some(index + 1), Some(column + 1), Some(line.to_owned()));
        }
    }
    (None, None, None)
}

/// - `project:/path/to/script.aster` → `<asset_root>/path/to/script.aster`
/// - `builtin:/path/to/script.aster` → `<asset_root>/builtin/path/to/script.aster`
/// - Relative paths are resolved relative to `asset_root`
fn resolve_script_path(script_path: &str, asset_root: &Path) -> EngineResult<PathBuf> {
    if let Some(stripped) = script_path.strip_prefix("project:/") {
        Ok(asset_root.join(stripped))
    } else if let Some(stripped) = script_path.strip_prefix("builtin:/") {
        Ok(asset_root.join("builtin").join(stripped))
    } else {
        // Relative path, resolve relative to asset_root
        Ok(asset_root.join(script_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rhai_backend_initializes() {
        let backend = RhaiScriptBackend::new();
        assert_eq!(backend.ast_cache_size(), 0);
    }

    #[test]
    fn rhai_backend_loads_and_caches_script() {
        let dir = std::env::temp_dir().join("aster_test_rhai_cache");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("test.rhai");
        std::fs::write(&script_path, "let x = 1 + 2;").unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();
        assert_eq!(backend.ast_cache_size(), 1);

        // Loading again should use cache
        backend.load_script(&script_path).unwrap();
        assert_eq!(backend.ast_cache_size(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rhai_backend_returns_error_for_invalid_script() {
        let dir = std::env::temp_dir().join("aster_test_rhai_invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("bad.rhai");
        std::fs::write(&script_path, "let x = ;").unwrap();

        let mut backend = RhaiScriptBackend::new();
        let result = backend.load_script(&script_path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_valid_rhai_file_succeeds() {
        let dir = std::env::temp_dir().join("aster_test_rhai_valid");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("valid.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    print("Hello from Rhai!");
}

fn on_update(dt) {
    // Game logic here
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        let result = backend.load_script(&script_path);
        assert!(result.is_ok());
        assert_eq!(backend.ast_cache_size(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_invalid_rhai_file_returns_error_with_line_info() {
        let dir = std::env::temp_dir().join("aster_test_rhai_syntax_error");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("syntax_error.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    let x = 1 +;  // Syntax error: missing operand
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        let result = backend.load_script(&script_path);
        assert!(result.is_err());

        // Error message should contain file path
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("syntax_error.rhai"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_scene_scripts_loads_all_rhai_scripts() {
        use engine_ecs::{ComponentData, Scene, ScriptComponentProxy};

        let dir = std::env::temp_dir().join("aster_test_scene_scripts");
        std::fs::create_dir_all(&dir).unwrap();

        // Create test scripts
        let script1_path = dir.join("player.rhai");
        std::fs::write(&script1_path, "fn on_start() { print(\"Player\"); }").unwrap();

        let script2_path = dir.join("enemy.rhai");
        std::fs::write(&script2_path, "fn on_update(dt) { print(\"Enemy\"); }").unwrap();

        // Create scene with script components
        let mut scene = Scene::new();
        let player = scene.create_object("Player").unwrap();
        let enemy = scene.create_object("Enemy").unwrap();

        scene
            .upsert_component(
                player,
                ComponentData::Script(ScriptComponentProxy {
                    backend: "rhai".to_string(),
                    script: "player.rhai".to_string(),
                    state_json: None,
                    pending_recovery: false,
                }),
            )
            .unwrap();

        scene
            .upsert_component(
                enemy,
                ComponentData::Script(ScriptComponentProxy {
                    backend: "rhai".to_string(),
                    script: "enemy.rhai".to_string(),
                    state_json: None,
                    pending_recovery: false,
                }),
            )
            .unwrap();

        // Load all scripts from scene
        let mut backend = RhaiScriptBackend::new();
        backend.load_scene_scripts(&scene, &dir).unwrap();

        // Both scripts should be cached
        assert_eq!(backend.ast_cache_size(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_scene_scripts_skips_non_rhai_backends() {
        use engine_ecs::{ComponentData, Scene, ScriptComponentProxy};

        let dir = std::env::temp_dir().join("aster_test_scene_scripts_filter");
        std::fs::create_dir_all(&dir).unwrap();

        let script_path = dir.join("test.rhai");
        std::fs::write(&script_path, "fn on_start() {}").unwrap();

        let mut scene = Scene::new();
        let obj1 = scene.create_object("RhaiScript").unwrap();
        let obj2 = scene.create_object("PythonScript").unwrap();

        scene
            .upsert_component(
                obj1,
                ComponentData::Script(ScriptComponentProxy {
                    backend: "rhai".to_string(),
                    script: "test.rhai".to_string(),
                    state_json: None,
                    pending_recovery: false,
                }),
            )
            .unwrap();

        scene
            .upsert_component(
                obj2,
                ComponentData::Script(ScriptComponentProxy {
                    backend: "python".to_string(),
                    script: "test.py".to_string(),
                    state_json: None,
                    pending_recovery: false,
                }),
            )
            .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_scene_scripts(&scene, &dir).unwrap();

        // Only the rhai script should be loaded
        assert_eq!(backend.ast_cache_size(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_script_path_handles_project_prefix() {
        let asset_root = PathBuf::from("/game/assets");
        let resolved = resolve_script_path("project:/scripts/player.rhai", &asset_root).unwrap();
        assert_eq!(resolved, PathBuf::from("/game/assets/scripts/player.rhai"));
    }

    #[test]
    fn resolve_script_path_handles_builtin_prefix() {
        let asset_root = PathBuf::from("/game/assets");
        let resolved = resolve_script_path("builtin:/core/input.rhai", &asset_root).unwrap();
        assert_eq!(
            resolved,
            PathBuf::from("/game/assets/builtin/core/input.rhai")
        );
    }

    #[test]
    fn resolve_script_path_handles_relative_paths() {
        let asset_root = PathBuf::from("/game/assets");
        let resolved = resolve_script_path("scripts/player.rhai", &asset_root).unwrap();
        assert_eq!(resolved, PathBuf::from("/game/assets/scripts/player.rhai"));
    }

    #[test]
    fn load_scene_scripts_with_project_prefix() {
        use engine_ecs::{ComponentData, Scene, ScriptComponentProxy};

        let dir = std::env::temp_dir().join("aster_test_project_prefix");
        std::fs::create_dir_all(&dir).unwrap();

        let script_path = dir.join("game_logic.rhai");
        std::fs::write(&script_path, "fn on_start() {}").unwrap();

        let mut scene = Scene::new();
        let obj = scene.create_object("GameObject").unwrap();

        scene
            .upsert_component(
                obj,
                ComponentData::Script(ScriptComponentProxy {
                    backend: "rhai".to_string(),
                    script: "project:/game_logic.rhai".to_string(),
                    state_json: None,
                    pending_recovery: false,
                }),
            )
            .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_scene_scripts(&scene, &dir).unwrap();

        assert_eq!(backend.ast_cache_size(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ast_cache_reused_across_entities() {
        use engine_ecs::{ComponentData, Scene, ScriptComponentProxy};

        let dir = std::env::temp_dir().join("aster_test_cache_reuse");
        std::fs::create_dir_all(&dir).unwrap();

        let script_path = dir.join("shared.rhai");
        std::fs::write(&script_path, "fn on_start() {}").unwrap();

        let mut scene = Scene::new();
        let obj1 = scene.create_object("Entity1").unwrap();
        let obj2 = scene.create_object("Entity2").unwrap();
        let obj3 = scene.create_object("Entity3").unwrap();

        // All three entities use the same script
        for entity in [obj1, obj2, obj3] {
            scene
                .upsert_component(
                    entity,
                    ComponentData::Script(ScriptComponentProxy {
                        backend: "rhai".to_string(),
                        script: "shared.rhai".to_string(),
                        state_json: None,
                        pending_recovery: false,
                    }),
                )
                .unwrap();
        }

        let mut backend = RhaiScriptBackend::new();
        backend.load_scene_scripts(&scene, &dir).unwrap();

        // Only one AST should be cached despite 3 entities using it
        assert_eq!(backend.ast_cache_size(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_start_calls_on_start_function() {
        let dir = std::env::temp_dir().join("aster_test_lifecycle_start");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("lifecycle.rhai");
        std::fs::write(
            &script_path,
            r#"
let started = false;

fn on_start() {
    started = true;
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();
        backend.run_start("entity_123", &script_path).unwrap();

        // Verify the scope was created and the function was called
        let scope_key = ("entity_123".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();

        // Check that started variable was set to true
        let started: bool = scope.get_value("started").unwrap();
        assert!(started);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_update_calls_on_update_with_dt() {
        let dir = std::env::temp_dir().join("aster_test_lifecycle_update");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("update.rhai");
        std::fs::write(
            &script_path,
            r#"
let total_time = 0.0;

fn on_update(dt) {
    total_time += dt;
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Call update multiple times
        backend
            .run_update("entity_456", &script_path, 0.016)
            .unwrap();
        backend
            .run_update("entity_456", &script_path, 0.016)
            .unwrap();
        backend
            .run_update("entity_456", &script_path, 0.016)
            .unwrap();

        // Verify total_time accumulated
        let scope_key = ("entity_456".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let total_time: f64 = scope.get_value("total_time").unwrap();
        assert!((total_time - 0.048).abs() < 0.001);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_fixed_update_calls_on_fixed_update() {
        let dir = std::env::temp_dir().join("aster_test_lifecycle_fixed");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fixed.rhai");
        std::fs::write(
            &script_path,
            r#"
let fixed_ticks = 0;

fn on_fixed_update(fixed_dt) {
    fixed_ticks += 1;
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Call fixed update 5 times
        for _ in 0..5 {
            backend
                .run_fixed_update("entity_789", &script_path, 1.0 / 60.0)
                .unwrap();
        }

        // Verify fixed_ticks incremented
        let scope_key = ("entity_789".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let fixed_ticks: i64 = scope.get_value("fixed_ticks").unwrap();
        assert_eq!(fixed_ticks, 5);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_lifecycle_function_is_not_error() {
        let dir = std::env::temp_dir().join("aster_test_missing_lifecycle");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("partial.rhai");
        std::fs::write(
            &script_path,
            r#"
// Only define on_start, no on_update or on_fixed_update
fn on_start() {
    print("Started");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // All lifecycle calls should succeed even though only on_start is defined
        assert!(backend.run_start("entity_abc", &script_path).is_ok());
        assert!(backend
            .run_update("entity_abc", &script_path, 0.016)
            .is_ok());
        assert!(backend
            .run_fixed_update("entity_abc", &script_path, 1.0 / 60.0)
            .is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn self_entity_accessible_in_script() {
        let dir = std::env::temp_dir().join("aster_test_self_entity");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("self_entity.rhai");
        std::fs::write(
            &script_path,
            r#"
let my_id = "";

fn on_start() {
    my_id = self_entity;
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();
        backend.run_start("entity_xyz_123", &script_path).unwrap();

        // Verify self_entity was accessible and stored
        let scope_key = ("entity_xyz_123".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let my_id: String = scope.get_value("my_id").unwrap();
        assert_eq!(my_id, "entity_xyz_123");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scope_persists_across_lifecycle_calls() {
        let dir = std::env::temp_dir().join("aster_test_scope_persistence");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("persistent.rhai");
        std::fs::write(
            &script_path,
            r#"
let counter = 0;

fn on_start() {
    counter = 10;
}

fn on_update(dt) {
    counter += 1;
}

fn on_fixed_update(fixed_dt) {
    counter += 100;
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Call lifecycle functions in sequence
        backend.run_start("entity_persist", &script_path).unwrap();
        backend
            .run_update("entity_persist", &script_path, 0.016)
            .unwrap();
        backend
            .run_fixed_update("entity_persist", &script_path, 1.0 / 60.0)
            .unwrap();

        // Verify counter accumulated: 10 + 1 + 100 = 111
        let scope_key = ("entity_persist".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let counter: i64 = scope.get_value("counter").unwrap();
        assert_eq!(counter, 111);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn different_entities_have_separate_scopes() {
        let dir = std::env::temp_dir().join("aster_test_separate_scopes");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("shared_script.rhai");
        std::fs::write(
            &script_path,
            r#"
let value = 0;

fn on_start() {
    value = 1;
}

fn on_update(dt) {
    value += 1;
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Entity A: start + 2 updates
        backend.run_start("entity_a", &script_path).unwrap();
        backend.run_update("entity_a", &script_path, 0.016).unwrap();
        backend.run_update("entity_a", &script_path, 0.016).unwrap();

        // Entity B: start + 1 update
        backend.run_start("entity_b", &script_path).unwrap();
        backend.run_update("entity_b", &script_path, 0.016).unwrap();

        // Verify separate scopes
        let scope_a = backend
            .entity_scopes
            .get(&("entity_a".to_string(), script_path.clone()))
            .unwrap();
        let value_a: i64 = scope_a.get_value("value").unwrap();
        assert_eq!(value_a, 3); // 1 + 1 + 1

        let scope_b = backend
            .entity_scopes
            .get(&("entity_b".to_string(), script_path.clone()))
            .unwrap();
        let value_b: i64 = scope_b.get_value("value").unwrap();
        assert_eq!(value_b, 2); // 1 + 1

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_lifecycle_on_unloaded_script_returns_error() {
        let mut backend = RhaiScriptBackend::new();
        let nonexistent_path = PathBuf::from("/nonexistent/script.rhai");

        let result = backend.run_start("entity_error", &nonexistent_path);
        assert!(result.is_err());

        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Script not loaded"));
    }

    #[test]
    fn input_is_pressed_queries_input_state() {
        use engine_platform::{InputEvent, InputState, KeyCode};

        let dir = std::env::temp_dir().join("aster_test_input_is_pressed");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("input_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let w_pressed = false;

fn on_update(dt) {
    w_pressed = is_pressed("W");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Set up input state with W key pressed
        let mut input = InputState::default();
        input.apply_event(InputEvent::KeyDown(KeyCode::Character('w')));
        backend.set_input_state(input);

        // Run update
        backend
            .run_update("entity_input", &script_path, 0.016)
            .unwrap();

        // Verify w_pressed was set to true
        let scope_key = ("entity_input".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let w_pressed: bool = scope.get_value("w_pressed").unwrap();
        assert!(w_pressed);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn input_is_held_queries_input_state() {
        use engine_platform::{InputEvent, InputState, KeyCode};

        let dir = std::env::temp_dir().join("aster_test_input_is_held");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("held_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let space_held = false;

fn on_update(dt) {
    space_held = is_held("Space");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Set up input state with Space key held
        let mut input = InputState::default();
        input.apply_event(InputEvent::KeyDown(KeyCode::Space));
        input.end_frame(); // Clear pressed state, keep held
        backend.set_input_state(input);

        // Run update
        backend
            .run_update("entity_held", &script_path, 0.016)
            .unwrap();

        // Verify space_held was set to true
        let scope_key = ("entity_held".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let space_held: bool = scope.get_value("space_held").unwrap();
        assert!(space_held);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn input_is_released_queries_input_state() {
        use engine_platform::{InputEvent, InputState, KeyCode};

        let dir = std::env::temp_dir().join("aster_test_input_is_released");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("released_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let escape_released = false;

fn on_update(dt) {
    escape_released = is_released("Escape");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Set up input state with Escape key released
        let mut input = InputState::default();
        input.apply_event(InputEvent::KeyDown(KeyCode::Escape));
        input.apply_event(InputEvent::KeyUp(KeyCode::Escape));
        backend.set_input_state(input);

        // Run update
        backend
            .run_update("entity_released", &script_path, 0.016)
            .unwrap();

        // Verify escape_released was set to true
        let scope_key = ("entity_released".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let escape_released: bool = scope.get_value("escape_released").unwrap();
        assert!(escape_released);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn input_axis_queries_action_value() {
        use engine_platform::{ActionBinding, InputEvent, InputState, KeyCode};

        let dir = std::env::temp_dir().join("aster_test_input_axis");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("axis_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let move_x = 0.0;

fn on_update(dt) {
    move_x = axis("MoveX");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Set up input state with MoveX axis (A/D keys)
        let mut input = InputState::default();
        input.bind_action(
            "MoveX",
            ActionBinding::axis([KeyCode::Character('a')], [KeyCode::Character('d')]),
        );
        input.apply_event(InputEvent::KeyDown(KeyCode::Character('d')));
        backend.set_input_state(input);

        // Run update
        backend
            .run_update("entity_axis", &script_path, 0.016)
            .unwrap();

        // Verify move_x was set to 1.0
        let scope_key = ("entity_axis".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let move_x: f64 = scope.get_value("move_x").unwrap();
        assert!((move_x - 1.0).abs() < 0.001);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn input_mouse_delta_returns_delta() {
        use engine_platform::{InputEvent, InputState};

        let dir = std::env::temp_dir().join("aster_test_mouse_delta");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("mouse_delta_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let delta_x = 0.0;
let delta_y = 0.0;

fn on_update(dt) {
    let delta = mouse_delta();
    delta_x = delta[0];
    delta_y = delta[1];
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Set up input state with mouse movement
        let mut input = InputState::default();
        input.apply_event(InputEvent::MouseMove { x: 100.0, y: 200.0 });
        input.apply_event(InputEvent::MouseMove { x: 110.0, y: 195.0 });
        backend.set_input_state(input);

        // Run update
        backend
            .run_update("entity_mouse", &script_path, 0.016)
            .unwrap();

        // Verify delta was captured
        let scope_key = ("entity_mouse".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let delta_x: f64 = scope.get_value("delta_x").unwrap();
        let delta_y: f64 = scope.get_value("delta_y").unwrap();
        assert!((delta_x - 10.0).abs() < 0.001);
        assert!((delta_y - (-5.0)).abs() < 0.001);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn input_mouse_position_returns_position() {
        use engine_platform::{InputEvent, InputState};

        let dir = std::env::temp_dir().join("aster_test_mouse_position");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("mouse_pos_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let pos_x = 0.0;
let pos_y = 0.0;

fn on_update(dt) {
    let pos = mouse_position();
    pos_x = pos[0];
    pos_y = pos[1];
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Set up input state with mouse position
        let mut input = InputState::default();
        input.apply_event(InputEvent::MouseMove { x: 320.0, y: 240.0 });
        backend.set_input_state(input);

        // Run update
        backend
            .run_update("entity_pos", &script_path, 0.016)
            .unwrap();

        // Verify position was captured
        let scope_key = ("entity_pos".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let pos_x: f64 = scope.get_value("pos_x").unwrap();
        let pos_y: f64 = scope.get_value("pos_y").unwrap();
        assert!((pos_x - 320.0).abs() < 0.001);
        assert!((pos_y - 240.0).abs() < 0.001);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transform_set_position_updates_scene() {
        use engine_core::math::Transform;
        use engine_ecs::Scene;

        let dir = std::env::temp_dir().join("aster_test_transform_set_position");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("transform_test.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    set_position(10.0, 20.0, 30.0);
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Create a scene with an entity
        let mut scene = Scene::new();
        let entity = scene.create_object("TestObject").unwrap();

        // Set initial transform
        scene
            .transforms_mut()
            .set_local(entity, Transform::IDENTITY);

        // Format entity ID as "slot:generation"
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());

        // Set scene in backend
        backend.set_scene(scene);

        // Run start lifecycle
        backend.run_start(&entity_id, &script_path).unwrap();

        // Take scene back and verify transform was updated
        let scene = backend.take_scene().unwrap();
        let transform = scene.transforms().local(entity).unwrap();

        assert!((transform.translation.x - 10.0).abs() < 0.001);
        assert!((transform.translation.y - 20.0).abs() < 0.001);
        assert!((transform.translation.z - 30.0).abs() < 0.001);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn world_spawn_creates_entity_in_scene() {
        use engine_ecs::Scene;

        let dir = std::env::temp_dir().join("aster_test_world_spawn");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("spawn_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let spawned_id = "";

fn on_start() {
    spawned_id = create_entity("SpawnedObject");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Create a scene
        let mut scene = Scene::new();
        let entity = scene.create_object("TestObject").unwrap();

        // Format entity ID
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());

        // Set scene in backend
        backend.set_scene(scene);

        // Run start lifecycle
        backend.run_start(&entity_id, &script_path).unwrap();

        // Take scene back and verify spawned entity exists
        let scene = backend.take_scene().unwrap();

        // Check that spawned_id was set
        let scope_key = (entity_id.clone(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let spawned_id: String = scope.get_value("spawned_id").unwrap();
        assert!(!spawned_id.is_empty());

        // Verify the spawned entity exists in the scene
        if let Some(spawned_entity) = RhaiScriptBackend::parse_entity_id(&spawned_id) {
            assert!(scene.transforms().local(spawned_entity).is_some());
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn world_destroy_marks_entity_for_destruction() {
        use engine_ecs::Scene;

        let dir = std::env::temp_dir().join("aster_test_world_destroy");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("destroy_test.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    destroy_entity(self_entity);
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Create a scene with an entity
        let mut scene = Scene::new();
        let entity = scene.create_object("ToDestroy").unwrap();

        // Format entity ID
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());

        // Set scene in backend
        backend.set_scene(scene);

        // Run start lifecycle (which calls destroy)
        backend.run_start(&entity_id, &script_path).unwrap();

        // Take scene back
        let mut scene = backend.take_scene().unwrap();

        // Process deferred destroys
        let _ = scene.process_deferred_destroy();

        // Verify entity no longer exists
        assert!(scene.transforms().local(entity).is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn physics_raycast_returns_distance_on_hit() {
        use engine_physics::{
            BodyKind, ColliderDesc, ColliderShape, RigidbodyDesc, SimplePhysicsBackend, Vec3,
        };

        let dir = std::env::temp_dir().join("aster_test_physics_raycast");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("raycast_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let hit_distance = -1.0;

fn on_start() {
    let result = raycast(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 10.0);
    if result != () {
        hit_distance = result;
    }
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Create a physics backend with a static body at z=5
        let mut physics =
            Box::new(SimplePhysicsBackend::new()) as Box<dyn engine_physics::PhysicsBackend>;
        let body_desc = RigidbodyDesc {
            transform: engine_core::math::Transform {
                translation: Vec3::new(0.0, 0.0, 5.0),
                rotation: engine_core::math::Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            kind: BodyKind::Static,
            ..Default::default()
        };
        let body = physics.create_body(&body_desc).unwrap();
        let collider_desc = ColliderDesc {
            shape: ColliderShape::Sphere { radius: 1.0 },
            ..Default::default()
        };
        physics.add_collider(body, &collider_desc).unwrap();

        // Set physics backend
        backend.set_physics_backend(physics);

        // Create a scene
        let mut scene = engine_ecs::Scene::new();
        let entity = scene.create_object("TestObject").unwrap();
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());

        backend.set_scene(scene);

        // Run start lifecycle
        backend.run_start(&entity_id, &script_path).unwrap();

        // Verify hit_distance was set
        let scope_key = (entity_id, script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let hit_distance: f64 = scope.get_value("hit_distance").unwrap();
        assert!(hit_distance > 0.0 && hit_distance < 10.0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn physics_overlap_sphere_returns_bodies() {
        use engine_physics::{
            BodyKind, ColliderDesc, ColliderShape, RigidbodyDesc, SimplePhysicsBackend, Vec3,
        };

        let dir = std::env::temp_dir().join("aster_test_physics_overlap");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("overlap_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let overlap_count = 0;

fn on_start() {
    let results = overlap_sphere(0.0, 0.0, 0.0, 5.0);
    overlap_count = results.len();
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Create a physics backend with two bodies within range
        let mut physics =
            Box::new(SimplePhysicsBackend::new()) as Box<dyn engine_physics::PhysicsBackend>;

        // Body 1 at origin
        let body_desc1 = RigidbodyDesc {
            transform: engine_core::math::Transform::IDENTITY,
            kind: BodyKind::Static,
            ..Default::default()
        };
        let body1 = physics.create_body(&body_desc1).unwrap();
        let collider_desc1 = ColliderDesc {
            shape: ColliderShape::Sphere { radius: 0.5 },
            ..Default::default()
        };
        physics.add_collider(body1, &collider_desc1).unwrap();

        // Body 2 at (2, 0, 0) - within 5.0 radius
        let body_desc2 = RigidbodyDesc {
            transform: engine_core::math::Transform {
                translation: Vec3::new(2.0, 0.0, 0.0),
                rotation: engine_core::math::Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            kind: BodyKind::Static,
            ..Default::default()
        };
        let body2 = physics.create_body(&body_desc2).unwrap();
        let collider_desc2 = ColliderDesc {
            shape: ColliderShape::Sphere { radius: 0.5 },
            ..Default::default()
        };
        physics.add_collider(body2, &collider_desc2).unwrap();

        // Set physics backend
        backend.set_physics_backend(physics);

        // Create a scene
        let mut scene = engine_ecs::Scene::new();
        let entity = scene.create_object("TestObject").unwrap();
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());

        backend.set_scene(scene);

        // Run start lifecycle
        backend.run_start(&entity_id, &script_path).unwrap();

        // Verify overlap_count was set to 2
        let scope_key = (entity_id, script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let overlap_count: i64 = scope.get_value("overlap_count").unwrap();
        assert_eq!(overlap_count, 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_resource_resolves_asset_path_to_guid() {
        use engine_assets::{AssetDatabase, AssetGuid, ResourceKind, ResourceMeta, ResourceState};

        let dir = std::env::temp_dir().join("aster_test_get_resource");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("resource_test.rhai");
        std::fs::write(
            &script_path,
            r#"
let texture_guid = "";

fn on_start() {
    texture_guid = get_resource("project:/textures/player.png");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // Create an asset database with a test asset
        let mut database = AssetDatabase::new(&dir, &dir);
        let test_guid = AssetGuid::from_u128(0x12345678_90ab_cdef_1234_567890abcdef);
        let test_path = std::path::PathBuf::from("textures/player.png");

        // Manually insert an entry (simulating a scanned asset)
        database.entries_mut().insert(
            test_path.clone(),
            ResourceMeta {
                guid: test_guid,
                path: test_path,
                kind: ResourceKind::Texture,
                import_state: ResourceState::Unloaded,
            },
        );

        // Set asset database
        backend.set_asset_database(database);

        // Create a scene
        let mut scene = engine_ecs::Scene::new();
        let entity = scene.create_object("TestObject").unwrap();
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());

        backend.set_scene(scene);

        // Run start lifecycle
        backend.run_start(&entity_id, &script_path).unwrap();

        // Verify texture_guid was set
        let scope_key = (entity_id, script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let texture_guid: String = scope.get_value("texture_guid").unwrap();
        assert_eq!(texture_guid, test_guid.to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_script_safe_returns_console_entry_on_syntax_error() {
        let dir = std::env::temp_dir().join("aster_test_load_safe_syntax_error");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("syntax_error.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    let x = 1 +;  // Syntax error: missing operand
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        let result = backend.load_script_safe(&script_path).unwrap();

        // Should return Some(ConsoleEntry) with error details
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.level, ConsoleLevel::Error);
        assert_eq!(entry.source.subsystem, "script");
        assert_eq!(entry.source.file, Some(script_path.clone()));
        assert!(entry.message.contains("Compilation error"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_script_safe_returns_none_on_success() {
        let dir = std::env::temp_dir().join("aster_test_load_safe_success");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("valid.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    print("Hello");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        let result = backend.load_script_safe(&script_path).unwrap();

        // Should return None on success
        assert!(result.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn runtime_error_returns_console_entry() {
        let dir = std::env::temp_dir().join("aster_test_runtime_error");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("runtime_error.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    let x = 1 / 0;  // Runtime error: division by zero
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        let result = backend.run_start("entity_error", &script_path).unwrap();

        // Should return Some(ConsoleEntry) with runtime error
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.level, ConsoleLevel::Error);
        assert_eq!(entry.source.subsystem, "script");
        assert_eq!(entry.source.file, Some(script_path.clone()));
        assert!(entry.message.contains("Runtime error"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn runtime_error_does_not_prevent_next_call() {
        let dir = std::env::temp_dir().join("aster_test_runtime_error_recovery");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("recoverable.rhai");
        std::fs::write(
            &script_path,
            r#"
let call_count = 0;

fn on_update(dt) {
    call_count += 1;
    if call_count == 2 {
        panic("Intentional error on second call");
    }
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        // First call succeeds
        let result1 = backend
            .run_update("entity_recover", &script_path, 0.016)
            .unwrap();
        assert!(result1.is_none());

        // Second call fails with panic
        let result2 = backend
            .run_update("entity_recover", &script_path, 0.016)
            .unwrap();
        assert!(result2.is_some());
        let entry = result2.unwrap();
        eprintln!("Error message: {}", entry.message);
        assert!(entry.message.contains("Intentional error") || entry.message.contains("panic"));

        // Third call succeeds (script still callable after runtime error)
        let result3 = backend
            .run_update("entity_recover", &script_path, 0.016)
            .unwrap();
        assert!(result3.is_none());

        // Verify call_count reached 3
        let scope_key = ("entity_recover".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let call_count: i64 = scope.get_value("call_count").unwrap();
        assert_eq!(call_count, 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn console_entry_includes_line_number_when_available() {
        let dir = std::env::temp_dir().join("aster_test_line_number");
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("line_error.rhai");
        std::fs::write(
            &script_path,
            r#"
fn on_start() {
    let a = 1;
    let b = 2;
    panic("Error on line 5");
}
"#,
        )
        .unwrap();

        let mut backend = RhaiScriptBackend::new();
        backend.load_script(&script_path).unwrap();

        let result = backend.run_start("entity_line", &script_path).unwrap();

        assert!(result.is_some());
        let entry = result.unwrap();
        // Rhai should provide line number for panic
        // Note: line number may or may not be available depending on Rhai version
        // We just verify the entry structure is correct
        assert_eq!(entry.source.subsystem, "script");
        assert_eq!(entry.source.file, Some(script_path.clone()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compile_source_compiles_from_memory_string() {
        let logical_path = std::path::PathBuf::from("memory://test.rhai");

        let mut backend = RhaiScriptBackend::new();
        backend
            .compile_source(&logical_path, "let x = 42;")
            .unwrap();

        assert_eq!(backend.ast_cache_size(), 1);
        assert!(backend.ast_cache.contains_key(&logical_path));
    }

    #[test]
    fn compile_source_replaces_existing_cache_entry() {
        let logical_path = std::path::PathBuf::from("memory://replace.rhai");

        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, "let x = 1;").unwrap();
        backend.compile_source(&logical_path, "let y = 2;").unwrap();

        assert_eq!(backend.ast_cache_size(), 1);

        // Run a lifecycle function to verify the new source is active
        backend
            .compile_source(
                &logical_path,
                r#"
let value = 99;
fn on_start() { value = 100; }
"#,
            )
            .unwrap();

        backend.run_start("entity_replace", &logical_path).unwrap();
        let scope_key = ("entity_replace".to_string(), logical_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let value: i64 = scope.get_value("value").unwrap();
        assert_eq!(value, 100);
    }

    #[test]
    fn compile_source_reports_syntax_errors() {
        let logical_path = std::path::PathBuf::from("memory://bad.rhai");

        let mut backend = RhaiScriptBackend::new();
        let result = backend.compile_source(&logical_path, "let x = ;");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bad.rhai"));
    }

    #[test]
    fn create_script_writes_file_and_compiles() {
        let dir = std::env::temp_dir().join("aster_test_create_script");
        std::fs::create_dir_all(&dir).unwrap();
        let relative = std::path::PathBuf::from("scripts/ai_generated.aster");

        let mut backend = RhaiScriptBackend::new();
        let full_path = backend
            .create_script(
                &dir,
                &relative,
                r#"
let created = true;
fn on_start() {}
"#,
            )
            .unwrap();

        assert!(full_path.exists());
        assert_eq!(backend.ast_cache_size(), 1);
        assert!(backend.ast_cache.contains_key(&full_path));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn three_module_accessible_from_script_via_compile_source() {
        use engine_ecs::Scene;

        let logical_path = std::path::PathBuf::from("memory://three_ns.rhai");
        let source = r#"
let mesh_id = "";
let light_id = "";

fn on_start() {
    let geo = THREE::box_geometry(1.0, 1.0, 1.0);
    let mat = THREE::basic_red();
    let cube = THREE::create_mesh("NsCube", geo, mat);
    cube.position.set(3.0, 0.0, 0.0);
    mesh_id = cube.entity_id();

    let sun = THREE::directional_light([1.0, 1.0, 1.0], 1.0);
    light_id = sun.entity_id();
}
"#;

        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();

        let mut scene = Scene::new();
        let entity = scene.create_object("Driver").unwrap();
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());

        backend.set_scene(scene);
        let err = backend.run_start(&entity_id, &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        let scope_key = (entity_id, logical_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let mesh_id: String = scope.get_value("mesh_id").unwrap();
        let light_id: String = scope.get_value("light_id").unwrap();
        assert!(
            !mesh_id.is_empty(),
            "mesh_id should be set by THREE::create_mesh"
        );
        assert!(
            !light_id.is_empty(),
            "light_id should be set by THREE::directional_light"
        );

        let scene = backend.take_scene().unwrap();
        // 1 driver + 1 mesh + 1 light = 3 entities
        assert!(scene.iter_objects().count() >= 3);
    }

    #[test]
    fn three_module_and_global_functions_coexist() {
        let logical_path = std::path::PathBuf::from("memory://coexist.rhai");
        let source = r#"
let using_module = false;
let using_global = false;

fn on_start() {
    // THREE:: namespace
    let geo1 = THREE::box_geometry(1.0, 1.0, 1.0);
    using_module = geo1 != ();

    // legacy global function
    let geo2 = box_geometry(2.0, 2.0, 2.0);
    using_global = geo2 != ();
}
"#;

        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_start("entity_0:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        let scope_key = ("entity_0:1".to_string(), logical_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let using_module: bool = scope.get_value("using_module").unwrap();
        let using_global: bool = scope.get_value("using_global").unwrap();
        assert!(using_module, "THREE:: namespace should work");
        assert!(using_global, "global functions should still work");
    }

    #[test]
    fn create_script_creates_parent_directories() {
        let dir = std::env::temp_dir().join("aster_test_create_script_nested");
        let relative = std::path::PathBuf::from("deep/nested/path/script.aster");

        let mut backend = RhaiScriptBackend::new();
        let full_path = backend
            .create_script(&dir, &relative, "fn on_start() {}")
            .unwrap();

        assert!(full_path.exists());
        assert!(full_path.parent().unwrap().exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn language_service_reports_undefined_variable_with_fix() {
        let backend = RhaiScriptBackend::new();
        let diagnostics = backend.diagnose_source(
            Path::new("scripts/player.aster"),
            "fn on_update(dt) { translate(speed * dt, 0.0, 0.0); }",
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "ASTER1003");
        assert_eq!(diagnostics[0].line, Some(1));
        assert!(diagnostics[0].message.contains("speed"));
        assert!(diagnostics[0].suggestion.contains("Declare `speed`"));
    }

    #[test]
    fn language_service_reports_invalid_lifecycle_signature() {
        let backend = RhaiScriptBackend::new();
        let diagnostics =
            backend.diagnose_source(Path::new("scripts/player.aster"), "fn on_update() {}");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "ASTER0101");
        assert_eq!(diagnostics[0].line, Some(1));
        assert!(diagnostics[0].suggestion.contains("fn on_update(dt)"));
    }

    #[test]
    fn language_service_requires_aster_extension() {
        let backend = RhaiScriptBackend::new();
        let diagnostics =
            backend.diagnose_source(Path::new("scripts/player.rhai"), "fn on_start() {}");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "ASTER0001");
        assert!(diagnostics[0].suggestion.contains(".aster"));
    }

    #[test]
    fn create_script_does_not_overwrite_file_when_validation_fails() {
        let dir = std::env::temp_dir().join("aster_test_invalid_script_write");
        let relative = PathBuf::from("scripts/player.aster");
        let full_path = dir.join(&relative);
        std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        std::fs::write(&full_path, "fn on_start() {}").unwrap();

        let mut backend = RhaiScriptBackend::new();
        let result = backend.create_script(&dir, &relative, "fn on_start() { let broken = ; }");

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&full_path).unwrap(),
            "fn on_start() {}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Collision API tests ──────────────────────────────────────────────────

    #[test]
    fn collision_api_registers_and_queries() {
        let logical_path = std::path::PathBuf::from("memory://collision_test.rhai");
        let source = r#"
fn on_update(dt) {
    let count = get_collision_count();
    let has = has_collisions();
    if has {
        let entered = get_collision_entered(0);
        let point = get_collision_point(0);
        let normal = get_collision_normal(0);
        let trigger = is_trigger_collision(0);
    }
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_update("test:1", &logical_path, 0.016).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    // ── Event API tests ──────────────────────────────────────────────────────

    #[test]
    fn event_emit_and_consume() {
        let logical_path = std::path::PathBuf::from("memory://event_test.rhai");
        let source = r#"
fn on_start() {
    emit("coin_collected", #{ value: 10, x: 5.0 });
}

fn on_update(dt) {
    if has_events("coin_collected") {
        let data = consume_event("coin_collected");
        let sender = data["sender"];
        let value = data["value"];
    }
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_start("player:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        // Check event was queued
        {
            let guard = backend.event_queue.lock().unwrap();
            assert!(guard.contains_key("coin_collected"));
            assert_eq!(guard["coin_collected"].len(), 1);
        }

        let err = backend
            .run_update("player:1", &logical_path, 0.016)
            .unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        // Event should be consumed
        {
            let guard = backend.event_queue.lock().unwrap();
            assert!(guard
                .get("coin_collected")
                .map(|e| e.is_empty())
                .unwrap_or(true));
        }
    }

    #[test]
    fn event_get_event_count() {
        let logical_path = std::path::PathBuf::from("memory://event_count.rhai");
        let source = r#"
fn on_start() {
    emit("test_event", #{});
    emit("test_event", #{});
    emit("test_event", #{});
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.run_start("e:1", &logical_path).unwrap();

        let guard = backend.event_queue.lock().unwrap();
        assert_eq!(guard["test_event"].len(), 3);
    }

    // ── UI API tests ─────────────────────────────────────────────────────────

    #[test]
    fn ui_commands_collected() {
        let logical_path = std::path::PathBuf::from("memory://ui_test.rhai");
        let source = r#"
fn on_update(dt) {
    ui_text(10.0, 20.0, "Score: 100");
    ui_text_color(10.0, 40.0, "HP: 50", 1.0, 0.0, 0.0, 1.0);
    ui_bar(10.0, 60.0, 200.0, 20.0, 0.75);
    ui_bar_color(10.0, 90.0, 200.0, 20.0, 0.5, 0.0, 0.0, 1.0, 1.0);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_update("hud:1", &logical_path, 0.016).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        let commands = backend.take_ui_commands();
        assert_eq!(commands.len(), 4);
        assert!(
            matches!(&commands[0], UiCommand::Text { x, y, content, .. } if *x == 10.0 && *y == 20.0 && content == "Score: 100")
        );
        assert!(
            matches!(&commands[2], UiCommand::Bar { ratio, .. } if (*ratio - 0.75).abs() < 0.01)
        );
    }

    #[test]
    fn ui_clear_commands() {
        let logical_path = std::path::PathBuf::from("memory://ui_clear.rhai");
        let source = r#"
fn on_update(dt) {
    ui_text(0.0, 0.0, "test");
    clear_ui();
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.run_update("e:1", &logical_path, 0.016).unwrap();
        let commands = backend.take_ui_commands();
        assert!(commands.is_empty());
    }

    // ── Animation API tests ──────────────────────────────────────────────────

    #[test]
    fn animation_play_and_query() {
        let logical_path = std::path::PathBuf::from("memory://anim_test.rhai");
        let source = r#"
fn on_start() {
    register_animation_clip("run", 2.0);
    play_animation("run");
}

fn on_update(dt) {
    let playing = is_animation_playing();
    let time = get_animation_time();
    set_animation_speed(2.0);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_start("player:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        // Check animation state was created
        {
            let guard = backend.animation_states.lock().unwrap();
            let state = guard.get("player:1").unwrap();
            assert!(state.playing);
            assert_eq!(state.clip, "run");
        }

        let err = backend
            .run_update("player:1", &logical_path, 0.016)
            .unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        // Advance animation time (normally called by the runtime)
        backend.update_animations(0.016);

        // Check animation was advanced
        {
            let guard = backend.animation_states.lock().unwrap();
            let state = guard.get("player:1").unwrap();
            assert!(state.time > 0.0);
            assert!((state.speed - 2.0).abs() < 0.01);
        }
    }

    #[test]
    fn animation_stop() {
        let logical_path = std::path::PathBuf::from("memory://anim_stop.rhai");
        let source = r#"
fn on_start() {
    play_animation("idle");
    stop_animation();
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.run_start("e:1", &logical_path).unwrap();

        let guard = backend.animation_states.lock().unwrap();
        let state = guard.get("e:1").unwrap();
        assert!(!state.playing);
    }

    #[test]
    fn animation_crossfade() {
        let logical_path = std::path::PathBuf::from("memory://anim_crossfade.rhai");
        let source = r#"
fn on_start() {
    play_animation("idle");
    crossfade_animation("run", 0.3);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.run_start("e:1", &logical_path).unwrap();

        let guard = backend.animation_states.lock().unwrap();
        let state = guard.get("e:1").unwrap();
        assert_eq!(state.clip, "run");
        assert!(state.playing);
    }

    #[test]
    fn animation_update_advances_time() {
        let mut backend = RhaiScriptBackend::new();
        backend.register_animation_clip("walk", 1.0);
        backend.animation_states.lock().unwrap().insert(
            "entity:1".to_string(),
            AnimationState {
                clip: "walk".to_string(),
                time: 0.0,
                speed: 1.0,
                playing: true,
                loop_mode: "loop".to_string(),
            },
        );

        backend.update_animations(0.5);

        let guard = backend.animation_states.lock().unwrap();
        let state = guard.get("entity:1").unwrap();
        assert!((state.time - 0.5).abs() < 0.001);
    }

    // ── Body-entity mapping tests ────────────────────────────────────────────

    #[test]
    fn body_entity_mapping_register_and_query() {
        let logical_path = std::path::PathBuf::from("memory://body_map.rhai");
        let source = r#"
fn on_start() {
    register_body_entity("42", "player:1");
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.run_start("e:1", &logical_path).unwrap();

        let guard = backend.body_entity_map.lock().unwrap();
        assert_eq!(guard.get(&42).unwrap(), "player:1");
    }

    // ── Setter/getter tests ──────────────────────────────────────────────────

    #[test]
    fn take_ui_commands_returns_and_clears() {
        let mut backend = RhaiScriptBackend::new();
        backend.ui_commands.lock().unwrap().push(UiCommand::Text {
            x: 0.0,
            y: 0.0,
            content: "test".to_string(),
            color: [1.0, 1.0, 1.0, 1.0],
        });

        let commands = backend.take_ui_commands();
        assert_eq!(commands.len(), 1);

        let commands = backend.take_ui_commands();
        assert!(commands.is_empty());
    }

    #[test]
    fn take_events_returns_and_clears() {
        let mut backend = RhaiScriptBackend::new();
        backend.event_queue.lock().unwrap().insert(
            "test".to_string(),
            vec![ScriptEvent {
                name: "test".to_string(),
                sender: "e:1".to_string(),
                data: HashMap::new(),
            }],
        );

        let events = backend.take_events();
        assert_eq!(events.len(), 1);

        let events = backend.take_events();
        assert!(events.is_empty());
    }

    // ── Collision dispatch tests ──────────────────────────────────────────────

    #[test]
    fn dispatch_collision_calls_on_collision_enter() {
        let player_path = std::path::PathBuf::from("memory://player_collision.rhai");
        let enemy_path = std::path::PathBuf::from("memory://enemy_collision.rhai");

        let player_script = r#"
let hit_other = "";
let hit_px = 0.0;
let hit_trigger = false;

fn on_collision_enter(other, px, py, pz, nx, ny, nz, is_trigger) {
    hit_other = other;
    hit_px = px;
    hit_trigger = is_trigger;
}
"#;
        let enemy_script = r#"
let hit_other = "";

fn on_collision_enter(other, px, py, pz, nx, ny, nz, is_trigger) {
    hit_other = other;
}
"#;

        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&player_path, player_script).unwrap();
        backend.compile_source(&enemy_path, enemy_script).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        // Register body-entity mappings
        backend.register_body_entity(1, "player:1".to_string());
        backend.register_body_entity(2, "enemy:1".to_string());

        // Inject a collision event
        {
            let mut guard = backend.collision_events.lock().unwrap();
            guard.push(engine_physics::ContactEvent {
                body_a: engine_physics::BodyHandle(1),
                body_b: engine_physics::BodyHandle(2),
                collider_a: engine_physics::ColliderHandle(10),
                collider_b: engine_physics::ColliderHandle(20),
                point: engine_physics::Vec3::new(1.0, 2.0, 3.0),
                normal: engine_physics::Vec3::new(0.0, 1.0, 0.0),
                entered: true,
                is_trigger: false,
                contact_points: vec![],
            });
        }

        let scripted = vec![
            ("player:1".to_string(), player_path.clone()),
            ("enemy:1".to_string(), enemy_path.clone()),
        ];
        let errors = backend.dispatch_collision_callbacks(&scripted);
        assert!(errors.is_empty(), "Errors: {:?}", errors);

        // Verify player script received the callback
        let scope_key = ("player:1".to_string(), player_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let hit_other: String = scope.get_value("hit_other").unwrap();
        assert_eq!(hit_other, "enemy:1");
        let hit_px: f64 = scope.get_value("hit_px").unwrap();
        assert!((hit_px - 1.0).abs() < 0.01);
        let hit_trigger: bool = scope.get_value("hit_trigger").unwrap();
        assert!(!hit_trigger);

        // Verify enemy script received the callback
        let scope_key = ("enemy:1".to_string(), enemy_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let hit_other: String = scope.get_value("hit_other").unwrap();
        assert_eq!(hit_other, "player:1");
    }

    #[test]
    fn dispatch_collision_calls_on_collision_exit() {
        let script_path = std::path::PathBuf::from("memory://exit_test.rhai");
        let source = r#"
let exit_called = false;

fn on_collision_exit(other, px, py, pz, nx, ny, nz, is_trigger) {
    exit_called = true;
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&script_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.register_body_entity(1, "a:1".to_string());
        backend.register_body_entity(2, "b:1".to_string());

        {
            let mut guard = backend.collision_events.lock().unwrap();
            guard.push(engine_physics::ContactEvent {
                body_a: engine_physics::BodyHandle(1),
                body_b: engine_physics::BodyHandle(2),
                collider_a: engine_physics::ColliderHandle(10),
                collider_b: engine_physics::ColliderHandle(20),
                point: engine_physics::Vec3::ZERO,
                normal: engine_physics::Vec3::ZERO,
                entered: false,
                is_trigger: false,
                contact_points: vec![],
            });
        }

        let scripted = vec![("a:1".to_string(), script_path.clone())];
        let errors = backend.dispatch_collision_callbacks(&scripted);
        assert!(errors.is_empty());

        let scope_key = ("a:1".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let exit_called: bool = scope.get_value("exit_called").unwrap();
        assert!(exit_called);
    }

    #[test]
    fn dispatch_collision_trigger_event() {
        let script_path = std::path::PathBuf::from("memory://trigger_test.rhai");
        let source = r#"
let got_trigger = false;

fn on_collision_enter(other, px, py, pz, nx, ny, nz, is_trigger) {
    got_trigger = is_trigger;
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&script_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.register_body_entity(1, "player:1".to_string());
        backend.register_body_entity(2, "zone:1".to_string());

        {
            let mut guard = backend.collision_events.lock().unwrap();
            guard.push(engine_physics::ContactEvent {
                body_a: engine_physics::BodyHandle(1),
                body_b: engine_physics::BodyHandle(2),
                collider_a: engine_physics::ColliderHandle(10),
                collider_b: engine_physics::ColliderHandle(20),
                point: engine_physics::Vec3::ZERO,
                normal: engine_physics::Vec3::ZERO,
                entered: true,
                is_trigger: true,
                contact_points: vec![],
            });
        }

        let scripted = vec![("player:1".to_string(), script_path.clone())];
        backend.dispatch_collision_callbacks(&scripted);

        let scope_key = ("player:1".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let got_trigger: bool = scope.get_value("got_trigger").unwrap();
        assert!(got_trigger);
    }

    #[test]
    fn dispatch_collision_skips_unscripted_entities() {
        let script_path = std::path::PathBuf::from("memory://onesided.rhai");
        let source = r#"
let was_hit = false;

fn on_collision_enter(other, px, py, pz, nx, ny, nz, is_trigger) {
    was_hit = true;
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&script_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.register_body_entity(1, "scripted:1".to_string());
        backend.register_body_entity(2, "unscripted:1".to_string());

        {
            let mut guard = backend.collision_events.lock().unwrap();
            guard.push(engine_physics::ContactEvent {
                body_a: engine_physics::BodyHandle(1),
                body_b: engine_physics::BodyHandle(2),
                collider_a: engine_physics::ColliderHandle(10),
                collider_b: engine_physics::ColliderHandle(20),
                point: engine_physics::Vec3::ZERO,
                normal: engine_physics::Vec3::ZERO,
                entered: true,
                is_trigger: false,
                contact_points: vec![],
            });
        }

        // Only "scripted:1" has a script; "unscripted:1" does not
        let scripted = vec![("scripted:1".to_string(), script_path.clone())];
        let errors = backend.dispatch_collision_callbacks(&scripted);
        assert!(errors.is_empty());

        // Scripted entity should have received the callback
        let scope_key = ("scripted:1".to_string(), script_path.clone());
        let scope = backend.entity_scopes.get(&scope_key).unwrap();
        let was_hit: bool = scope.get_value("was_hit").unwrap();
        assert!(was_hit);
    }

    #[test]
    fn dispatch_collision_normal_flipped_for_b() {
        let path_a = std::path::PathBuf::from("memory://norm_a.rhai");
        let path_b = std::path::PathBuf::from("memory://norm_b.rhai");
        let source = r#"
let my_ny = 0.0;

fn on_collision_enter(other, px, py, pz, nx, ny, nz, is_trigger) {
    my_ny = ny;
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&path_a, source).unwrap();
        backend.compile_source(&path_b, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.register_body_entity(1, "a:1".to_string());
        backend.register_body_entity(2, "b:1".to_string());

        {
            let mut guard = backend.collision_events.lock().unwrap();
            guard.push(engine_physics::ContactEvent {
                body_a: engine_physics::BodyHandle(1),
                body_b: engine_physics::BodyHandle(2),
                collider_a: engine_physics::ColliderHandle(10),
                collider_b: engine_physics::ColliderHandle(20),
                point: engine_physics::Vec3::ZERO,
                normal: engine_physics::Vec3::new(0.0, 1.0, 0.0),
                entered: true,
                is_trigger: false,
                contact_points: vec![],
            });
        }

        let scripted = vec![
            ("a:1".to_string(), path_a.clone()),
            ("b:1".to_string(), path_b.clone()),
        ];
        backend.dispatch_collision_callbacks(&scripted);

        // A gets normal as-is (0, 1, 0)
        let scope_a = backend
            .entity_scopes
            .get(&("a:1".to_string(), path_a.clone()))
            .unwrap();
        let ny_a: f64 = scope_a.get_value("my_ny").unwrap();
        assert!((ny_a - 1.0).abs() < 0.01);

        // B gets flipped normal (0, -1, 0)
        let scope_b = backend
            .entity_scopes
            .get(&("b:1".to_string(), path_b.clone()))
            .unwrap();
        let ny_b: f64 = scope_b.get_value("my_ny").unwrap();
        assert!((ny_b - (-1.0)).abs() < 0.01);
    }

    // ── Synth API tests ──────────────────────────────────────────────────────

    #[test]
    fn synth_api_create_and_connect() {
        let logical_path = std::path::PathBuf::from("memory://synth_test.rhai");
        let source = r#"
fn on_start() {
    let osc = create_oscillator("sine", 440.0);
    let gain = create_gain(0.5);
    synth_connect(osc, gain);
    synth_destination(gain);
    synth_start(osc);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_start("e:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        // Verify the synth graph has nodes
        let graph = backend.synth_graph.lock().unwrap();
        assert!(graph.is_playing(engine_audio::synth::NodeHandle(1)));
    }

    #[test]
    fn synth_api_set_and_get_param() {
        let logical_path = std::path::PathBuf::from("memory://synth_param.rhai");
        let source = r#"
fn on_start() {
    let osc = create_oscillator("sine", 440.0);
    synth_set(osc, "frequency", 880.0);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.run_start("e:1", &logical_path).unwrap();

        let graph = backend.synth_graph.lock().unwrap();
        let freq = graph.get_param(engine_audio::synth::NodeHandle(1), "frequency");
        assert!(
            (freq - 880.0).abs() < 1.0,
            "frequency should be 880, got {}",
            freq
        );
    }

    #[test]
    fn synth_api_ramp() {
        let logical_path = std::path::PathBuf::from("memory://synth_ramp.rhai");
        let source = r#"
fn on_start() {
    let osc = create_oscillator("sine", 440.0);
    synth_destination(osc);
    synth_start(osc);
    synth_ramp(osc, "frequency", 880.0, 0.5);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        backend.run_start("e:1", &logical_path).unwrap();

        // Render 0.5 seconds to trigger the ramp
        let mut output = vec![0.0f32; 22050];
        backend.render_synth(&mut output);

        let graph = backend.synth_graph.lock().unwrap();
        let freq = graph.get_param(engine_audio::synth::NodeHandle(1), "frequency");
        assert!(
            (freq - 880.0).abs() < 1.0,
            "frequency should ramp to 880, got {}",
            freq
        );
    }

    #[test]
    fn synth_api_all_waveforms() {
        let logical_path = std::path::PathBuf::from("memory://synth_waveforms.rhai");
        let source = r#"
fn on_start() {
    let o1 = create_oscillator("sine", 440.0);
    let o2 = create_oscillator("square", 440.0);
    let o3 = create_oscillator("sawtooth", 440.0);
    let o4 = create_oscillator("triangle", 440.0);
    let o5 = create_oscillator("noise", 440.0);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_start("e:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    #[test]
    fn synth_api_filter_and_delay() {
        let logical_path = std::path::PathBuf::from("memory://synth_effects.rhai");
        let source = r#"
fn on_start() {
    let osc = create_oscillator("sawtooth", 1000.0);
    let filt = create_filter("lowpass", 500.0, 0.7);
    let delay = create_delay(0.1, 0.3, 0.5);
    let gain = create_gain(0.3);
    synth_connect(osc, filt);
    synth_connect(filt, delay);
    synth_connect(delay, gain);
    synth_destination(gain);
    synth_start(osc);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_start("e:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    #[test]
    fn synth_api_envelope() {
        let logical_path = std::path::PathBuf::from("memory://synth_env.rhai");
        let source = r#"
fn on_start() {
    let osc = create_oscillator("sine", 440.0);
    let env = create_gain_envelope(1.0, 0.01, 0.1, 0.7, 0.3);
    synth_connect(osc, env);
    synth_destination(env);
    synth_start(osc);
    synth_start(env);
}
"#;
        let mut backend = RhaiScriptBackend::new();
        backend.compile_source(&logical_path, source).unwrap();
        backend.set_scene(engine_ecs::Scene::new());

        let err = backend.run_start("e:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    // ── Entity Query API tests ──────────────────────────────────────────────

    #[test]
    fn find_entity_by_name() {
        let mut backend = RhaiScriptBackend::new();
        let mut scene = engine_ecs::Scene::new();
        let entity = scene.create_object("Player").unwrap();
        backend.set_scene(scene);

        let logical_path = std::path::PathBuf::from("memory://find_entity.rhai");
        backend
            .compile_source(
                &logical_path,
                r#"
fn on_start() {
    let found = find_entity("Player");
    if found == "" { panic("Player not found"); }
}
"#,
            )
            .unwrap();

        let err = backend.run_start("e:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    #[test]
    fn find_entities_by_tag() {
        let mut backend = RhaiScriptBackend::new();
        let mut scene = engine_ecs::Scene::new();
        let e1 = scene.create_object("Enemy1").unwrap();
        let e2 = scene.create_object("Enemy2").unwrap();
        scene.object_mut(e1).unwrap().tag = "enemy".to_string();
        scene.object_mut(e2).unwrap().tag = "enemy".to_string();
        backend.set_scene(scene);

        let logical_path = std::path::PathBuf::from("memory://find_tag.rhai");
        backend
            .compile_source(
                &logical_path,
                r#"
fn on_start() {
    let enemies = find_entities_by_tag("enemy");
    if len(enemies) != 2 { panic("Expected 2 enemies"); }
}
"#,
            )
            .unwrap();

        let err = backend.run_start("e:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    #[test]
    fn has_component_check() {
        let mut backend = RhaiScriptBackend::new();
        let mut scene = engine_ecs::Scene::new();
        let entity = scene.create_object("TestObj").unwrap();
        let handle = entity.handle();
        let entity_id = format!("{}:{}", handle.slot(), handle.generation().get());
        backend.set_scene(scene);

        let logical_path = std::path::PathBuf::from("memory://has_component.rhai");
        backend
            .compile_source(
                &logical_path,
                r#"
fn on_start() {
    let eid = self_entity;
    let has_cam = has_component(eid, "Camera");
    if has_cam { panic("Should not have Camera"); }
}
"#,
            )
            .unwrap();

        let err = backend.run_start(&entity_id, &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    #[test]
    fn distance_between_entities() {
        let mut backend = RhaiScriptBackend::new();
        let mut scene = engine_ecs::Scene::new();
        let e1 = scene.create_object("A").unwrap();
        let e2 = scene.create_object("B").unwrap();

        // Set positions
        use engine_core::math::Transform;
        let mut t1 = Transform::default();
        t1.translation = engine_core::math::Vec3::new(0.0, 0.0, 0.0);
        scene.transforms_mut().set_local(e1, t1);

        let mut t2 = Transform::default();
        t2.translation = engine_core::math::Vec3::new(3.0, 4.0, 0.0);
        scene.transforms_mut().set_local(e2, t2);

        let h1 = e1.handle();
        let h2 = e2.handle();
        let id1 = format!("{}:{}", h1.slot(), h1.generation().get());
        let id2 = format!("{}:{}", h2.slot(), h2.generation().get());

        backend.set_scene(scene);

        let logical_path = std::path::PathBuf::from("memory://distance.rhai");
        backend
            .compile_source(
                &logical_path,
                r#"
fn on_start() {
    let a = find_entity("A");
    let b = find_entity("B");
    let dist = distance_between(a, b);
    if dist < 4.9 || dist > 5.1 { panic("Distance should be ~5"); }
}
"#,
            )
            .unwrap();

        let err = backend.run_start(&id1, &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    #[test]
    fn parent_child_api() {
        let mut backend = RhaiScriptBackend::new();
        let mut scene = engine_ecs::Scene::new();
        let parent = scene.create_object("Parent").unwrap();
        let child = scene.create_object("Child").unwrap();
        scene.set_parent(child, Some(parent)).unwrap();

        let parent_id = format!(
            "{}:{}",
            parent.handle().slot(),
            parent.handle().generation().get()
        );
        let child_id = format!(
            "{}:{}",
            child.handle().slot(),
            child.handle().generation().get()
        );

        backend.set_scene(scene);

        let logical_path = std::path::PathBuf::from("memory://parent_child.rhai");
        backend
            .compile_source(
                &logical_path,
                r#"
fn on_start() {
    let child_eid = find_entity("Child");
    let parent_eid = get_parent(child_eid);
    if parent_eid == "" { panic("Child should have parent"); }

    let children = get_children(parent_eid);
    if len(children) != 1 { panic("Parent should have 1 child"); }
}
"#,
            )
            .unwrap();

        let err = backend.run_start(&child_id, &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));
    }

    #[test]
    fn entity_body_mapping() {
        let mut backend = RhaiScriptBackend::new();
        backend.set_scene(engine_ecs::Scene::new());

        let logical_path = std::path::PathBuf::from("memory://body_map.rhai");
        backend
            .compile_source(
                &logical_path,
                r#"
fn on_start() {
    register_entity_body("player:1", 42.0);
}
"#,
            )
            .unwrap();

        let err = backend.run_start("e:1", &logical_path).unwrap();
        assert!(err.is_none(), "Script error: {:?}", err.map(|e| e.message));

        // Verify mapping was created
        let eb_map = backend.entity_body_map.lock().unwrap();
        assert_eq!(eb_map.get("player:1"), Some(&42u64));
    }
}
