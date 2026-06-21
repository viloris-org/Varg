//! Physics synchronization between ECS and the physics backend.

use std::collections::{HashMap, HashSet};

use engine_core::EntityId;
use engine_ecs::{ColliderComponentData, ComponentData, Scene};
use engine_physics::{
    BodyHandle, BodyKind, ColliderDesc, ColliderHandle, ColliderShape, PhysicsWorld, RigidbodyDesc,
    Vec3, built_in_physical_material,
};

/// Distance-based activation settings for large scenes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsActivationConfig {
    /// Center of the active physics area, typically the player or camera.
    pub center: Vec3,
    /// Entities within this radius are bound into the physics backend.
    pub active_radius: f32,
    /// Already-bound entities beyond this radius are removed from the backend.
    pub release_radius: f32,
    /// Dynamic entities beyond this radius are put to sleep while still bound.
    pub sleep_radius: f32,
}

impl PhysicsActivationConfig {
    /// Creates activation settings with a release hysteresis band.
    pub fn new(center: Vec3, active_radius: f32) -> Self {
        let active_radius = active_radius.max(0.0);
        Self {
            center,
            active_radius,
            release_radius: active_radius * 1.25,
            sleep_radius: active_radius,
        }
    }

    fn should_activate(&self, position: Vec3) -> bool {
        distance_squared(position, self.center) <= self.active_radius * self.active_radius
    }

    fn should_release(&self, position: Vec3) -> bool {
        distance_squared(position, self.center) > self.release_radius * self.release_radius
    }

    fn should_sleep(&self, position: Vec3) -> bool {
        distance_squared(position, self.center) > self.sleep_radius * self.sleep_radius
    }
}

/// Synchronizes ECS RigidbodyComponent/ColliderComponent with the physics backend.
///
/// PhysicsSync watches the Scene for entities with rigidbody and collider components,
/// creates corresponding physics bodies and colliders, and syncs transforms each fixed update.
pub struct PhysicsSync {
    /// Mapping from entity ID to (physics body handle, body kind).
    body_map: HashMap<EntityId, (BodyHandle, BodyKind)>,
    /// Mapping from (entity ID, collider index) to collider handle.
    collider_map: HashMap<(EntityId, usize), ColliderHandle>,
    /// Optional distance-based activation for large scenes.
    activation: Option<PhysicsActivationConfig>,
}

impl Default for PhysicsSync {
    fn default() -> Self {
        Self::new()
    }
}

impl PhysicsSync {
    /// Creates a new PhysicsSync.
    pub fn new() -> Self {
        Self {
            body_map: HashMap::new(),
            collider_map: HashMap::new(),
            activation: None,
        }
    }

    /// Sets distance-based activation settings.
    pub fn set_activation(&mut self, activation: Option<PhysicsActivationConfig>) {
        self.activation = activation;
    }

    /// Returns the current activation settings.
    pub fn activation(&self) -> Option<PhysicsActivationConfig> {
        self.activation
    }

    /// Updates the activation center without changing configured radii.
    pub fn set_activation_center(&mut self, center: Vec3) {
        if let Some(activation) = &mut self.activation {
            activation.center = center;
        }
    }

    /// Synchronizes creation: creates physics bodies for entities with RigidbodyComponent
    /// that don't yet have a body handle, creates colliders for ColliderComponents,
    /// and applies configured distance-based activation.
    pub fn sync_creation(
        &mut self,
        scene: &Scene,
        physics: &mut PhysicsWorld,
    ) -> engine_core::EngineResult<()> {
        for (entity, object) in scene.iter_objects() {
            // Find rigidbody component
            let rigidbody = object.components.iter().find_map(|c| {
                if let ComponentData::Rigidbody(rb) = c {
                    Some(rb)
                } else {
                    None
                }
            });

            let Some(rb_data) = rigidbody else {
                continue;
            };

            if let Some(activation) = self.activation {
                let transform = scene.transforms().local(entity).unwrap_or_default();
                if !self.body_map.contains_key(&object.id)
                    && !activation.should_activate(transform.translation)
                {
                    continue;
                }
            }

            let body = match self.body_map.get(&object.id).copied() {
                Some((handle, _)) => handle,
                None => {
                    let local_transform = scene.transforms().local(entity).unwrap_or_default();
                    let body_kind = match rb_data.body_type.as_str() {
                        "static" => BodyKind::Static,
                        "kinematic" => BodyKind::Kinematic,
                        _ => BodyKind::Dynamic,
                    };
                    let desc = RigidbodyDesc {
                        transform: local_transform,
                        kind: body_kind,
                        linear_damping: rb_data.linear_damping,
                        angular_damping: rb_data.angular_damping,
                        gravity_scale: if rb_data.use_gravity { 1.0 } else { 0.0 },
                        ..RigidbodyDesc::default()
                    };
                    let new_body = physics.backend_mut().create_body(&desc)?;
                    self.body_map.insert(object.id, (new_body, body_kind));
                    new_body
                }
            };

            let collider_components = object.components.iter().filter_map(|c| {
                if let ComponentData::Collider(col) = c {
                    Some(col)
                } else {
                    None
                }
            });

            for (idx, collider_data) in collider_components.enumerate() {
                if !self.collider_map.contains_key(&(object.id, idx)) {
                    let material = built_in_physical_material(&collider_data.physics_material);
                    let desc = ColliderDesc {
                        shape: collider_shape_from_data(collider_data),
                        friction: material.friction,
                        restitution: material.restitution,
                        is_trigger: collider_data.is_trigger,
                        layer: object.layer,
                        mask: collider_data.mask,
                        friction_combine: material.friction_combine,
                        restitution_combine: material.restitution_combine,
                        active_contact_events: false,
                    };
                    let collider_handle = physics.backend_mut().add_collider(body, &desc)?;
                    self.collider_map.insert((object.id, idx), collider_handle);
                }
            }
        }

        if self.activation.is_some() {
            let _ = self.sync_activation(scene, physics)?;
        }

        Ok(())
    }

    /// Applies distance-based activation and returns entity IDs that were released.
    pub fn sync_activation(
        &mut self,
        scene: &Scene,
        physics: &mut PhysicsWorld,
    ) -> engine_core::EngineResult<Vec<EntityId>> {
        let Some(activation) = self.activation else {
            return Ok(Vec::new());
        };

        let bodies_to_release = self
            .body_map
            .iter()
            .filter_map(|(eid, (handle, _))| {
                let entity = scene.find_by_id(*eid)?;
                let transform = scene.transforms().local(entity).unwrap_or_default();
                activation
                    .should_release(transform.translation)
                    .then_some((*eid, *handle))
            })
            .collect::<Vec<_>>();

        let mut released = Vec::new();
        for (eid, handle) in bodies_to_release {
            self.remove_binding(eid, handle, physics);
            released.push(eid);
        }

        for (eid, (body_handle, body_kind)) in &self.body_map {
            if *body_kind != BodyKind::Dynamic {
                continue;
            }
            let Some(entity) = scene.find_by_id(*eid) else {
                continue;
            };
            let transform = scene.transforms().local(entity).unwrap_or_default();
            physics
                .backend_mut()
                .set_body_sleep(*body_handle, activation.should_sleep(transform.translation))?;
        }

        Ok(released)
    }

    /// Synchronizes destruction: removes physics bodies for entities that have been destroyed.
    /// Returns entity IDs that were destroyed.
    pub fn sync_destruction(
        &mut self,
        scene: &Scene,
        physics: &mut PhysicsWorld,
    ) -> engine_core::EngineResult<Vec<EntityId>> {
        // Collect entity IDs that exist in the scene
        let active_entities: HashSet<_> = scene.iter_objects().map(|(_, obj)| obj.id).collect();

        let mut destroyed = Vec::new();

        // Find bodies whose entities no longer exist
        let bodies_to_remove: Vec<_> = self
            .body_map
            .iter()
            .filter(|(eid, _)| !active_entities.contains(eid))
            .map(|(eid, (handle, _))| (*eid, *handle))
            .collect();

        for (eid, handle) in bodies_to_remove {
            self.remove_binding(eid, handle, physics);
            destroyed.push(eid);
        }

        Ok(destroyed)
    }

    /// Synchronizes transforms from ECS to physics (scene → physics).
    pub fn sync_transforms_to_physics(
        &self,
        scene: &Scene,
        physics: &mut PhysicsWorld,
    ) -> engine_core::EngineResult<()> {
        for (eid, (body_handle, _)) in &self.body_map {
            if let Some(entity) = scene.find_by_id(*eid) {
                if let Some(local_transform) = scene.transforms().local(entity) {
                    physics
                        .backend_mut()
                        .set_body_transform(*body_handle, local_transform)?;
                }
            }
        }
        Ok(())
    }

    /// Synchronizes transforms from physics to ECS (physics → scene).
    /// Only syncs dynamic and kinematic bodies — static bodies don't move.
    pub fn sync_transforms_from_physics(
        &self,
        scene: &mut Scene,
        physics: &mut PhysicsWorld,
    ) -> engine_core::EngineResult<()> {
        for (eid, (body_handle, body_kind)) in &self.body_map {
            if *body_kind == BodyKind::Static {
                continue;
            }
            if let Some(entity) = scene.find_by_id(*eid) {
                if let Ok(transform) = physics.backend().body_transform(*body_handle) {
                    scene.transforms_mut().set_local(entity, transform);
                }
            }
        }
        Ok(())
    }

    /// Clears all physics bindings.
    pub fn clear(&mut self) {
        self.body_map.clear();
        self.collider_map.clear();
    }

    /// Returns the number of physics bodies managed by this sync.
    pub fn body_count(&self) -> usize {
        self.body_map.len()
    }

    /// Returns the number of physics colliders managed by this sync.
    pub fn collider_count(&self) -> usize {
        self.collider_map.len()
    }

    fn remove_binding(
        &mut self,
        entity_id: EntityId,
        body_handle: BodyHandle,
        physics: &mut PhysicsWorld,
    ) {
        let colliders_to_remove: Vec<_> = self
            .collider_map
            .iter()
            .filter(|((mapped_entity_id, _), _)| *mapped_entity_id == entity_id)
            .map(|(_, collider)| *collider)
            .collect();

        for collider_handle in colliders_to_remove {
            let _ = physics.backend_mut().remove_collider(collider_handle);
        }
        self.collider_map
            .retain(|(mapped_entity_id, _), _| *mapped_entity_id != entity_id);

        let _ = physics.backend_mut().destroy_body(body_handle);
        self.body_map.remove(&entity_id);
    }
}

/// Helper to convert ColliderComponentData to ColliderShape.
fn collider_shape_from_data(data: &ColliderComponentData) -> ColliderShape {
    let half = data.size * 0.5;
    match data.shape.as_str() {
        "sphere" => ColliderShape::Sphere {
            radius: half.x.max(half.y).max(half.z),
        },
        "capsule" => ColliderShape::Capsule {
            half_height: half.y,
            radius: half.x.max(half.z),
        },
        _ => ColliderShape::Box { half_extents: half },
    }
}

fn distance_squared(left: Vec3, right: Vec3) -> f32 {
    let delta = left - right;
    delta.length_squared()
}

#[cfg(feature = "physics")]
#[cfg(test)]
mod tests {
    use super::*;
    use engine_ecs::{ColliderComponentData, ComponentData, RigidbodyComponentData};
    use engine_physics::{PhysicsWorld, SimplePhysicsBackend};

    #[test]
    fn physics_sync_creates_body_for_entity_with_rigidbody() {
        let mut scene = Scene::new();
        let entity = scene.create_object("TestObject").unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData::default()),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();

        assert_eq!(sync.body_count(), 1);
    }

    #[test]
    fn physics_sync_creates_collider_for_entity_with_collider() {
        let mut scene = Scene::new();
        let entity = scene.create_object("TestObject").unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData::default()),
            )
            .unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Collider(ColliderComponentData::default()),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();

        assert_eq!(sync.body_count(), 1);
        assert_eq!(sync.collider_count(), 1);
    }

    #[test]
    fn physics_sync_removes_body_when_entity_destroyed() {
        let mut scene = Scene::new();
        let entity = scene.create_object("TestObject").unwrap();
        let object_id = scene.object(entity).unwrap().id;
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData::default()),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();
        assert_eq!(sync.body_count(), 1);

        // Destroy the entity
        scene.destroy_deferred(entity).unwrap();
        scene.process_deferred_destroy().unwrap();

        // Sync destruction
        let destroyed = sync.sync_destruction(&scene, &mut world).unwrap();
        assert!(destroyed.contains(&object_id));
        assert_eq!(sync.body_count(), 0);
    }

    #[test]
    fn physics_sync_skips_entities_without_rigidbody() {
        let mut scene = Scene::new();
        let _entity = scene.create_object("TestObject").unwrap();
        // No RigidbodyComponent

        let mut sync = PhysicsSync::new();
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();

        assert_eq!(sync.body_count(), 0);
    }

    #[test]
    fn physics_sync_activation_skips_far_entities_until_they_enter_range() {
        let mut scene = Scene::new();
        let entity = scene.create_object("FarObject").unwrap();
        scene.transforms_mut().set_local(
            entity,
            engine_core::math::Transform {
                translation: Vec3::new(100.0, 0.0, 0.0),
                ..engine_core::math::Transform::IDENTITY
            },
        );
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData::default()),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        sync.set_activation(Some(PhysicsActivationConfig::new(Vec3::ZERO, 10.0)));
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();
        assert_eq!(sync.body_count(), 0);

        scene.transforms_mut().set_local(
            entity,
            engine_core::math::Transform {
                translation: Vec3::new(5.0, 0.0, 0.0),
                ..engine_core::math::Transform::IDENTITY
            },
        );
        sync.sync_creation(&scene, &mut world).unwrap();

        assert_eq!(sync.body_count(), 1);
    }

    #[test]
    fn physics_sync_activation_releases_bodies_outside_release_radius() {
        let mut scene = Scene::new();
        let entity = scene.create_object("StreamingObject").unwrap();
        let object_id = scene.object(entity).unwrap().id;
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData::default()),
            )
            .unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Collider(ColliderComponentData::default()),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        sync.set_activation(Some(PhysicsActivationConfig::new(Vec3::ZERO, 10.0)));
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();
        assert_eq!(sync.body_count(), 1);
        assert_eq!(sync.collider_count(), 1);

        scene.transforms_mut().set_local(
            entity,
            engine_core::math::Transform {
                translation: Vec3::new(20.0, 0.0, 0.0),
                ..engine_core::math::Transform::IDENTITY
            },
        );
        let released = sync.sync_activation(&scene, &mut world).unwrap();

        assert_eq!(released, vec![object_id]);
        assert_eq!(sync.body_count(), 0);
        assert_eq!(sync.collider_count(), 0);
        assert_eq!(world.stats().body_count, 0);
    }

    #[test]
    fn physics_sync_activation_sleeps_dynamic_bodies_outside_sleep_radius() {
        let mut scene = Scene::new();
        let entity = scene.create_object("SleepyObject").unwrap();
        scene.transforms_mut().set_local(
            entity,
            engine_core::math::Transform {
                translation: Vec3::new(9.0, 0.0, 0.0),
                ..engine_core::math::Transform::IDENTITY
            },
        );
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData {
                    body_type: "dynamic".to_string(),
                    ..RigidbodyComponentData::default()
                }),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        sync.set_activation(Some(PhysicsActivationConfig {
            center: Vec3::ZERO,
            active_radius: 10.0,
            release_radius: 20.0,
            sleep_radius: 8.0,
        }));
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();
        sync.sync_activation(&scene, &mut world).unwrap();

        assert_eq!(world.stats().sleeping_count, 1);

        scene.transforms_mut().set_local(
            entity,
            engine_core::math::Transform {
                translation: Vec3::new(2.0, 0.0, 0.0),
                ..engine_core::math::Transform::IDENTITY
            },
        );
        sync.sync_activation(&scene, &mut world).unwrap();

        assert_eq!(world.stats().sleeping_count, 0);
    }

    #[test]
    fn physics_sync_transform_writeback_updates_dynamic_body() {
        let mut scene = Scene::new();
        let entity = scene.create_object("FallingObject").unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData {
                    body_type: "dynamic".to_string(),
                    use_gravity: true,
                    ..RigidbodyComponentData::default()
                }),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();

        // Step physics (dynamic body falls due to gravity)
        world.fixed_update(1.0 / 60.0);

        // Write back transforms
        sync.sync_transforms_from_physics(&mut scene, &mut world)
            .unwrap();

        // Verify the entity's transform position changed (y should decrease due to gravity)
        let transform = scene.transforms().local(entity).unwrap();
        assert!(
            transform.translation.y < 0.0,
            "dynamic body should have fallen below origin, got y={}",
            transform.translation.y
        );
    }

    #[test]
    fn physics_sync_transform_writeback_skips_static_body() {
        let mut scene = Scene::new();
        let entity = scene.create_object("StaticObject").unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData {
                    body_type: "static".to_string(),
                    ..RigidbodyComponentData::default()
                }),
            )
            .unwrap();

        let mut sync = PhysicsSync::new();
        let mut world = PhysicsWorld::new(SimplePhysicsBackend::new());

        sync.sync_creation(&scene, &mut world).unwrap();

        // Step physics
        world.fixed_update(1.0 / 60.0);

        // Write back transforms — static body should NOT be updated
        sync.sync_transforms_from_physics(&mut scene, &mut world)
            .unwrap();

        // Verify the entity's transform position is still at origin (default)
        let transform = scene.transforms().local(entity).unwrap();
        assert_eq!(transform.translation.y, 0.0, "static body should not move");
    }
}
