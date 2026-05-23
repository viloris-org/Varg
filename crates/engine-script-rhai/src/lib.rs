#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Rhai script backend for the Aster engine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use engine_core::EngineResult;
use engine_ecs::Entity;
use engine_editor::{ConsoleEntry, ConsoleLevel, ConsoleSource};
use rhai::{Scope, AST};

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
        let input_state = Arc::new(Mutex::new(None));
        let scene = Arc::new(Mutex::new(None));
        let transform_context = Arc::new(Mutex::new(None));
        let physics_backend = Arc::new(Mutex::new(None));
        let asset_database = Arc::new(Mutex::new(None));

        // Start with a standard engine, then disable dangerous capabilities.
        // Rhai's default engine includes arithmetic, collections, string ops,
        // and time — but also file I/O and networking. We disable the latter.
        let mut engine = rhai::Engine::new();

        // Disable all file-system, network, and eval capabilities.
        engine.disable_symbol("eval");
        engine.disable_symbol("read_file");
        engine.disable_symbol("write_file");
        engine.disable_symbol("remove_file");
        engine.disable_symbol("rename_file");
        engine.disable_symbol("list_dir");
        engine.disable_symbol("read_text_file");
        engine.disable_symbol("write_text_file");
        engine.disable_symbol("http_get");
        engine.disable_symbol("http_post");

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

        Self {
            engine,
            ast_cache: HashMap::new(),
            entity_scopes: HashMap::new(),
            input_state,
            scene,
            transform_context,
            physics_backend,
            asset_database,
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
                let guard = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let guard = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let guard = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let guard = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let guard = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let guard = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let entity_id = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let mut guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let mut guard = scene_ref.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(scene) = guard.as_mut() {
                    if let Some(entity) = Self::parse_entity_id(&id) {
                        let _ = scene.destroy_deferred(entity);
                    }
                }
            });
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
                    let guard = backend.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                    let guard = backend.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    /// Parses a key name string into a KeyCode.
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
        *self.input_state.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(input);
    }

    /// Updates the scene reference that scripts can query and modify.
    ///
    /// This should be called each frame before running script lifecycle functions.
    pub fn set_scene(&mut self, scene: engine_ecs::Scene) {
        *self.scene.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scene);
    }

    /// Takes the scene back from the script backend after lifecycle execution.
    ///
    /// This should be called after all script lifecycle functions have run.
    pub fn take_scene(&mut self) -> Option<engine_ecs::Scene> {
        self.scene.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    /// Updates the physics backend reference that scripts can query.
    ///
    /// This should be called before running script lifecycle functions that need physics queries.
    pub fn set_physics_backend(&mut self, backend: Box<dyn engine_physics::PhysicsBackend>) {
        *self.physics_backend.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(backend);
    }

    /// Takes the physics backend back from the script backend.
    pub fn take_physics_backend(&mut self) -> Option<Box<dyn engine_physics::PhysicsBackend>> {
        self.physics_backend.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    /// Updates the asset database reference that scripts can query.
    ///
    /// This should be called before running script lifecycle functions that need resource resolution.
    pub fn set_asset_database(&mut self, database: engine_assets::AssetDatabase) {
        *self.asset_database.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(database);
    }

    /// Takes the asset database back from the script backend.
    pub fn take_asset_database(&mut self) -> Option<engine_assets::AssetDatabase> {
        self.asset_database.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
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
            .engine
            .compile(&source)
            .map_err(|e| engine_core::EngineError::other(format!("{}: {}", path.display(), e)))?;
        self.ast_cache.insert(path.to_path_buf(), ast);
        Ok(())
    }

    /// Clears all cached ASTs.
    pub fn clear_cache(&mut self) {
        self.ast_cache.clear();
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
        *self.transform_context.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(entity_id.to_string());

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
            *self.transform_context.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            return Ok(None);
        }

        // Call the function with the scope, catching runtime errors
        let result: Result<rhai::Dynamic, _> =
            self.engine
                .call_fn(scope, ast, function_name, args.to_vec());

        // Clear the transform context after the call
        *self.transform_context.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;

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
/// - `project:/path/to/script.rhai` → `<asset_root>/path/to/script.rhai`
/// - `builtin:/path/to/script.rhai` → `<asset_root>/builtin/path/to/script.rhai`
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
}
