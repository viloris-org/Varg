use std::collections::HashMap;

use rapier3d::{
    control as rpc,
    crossbeam::channel::{Receiver, unbounded},
    na::{self, Point3, Quaternion, Translation3, UnitQuaternion, Vector3},
    parry::query::ShapeCastOptions,
    prelude as rp,
};

use engine_core::{EngineError, EngineResult};

use crate::collision::validate_collider_shape;
use crate::{
    BodyHandle, BodyKind, CcdMode, CharacterControllerDesc, CharacterControllerOutput,
    ColliderDesc, ColliderHandle, ColliderShape, ColliderShapeRef, CombineMode, ContactEvent,
    JointDesc, JointHandle, JointLimits, JointMotor, JointState, JointType, OverlapResult,
    PhysicsBackend, PhysicsStats, Quat, QueryFilter, RayHit, RayHitAll, RigidbodyDesc, SweepHit,
    Transform, Vec3, VehicleDesc, VehicleHandle, VehicleInput, VehicleState,
};

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
    next_joint: u64,
    joint_handles: HashMap<JointHandle, rp::ImpulseJointHandle>,
    rapier_joints: HashMap<rp::ImpulseJointHandle, JointHandle>,
    joint_types: HashMap<JointHandle, crate::joints::JointType>,
    next_vehicle: u64,
    vehicles: HashMap<VehicleHandle, rpc::DynamicRayCastVehicleController>,
    vehicle_handles: HashMap<VehicleHandle, rp::RigidBodyHandle>,
    rapier_vehicles: HashMap<rp::RigidBodyHandle, VehicleHandle>,
    stats: PhysicsStats,
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
            next_joint: 1,
            joint_handles: HashMap::new(),
            rapier_joints: HashMap::new(),
            joint_types: HashMap::new(),
            next_vehicle: 1,
            vehicles: HashMap::new(),
            vehicle_handles: HashMap::new(),
            rapier_vehicles: HashMap::new(),
            stats: PhysicsStats::default(),
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
            let collider_a = self
                .rapier_colliders
                .get(&event.collider1())
                .copied()
                .unwrap_or(ColliderHandle(0));
            let collider_b = self
                .rapier_colliders
                .get(&event.collider2())
                .copied()
                .unwrap_or(ColliderHandle(0));
            self.pending_contacts.push(ContactEvent {
                body_a,
                body_b,
                collider_a,
                collider_b,
                point: Vec3::ZERO,
                normal: Vec3::ZERO,
                entered: event.started(),
                is_trigger: event.sensor(),
                contact_points: Vec::new(),
            });
        }
        while let Ok(event) = self.contact_force_events.try_recv() {
            let Some(body_a) = self.collider_owner(event.collider1) else {
                continue;
            };
            let Some(body_b) = self.collider_owner(event.collider2) else {
                continue;
            };
            let collider_a = self
                .rapier_colliders
                .get(&event.collider1)
                .copied()
                .unwrap_or(ColliderHandle(0));
            let collider_b = self
                .rapier_colliders
                .get(&event.collider2)
                .copied()
                .unwrap_or(ColliderHandle(0));
            let normal = if event.total_force_magnitude > f32::EPSILON {
                vec3(event.total_force).normalized()
            } else {
                Vec3::ZERO
            };
            self.pending_contacts.push(ContactEvent {
                body_a,
                body_b,
                collider_a,
                collider_b,
                point: Vec3::ZERO,
                normal,
                entered: true,
                is_trigger: false,
                contact_points: Vec::new(),
            });
        }
    }
}

impl PhysicsBackend for RapierPhysicsBackend {
    fn fixed_update(&mut self, dt: f32) {
        let start = std::time::Instant::now();
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

        self.stats = PhysicsStats {
            step_us: start.elapsed().as_micros() as u64,
            body_count: self.body_handles.len(),
            collider_count: self.collider_handles.len(),
            contact_count: self.narrow_phase.contact_pairs().count(),
            island_count: 0,
            sleeping_count: self
                .bodies
                .iter()
                .filter(|(_, body)| body.is_sleeping())
                .count(),
            joint_count: self.joint_handles.len(),
            vehicle_count: self.vehicles.len(),
        };
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
        .gravity_scale(desc.gravity_scale)
        .ccd_enabled(desc.ccd == CcdMode::Enabled);
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
        validate_collider_shape(&desc.shape)?;
        let mut active_events = rp::ActiveEvents::COLLISION_EVENTS;
        if desc.active_contact_events {
            active_events |= rp::ActiveEvents::CONTACT_FORCE_EVENTS;
        }
        let builder = collider_builder(desc)
            .friction(desc.friction)
            .restitution(desc.restitution)
            .friction_combine_rule(combine_mode(desc.friction_combine))
            .restitution_combine_rule(combine_mode(desc.restitution_combine))
            .sensor(desc.is_trigger)
            .collision_groups(interaction_groups(desc.layer, desc.mask))
            .solver_groups(interaction_groups(desc.layer, desc.mask))
            .active_events(active_events);
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

    fn set_body_transform(&mut self, body: BodyHandle, transform: Transform) -> EngineResult<()> {
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

    fn overlap_sphere(&self, center: Vec3, radius: f32, filter: QueryFilter) -> Vec<OverlapResult> {
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
        let ray = rp::Ray::new(point3(origin), vector3(direction));
        let mut hits = Vec::new();
        self.query_pipeline.intersections_with_ray(
            &self.bodies,
            &self.colliders,
            &ray,
            max_distance,
            true,
            query_filter(filter),
            |rapier_collider, hit| {
                if let (Some(collider), Some(body)) = (
                    self.rapier_colliders.get(&rapier_collider).copied(),
                    self.collider_owner(rapier_collider),
                ) {
                    hits.push(RayHit {
                        body,
                        collider,
                        point: origin + direction * hit.time_of_impact,
                        normal: vec3(hit.normal),
                        distance: hit.time_of_impact,
                    });
                }
                true
            },
        );
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        RayHitAll { hits }
    }

    fn sweep_box(
        &self,
        center: Vec3,
        half_extents: Vec3,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<SweepHit> {
        let direction = direction.normalized();
        if direction == Vec3::ZERO {
            return None;
        }
        let shape = rp::SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z);
        let pos = isometry(Transform {
            translation: center,
            rotation,
            scale: Vec3::ONE,
        });
        self.query_pipeline
            .cast_shape(
                &self.bodies,
                &self.colliders,
                &pos,
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
                Some(SweepHit {
                    body,
                    collider,
                    point: center + direction * hit.time_of_impact,
                    normal: vec3(*hit.normal1),
                    distance: hit.time_of_impact,
                })
            })
    }

    fn sweep_capsule(
        &self,
        center: Vec3,
        half_height: f32,
        radius: f32,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<SweepHit> {
        let direction = direction.normalized();
        if direction == Vec3::ZERO {
            return None;
        }
        let shape = rp::SharedShape::capsule_y(half_height, radius);
        let pos = isometry(Transform {
            translation: center,
            rotation,
            scale: Vec3::ONE,
        });
        self.query_pipeline
            .cast_shape(
                &self.bodies,
                &self.colliders,
                &pos,
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
                Some(SweepHit {
                    body,
                    collider,
                    point: center + direction * hit.time_of_impact,
                    normal: vec3(*hit.normal1),
                    distance: hit.time_of_impact,
                })
            })
    }

    fn overlap_box(
        &self,
        center: Vec3,
        half_extents: Vec3,
        rotation: Quat,
        filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        let shape = rp::SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z);
        let pos = isometry(Transform {
            translation: center,
            rotation,
            scale: Vec3::ONE,
        });
        let mut results = Vec::new();
        self.query_pipeline.intersections_with_shape(
            &self.bodies,
            &self.colliders,
            &pos,
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

    fn overlap_capsule(
        &self,
        center: Vec3,
        half_height: f32,
        radius: f32,
        rotation: Quat,
        filter: QueryFilter,
    ) -> Vec<OverlapResult> {
        let shape = rp::SharedShape::capsule_y(half_height, radius);
        let pos = isometry(Transform {
            translation: center,
            rotation,
            scale: Vec3::ONE,
        });
        let mut results = Vec::new();
        self.query_pipeline.intersections_with_shape(
            &self.bodies,
            &self.colliders,
            &pos,
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

    fn apply_force(&mut self, body: BodyHandle, force: Vec3) -> EngineResult<()> {
        let rapier = self.rapier_body(body)?;
        let body = self
            .bodies
            .get_mut(rapier)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
        body.add_force(vector3(force), true);
        body.wake_up(true);
        Ok(())
    }

    fn apply_torque(&mut self, body: BodyHandle, torque: Vec3) -> EngineResult<()> {
        let rapier = self.rapier_body(body)?;
        let body = self
            .bodies
            .get_mut(rapier)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
        body.add_torque(vector3(torque), true);
        body.wake_up(true);
        Ok(())
    }

    fn clear_forces(&mut self, body: BodyHandle) -> EngineResult<()> {
        let rapier = self.rapier_body(body)?;
        let body = self
            .bodies
            .get_mut(rapier)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
        body.reset_forces(true);
        body.reset_torques(true);
        Ok(())
    }

    fn set_body_sleep(&mut self, body: BodyHandle, sleeping: bool) -> EngineResult<()> {
        let rapier = self.rapier_body(body)?;
        let body = self
            .bodies
            .get_mut(rapier)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
        if sleeping {
            body.sleep();
        } else {
            body.wake_up(true);
        }
        Ok(())
    }

    fn is_body_sleeping(&self, body: BodyHandle) -> EngineResult<bool> {
        let rapier = self.rapier_body(body)?;
        let body = self
            .bodies
            .get(rapier)
            .ok_or_else(|| EngineError::invalid_handle("physics body does not exist"))?;
        Ok(body.is_sleeping())
    }

    fn create_joint(&mut self, desc: &JointDesc) -> EngineResult<JointHandle> {
        let rapier_a = self.rapier_body(desc.body_a)?;
        let rapier_b = self.rapier_body(desc.body_b)?;
        let local_a = isometry(Transform {
            translation: match &desc.joint_type {
                JointType::Pin { anchor_a, .. }
                | JointType::Hinge { anchor_a, .. }
                | JointType::Slider { anchor_a, .. }
                | JointType::SpringArm { anchor_a, .. }
                | JointType::ConeTwist { anchor_a, .. }
                | JointType::Generic6DOF { anchor_a, .. } => *anchor_a,
            },
            ..Transform::IDENTITY
        });
        let local_b = isometry(Transform {
            translation: match &desc.joint_type {
                JointType::Pin { anchor_b, .. }
                | JointType::Hinge { anchor_b, .. }
                | JointType::Slider { anchor_b, .. }
                | JointType::SpringArm { anchor_b, .. }
                | JointType::ConeTwist { anchor_b, .. }
                | JointType::Generic6DOF { anchor_b, .. } => *anchor_b,
            },
            ..Transform::IDENTITY
        });
        let mut joint = match &desc.joint_type {
            JointType::Pin { .. } => {
                let j = rp::FixedJoint::new();
                rp::GenericJoint::from(j)
            }
            JointType::Hinge {
                axis_a,
                limits,
                motor,
                ..
            } => {
                let axis = na::Unit::new_normalize(vector3(*axis_a));
                let mut j = rp::RevoluteJoint::new(axis);
                j.set_limits([limits.min, limits.max]);
                if let Some(m) = motor {
                    if m.enabled {
                        j.set_motor_velocity(m.target_velocity, m.max_force);
                    }
                }
                rp::GenericJoint::from(j)
            }
            JointType::Slider {
                axis_a,
                limits,
                motor,
                ..
            } => {
                let axis = na::Unit::new_normalize(vector3(*axis_a));
                let mut j = rp::PrismaticJoint::new(axis);
                j.set_limits([limits.min, limits.max]);
                if let Some(m) = motor {
                    if m.enabled {
                        j.set_motor_velocity(m.target_velocity, m.max_force);
                    }
                }
                rp::GenericJoint::from(j)
            }
            JointType::SpringArm {
                rest_length,
                stiffness,
                damping,
                ..
            } => {
                let mut j = rp::GenericJoint::default();
                j.set_motor(rp::JointAxis::LinX, 0.0, *rest_length, *stiffness, *damping);
                j
            }
            JointType::ConeTwist {
                swing_limits,
                twist_limits,
                ..
            } => {
                let mut j = rp::GenericJoint::default();
                for axis in [
                    rp::JointAxis::LinX,
                    rp::JointAxis::LinY,
                    rp::JointAxis::LinZ,
                ] {
                    j.set_limits(axis, [0.0, 0.0]);
                }
                j.set_limits(rp::JointAxis::AngX, [twist_limits.min, twist_limits.max]);
                j.set_limits(rp::JointAxis::AngY, [-swing_limits.max, swing_limits.max]);
                j.set_limits(rp::JointAxis::AngZ, [-swing_limits.max, swing_limits.max]);
                j
            }
            JointType::Generic6DOF {
                linear_limits,
                angular_limits,
                motors,
                ..
            } => {
                let mut j = rp::GenericJoint::default();
                let axes = [
                    rp::JointAxis::LinX,
                    rp::JointAxis::LinY,
                    rp::JointAxis::LinZ,
                    rp::JointAxis::AngX,
                    rp::JointAxis::AngY,
                    rp::JointAxis::AngZ,
                ];
                for (i, axis) in axes.iter().enumerate() {
                    let limit = if i < 3 {
                        &linear_limits[i]
                    } else {
                        &angular_limits[i - 3]
                    };
                    j.set_limits(*axis, [limit.min, limit.max]);
                    if let Some(m) = motors.get(i).and_then(|m| m.as_ref()) {
                        if m.enabled {
                            j.set_motor(*axis, m.target_velocity, 0.0, m.max_force, 0.0);
                        }
                    }
                }
                j
            }
        };
        joint.local_frame1 = local_a;
        joint.local_frame2 = local_b;
        let rapier = self.impulse_joints.insert(rapier_a, rapier_b, joint, true);
        let handle = JointHandle(self.next_joint);
        self.next_joint = self.next_joint.saturating_add(1).max(1);
        self.joint_handles.insert(handle, rapier);
        self.rapier_joints.insert(rapier, handle);
        self.joint_types.insert(handle, desc.joint_type.clone());
        Ok(handle)
    }

    fn destroy_joint(&mut self, joint: JointHandle) -> EngineResult<()> {
        let rapier = self
            .joint_handles
            .remove(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        self.impulse_joints.remove(rapier, true);
        self.rapier_joints.remove(&rapier);
        self.joint_types.remove(&joint);
        Ok(())
    }

    fn joint_state(&self, joint: JointHandle) -> EngineResult<JointState> {
        let rapier = self
            .joint_handles
            .get(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let j = self
            .impulse_joints
            .get(*rapier)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let joint_type = self
            .joint_types
            .get(&joint)
            .cloned()
            .ok_or_else(|| EngineError::invalid_handle("joint type not found"))?;
        let body_a = self
            .rapier_bodies
            .get(&j.body1)
            .copied()
            .unwrap_or(BodyHandle(0));
        let body_b = self
            .rapier_bodies
            .get(&j.body2)
            .copied()
            .unwrap_or(BodyHandle(0));

        let positions = compute_joint_positions(&self.bodies, j.body1, j.body2, &joint_type);
        let velocities = [0.0_f32; 6];
        Ok(JointState {
            handle: joint,
            desc: JointDesc {
                joint_type,
                body_a,
                body_b,
                break_force: 0.0,
                break_torque: 0.0,
            },
            positions,
            velocities,
        })
    }

    fn set_joint_motor(&mut self, joint: JointHandle, motor: JointMotor) -> EngineResult<()> {
        let rapier = self
            .joint_handles
            .get(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let j = self
            .impulse_joints
            .get_mut(*rapier)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let joint_type = self
            .joint_types
            .get(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint type not found"))?;
        if motor.enabled {
            let axis = motor_axis_for_joint(joint_type);
            j.data
                .set_motor(axis, motor.target_velocity, 0.0, motor.max_force, 0.0);
        }
        Ok(())
    }

    fn set_joint_limits(&mut self, joint: JointHandle, limits: JointLimits) -> EngineResult<()> {
        let rapier = self
            .joint_handles
            .get(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let j = self
            .impulse_joints
            .get_mut(*rapier)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let joint_type = self
            .joint_types
            .get(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint type not found"))?;
        let axis = motor_axis_for_joint(joint_type);
        j.data.set_limits(axis, [limits.min, limits.max]);
        Ok(())
    }

    fn joint_forces(&self, joint: JointHandle) -> EngineResult<(f32, f32)> {
        let rapier = self
            .joint_handles
            .get(&joint)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let j = self
            .impulse_joints
            .get(*rapier)
            .ok_or_else(|| EngineError::invalid_handle("joint does not exist"))?;
        let linear = j.impulses.fixed_rows::<3>(0).norm();
        let angular = j.impulses.fixed_rows::<3>(3).norm();
        Ok((linear, angular))
    }

    fn create_vehicle(&mut self, desc: &VehicleDesc) -> EngineResult<VehicleHandle> {
        let rapier_chassis = self.rapier_body(desc.chassis)?;
        let mut controller = rpc::DynamicRayCastVehicleController::new(rapier_chassis);
        for wheel in &desc.wheels {
            let connection = point3(wheel.chassis_connection);
            let direction = vector3(Vec3::new(0.0, -1.0, 0.0));
            let axle = vector3(Vec3::new(-1.0, 0.0, 0.0));
            let tuning = rpc::WheelTuning {
                suspension_stiffness: wheel.suspension_stiffness,
                suspension_compression: wheel.suspension_damping,
                suspension_damping: wheel.suspension_damping * 0.5,
                max_suspension_travel: wheel.suspension_travel,
                side_friction_stiffness: wheel.lateral_friction_stiffness,
                friction_slip: wheel.longitudinal_friction_stiffness,
                max_suspension_force: wheel.max_suspension_force,
            };
            controller.add_wheel(
                connection,
                direction,
                axle,
                wheel.suspension_rest,
                wheel.radius,
                &tuning,
            );
        }
        let handle = VehicleHandle(self.next_vehicle);
        self.next_vehicle = self.next_vehicle.saturating_add(1).max(1);
        self.vehicle_handles.insert(handle, rapier_chassis);
        self.rapier_vehicles.insert(rapier_chassis, handle);
        self.vehicles.insert(handle, controller);
        Ok(handle)
    }

    fn destroy_vehicle(&mut self, vehicle: VehicleHandle) -> EngineResult<()> {
        let rapier_chassis = self
            .vehicle_handles
            .remove(&vehicle)
            .ok_or_else(|| EngineError::invalid_handle("vehicle does not exist"))?;
        self.rapier_vehicles.remove(&rapier_chassis);
        self.vehicles
            .remove(&vehicle)
            .ok_or_else(|| EngineError::invalid_handle("vehicle does not exist"))?;
        Ok(())
    }

    fn update_vehicle(
        &mut self,
        vehicle: VehicleHandle,
        input: VehicleInput,
    ) -> EngineResult<VehicleState> {
        let controller = self
            .vehicles
            .get_mut(&vehicle)
            .ok_or_else(|| EngineError::invalid_handle("vehicle does not exist"))?;

        let wheel_count = controller.wheels().len();
        for i in 0..wheel_count {
            let wheel = &mut controller.wheels_mut()[i];
            wheel.engine_force = input.throttle * 500.0;
            wheel.brake = input.brake * 100.0;
            wheel.steering = input.steering * 0.6;
        }
        if input.handbrake {
            for i in 0..wheel_count {
                controller.wheels_mut()[i].brake = 200.0;
            }
        }

        controller.update_vehicle(
            self.integration.dt.max(1.0 / 240.0),
            &mut self.bodies,
            &self.colliders,
            &self.query_pipeline,
            rp::QueryFilter::default(),
        );

        let speed = controller.current_vehicle_speed;
        let gear = if speed.abs() < 0.01 {
            0
        } else if speed > 0.0 {
            1
        } else {
            -1
        };
        let mut wheel_transforms = Vec::new();
        let mut suspension_displacements = Vec::new();
        let mut grounded = false;
        for wheel in controller.wheels() {
            let center = vec3(wheel.center().coords);
            wheel_transforms.push(Transform {
                translation: center,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            });
            suspension_displacements.push(wheel.raycast_info().suspension_length);
            if wheel.raycast_info().is_in_contact {
                grounded = true;
            }
        }
        Ok(VehicleState {
            speed,
            gear,
            wheel_transforms,
            suspension_displacements,
            grounded,
        })
    }

    fn stats(&self) -> PhysicsStats {
        self.stats
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
        ColliderShape::TriMesh { vertices, indices } => {
            let points: Vec<Point3<rp::Real>> = vertices
                .chunks_exact(3)
                .map(|chunk| Point3::new(chunk[0], chunk[1], chunk[2]))
                .collect();
            let triangles: Vec<[u32; 3]> = indices
                .chunks_exact(3)
                .map(|chunk| [chunk[0], chunk[1], chunk[2]])
                .collect();
            rp::ColliderBuilder::trimesh(points, triangles)
        }
        ColliderShape::Heightfield {
            num_x,
            num_z,
            heights,
            scale,
        } => {
            let nrows = *num_x as usize;
            let ncols = *num_z as usize;
            let heights_matrix =
                na::DMatrix::from_fn(nrows, ncols, |r, c| heights[r * ncols + c] * scale.y);
            let scale_vec = rp::Vector::new(scale.x, 1.0, scale.z);
            rp::ColliderBuilder::heightfield(heights_matrix, scale_vec)
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

fn combine_mode(mode: CombineMode) -> rp::CoefficientCombineRule {
    match mode {
        CombineMode::Average => rp::CoefficientCombineRule::Average,
        CombineMode::Min => rp::CoefficientCombineRule::Min,
        CombineMode::Multiply => rp::CoefficientCombineRule::Multiply,
        CombineMode::Max => rp::CoefficientCombineRule::Max,
    }
}

fn compute_joint_positions(
    bodies: &rp::RigidBodySet,
    body1: rp::RigidBodyHandle,
    body2: rp::RigidBodyHandle,
    joint_type: &crate::joints::JointType,
) -> [f32; 6] {
    use crate::joints::JointType;
    let zero = [0.0_f32; 6];
    let (Some(b1), Some(b2)) = (bodies.get(body1), bodies.get(body2)) else {
        return zero;
    };
    let iso1 = b1.position();
    let iso2 = b2.position();
    match joint_type {
        JointType::Hinge {
            anchor_a,
            anchor_b,
            axis_a,
            ..
        } => {
            let world_a = iso1 * point3(*anchor_a);
            let world_b = iso2 * point3(*anchor_b);
            let world_axis = (iso1 * vector3(*axis_a)).normalize();
            let da = (iso1.translation.vector - world_a.coords).cross(&world_axis);
            let db = (iso2.translation.vector - world_b.coords).cross(&world_axis);
            let angle = db.norm().atan2(da.norm()).copysign(da.dot(&world_axis));
            let mut pos = zero;
            pos[3] = angle;
            pos
        }
        JointType::Slider {
            anchor_a,
            anchor_b,
            axis_a,
            ..
        } => {
            let world_a = iso1 * point3(*anchor_a);
            let world_b = iso2 * point3(*anchor_b);
            let delta = world_b - world_a;
            let world_axis = (iso1 * vector3(*axis_a)).normalize();
            let mut pos = zero;
            pos[0] = delta.dot(&world_axis);
            pos
        }
        _ => zero,
    }
}

fn motor_axis_for_joint(joint_type: &crate::joints::JointType) -> rp::JointAxis {
    use crate::joints::JointType;
    match joint_type {
        JointType::Hinge { .. } => rp::JointAxis::AngX,
        JointType::Slider { .. } => rp::JointAxis::LinX,
        JointType::Generic6DOF { .. } => rp::JointAxis::AngX,
        _ => rp::JointAxis::AngX,
    }
}
