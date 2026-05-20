#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Physics abstraction and null backend for the Aster engine.
//!
//! The null backend compiles everywhere and satisfies the trait contract without
//! linking any physics library. The `rapier` feature enables [`RapierPhysicsBackend`],
//! the production backend used by runtime-game builds.

use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

pub use engine_core::math::{Quat, Transform, Vec3};

// ── Primitive types ──────────────────────────────────────────────────────────

/// Opaque handle to a physics body.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BodyHandle(pub u64);

/// Opaque handle to a collider.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ColliderHandle(pub u64);

/// Physics body kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// Fully simulated body.
    #[default]
    Dynamic,
    /// Moved by the user, pushes dynamic bodies.
    Kinematic,
    /// Never moves.
    Static,
}

/// Rigidbody creation parameters.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RigidbodyDesc {
    /// Initial world-space transform.
    pub transform: Transform,
    /// Body kind.
    pub kind: BodyKind,
    /// Linear damping coefficient.
    pub linear_damping: f32,
    /// Angular damping coefficient.
    pub angular_damping: f32,
    /// Gravity scale multiplier.
    pub gravity_scale: f32,
}

impl Default for RigidbodyDesc {
    fn default() -> Self {
        Self {
            transform: Transform::IDENTITY,
            kind: BodyKind::Dynamic,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 1.0,
        }
    }
}

// ── Collider shapes ──────────────────────────────────────────────────────────

/// Collider shape.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ColliderShape {
    /// Axis-aligned box.
    Box {
        /// Half-extents on each axis.
        half_extents: Vec3,
    },
    /// Sphere.
    Sphere {
        /// Radius.
        radius: f32,
    },
    /// Capsule aligned along the Y axis.
    Capsule {
        /// Half-height of the cylindrical section.
        half_height: f32,
        /// Radius of the end caps.
        radius: f32,
    },
    /// Convex mesh approximation (vertex soup).
    Mesh {
        /// Flat list of vertex positions (x,y,z triplets).
        vertices: Vec<f32>,
    },
}

/// Collider creation parameters.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ColliderDesc {
    /// Shape.
    pub shape: ColliderShape,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
    /// When true the collider fires overlap events instead of resolving contacts.
    pub is_trigger: bool,
    /// Collision layer this collider belongs to.
    pub layer: u32,
    /// Bitmask of layers this collider collides with.
    pub mask: u32,
}

impl Default for ColliderDesc {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
            friction: 0.5,
            restitution: 0.0,
            is_trigger: false,
            layer: 1,
            mask: !0,
        }
    }
}

// ── Contact callbacks ────────────────────────────────────────────────────────

/// A contact event between two bodies.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactEvent {
    /// First body.
    pub body_a: BodyHandle,
    /// Second body.
    pub body_b: BodyHandle,
    /// Contact point in world space.
    pub point: Vec3,
    /// Contact normal pointing from B toward A.
    pub normal: Vec3,
    /// Whether this is an enter (true) or exit (false) event.
    pub entered: bool,
    /// Whether at least one collider in the pair is a trigger/sensor.
    pub is_trigger: bool,
}

// ── Query types ──────────────────────────────────────────────────────────────

/// A single raycast hit.
#[derive(Clone, Debug, PartialEq)]
pub struct RayHit {
    /// Hit body.
    pub body: BodyHandle,
    /// Hit collider.
    pub collider: ColliderHandle,
    /// Hit point in world space.
    pub point: Vec3,
    /// Surface normal at the hit point.
    pub normal: Vec3,
    /// Distance from ray origin.
    pub distance: f32,
}

/// A single overlap result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlapResult {
    /// Overlapping body.
    pub body: BodyHandle,
    /// Overlapping collider.
    pub collider: ColliderHandle,
}

/// Query filter controlling which layers are tested.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QueryFilter {
    /// Layer mask; zero means test all layers.
    pub mask: u32,
}

/// Kinematic character movement request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterControllerDesc {
    /// Character collider shape used for sweeps.
    pub shape: ColliderShapeRef,
    /// Desired world-space translation for this step.
    pub translation: Vec3,
    /// Step time in seconds.
    pub dt: f32,
    /// Layers considered solid for the controller.
    pub filter: QueryFilter,
}

/// Borrowable collider shape for query/controller APIs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderShapeRef {
    /// Axis-aligned box.
    Box {
        /// Half-extents on each axis.
        half_extents: Vec3,
    },
    /// Sphere.
    Sphere {
        /// Radius.
        radius: f32,
    },
    /// Capsule aligned along the Y axis.
    Capsule {
        /// Half-height of the cylindrical section.
        half_height: f32,
        /// Radius of the end caps.
        radius: f32,
    },
}

impl Default for ColliderShapeRef {
    fn default() -> Self {
        Self::Capsule {
            half_height: 0.5,
            radius: 0.25,
        }
    }
}

/// Result of a kinematic character movement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CharacterControllerOutput {
    /// Effective translation applied after collisions/sliding.
    pub translation: Vec3,
    /// True when the controller ended grounded.
    pub grounded: bool,
    /// Number of collision callbacks observed during the move.
    pub collisions: usize,
}

// ── Layer matrix ─────────────────────────────────────────────────────────────

/// 32-layer collision matrix.
#[derive(Clone, Debug)]
pub struct LayerMatrix {
    rows: [u32; 32],
}

impl Default for LayerMatrix {
    fn default() -> Self {
        // All layers collide with all layers by default.
        Self { rows: [!0u32; 32] }
    }
}

impl LayerMatrix {
    /// Returns whether layer `a` collides with layer `b`.
    pub fn collides(&self, a: u32, b: u32) -> bool {
        let a = (a as usize).min(31);
        let b = (b as usize).min(31);
        self.rows[a] & (1 << b) != 0
    }

    /// Sets whether layer `a` collides with layer `b` (symmetric).
    pub fn set(&mut self, a: u32, b: u32, enabled: bool) {
        let a = (a as usize).min(31);
        let b = (b as usize).min(31);
        if enabled {
            self.rows[a] |= 1 << b;
            self.rows[b] |= 1 << a;
        } else {
            self.rows[a] &= !(1 << b);
            self.rows[b] &= !(1 << a);
        }
    }
}

// ── Backend trait ────────────────────────────────────────────────────────────

/// Pluggable physics backend contract.
///
/// Implementations are expected to step the simulation on `fixed_update` and
/// synchronise body transforms with the ECS on `sync_transforms`.
pub trait PhysicsBackend: Send + Sync {
    /// Advances the simulation by `dt` seconds.
    fn fixed_update(&mut self, dt: f32);

    /// Creates a rigidbody and returns its handle.
    fn create_body(&mut self, desc: &RigidbodyDesc) -> EngineResult<BodyHandle>;

    /// Destroys a body and all attached colliders.
    fn destroy_body(&mut self, body: BodyHandle) -> EngineResult<()>;

    /// Attaches a collider to a body.
    fn add_collider(
        &mut self,
        body: BodyHandle,
        desc: &ColliderDesc,
    ) -> EngineResult<ColliderHandle>;

    /// Removes a collider.
    fn remove_collider(&mut self, collider: ColliderHandle) -> EngineResult<()>;

    /// Returns the current world-space transform of a body.
    fn body_transform(&self, body: BodyHandle) -> EngineResult<Transform>;

    /// Teleports a body to a new world-space transform.
    fn set_body_transform(&mut self, body: BodyHandle, transform: Transform) -> EngineResult<()>;

    /// Applies a linear impulse to a body.
    fn apply_impulse(&mut self, body: BodyHandle, impulse: Vec3) -> EngineResult<()>;

    /// Casts a ray and returns the closest hit, if any.
    fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit>;

    /// Returns all colliders overlapping a sphere.
    fn overlap_sphere(&self, center: Vec3, radius: f32, filter: QueryFilter) -> Vec<OverlapResult>;

    /// Sweeps a sphere along a direction and returns the first hit, if any.
    fn sweep_sphere(
        &self,
        center: Vec3,
        radius: f32,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit>;

    /// Drains pending contact events since the last call.
    fn drain_contacts(&mut self) -> Vec<ContactEvent>;

    /// Moves a kinematic body with a basic character controller.
    fn move_character(
        &mut self,
        body: BodyHandle,
        desc: CharacterControllerDesc,
    ) -> EngineResult<CharacterControllerOutput>;
}

// ── Null backend ─────────────────────────────────────────────────────────────

/// No-op physics backend. Compiles everywhere; produces no simulation.
#[derive(Default)]
pub struct NullPhysicsBackend;

impl PhysicsBackend for NullPhysicsBackend {
    fn fixed_update(&mut self, _dt: f32) {}

    fn create_body(&mut self, _desc: &RigidbodyDesc) -> EngineResult<BodyHandle> {
        Err(EngineError::other("null physics backend"))
    }

    fn destroy_body(&mut self, _body: BodyHandle) -> EngineResult<()> {
        Ok(())
    }

    fn add_collider(
        &mut self,
        _body: BodyHandle,
        _desc: &ColliderDesc,
    ) -> EngineResult<ColliderHandle> {
        Err(EngineError::other("null physics backend"))
    }

    fn remove_collider(&mut self, _collider: ColliderHandle) -> EngineResult<()> {
        Ok(())
    }

    fn body_transform(&self, _body: BodyHandle) -> EngineResult<Transform> {
        Err(EngineError::other("null physics backend"))
    }

    fn set_body_transform(&mut self, _body: BodyHandle, _transform: Transform) -> EngineResult<()> {
        Ok(())
    }

    fn apply_impulse(&mut self, _body: BodyHandle, _impulse: Vec3) -> EngineResult<()> {
        Ok(())
    }

    fn raycast(
        &self,
        _origin: Vec3,
        _direction: Vec3,
        _max_distance: f32,
        _filter: QueryFilter,
    ) -> Option<RayHit> {
        None
    }

    fn overlap_sphere(
        &self,
        _center: Vec3,
        _radius: f32,
        _filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        Vec::new()
    }

    fn sweep_sphere(
        &self,
        _center: Vec3,
        _radius: f32,
        _direction: Vec3,
        _max_distance: f32,
        _filter: QueryFilter,
    ) -> Option<RayHit> {
        None
    }

    fn drain_contacts(&mut self) -> Vec<ContactEvent> {
        Vec::new()
    }

    fn move_character(
        &mut self,
        _body: BodyHandle,
        _desc: CharacterControllerDesc,
    ) -> EngineResult<CharacterControllerOutput> {
        Err(EngineError::other("null physics backend"))
    }
}

// ── World-level physics context ───────────────────────────────────────────────

/// Physics world that owns a backend and the layer matrix.
pub struct PhysicsWorld {
    backend: Box<dyn PhysicsBackend>,
    /// Layer collision matrix.
    pub layer_matrix: LayerMatrix,
}

impl fmt::Debug for PhysicsWorld {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PhysicsWorld")
            .field("layer_matrix", &self.layer_matrix)
            .finish_non_exhaustive()
    }
}

impl PhysicsWorld {
    /// Creates a physics world with the given backend.
    pub fn new(backend: impl PhysicsBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            layer_matrix: LayerMatrix::default(),
        }
    }

    /// Creates a physics world backed by the null backend.
    pub fn null() -> Self {
        Self::new(NullPhysicsBackend)
    }

    /// Steps the simulation.
    pub fn fixed_update(&mut self, dt: f32) {
        self.backend.fixed_update(dt);
    }

    /// Delegates to the backend.
    pub fn backend_mut(&mut self) -> &mut dyn PhysicsBackend {
        self.backend.as_mut()
    }

    /// Delegates to the backend (read-only).
    pub fn backend(&self) -> &dyn PhysicsBackend {
        self.backend.as_ref()
    }
}

// ── Simple deterministic backend ─────────────────────────────────────────────

#[derive(Clone, Debug)]
struct SimpleBody {
    desc: RigidbodyDesc,
    transform: Transform,
    velocity: Vec3,
    colliders: Vec<ColliderHandle>,
}

#[derive(Clone, Debug)]
struct SimpleCollider {
    body: BodyHandle,
    desc: ColliderDesc,
}

/// Small deterministic physics backend used until a native Rapier/Jolt backend is wired.
///
/// The backend supports rigidbody creation, collider lifetime, gravity for dynamic
/// bodies, sphere/box overlap, raycast, sphere sweep, and enter/exit events. It is
/// intentionally conservative: collision resolution is not attempted yet, so game
/// code can rely on queries and triggers while the engine keeps a dependency-light
/// default path.
#[derive(Debug)]
pub struct SimplePhysicsBackend {
    next_body: u64,
    next_collider: u64,
    bodies: HashMap<BodyHandle, SimpleBody>,
    colliders: HashMap<ColliderHandle, SimpleCollider>,
    active_pairs: HashSet<(ColliderHandle, ColliderHandle)>,
    contacts: Vec<ContactEvent>,
    gravity: Vec3,
}

impl Default for SimplePhysicsBackend {
    fn default() -> Self {
        Self {
            next_body: 1,
            next_collider: 1,
            bodies: HashMap::new(),
            colliders: HashMap::new(),
            active_pairs: HashSet::new(),
            contacts: Vec::new(),
            gravity: Vec3::new(0.0, -9.81, 0.0),
        }
    }
}

impl SimplePhysicsBackend {
    /// Creates a new simple physics backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of live bodies.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Returns the number of live colliders.
    pub fn collider_count(&self) -> usize {
        self.colliders.len()
    }

    fn body(&self, handle: BodyHandle) -> EngineResult<&SimpleBody> {
        self.bodies
            .get(&handle)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))
    }

    fn body_mut(&mut self, handle: BodyHandle) -> EngineResult<&mut SimpleBody> {
        self.bodies
            .get_mut(&handle)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))
    }

    fn collider_world_sphere(&self, collider: ColliderHandle) -> Option<(Vec3, f32)> {
        let collider = self.colliders.get(&collider)?;
        let body = self.bodies.get(&collider.body)?;
        Some(shape_world_sphere(
            body.transform.translation,
            &collider.desc.shape,
        ))
    }

    fn collide(&self, a: ColliderHandle, b: ColliderHandle) -> Option<ContactEvent> {
        let collider_a = self.colliders.get(&a)?;
        let collider_b = self.colliders.get(&b)?;
        if collider_a.body == collider_b.body {
            return None;
        }
        if !layers_match(&collider_a.desc, &collider_b.desc) {
            return None;
        }
        let (center_a, radius_a) = self.collider_world_sphere(a)?;
        let (center_b, radius_b) = self.collider_world_sphere(b)?;
        let delta = center_a - center_b;
        let distance_squared = delta.length_squared();
        let radius = radius_a + radius_b;
        if distance_squared > radius * radius {
            return None;
        }
        let normal = if distance_squared <= f32::EPSILON {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            delta.normalized()
        };
        Some(ContactEvent {
            body_a: collider_a.body,
            body_b: collider_b.body,
            point: center_b + normal * radius_b,
            normal,
            entered: true,
            is_trigger: collider_a.desc.is_trigger || collider_b.desc.is_trigger,
        })
    }

    fn update_contacts(&mut self) {
        let handles = self.colliders.keys().copied().collect::<Vec<_>>();
        let mut current_pairs = HashSet::new();
        for (index, left) in handles.iter().enumerate() {
            for right in handles.iter().skip(index + 1) {
                let pair = ordered_pair(*left, *right);
                if let Some(mut event) = self.collide(*left, *right) {
                    current_pairs.insert(pair);
                    if !self.active_pairs.contains(&pair) {
                        event.entered = true;
                        self.contacts.push(event);
                    }
                }
            }
        }
        for pair in self.active_pairs.difference(&current_pairs) {
            if let (Some(left), Some(right)) =
                (self.colliders.get(&pair.0), self.colliders.get(&pair.1))
            {
                self.contacts.push(ContactEvent {
                    body_a: left.body,
                    body_b: right.body,
                    point: Vec3::ZERO,
                    normal: Vec3::ZERO,
                    entered: false,
                    is_trigger: left.desc.is_trigger || right.desc.is_trigger,
                });
            }
        }
        self.active_pairs = current_pairs;
    }

    fn resolve_contacts(&mut self) {
        let handles = self.colliders.keys().copied().collect::<Vec<_>>();
        for (index, left) in handles.iter().enumerate() {
            for right in handles.iter().skip(index + 1) {
                let Some((normal, penetration)) = self.collision_penetration(*left, *right) else {
                    continue;
                };
                let Some(left_collider) = self.colliders.get(left) else {
                    continue;
                };
                let Some(right_collider) = self.colliders.get(right) else {
                    continue;
                };
                if left_collider.desc.is_trigger || right_collider.desc.is_trigger {
                    continue;
                }
                self.apply_separation(left_collider.body, right_collider.body, normal, penetration);
            }
        }
    }

    fn collision_penetration(&self, a: ColliderHandle, b: ColliderHandle) -> Option<(Vec3, f32)> {
        let collider_a = self.colliders.get(&a)?;
        let collider_b = self.colliders.get(&b)?;
        if collider_a.body == collider_b.body || !layers_match(&collider_a.desc, &collider_b.desc) {
            return None;
        }
        let (center_a, radius_a) = self.collider_world_sphere(a)?;
        let (center_b, radius_b) = self.collider_world_sphere(b)?;
        let delta = center_a - center_b;
        let distance = delta.length();
        let penetration = radius_a + radius_b - distance;
        if penetration <= 0.0 {
            return None;
        }
        let normal = if distance <= f32::EPSILON {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            delta / distance
        };
        Some((normal, penetration))
    }

    fn apply_separation(
        &mut self,
        body_a: BodyHandle,
        body_b: BodyHandle,
        normal: Vec3,
        penetration: f32,
    ) {
        let (kind_a, kind_b) = match (self.bodies.get(&body_a), self.bodies.get(&body_b)) {
            (Some(a), Some(b)) => (a.desc.kind, b.desc.kind),
            _ => return,
        };
        match (kind_a, kind_b) {
            (BodyKind::Dynamic, BodyKind::Dynamic) => {
                self.translate_body(body_a, normal * (penetration * 0.5));
                self.translate_body(body_b, normal * (-penetration * 0.5));
                self.cancel_velocity(body_a, normal);
                self.cancel_velocity(body_b, normal * -1.0);
            }
            (BodyKind::Dynamic, _) => {
                self.translate_body(body_a, normal * penetration);
                self.cancel_velocity(body_a, normal);
            }
            (_, BodyKind::Dynamic) => {
                self.translate_body(body_b, normal * -penetration);
                self.cancel_velocity(body_b, normal * -1.0);
            }
            _ => {}
        }
    }

    fn translate_body(&mut self, body: BodyHandle, delta: Vec3) {
        if let Some(body) = self.bodies.get_mut(&body) {
            body.transform.translation += delta;
        }
    }

    fn cancel_velocity(&mut self, body: BodyHandle, normal: Vec3) {
        if let Some(body) = self.bodies.get_mut(&body) {
            let into_surface = body.velocity.dot(normal);
            if into_surface < 0.0 {
                body.velocity -= normal * into_surface;
            }
        }
    }
}

impl PhysicsBackend for SimplePhysicsBackend {
    fn fixed_update(&mut self, dt: f32) {
        for body in self.bodies.values_mut() {
            if body.desc.kind == BodyKind::Dynamic {
                body.velocity += self.gravity * body.desc.gravity_scale * dt;
                body.transform.translation += body.velocity * dt;
            }
        }
        self.update_contacts();
        self.resolve_contacts();
    }

    fn create_body(&mut self, desc: &RigidbodyDesc) -> EngineResult<BodyHandle> {
        let handle = BodyHandle(self.next_body);
        self.next_body = self.next_body.saturating_add(1).max(1);
        self.bodies.insert(
            handle,
            SimpleBody {
                desc: desc.clone(),
                transform: desc.transform,
                velocity: Vec3::ZERO,
                colliders: Vec::new(),
            },
        );
        Ok(handle)
    }

    fn destroy_body(&mut self, body: BodyHandle) -> EngineResult<()> {
        let body = self
            .bodies
            .remove(&body)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
        for collider in body.colliders {
            self.colliders.remove(&collider);
        }
        self.active_pairs.retain(|(left, right)| {
            self.colliders.contains_key(left) && self.colliders.contains_key(right)
        });
        Ok(())
    }

    fn add_collider(
        &mut self,
        body: BodyHandle,
        desc: &ColliderDesc,
    ) -> EngineResult<ColliderHandle> {
        self.body(body)?;
        let handle = ColliderHandle(self.next_collider);
        self.next_collider = self.next_collider.saturating_add(1).max(1);
        self.colliders.insert(
            handle,
            SimpleCollider {
                body,
                desc: desc.clone(),
            },
        );
        self.body_mut(body)?.colliders.push(handle);
        Ok(handle)
    }

    fn remove_collider(&mut self, collider: ColliderHandle) -> EngineResult<()> {
        let removed = self
            .colliders
            .remove(&collider)
            .ok_or_else(|| EngineError::invalid_handle("physics collider does not exist"))?;
        if let Some(body) = self.bodies.get_mut(&removed.body) {
            body.colliders.retain(|candidate| *candidate != collider);
        }
        self.active_pairs
            .retain(|(left, right)| *left != collider && *right != collider);
        Ok(())
    }

    fn body_transform(&self, body: BodyHandle) -> EngineResult<Transform> {
        Ok(self.body(body)?.transform)
    }

    fn set_body_transform(&mut self, body: BodyHandle, transform: Transform) -> EngineResult<()> {
        self.body_mut(body)?.transform = transform;
        Ok(())
    }

    fn apply_impulse(&mut self, body: BodyHandle, impulse: Vec3) -> EngineResult<()> {
        let body = self.body_mut(body)?;
        if body.desc.kind == BodyKind::Dynamic {
            body.velocity += impulse;
        }
        Ok(())
    }

    fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit> {
        let direction = direction.normalized();
        if direction == Vec3::ZERO {
            return None;
        }
        self.colliders
            .iter()
            .filter(|(_, collider)| filter_matches(collider.desc.layer, filter))
            .filter_map(|(handle, collider)| {
                let (center, radius) = self.collider_world_sphere(*handle)?;
                ray_sphere(origin, direction, max_distance, center, radius).map(|distance| RayHit {
                    body: collider.body,
                    collider: *handle,
                    point: origin + direction * distance,
                    normal: (origin + direction * distance - center).normalized(),
                    distance,
                })
            })
            .min_by(|left, right| left.distance.total_cmp(&right.distance))
    }

    fn overlap_sphere(&self, center: Vec3, radius: f32, filter: QueryFilter) -> Vec<OverlapResult> {
        self.colliders
            .iter()
            .filter(|(_, collider)| filter_matches(collider.desc.layer, filter))
            .filter_map(|(handle, collider)| {
                let (other_center, other_radius) = self.collider_world_sphere(*handle)?;
                ((center - other_center).length_squared() <= (radius + other_radius).powi(2))
                    .then_some(OverlapResult {
                        body: collider.body,
                        collider: *handle,
                    })
            })
            .collect()
    }

    fn sweep_sphere(
        &self,
        center: Vec3,
        radius: f32,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit> {
        let direction = direction.normalized();
        if direction == Vec3::ZERO {
            return None;
        }
        self.colliders
            .iter()
            .filter(|(_, collider)| filter_matches(collider.desc.layer, filter))
            .filter_map(|(handle, collider)| {
                let (other_center, other_radius) = self.collider_world_sphere(*handle)?;
                ray_sphere(
                    center,
                    direction,
                    max_distance,
                    other_center,
                    radius + other_radius,
                )
                .map(|distance| RayHit {
                    body: collider.body,
                    collider: *handle,
                    point: center + direction * distance,
                    normal: (center + direction * distance - other_center).normalized(),
                    distance,
                })
            })
            .min_by(|left, right| left.distance.total_cmp(&right.distance))
    }

    fn drain_contacts(&mut self) -> Vec<ContactEvent> {
        std::mem::take(&mut self.contacts)
    }

    fn move_character(
        &mut self,
        body: BodyHandle,
        desc: CharacterControllerDesc,
    ) -> EngineResult<CharacterControllerOutput> {
        let radius = match desc.shape {
            ColliderShapeRef::Box { half_extents } => half_extents.length(),
            ColliderShapeRef::Sphere { radius } => radius,
            ColliderShapeRef::Capsule {
                half_height,
                radius,
            } => half_height + radius,
        };
        let transform = self.body(body)?.transform;
        let distance = desc.translation.length();
        let direction = desc.translation.normalized();
        let hit = if distance > f32::EPSILON {
            self.sweep_sphere(
                transform.translation,
                radius,
                direction,
                distance,
                desc.filter,
            )
        } else {
            None
        };
        let translation = hit
            .as_ref()
            .map(|hit| direction * hit.distance.max(0.0))
            .unwrap_or(desc.translation);
        self.body_mut(body)?.transform.translation += translation;
        Ok(CharacterControllerOutput {
            translation,
            grounded: translation.y <= f32::EPSILON
                && self
                    .sweep_sphere(
                        transform.translation + translation,
                        radius,
                        Vec3::new(0.0, -1.0, 0.0),
                        0.08,
                        desc.filter,
                    )
                    .is_some(),
            collisions: usize::from(hit.is_some()),
        })
    }
}

fn ordered_pair(left: ColliderHandle, right: ColliderHandle) -> (ColliderHandle, ColliderHandle) {
    if left.0 <= right.0 {
        (left, right)
    } else {
        (right, left)
    }
}

fn layers_match(left: &ColliderDesc, right: &ColliderDesc) -> bool {
    (left.mask & (1 << right.layer.min(31))) != 0 && (right.mask & (1 << left.layer.min(31))) != 0
}

fn filter_matches(layer: u32, filter: QueryFilter) -> bool {
    filter.mask == 0 || (filter.mask & (1 << layer.min(31))) != 0
}

fn shape_world_sphere(center: Vec3, shape: &ColliderShape) -> (Vec3, f32) {
    let radius = match shape {
        ColliderShape::Box { half_extents } => half_extents.length(),
        ColliderShape::Sphere { radius } => *radius,
        ColliderShape::Capsule {
            half_height,
            radius,
        } => half_height + radius,
        ColliderShape::Mesh { vertices } => vertices
            .chunks_exact(3)
            .map(|chunk| Vec3::new(chunk[0], chunk[1], chunk[2]).length())
            .fold(0.0, f32::max),
    };
    (center, radius)
}

fn ray_sphere(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    center: Vec3,
    radius: f32,
) -> Option<f32> {
    let to_center = center - origin;
    let projection = to_center.dot(direction);
    let closest_squared = to_center.length_squared() - projection * projection;
    let radius_squared = radius * radius;
    if closest_squared > radius_squared {
        return None;
    }
    let offset = (radius_squared - closest_squared).sqrt();
    let distance = if projection - offset >= 0.0 {
        projection - offset
    } else {
        projection + offset
    };
    (distance >= 0.0 && distance <= max_distance).then_some(distance)
}

// ── Rapier backend ───────────────────────────────────────────────────────────

#[cfg(feature = "rapier")]
mod rapier_backend {
    use std::collections::HashMap;

    use rapier3d::{
        control as rpc,
        crossbeam::channel::{unbounded, Receiver},
        na::{Point3, Quaternion, Translation3, UnitQuaternion, Vector3},
        parry::query::ShapeCastOptions,
        prelude as rp,
    };

    use super::*;

    /// Rapier-backed physics implementation for runtime-game builds.
    pub struct RapierPhysicsBackend {
        pipeline: rp::PhysicsPipeline,
        gravity: rp::Vector<rp::Real>,
        integration: rp::IntegrationParameters,
        islands: rp::IslandManager,
        broad_phase: rp::BroadPhaseMultiSap,
        narrow_phase: rp::NarrowPhase,
        bodies: rp::RigidBodySet,
        colliders: rp::ColliderSet,
        impulse_joints: rp::ImpulseJointSet,
        multibody_joints: rp::MultibodyJointSet,
        ccd_solver: rp::CCDSolver,
        query_pipeline: rp::QueryPipeline,
        collision_events: Receiver<rp::CollisionEvent>,
        contact_force_events: Receiver<rp::ContactForceEvent>,
        event_handler: rp::ChannelEventCollector,
        next_body: u64,
        next_collider: u64,
        body_handles: HashMap<BodyHandle, rp::RigidBodyHandle>,
        collider_handles: HashMap<ColliderHandle, rp::ColliderHandle>,
        rapier_bodies: HashMap<rp::RigidBodyHandle, BodyHandle>,
        rapier_colliders: HashMap<rp::ColliderHandle, ColliderHandle>,
        pending_contacts: Vec<ContactEvent>,
    }

    impl Default for RapierPhysicsBackend {
        fn default() -> Self {
            let (collision_send, collision_events) = unbounded();
            let (contact_force_send, contact_force_events) = unbounded();
            Self {
                pipeline: rp::PhysicsPipeline::new(),
                gravity: vector3(Vec3::new(0.0, -9.81, 0.0)),
                integration: rp::IntegrationParameters::default(),
                islands: rp::IslandManager::new(),
                broad_phase: rp::BroadPhaseMultiSap::new(),
                narrow_phase: rp::NarrowPhase::new(),
                bodies: rp::RigidBodySet::new(),
                colliders: rp::ColliderSet::new(),
                impulse_joints: rp::ImpulseJointSet::new(),
                multibody_joints: rp::MultibodyJointSet::new(),
                ccd_solver: rp::CCDSolver::new(),
                query_pipeline: rp::QueryPipeline::new(),
                collision_events,
                contact_force_events,
                event_handler: rp::ChannelEventCollector::new(collision_send, contact_force_send),
                next_body: 1,
                next_collider: 1,
                body_handles: HashMap::new(),
                collider_handles: HashMap::new(),
                rapier_bodies: HashMap::new(),
                rapier_colliders: HashMap::new(),
                pending_contacts: Vec::new(),
            }
        }
    }

    impl RapierPhysicsBackend {
        /// Creates an empty Rapier physics backend.
        pub fn new() -> Self {
            Self::default()
        }

        /// Returns the number of live rigid bodies.
        pub fn body_count(&self) -> usize {
            self.body_handles.len()
        }

        /// Returns the number of live colliders.
        pub fn collider_count(&self) -> usize {
            self.collider_handles.len()
        }

        fn rapier_body(&self, body: BodyHandle) -> EngineResult<rp::RigidBodyHandle> {
            self.body_handles
                .get(&body)
                .copied()
                .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))
        }

        fn rapier_collider(&self, collider: ColliderHandle) -> EngineResult<rp::ColliderHandle> {
            self.collider_handles
                .get(&collider)
                .copied()
                .ok_or_else(|| EngineError::invalid_handle("physics collider does not exist"))
        }

        fn collider_owner(&self, collider: rp::ColliderHandle) -> Option<BodyHandle> {
            self.colliders
                .get(collider)
                .and_then(|collider| collider.parent())
                .and_then(|parent| self.rapier_bodies.get(&parent).copied())
        }

        fn drain_rapier_events(&mut self) {
            while let Ok(event) = self.collision_events.try_recv() {
                let Some(body_a) = self.collider_owner(event.collider1()) else {
                    continue;
                };
                let Some(body_b) = self.collider_owner(event.collider2()) else {
                    continue;
                };
                self.pending_contacts.push(ContactEvent {
                    body_a,
                    body_b,
                    point: Vec3::ZERO,
                    normal: Vec3::ZERO,
                    entered: event.started(),
                    is_trigger: event.sensor(),
                });
            }
            while self.contact_force_events.try_recv().is_ok() {}
        }
    }

    impl PhysicsBackend for RapierPhysicsBackend {
        fn fixed_update(&mut self, dt: f32) {
            self.integration.dt = dt.max(0.0);
            self.pipeline.step(
                &self.gravity,
                &self.integration,
                &mut self.islands,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.bodies,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                &mut self.ccd_solver,
                Some(&mut self.query_pipeline),
                &(),
                &self.event_handler,
            );
            self.drain_rapier_events();
        }

        fn create_body(&mut self, desc: &RigidbodyDesc) -> EngineResult<BodyHandle> {
            let builder = match desc.kind {
                BodyKind::Dynamic => rp::RigidBodyBuilder::dynamic(),
                BodyKind::Kinematic => rp::RigidBodyBuilder::kinematic_position_based(),
                BodyKind::Static => rp::RigidBodyBuilder::fixed(),
            }
            .position(isometry(desc.transform))
            .linear_damping(desc.linear_damping)
            .angular_damping(desc.angular_damping)
            .gravity_scale(desc.gravity_scale);
            let rapier = self.bodies.insert(builder.build());
            let handle = BodyHandle(self.next_body);
            self.next_body = self.next_body.saturating_add(1).max(1);
            self.body_handles.insert(handle, rapier);
            self.rapier_bodies.insert(rapier, handle);
            Ok(handle)
        }

        fn destroy_body(&mut self, body: BodyHandle) -> EngineResult<()> {
            let rapier = self.rapier_body(body)?;
            self.bodies.remove(
                rapier,
                &mut self.islands,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                true,
            );
            self.body_handles.remove(&body);
            self.rapier_bodies.remove(&rapier);
            self.collider_handles
                .retain(|_, rapier_collider| self.colliders.get(*rapier_collider).is_some());
            self.rapier_colliders
                .retain(|rapier_collider, _| self.colliders.get(*rapier_collider).is_some());
            self.query_pipeline.update(&self.colliders);
            Ok(())
        }

        fn add_collider(
            &mut self,
            body: BodyHandle,
            desc: &ColliderDesc,
        ) -> EngineResult<ColliderHandle> {
            let rapier_body = self.rapier_body(body)?;
            let builder = collider_builder(desc)
                .friction(desc.friction)
                .restitution(desc.restitution)
                .sensor(desc.is_trigger)
                .collision_groups(interaction_groups(desc.layer, desc.mask))
                .solver_groups(interaction_groups(desc.layer, desc.mask))
                .active_events(rp::ActiveEvents::COLLISION_EVENTS);
            let rapier =
                self.colliders
                    .insert_with_parent(builder.build(), rapier_body, &mut self.bodies);
            let handle = ColliderHandle(self.next_collider);
            self.next_collider = self.next_collider.saturating_add(1).max(1);
            self.collider_handles.insert(handle, rapier);
            self.rapier_colliders.insert(rapier, handle);
            self.query_pipeline.update(&self.colliders);
            Ok(handle)
        }

        fn remove_collider(&mut self, collider: ColliderHandle) -> EngineResult<()> {
            let rapier = self.rapier_collider(collider)?;
            self.colliders
                .remove(rapier, &mut self.islands, &mut self.bodies, true);
            self.collider_handles.remove(&collider);
            self.rapier_colliders.remove(&rapier);
            self.query_pipeline.update(&self.colliders);
            Ok(())
        }

        fn body_transform(&self, body: BodyHandle) -> EngineResult<Transform> {
            let rapier = self.rapier_body(body)?;
            let body = self
                .bodies
                .get(rapier)
                .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
            Ok(transform(*body.position()))
        }

        fn set_body_transform(
            &mut self,
            body: BodyHandle,
            transform: Transform,
        ) -> EngineResult<()> {
            let rapier = self.rapier_body(body)?;
            let body = self
                .bodies
                .get_mut(rapier)
                .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
            let pos = isometry(transform);
            if body.is_kinematic() {
                body.set_next_kinematic_position(pos);
            } else {
                body.set_position(pos, true);
            }
            self.query_pipeline.update(&self.colliders);
            Ok(())
        }

        fn apply_impulse(&mut self, body: BodyHandle, impulse: Vec3) -> EngineResult<()> {
            let rapier = self.rapier_body(body)?;
            let body = self
                .bodies
                .get_mut(rapier)
                .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
            if body.is_dynamic() {
                body.apply_impulse(vector3(impulse), true);
            }
            Ok(())
        }

        fn raycast(
            &self,
            origin: Vec3,
            direction: Vec3,
            max_distance: f32,
            filter: QueryFilter,
        ) -> Option<RayHit> {
            let direction = direction.normalized();
            if direction == Vec3::ZERO {
                return None;
            }
            let ray = rp::Ray::new(point3(origin), vector3(direction));
            self.query_pipeline
                .cast_ray_and_get_normal(
                    &self.bodies,
                    &self.colliders,
                    &ray,
                    max_distance,
                    true,
                    query_filter(filter),
                )
                .and_then(|(rapier_collider, hit)| {
                    let collider = self.rapier_colliders.get(&rapier_collider).copied()?;
                    let body = self.collider_owner(rapier_collider)?;
                    Some(RayHit {
                        body,
                        collider,
                        point: origin + direction * hit.time_of_impact,
                        normal: vec3(hit.normal),
                        distance: hit.time_of_impact,
                    })
                })
        }

        fn overlap_sphere(
            &self,
            center: Vec3,
            radius: f32,
            filter: QueryFilter,
        ) -> Vec<OverlapResult> {
            let shape = rp::SharedShape::ball(radius);
            let mut results = Vec::new();
            self.query_pipeline.intersections_with_shape(
                &self.bodies,
                &self.colliders,
                &isometry(Transform {
                    translation: center,
                    ..Transform::IDENTITY
                }),
                shape.as_ref(),
                query_filter(filter),
                |rapier_collider| {
                    if let (Some(collider), Some(body)) = (
                        self.rapier_colliders.get(&rapier_collider).copied(),
                        self.collider_owner(rapier_collider),
                    ) {
                        results.push(OverlapResult { body, collider });
                    }
                    true
                },
            );
            results
        }

        fn sweep_sphere(
            &self,
            center: Vec3,
            radius: f32,
            direction: Vec3,
            max_distance: f32,
            filter: QueryFilter,
        ) -> Option<RayHit> {
            let direction = direction.normalized();
            if direction == Vec3::ZERO {
                return None;
            }
            let shape = rp::SharedShape::ball(radius);
            self.query_pipeline
                .cast_shape(
                    &self.bodies,
                    &self.colliders,
                    &isometry(Transform {
                        translation: center,
                        ..Transform::IDENTITY
                    }),
                    &vector3(direction),
                    shape.as_ref(),
                    ShapeCastOptions {
                        max_time_of_impact: max_distance,
                        ..ShapeCastOptions::default()
                    },
                    query_filter(filter),
                )
                .and_then(|(rapier_collider, hit)| {
                    let collider = self.rapier_colliders.get(&rapier_collider).copied()?;
                    let body = self.collider_owner(rapier_collider)?;
                    Some(RayHit {
                        body,
                        collider,
                        point: center + direction * hit.time_of_impact,
                        normal: vec3(*hit.normal1),
                        distance: hit.time_of_impact,
                    })
                })
        }

        fn drain_contacts(&mut self) -> Vec<ContactEvent> {
            self.drain_rapier_events();
            std::mem::take(&mut self.pending_contacts)
        }

        fn move_character(
            &mut self,
            body: BodyHandle,
            desc: CharacterControllerDesc,
        ) -> EngineResult<CharacterControllerOutput> {
            let rapier = self.rapier_body(body)?;
            let body_ref = self
                .bodies
                .get(rapier)
                .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
            let shape = shared_shape(desc.shape);
            let mut collisions = 0usize;
            let controller = rpc::KinematicCharacterController::default();
            let movement = controller.move_shape(
                desc.dt,
                &self.bodies,
                &self.colliders,
                &self.query_pipeline,
                shape.as_ref(),
                body_ref.position(),
                vector3(desc.translation),
                query_filter(desc.filter),
                |_| collisions = collisions.saturating_add(1),
            );
            let body_mut = self
                .bodies
                .get_mut(rapier)
                .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
            let next = *body_mut.position() * Translation3::from(movement.translation);
            if body_mut.is_kinematic() {
                body_mut.set_next_kinematic_position(next);
            } else {
                body_mut.set_position(next, true);
            }
            self.query_pipeline.update(&self.colliders);
            Ok(CharacterControllerOutput {
                translation: vec3(movement.translation),
                grounded: movement.grounded,
                collisions,
            })
        }
    }

    fn collider_builder(desc: &ColliderDesc) -> rp::ColliderBuilder {
        match &desc.shape {
            ColliderShape::Box { half_extents } => {
                rp::ColliderBuilder::cuboid(half_extents.x, half_extents.y, half_extents.z)
            }
            ColliderShape::Sphere { radius } => rp::ColliderBuilder::ball(*radius),
            ColliderShape::Capsule {
                half_height,
                radius,
            } => rp::ColliderBuilder::capsule_y(*half_height, *radius),
            ColliderShape::Mesh { vertices } => {
                let points = vertices
                    .chunks_exact(3)
                    .map(|chunk| Point3::new(chunk[0], chunk[1], chunk[2]))
                    .collect::<Vec<_>>();
                rp::ColliderBuilder::convex_hull(&points)
                    .unwrap_or_else(|| rp::ColliderBuilder::ball(0.5))
            }
        }
    }

    fn shared_shape(shape: ColliderShapeRef) -> rp::SharedShape {
        match shape {
            ColliderShapeRef::Box { half_extents } => {
                rp::SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z)
            }
            ColliderShapeRef::Sphere { radius } => rp::SharedShape::ball(radius),
            ColliderShapeRef::Capsule {
                half_height,
                radius,
            } => rp::SharedShape::capsule_y(half_height, radius),
        }
    }

    fn interaction_groups(layer: u32, mask: u32) -> rp::InteractionGroups {
        let membership = 1_u32 << layer.min(31);
        let filter = if mask == 0 { u32::MAX } else { mask };
        rp::InteractionGroups::new(
            rp::Group::from_bits_truncate(membership),
            rp::Group::from_bits_truncate(filter),
        )
    }

    fn query_filter(filter: QueryFilter) -> rp::QueryFilter<'static> {
        if filter.mask == 0 {
            rp::QueryFilter::default()
        } else {
            rp::QueryFilter::default().groups(rp::InteractionGroups::new(
                rp::Group::from_bits_truncate(u32::MAX),
                rp::Group::from_bits_truncate(filter.mask),
            ))
        }
    }

    fn isometry(transform: Transform) -> rp::Isometry<rp::Real> {
        rp::Isometry::from_parts(
            Translation3::new(
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ),
            UnitQuaternion::from_quaternion(Quaternion::new(
                transform.rotation.w,
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
            )),
        )
    }

    fn transform(isometry: rp::Isometry<rp::Real>) -> Transform {
        let rotation = isometry.rotation.quaternion();
        Transform {
            translation: vec3(isometry.translation.vector),
            rotation: Quat {
                x: rotation.i,
                y: rotation.j,
                z: rotation.k,
                w: rotation.w,
            },
            scale: Vec3::ONE,
        }
    }

    fn vector3(value: Vec3) -> Vector3<rp::Real> {
        Vector3::new(value.x, value.y, value.z)
    }

    fn point3(value: Vec3) -> Point3<rp::Real> {
        Point3::new(value.x, value.y, value.z)
    }

    fn vec3(value: Vector3<rp::Real>) -> Vec3 {
        Vec3::new(value.x, value.y, value.z)
    }
}

#[cfg(feature = "rapier")]
pub use rapier_backend::RapierPhysicsBackend;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_backend_raycast_returns_none() {
        let world = PhysicsWorld::null();
        let hit = world.backend().raycast(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 1.0),
            100.0,
            QueryFilter::default(),
        );
        assert!(hit.is_none());
    }

    #[test]
    fn null_backend_overlap_returns_empty() {
        let world = PhysicsWorld::null();
        let results = world
            .backend()
            .overlap_sphere(Vec3::ZERO, 1.0, QueryFilter::default());
        assert!(results.is_empty());
    }

    #[test]
    fn null_backend_contacts_are_empty() {
        let mut world = PhysicsWorld::null();
        assert!(world.backend_mut().drain_contacts().is_empty());
    }

    #[test]
    fn layer_matrix_symmetric_disable() {
        let mut matrix = LayerMatrix::default();
        assert!(matrix.collides(0, 1));
        matrix.set(0, 1, false);
        assert!(!matrix.collides(0, 1));
        assert!(!matrix.collides(1, 0));
    }

    #[test]
    fn collider_desc_defaults_are_sensible() {
        let desc = ColliderDesc::default();
        assert!(!desc.is_trigger);
        assert_eq!(desc.friction, 0.5);
    }

    #[test]
    fn simple_backend_raycast_hits_closest_collider() {
        let mut backend = SimplePhysicsBackend::new();
        let body = backend
            .create_body(&RigidbodyDesc {
                transform: Transform {
                    translation: Vec3::new(0.0, 0.0, 5.0),
                    ..Transform::IDENTITY
                },
                kind: BodyKind::Static,
                ..RigidbodyDesc::default()
            })
            .unwrap();
        backend
            .add_collider(body, &ColliderDesc::default())
            .unwrap();

        let hit = backend
            .raycast(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
                10.0,
                QueryFilter::default(),
            )
            .unwrap();

        assert_eq!(hit.body, body);
        assert!(hit.distance > 4.0);
    }

    #[test]
    fn simple_backend_emits_enter_and_exit_events() {
        let mut backend = SimplePhysicsBackend::new();
        let first = backend.create_body(&RigidbodyDesc::default()).unwrap();
        let second = backend
            .create_body(&RigidbodyDesc {
                transform: Transform {
                    translation: Vec3::new(0.5, 0.0, 0.0),
                    ..Transform::IDENTITY
                },
                ..RigidbodyDesc::default()
            })
            .unwrap();
        backend
            .add_collider(first, &ColliderDesc::default())
            .unwrap();
        backend
            .add_collider(second, &ColliderDesc::default())
            .unwrap();

        backend.fixed_update(0.0);
        assert!(backend.drain_contacts().iter().any(|event| event.entered));

        backend
            .set_body_transform(
                second,
                Transform {
                    translation: Vec3::new(10.0, 0.0, 0.0),
                    ..Transform::IDENTITY
                },
            )
            .unwrap();
        backend.fixed_update(0.0);
        assert!(backend.drain_contacts().iter().any(|event| !event.entered));
    }

    #[test]
    fn simple_backend_overlap_sphere_filters_by_layer() {
        let mut backend = SimplePhysicsBackend::new();
        let body = backend.create_body(&RigidbodyDesc::default()).unwrap();
        backend
            .add_collider(
                body,
                &ColliderDesc {
                    layer: 3,
                    ..ColliderDesc::default()
                },
            )
            .unwrap();

        assert_eq!(
            backend
                .overlap_sphere(Vec3::ZERO, 2.0, QueryFilter { mask: 1 << 3 },)
                .len(),
            1
        );
        assert!(backend
            .overlap_sphere(Vec3::ZERO, 2.0, QueryFilter { mask: 1 << 2 })
            .is_empty());
    }

    #[test]
    fn simple_backend_resolves_dynamic_body_out_of_static_contact() {
        let mut backend = SimplePhysicsBackend::new();
        let dynamic = backend
            .create_body(&RigidbodyDesc {
                transform: Transform {
                    translation: Vec3::new(0.0, 0.5, 0.0),
                    ..Transform::IDENTITY
                },
                gravity_scale: 0.0,
                ..RigidbodyDesc::default()
            })
            .unwrap();
        let ground = backend
            .create_body(&RigidbodyDesc {
                kind: BodyKind::Static,
                ..RigidbodyDesc::default()
            })
            .unwrap();
        backend
            .add_collider(
                dynamic,
                &ColliderDesc {
                    shape: ColliderShape::Sphere { radius: 0.5 },
                    ..ColliderDesc::default()
                },
            )
            .unwrap();
        backend
            .add_collider(
                ground,
                &ColliderDesc {
                    shape: ColliderShape::Sphere { radius: 0.5 },
                    ..ColliderDesc::default()
                },
            )
            .unwrap();

        backend.fixed_update(0.0);

        let transform = backend.body_transform(dynamic).unwrap();
        assert!(transform.translation.y >= 1.0);
    }

    #[cfg(feature = "rapier")]
    #[test]
    fn rapier_backend_raycast_hits_collider_with_layer_filter() {
        let mut backend = RapierPhysicsBackend::new();
        let body = backend
            .create_body(&RigidbodyDesc {
                transform: Transform {
                    translation: Vec3::new(0.0, 0.0, 5.0),
                    ..Transform::IDENTITY
                },
                kind: BodyKind::Static,
                ..RigidbodyDesc::default()
            })
            .unwrap();
        backend
            .add_collider(
                body,
                &ColliderDesc {
                    layer: 4,
                    ..ColliderDesc::default()
                },
            )
            .unwrap();

        assert!(backend
            .raycast(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
                10.0,
                QueryFilter { mask: 1 << 3 },
            )
            .is_none());

        let hit = backend
            .raycast(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
                10.0,
                QueryFilter { mask: 1 << 4 },
            )
            .unwrap();
        assert_eq!(hit.body, body);
        assert!(hit.distance > 4.0);
    }

    #[cfg(feature = "rapier")]
    #[test]
    fn rapier_backend_emits_trigger_events() {
        let mut backend = RapierPhysicsBackend::new();
        let first = backend.create_body(&RigidbodyDesc::default()).unwrap();
        let second = backend
            .create_body(&RigidbodyDesc {
                transform: Transform {
                    translation: Vec3::new(0.25, 0.0, 0.0),
                    ..Transform::IDENTITY
                },
                kind: BodyKind::Static,
                ..RigidbodyDesc::default()
            })
            .unwrap();
        backend
            .add_collider(
                first,
                &ColliderDesc {
                    is_trigger: true,
                    ..ColliderDesc::default()
                },
            )
            .unwrap();
        backend
            .add_collider(second, &ColliderDesc::default())
            .unwrap();

        backend.fixed_update(1.0 / 60.0);
        let contacts = backend.drain_contacts();

        assert!(contacts
            .iter()
            .any(|event| event.entered && event.is_trigger));
    }

    #[cfg(feature = "rapier")]
    #[test]
    fn rapier_backend_moves_kinematic_character_with_slide_result() {
        let mut backend = RapierPhysicsBackend::new();
        let character = backend
            .create_body(&RigidbodyDesc {
                kind: BodyKind::Kinematic,
                gravity_scale: 0.0,
                ..RigidbodyDesc::default()
            })
            .unwrap();
        let wall = backend
            .create_body(&RigidbodyDesc {
                transform: Transform {
                    translation: Vec3::new(0.0, 0.0, 1.0),
                    ..Transform::IDENTITY
                },
                kind: BodyKind::Static,
                ..RigidbodyDesc::default()
            })
            .unwrap();
        backend
            .add_collider(
                wall,
                &ColliderDesc {
                    shape: ColliderShape::Box {
                        half_extents: Vec3::new(0.5, 0.5, 0.5),
                    },
                    ..ColliderDesc::default()
                },
            )
            .unwrap();

        let movement = backend
            .move_character(
                character,
                CharacterControllerDesc {
                    shape: ColliderShapeRef::Sphere { radius: 0.25 },
                    translation: Vec3::new(0.0, 0.0, 2.0),
                    dt: 1.0 / 60.0,
                    filter: QueryFilter::default(),
                },
            )
            .unwrap();

        assert!(movement.translation.z < 2.0);
        assert!(movement.collisions > 0);
    }
}
