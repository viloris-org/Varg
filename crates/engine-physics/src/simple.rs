use std::collections::{HashMap, HashSet};

use engine_core::{EngineError, EngineResult};

use crate::collision::validate_collider_shape;
use crate::{
    BodyHandle, BodyKind, CcdMode, CharacterControllerDesc, CharacterControllerOutput,
    ColliderDesc, ColliderHandle, ColliderShape, ColliderShapeRef, ContactEvent, JointDesc,
    JointHandle, JointLimits, JointMotor, JointState, JointType, OverlapResult, PhysicsBackend,
    PhysicsStats, Quat, QueryFilter, RayHit, RayHitAll, RigidbodyDesc, SweepHit, Transform, Vec3,
};

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
    next_joint: u64,
    bodies: HashMap<BodyHandle, SimpleBody>,
    colliders: HashMap<ColliderHandle, SimpleCollider>,
    joints: HashMap<JointHandle, JointDesc>,
    active_pairs: HashSet<(ColliderHandle, ColliderHandle)>,
    contacts: Vec<ContactEvent>,
    gravity: Vec3,
    forces: HashMap<BodyHandle, Vec3>,
    sleep_timers: HashMap<BodyHandle, f32>,
    sleeping: HashSet<BodyHandle>,
}

impl Default for SimplePhysicsBackend {
    fn default() -> Self {
        Self {
            next_body: 1,
            next_collider: 1,
            next_joint: 1,
            bodies: HashMap::new(),
            colliders: HashMap::new(),
            joints: HashMap::new(),
            active_pairs: HashSet::new(),
            contacts: Vec::new(),
            gravity: Vec3::new(0.0, -9.81, 0.0),
            forces: HashMap::new(),
            sleep_timers: HashMap::new(),
            sleeping: HashSet::new(),
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
            collider_a: a,
            collider_b: b,
            point: center_b + normal * radius_b,
            normal,
            entered: true,
            is_trigger: collider_a.desc.is_trigger || collider_b.desc.is_trigger,
            contact_points: vec![center_b + normal * radius_b],
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
                    collider_a: pair.0,
                    collider_b: pair.1,
                    point: Vec3::ZERO,
                    normal: Vec3::ZERO,
                    entered: false,
                    is_trigger: left.desc.is_trigger || right.desc.is_trigger,
                    contact_points: Vec::new(),
                });
            }
        }
        let colliding_pairs = current_pairs.iter().copied().collect::<Vec<_>>();
        self.active_pairs = current_pairs;
        for (left, right) in colliding_pairs {
            let Some((normal, penetration)) = self.collision_penetration(left, right) else {
                continue;
            };
            let Some(left_collider) = self.colliders.get(&left) else {
                continue;
            };
            let Some(right_collider) = self.colliders.get(&right) else {
                continue;
            };
            if left_collider.desc.is_trigger || right_collider.desc.is_trigger {
                continue;
            }
            self.apply_separation(left_collider.body, right_collider.body, normal, penetration);
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

    fn update_sleep(&mut self, dt: f32) {
        let handles: Vec<_> = self.bodies.keys().copied().collect();
        for handle in handles {
            let Some(body) = self.bodies.get(&handle) else {
                continue;
            };
            if body.desc.kind != BodyKind::Dynamic {
                continue;
            }
            let params = body.desc.sleep_params.unwrap_or_default();
            let speed = body.velocity.length();
            if speed < params.linear_threshold {
                let timer = self.sleep_timers.entry(handle).or_insert(0.0);
                *timer += dt;
                if *timer >= params.time_before_sleep {
                    self.sleeping.insert(handle);
                }
            } else {
                self.sleep_timers.insert(handle, 0.0);
                self.sleeping.remove(&handle);
            }
        }
    }
}

impl PhysicsBackend for SimplePhysicsBackend {
    fn fixed_update(&mut self, dt: f32) {
        let forces = std::mem::take(&mut self.forces);
        for (handle, force) in forces {
            if let Some(body) = self.bodies.get_mut(&handle) {
                if body.desc.kind == BodyKind::Dynamic && !self.sleeping.contains(&handle) {
                    body.velocity += force * dt;
                }
            }
        }

        let has_ccd = self
            .bodies
            .values()
            .any(|b| b.desc.ccd == CcdMode::Enabled && b.desc.kind == BodyKind::Dynamic);
        let sub_steps: u32 = if has_ccd { 8 } else { 1 };
        let sub_dt = dt / sub_steps as f32;

        for _ in 0..sub_steps {
            for (handle, body) in self.bodies.iter_mut() {
                if body.desc.kind == BodyKind::Dynamic && !self.sleeping.contains(handle) {
                    body.velocity += self.gravity * body.desc.gravity_scale * sub_dt;
                    body.transform.translation += body.velocity * sub_dt;
                }
            }
            self.solve_joints(sub_dt);
        }
        self.update_contacts();
        self.update_sleep(dt);
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
        validate_collider_shape(&desc.shape)?;
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

    fn raycast_all(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> RayHitAll {
        let direction = direction.normalized();
        if direction == Vec3::ZERO {
            return RayHitAll { hits: Vec::new() };
        }
        let mut hits: Vec<RayHit> = self
            .colliders
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
            .collect();
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        RayHitAll { hits }
    }

    fn sweep_box(
        &self,
        center: Vec3,
        half_extents: Vec3,
        _rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<SweepHit> {
        let direction = direction.normalized();
        if direction == Vec3::ZERO {
            return None;
        }
        let radius = half_extents.length();
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
                .map(|distance| SweepHit {
                    body: collider.body,
                    collider: *handle,
                    point: center + direction * distance,
                    normal: (center + direction * distance - other_center).normalized(),
                    distance,
                })
            })
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    fn sweep_capsule(
        &self,
        center: Vec3,
        half_height: f32,
        radius: f32,
        _rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<SweepHit> {
        let direction = direction.normalized();
        if direction == Vec3::ZERO {
            return None;
        }
        let sweep_radius = half_height + radius;
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
                    sweep_radius + other_radius,
                )
                .map(|distance| SweepHit {
                    body: collider.body,
                    collider: *handle,
                    point: center + direction * distance,
                    normal: (center + direction * distance - other_center).normalized(),
                    distance,
                })
            })
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    fn overlap_box(
        &self,
        center: Vec3,
        half_extents: Vec3,
        _rotation: Quat,
        filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        let radius = half_extents.length();
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

    fn overlap_capsule(
        &self,
        center: Vec3,
        half_height: f32,
        radius: f32,
        _rotation: Quat,
        filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        let sweep_radius = half_height + radius;
        self.colliders
            .iter()
            .filter(|(_, collider)| filter_matches(collider.desc.layer, filter))
            .filter_map(|(handle, collider)| {
                let (other_center, other_radius) = self.collider_world_sphere(*handle)?;
                ((center - other_center).length_squared() <= (sweep_radius + other_radius).powi(2))
                    .then_some(OverlapResult {
                        body: collider.body,
                        collider: *handle,
                    })
            })
            .collect()
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

    fn apply_force(&mut self, body: BodyHandle, force: Vec3) -> EngineResult<()> {
        self.body(body)?;
        *self.forces.entry(body).or_insert(Vec3::ZERO) += force;
        self.sleeping.remove(&body);
        Ok(())
    }

    fn apply_torque(&mut self, body: BodyHandle, _torque: Vec3) -> EngineResult<()> {
        self.body(body)?;
        Ok(())
    }

    fn clear_forces(&mut self, body: BodyHandle) -> EngineResult<()> {
        self.body(body)?;
        self.forces.remove(&body);
        Ok(())
    }

    fn set_body_sleep(&mut self, body: BodyHandle, sleeping: bool) -> EngineResult<()> {
        self.body(body)?;
        if sleeping {
            self.sleeping.insert(body);
        } else {
            self.sleeping.remove(&body);
        }
        Ok(())
    }

    fn is_body_sleeping(&self, body: BodyHandle) -> EngineResult<bool> {
        self.body(body)?;
        Ok(self.sleeping.contains(&body))
    }

    fn create_joint(&mut self, desc: &JointDesc) -> EngineResult<JointHandle> {
        let handle = JointHandle(self.next_joint);
        self.next_joint = self.next_joint.saturating_add(1).max(1);
        self.joints.insert(handle, desc.clone());
        Ok(handle)
    }

    fn destroy_joint(&mut self, joint: JointHandle) -> EngineResult<()> {
        self.joints
            .remove(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        Ok(())
    }

    fn joint_state(&self, joint: JointHandle) -> EngineResult<JointState> {
        let desc = self
            .joints
            .get(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        Ok(JointState {
            handle: joint,
            desc: desc.clone(),
            positions: [0.0; 6],
            velocities: [0.0; 6],
        })
    }

    fn set_joint_motor(&mut self, joint: JointHandle, motor: JointMotor) -> EngineResult<()> {
        let desc = self
            .joints
            .get_mut(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        match &mut desc.joint_type {
            JointType::Hinge { motor: m, .. } | JointType::Slider { motor: m, .. } => {
                *m = Some(motor)
            }
            _ => {}
        }
        Ok(())
    }

    fn set_joint_limits(&mut self, joint: JointHandle, limits: JointLimits) -> EngineResult<()> {
        let desc = self
            .joints
            .get_mut(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        match &mut desc.joint_type {
            JointType::Hinge { limits: l, .. } | JointType::Slider { limits: l, .. } => *l = limits,
            _ => {}
        }
        Ok(())
    }

    fn joint_forces(&self, _joint: JointHandle) -> EngineResult<(f32, f32)> {
        Ok((0.0, 0.0))
    }

    fn stats(&self) -> PhysicsStats {
        PhysicsStats {
            body_count: self.bodies.len(),
            collider_count: self.colliders.len(),
            contact_count: self.active_pairs.len(),
            sleeping_count: self.sleeping.len(),
            joint_count: self.joints.len(),
            ..PhysicsStats::default()
        }
    }
}

impl SimplePhysicsBackend {
    fn solve_joints(&mut self, dt: f32) {
        let joints = self.joints.clone();
        for (_handle, desc) in &joints {
            match &desc.joint_type {
                JointType::Pin { anchor_a, anchor_b } => {
                    let transform_a = self.bodies.get(&desc.body_a).map(|b| b.transform);
                    let transform_b = self.bodies.get(&desc.body_b).map(|b| b.transform);
                    if let (Some(ta), Some(tb)) = (transform_a, transform_b) {
                        let world_a = ta.transform_point(*anchor_a);
                        let world_b = tb.transform_point(*anchor_b);
                        let correction = world_b - world_a;
                        if let Some(body) = self.bodies.get_mut(&desc.body_a) {
                            if body.desc.kind == BodyKind::Dynamic {
                                body.transform.translation += correction;
                            }
                        }
                    }
                }
                JointType::SpringArm {
                    anchor_a,
                    anchor_b,
                    rest_length,
                    stiffness,
                    damping,
                } => {
                    let transform_a = self.bodies.get(&desc.body_a).map(|b| b.transform);
                    let transform_b = self.bodies.get(&desc.body_b).map(|b| b.transform);
                    if let (Some(ta), Some(tb)) = (transform_a, transform_b) {
                        let world_a = ta.transform_point(*anchor_a);
                        let world_b = tb.transform_point(*anchor_b);
                        let delta = world_b - world_a;
                        let distance = delta.length();
                        if distance > f32::EPSILON {
                            let direction = delta / distance;
                            // Scale force and damping by dt for frame-rate independence
                            let dt = dt.max(0.0001);
                            let force = direction * ((distance - rest_length) * *stiffness * dt);
                            let damping_factor = (1.0 - (*damping).min(1.0)).powf(dt * 60.0);
                            if let Some(body) = self.bodies.get_mut(&desc.body_a) {
                                if body.desc.kind == BodyKind::Dynamic {
                                    body.velocity += force;
                                    body.velocity = body.velocity * damping_factor;
                                }
                            }
                            if let Some(body) = self.bodies.get_mut(&desc.body_b) {
                                if body.desc.kind == BodyKind::Dynamic {
                                    body.velocity -= force;
                                    body.velocity = body.velocity * damping_factor;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
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
        ColliderShape::Mesh { vertices } | ColliderShape::TriMesh { vertices, .. } => vertices
            .chunks_exact(3)
            .map(|chunk| Vec3::new(chunk[0], chunk[1], chunk[2]).length())
            .fold(0.0, f32::max),
        ColliderShape::Heightfield {
            num_x,
            num_z,
            heights,
            scale,
        } => {
            let max_height = heights.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
            let half_x = (*num_x as f32 - 1.0) * scale.x * 0.5;
            let half_z = (*num_z as f32 - 1.0) * scale.z * 0.5;
            Vec3::new(half_x, max_height * scale.y, half_z).length()
        }
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
