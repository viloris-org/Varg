//! Three.js-compatible Scene and Object3D types for Rhai scripts.
//!
//! Mirrors `THREE.Scene`, `THREE.Mesh`, `THREE.Light`, `THREE.Camera`, `THREE.Group`.
//! Each Object3D wraps an Aster ECS entity ID and writes transforms through a shared
//! [`SceneContext`] so that Rhai-side mutations immediately affect the engine state.

use std::sync::{Arc, Mutex};

use crate::threesh::{geometry::Geometry, material::Material, vector3::Vector3};

/// Shared context that bridges Rhai Object3D instances to the Aster ECS scene.
///
/// Cloned into every Object3D so position/rotation/scale setters can
/// write through to the engine immediately.
#[derive(Clone)]
pub struct SceneContext {
    /// The Aster ECS scene (shared with [`RhaiScriptBackend`]).
    pub inner: Arc<Mutex<Option<engine_ecs::Scene>>>,
    /// Optional physics backend for rigidbody/collider creation.
    pub physics: Arc<Mutex<Option<Box<dyn engine_physics::PhysicsBackend>>>>,
    /// Optional asset database for resource resolution.
    pub assets: Arc<Mutex<Option<engine_assets::AssetDatabase>>>,
}

impl std::fmt::Debug for SceneContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneContext")
            .field("inner", &self.inner)
            .field("physics", &"<PhysicsBackend>")
            .field("assets", &self.assets)
            .finish()
    }
}

impl SceneContext {
    /// Create a new empty context (no scene attached yet).
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            physics: Arc::new(Mutex::new(None)),
            assets: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the Aster scene.
    pub fn set_scene(&self, scene: engine_ecs::Scene) {
        *self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scene);
    }

    /// Take the Aster scene back.
    pub fn take_scene(&self) -> Option<engine_ecs::Scene> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Parse "slot:gen" string to Entity.
    pub(crate) fn parse_entity(id: &str) -> Option<engine_ecs::Entity> {
        let parts: Vec<&str> = id.split(':').collect();
        if parts.len() == 2 {
            let slot = parts[0].parse::<u32>().ok()?;
            let _gen = parts[1].parse::<u32>().ok()?;
            Some(engine_ecs::Entity::from_handle(engine_core::Handle::new(
                slot,
                engine_core::Generation::FIRST,
            )))
        } else {
            None
        }
    }

    /// Read the current position of an entity from the engine.
    pub fn read_position(&self, entity_id: &str) -> Vector3 {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_ref() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                if let Some(transform) = scene.transforms().local(entity) {
                    return Vector3::new(
                        transform.translation.x,
                        transform.translation.y,
                        transform.translation.z,
                    );
                }
            }
        }
        Vector3::new(0.0, 0.0, 0.0)
    }

    /// Write a position update through to the engine.
    pub fn write_position(&self, entity_id: &str, x: f32, y: f32, z: f32) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                if let Some(mut transform) = scene.transforms().local(entity) {
                    transform.translation = engine_core::math::Vec3::new(x, y, z);
                    scene.transforms_mut().set_local(entity, transform);
                }
            }
        }
    }

    /// Read rotation as Euler angles (x, y, z) in radians, matching three.js convention
    /// where x = pitch, y = yaw, z = roll.
    pub fn read_rotation(&self, entity_id: &str) -> Vector3 {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_ref() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                if let Some(transform) = scene.transforms().local(entity) {
                    // to_euler returns (yaw, pitch, roll) → map to three.js (pitch, yaw, roll)
                    let (yaw, pitch, roll) = transform.rotation.to_euler();
                    return Vector3::new(pitch, yaw, roll);
                }
            }
        }
        Vector3::new(0.0, 0.0, 0.0)
    }

    /// Write rotation as Euler angles (x=pitch, y=yaw, z=roll in radians),
    /// matching three.js `rotation.set(x, y, z)` convention.
    pub fn write_rotation(&self, entity_id: &str, pitch: f32, yaw: f32, roll: f32) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                if let Some(mut transform) = scene.transforms().local(entity) {
                    // from_euler(yaw, pitch, roll)
                    transform.rotation = engine_core::math::Quat::from_euler(yaw, pitch, roll);
                    scene.transforms_mut().set_local(entity, transform);
                }
            }
        }
    }

    /// Read scale.
    pub fn read_scale(&self, entity_id: &str) -> Vector3 {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_ref() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                if let Some(transform) = scene.transforms().local(entity) {
                    return Vector3::new(transform.scale.x, transform.scale.y, transform.scale.z);
                }
            }
        }
        Vector3::new(1.0, 1.0, 1.0)
    }

    /// Write scale.
    pub fn write_scale(&self, entity_id: &str, x: f32, y: f32, z: f32) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                if let Some(mut transform) = scene.transforms().local(entity) {
                    transform.scale = engine_core::math::Vec3::new(x, y, z);
                    scene.transforms_mut().set_local(entity, transform);
                }
            }
        }
    }

    /// Create a raw entity in the scene, return its ID string.
    pub fn create_entity(&self, name: &str) -> String {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Ok(entity) = scene.create_object(name) {
                let handle = entity.handle();
                return format!("{}:{}", handle.slot(), handle.generation().get());
            }
        }
        String::new()
    }

    /// Destroy an entity.
    pub fn destroy_entity(&self, entity_id: &str) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                let _ = scene.destroy_deferred(entity);
            }
        }
    }

    /// Add a MeshRenderer component and set initial transform.
    pub fn add_mesh_component(&self, entity_id: &str) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                let _ = scene.upsert_component(
                    entity,
                    engine_ecs::ComponentData::MeshRenderer(
                        engine_ecs::MeshRendererComponentData {
                            mesh: None,
                            builtin_mesh: None,
                            material: engine_ecs::MaterialRef::debug(),
                            casts_shadows: false,
                            receive_shadows: true,
                        },
                    ),
                );
            }
        }
    }

    /// Add a Light component.
    pub fn add_light_component(
        &self,
        entity_id: &str,
        kind: &str,
        color: [f32; 3],
        intensity: f32,
    ) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                let _ = scene.upsert_component(
                    entity,
                    engine_ecs::ComponentData::Light(engine_ecs::LightComponentData {
                        color: engine_core::math::Vec3::new(color[0], color[1], color[2]),
                        intensity,
                        kind: kind.to_string(),
                        range: 100.0,
                        spot_angle: std::f32::consts::PI / 4.0,
                    }),
                );
            }
        }
    }

    /// Add a Camera component.
    pub fn add_camera_component(&self, entity_id: &str, fov: f32, near: f32, far: f32) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(scene) = guard.as_mut() {
            if let Some(entity) = Self::parse_entity(entity_id) {
                let _ = scene.upsert_component(
                    entity,
                    engine_ecs::ComponentData::Camera(engine_ecs::CameraComponentData {
                        vertical_fov_degrees: fov,
                        near,
                        far,
                        aspect_ratio: None,
                        primary: true,
                        clear_color: engine_core::math::Vec3::new(0.1, 0.1, 0.15),
                    }),
                );
            }
        }
    }
}

impl Default for SceneContext {
    fn default() -> Self {
        Self::new()
    }
}

// ── Object3D (base class) ──

/// Base class for all scene objects (`THREE.Object3D`).
#[derive(Clone, Debug)]
pub struct Object3D {
    /// Aster entity ID in "slot:gen" format.
    pub entity_id: String,
    /// Object name.
    pub name: String,
    /// Shared scene context for read/write through to engine.
    pub ctx: SceneContext,
}

impl Object3D {
    /// Create a new Object3D — creates the backing Aster entity immediately.
    pub fn new(name: &str, ctx: &SceneContext) -> Self {
        let entity_id = ctx.create_entity(name);
        Self {
            entity_id,
            name: name.to_string(),
            ctx: ctx.clone(),
        }
    }

    // ── Getters / Setters matching THREE.Object3D ──

    /// `object.position` — returns current position from engine.
    /// Rhai calls `get_position` as a property getter.
    pub fn get_position(&self) -> Vector3 {
        self.ctx.read_position(&self.entity_id)
    }

    /// `object.position.set(x, y, z)` — write position through to engine.
    pub fn set_position(&mut self, x: f32, y: f32, z: f32) {
        self.ctx.write_position(&self.entity_id, x, y, z);
    }

    /// `object.position` as a mutable Vector3 that writes through.
    /// Returns a Vector3 that, when mutated, updates the engine.
    /// This is the three.js pattern: `obj.position.set(1, 2, 3)`.
    pub fn position(&mut self) -> PositionProxy {
        PositionProxy {
            entity_id: self.entity_id.clone(),
            ctx: self.ctx.clone(),
        }
    }

    /// `object.rotation` — current Euler rotation from engine (radians).
    pub fn get_rotation(&self) -> Vector3 {
        self.ctx.read_rotation(&self.entity_id)
    }

    /// `object.rotation.set(x, y, z)` — set Euler rotation (radians).
    pub fn set_rotation(&mut self, x: f32, y: f32, z: f32) {
        self.ctx.write_rotation(&self.entity_id, x, y, z);
    }

    /// Return a proxy for `obj.rotation.set(x, y, z)` pattern.
    pub fn rotation(&mut self) -> RotationProxy {
        RotationProxy {
            entity_id: self.entity_id.clone(),
            ctx: self.ctx.clone(),
        }
    }

    /// `object.scale` — current scale from engine.
    pub fn get_scale(&self) -> Vector3 {
        self.ctx.read_scale(&self.entity_id)
    }

    /// `object.scale.set(x, y, z)` — set scale.
    pub fn set_scale(&mut self, x: f32, y: f32, z: f32) {
        self.ctx.write_scale(&self.entity_id, x, y, z);
    }

    /// Return a proxy for `obj.scale.set(x, y, z)` pattern.
    pub fn scale(&mut self) -> ScaleProxy {
        ScaleProxy {
            entity_id: self.entity_id.clone(),
            ctx: self.ctx.clone(),
        }
    }

    /// `object.translateX(d)` — translate along local X.
    pub fn translate_x(&mut self, d: f32) {
        let pos = self.ctx.read_position(&self.entity_id);
        self.ctx
            .write_position(&self.entity_id, pos.x + d, pos.y, pos.z);
    }

    /// `object.translateY(d)` — translate along local Y.
    pub fn translate_y(&mut self, d: f32) {
        let pos = self.ctx.read_position(&self.entity_id);
        self.ctx
            .write_position(&self.entity_id, pos.x, pos.y + d, pos.z);
    }

    /// `object.translateZ(d)` — translate along local Z (forward in three.js is -Z).
    pub fn translate_z(&mut self, d: f32) {
        let pos = self.ctx.read_position(&self.entity_id);
        self.ctx
            .write_position(&self.entity_id, pos.x, pos.y, pos.z - d);
    }

    /// `object.lookAt(x, y, z)` — rotate to face a world-space target.
    pub fn look_at(&mut self, tx: f32, ty: f32, tz: f32) {
        let pos = self.ctx.read_position(&self.entity_id);
        let dx = tx - pos.x;
        let dy = ty - pos.y;
        let dz = tz - pos.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq < f32::EPSILON {
            return;
        }
        let dist = dist_sq.sqrt();
        let yaw = dz.atan2(dx);
        let pitch = (dy / dist).asin();
        // write_rotation(pitch, yaw, roll) matching three.js convention
        self.ctx.write_rotation(&self.entity_id, pitch, yaw, 0.0);
    }

    /// Destroy this object.
    pub fn destroy(&mut self) {
        self.ctx.destroy_entity(&self.entity_id);
    }
}

// ── Transform proxies for three.js `obj.position.set(...)` pattern ──

/// Proxy for `object.position.set(x, y, z)`.
#[derive(Clone, Debug)]
pub struct PositionProxy {
    entity_id: String,
    ctx: SceneContext,
}

impl PositionProxy {
    pub fn set(&self, x: f32, y: f32, z: f32) {
        self.ctx.write_position(&self.entity_id, x, y, z);
    }
}

/// Proxy for `object.rotation.set(x, y, z)`.
#[derive(Clone, Debug)]
pub struct RotationProxy {
    entity_id: String,
    ctx: SceneContext,
}

impl RotationProxy {
    pub fn set(&self, x: f32, y: f32, z: f32) {
        self.ctx.write_rotation(&self.entity_id, x, y, z);
    }
}

/// Proxy for `object.scale.set(x, y, z)`.
#[derive(Clone, Debug)]
pub struct ScaleProxy {
    entity_id: String,
    ctx: SceneContext,
}

impl ScaleProxy {
    pub fn set(&self, x: f32, y: f32, z: f32) {
        self.ctx.write_scale(&self.entity_id, x, y, z);
    }
}

// ── Mesh ──

/// `THREE.Mesh` — a renderable object with geometry and material.
#[derive(Clone, Debug)]
pub struct Mesh {
    pub base: Object3D,
    pub geometry: Geometry,
    pub material: Material,
}

impl Mesh {
    /// Create a new Mesh with geometry and material.
    /// Backs it with an Aster ECS entity + MeshRenderer component.
    pub fn new(name: &str, geometry: Geometry, material: Material, ctx: &SceneContext) -> Self {
        let base = Object3D::new(name, ctx);
        ctx.add_mesh_component(&base.entity_id);
        Self {
            base,
            geometry,
            material,
        }
    }

    // Delegate Object3D methods
    pub fn get_position(&self) -> Vector3 {
        self.base.get_position()
    }
    pub fn set_position(&mut self, x: f32, y: f32, z: f32) {
        self.base.set_position(x, y, z);
    }
    pub fn position(&mut self) -> PositionProxy {
        self.base.position()
    }
    pub fn set_rotation(&mut self, x: f32, y: f32, z: f32) {
        self.base.set_rotation(x, y, z);
    }
    pub fn get_rotation(&self) -> Vector3 {
        self.base.get_rotation()
    }
    pub fn rotation(&mut self) -> RotationProxy {
        self.base.rotation()
    }
    pub fn set_scale(&mut self, x: f32, y: f32, z: f32) {
        self.base.set_scale(x, y, z);
    }
    pub fn get_scale(&self) -> Vector3 {
        self.base.get_scale()
    }
    pub fn scale(&mut self) -> ScaleProxy {
        self.base.scale()
    }
    pub fn translate_x(&mut self, d: f32) {
        self.base.translate_x(d);
    }
    pub fn translate_y(&mut self, d: f32) {
        self.base.translate_y(d);
    }
    pub fn translate_z(&mut self, d: f32) {
        self.base.translate_z(d);
    }
    pub fn look_at(&mut self, tx: f32, ty: f32, tz: f32) {
        self.base.look_at(tx, ty, tz);
    }
    pub fn destroy(&mut self) {
        self.base.destroy();
    }
    pub fn entity_id(&self) -> String {
        self.base.entity_id.clone()
    }
}

// ── Light ──

/// Light kind matching three.js light types.
#[derive(Clone, Debug, PartialEq)]
pub enum LightKind {
    Directional,
    Point,
    Spot,
    Ambient,
}

/// `THREE.DirectionalLight`, `THREE.PointLight`, etc.
#[derive(Clone, Debug)]
pub struct Light {
    pub base: Object3D,
    pub light_kind: LightKind,
    pub color: [f32; 3],
    pub intensity: f32,
    /// For point/spot: max range.
    pub range: f32,
}

impl Light {
    /// `new THREE.DirectionalLight(color, intensity)`.
    pub fn directional_light(color: [f32; 3], intensity: f32, ctx: &SceneContext) -> Self {
        let base = Object3D::new("DirectionalLight", ctx);
        ctx.add_light_component(&base.entity_id, "directional", color, intensity);
        Self {
            base,
            light_kind: LightKind::Directional,
            color,
            intensity,
            range: 0.0,
        }
    }

    /// `new THREE.PointLight(color, intensity, distance)`.
    pub fn point_light(color: [f32; 3], intensity: f32, range: f32, ctx: &SceneContext) -> Self {
        let base = Object3D::new("PointLight", ctx);
        ctx.add_light_component(&base.entity_id, "point", color, intensity);
        Self {
            base,
            light_kind: LightKind::Point,
            color,
            intensity,
            range,
        }
    }

    /// `new THREE.SpotLight(color, intensity, distance, angle)`.
    pub fn spot_light(
        color: [f32; 3],
        intensity: f32,
        range: f32,
        _angle: f32,
        ctx: &SceneContext,
    ) -> Self {
        let base = Object3D::new("SpotLight", ctx);
        ctx.add_light_component(&base.entity_id, "spot", color, intensity);
        Self {
            base,
            light_kind: LightKind::Spot,
            color,
            intensity,
            range,
        }
    }

    /// `new THREE.AmbientLight(color, intensity)`.
    pub fn ambient_light(color: [f32; 3], intensity: f32, ctx: &SceneContext) -> Self {
        let base = Object3D::new("AmbientLight", ctx);
        ctx.add_light_component(&base.entity_id, "ambient", color, intensity);
        Self {
            base,
            light_kind: LightKind::Ambient,
            color,
            intensity,
            range: 0.0,
        }
    }

    pub fn set_position(&mut self, x: f32, y: f32, z: f32) {
        self.base.set_position(x, y, z);
    }
    pub fn get_position(&self) -> Vector3 {
        self.base.get_position()
    }
    pub fn position(&mut self) -> PositionProxy {
        self.base.position()
    }
    pub fn entity_id(&self) -> String {
        self.base.entity_id.clone()
    }
}

// ── Camera ──

/// `THREE.PerspectiveCamera` and `THREE.OrthographicCamera`.
#[derive(Clone, Debug)]
pub struct Camera {
    pub base: Object3D,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub is_orthographic: bool,
    pub ortho_size: f32,
}

impl Camera {
    /// `new THREE.PerspectiveCamera(fov, aspect, near, far)`.
    pub fn perspective_camera(
        fov: f32,
        _aspect: f32,
        near: f32,
        far: f32,
        ctx: &SceneContext,
    ) -> Self {
        let base = Object3D::new("Camera", ctx);
        ctx.add_camera_component(&base.entity_id, fov, near, far);
        Self {
            base,
            fov,
            near,
            far,
            is_orthographic: false,
            ortho_size: 5.0,
        }
    }

    /// `new THREE.OrthographicCamera(left, right, top, bottom, near, far)`.
    /// We simplify to size-based orthographic.
    pub fn orthographic_camera(
        _left: f32,
        _right: f32,
        _top: f32,
        _bottom: f32,
        near: f32,
        far: f32,
        ctx: &SceneContext,
    ) -> Self {
        let base = Object3D::new("OrthoCamera", ctx);
        ctx.add_camera_component(&base.entity_id, 60.0, near, far);
        Self {
            base,
            fov: 60.0,
            near,
            far,
            is_orthographic: true,
            ortho_size: 5.0,
        }
    }

    pub fn set_position(&mut self, x: f32, y: f32, z: f32) {
        self.base.set_position(x, y, z);
    }
    pub fn get_position(&self) -> Vector3 {
        self.base.get_position()
    }
    pub fn position(&mut self) -> PositionProxy {
        self.base.position()
    }
    pub fn look_at(&mut self, tx: f32, ty: f32, tz: f32) {
        self.base.look_at(tx, ty, tz);
    }
    pub fn entity_id(&self) -> String {
        self.base.entity_id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_context_create_and_read_entity() {
        let ctx = SceneContext::new();
        let mut scene = engine_ecs::Scene::new();
        let entity = scene.create_object("Test").unwrap();
        let handle = entity.handle();
        let eid = format!("{}:{}", handle.slot(), handle.generation().get());

        // Set initial transform
        scene
            .transforms_mut()
            .set_local(entity, engine_core::math::Transform::IDENTITY);
        ctx.set_scene(scene);

        // Read position
        let pos = ctx.read_position(&eid);
        assert_eq!(pos, Vector3::new(0.0, 0.0, 0.0));

        // Write position
        ctx.write_position(&eid, 5.0, 10.0, 15.0);
        let pos = ctx.read_position(&eid);
        assert_eq!(pos, Vector3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn object3d_creates_and_destroys_entity() {
        let ctx = SceneContext::new();
        let mut scene = engine_ecs::Scene::new();
        ctx.set_scene(scene);

        let mut obj = Object3D::new("TestObj", &ctx);
        assert!(!obj.entity_id.is_empty());

        // Set position
        obj.set_position(1.0, 2.0, 3.0);
        let pos = obj.get_position();
        assert_eq!(pos, Vector3::new(1.0, 2.0, 3.0));

        // Destroy
        obj.destroy();
        let mut scene = ctx.take_scene().unwrap();
        let _ = scene.process_deferred_destroy();
    }

    #[test]
    fn mesh_creates_with_geometry_and_material() {
        let ctx = SceneContext::new();
        let scene = engine_ecs::Scene::new();
        ctx.set_scene(scene);

        let geometry = Geometry::box_geometry(1.0, 1.0, 1.0);
        let material = Material::basic_red();
        let mut mesh = Mesh::new("Cube", geometry, material, &ctx);

        assert!(!mesh.entity_id().is_empty());
        mesh.set_position(0.0, 2.0, 0.0);
        let pos = mesh.get_position();
        assert_eq!(pos, Vector3::new(0.0, 2.0, 0.0));

        // Verify entity exists in scene
        let scene = ctx.take_scene().unwrap();
        let entity = SceneContext::parse_entity(&mesh.entity_id()).unwrap();
        assert!(scene.transforms().local(entity).is_some());
    }
}
