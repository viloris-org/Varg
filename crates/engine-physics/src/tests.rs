use super::*;

#[test]
fn collision_profiles_honor_overrides_and_overlap_semantics() {
    let registry = CollisionProfileRegistry::default();
    let mut pawn = CollisionProfile {
        channel: "Pawn".into(),
        generate_overlap_events: true,
        ..CollisionProfile::default()
    };
    let projectile = CollisionProfile {
        channel: "Projectile".into(),
        generate_overlap_events: true,
        ..CollisionProfile::default()
    };

    assert!(!registry.should_collide(&pawn, &projectile));
    assert!(registry.should_overlap(&pawn, &projectile));

    pawn.responses
        .insert("Projectile".into(), CollisionResponse::Ignore);
    assert!(!registry.should_collide(&pawn, &projectile));
    assert!(!registry.should_overlap(&pawn, &projectile));
}

#[test]
fn built_in_physical_material_matches_names_case_insensitively() {
    let rubber = built_in_physical_material("rubber");
    let rubber_title = built_in_physical_material("Rubber");
    let unknown = built_in_physical_material("missing");

    assert_eq!(rubber, rubber_title);
    assert_eq!(rubber.friction, 0.9);
    assert_eq!(rubber.restitution, 0.8);
    assert_eq!(unknown, PhysicalMaterial::default());
}

#[test]
fn physics_world_filters_drained_contact_events() {
    let mut backend = SimplePhysicsBackend::new();
    let ignored = backend.create_body(&RigidbodyDesc::default()).unwrap();
    let other = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.25, 0.0, 0.0),
                ..Transform::IDENTITY
            },
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(ignored, &ColliderDesc::default())
        .unwrap();
    backend
        .add_collider(other, &ColliderDesc::default())
        .unwrap();

    let mut world = PhysicsWorld::new(backend);
    world
        .contact_filter_chain
        .push(ContactFilter::IgnoreBody { body: ignored });
    world.fixed_update(0.0);

    assert!(world.drain_contacts().is_empty());
}

#[test]
fn invalid_heightfield_is_rejected_without_panicking() {
    let mut backend = SimplePhysicsBackend::default();
    let body = backend.create_body(&RigidbodyDesc::default()).unwrap();
    let result = backend.add_collider(
        body,
        &ColliderDesc {
            shape: ColliderShape::Heightfield {
                num_x: 3,
                num_z: 3,
                heights: vec![0.0; 8],
                scale: Vec3::ONE,
            },
            ..ColliderDesc::default()
        },
    );

    assert!(result.is_err());
}

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
    assert!(
        backend
            .overlap_sphere(Vec3::ZERO, 2.0, QueryFilter { mask: 1 << 2 })
            .is_empty()
    );
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

    assert!(
        backend
            .raycast(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
                10.0,
                QueryFilter { mask: 1 << 3 },
            )
            .is_none()
    );

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

    assert!(
        contacts
            .iter()
            .any(|event| event.entered && event.is_trigger)
    );
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

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_dynamic_body_falls_due_to_gravity() {
    let mut backend = RapierPhysicsBackend::new();
    let body = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, 5.0, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Dynamic,
            gravity_scale: 1.0,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            body,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Step simulation for 1 second (60 frames at 1/60s each)
    for _ in 0..60 {
        backend.fixed_update(1.0 / 60.0);
    }

    let transform = backend.body_transform(body).unwrap();
    // After 1 second of falling with gravity, the body should be below starting position
    assert!(
        transform.translation.y < 4.0,
        "Dynamic body should have fallen below starting position, got y={}",
        transform.translation.y
    );
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_static_body_does_not_move() {
    let mut backend = RapierPhysicsBackend::new();
    let body = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, 5.0, 0.0),
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
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Step simulation for 1 second (60 frames at 1/60s each)
    for _ in 0..60 {
        backend.fixed_update(1.0 / 60.0);
    }

    let transform = backend.body_transform(body).unwrap();
    // Static body should remain at original position
    assert!(
        (transform.translation.y - 5.0).abs() < 0.001,
        "Static body should not move, got y={}",
        transform.translation.y
    );
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_create_collider_attach_and_step_no_crash() {
    let mut backend = RapierPhysicsBackend::new();

    // Box collider on static body
    let static_body = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, -0.5, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let box_collider = backend
        .add_collider(
            static_body,
            &ColliderDesc {
                shape: ColliderShape::Box {
                    half_extents: Vec3::new(5.0, 0.5, 5.0),
                },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Sphere collider on dynamic body
    let dynamic_body = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, 5.0, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Dynamic,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let sphere_collider = backend
        .add_collider(
            dynamic_body,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Capsule collider on kinematic body
    let kinematic_body = backend
        .create_body(&RigidbodyDesc {
            kind: BodyKind::Kinematic,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let capsule_collider = backend
        .add_collider(
            kinematic_body,
            &ColliderDesc {
                shape: ColliderShape::Capsule {
                    half_height: 0.5,
                    radius: 0.25,
                },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Step simulation — no crash
    for _ in 0..60 {
        backend.fixed_update(1.0 / 60.0);
    }

    // Verify dynamic body fell
    let dynamic_transform = backend.body_transform(dynamic_body).unwrap();
    assert!(dynamic_transform.translation.y < 5.0);

    // Remove sphere collider, step again — no crash
    backend.remove_collider(sphere_collider).unwrap();
    for _ in 0..10 {
        backend.fixed_update(1.0 / 60.0);
    }

    // Verify collider count decreased
    assert_eq!(backend.collider_count(), 2);

    // Ensure all handles are distinct
    assert_ne!(box_collider, sphere_collider);
    assert_ne!(sphere_collider, capsule_collider);
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_stats_track_bodies_colliders_and_joints() {
    let mut backend = RapierPhysicsBackend::new();
    let static_body = backend
        .create_body(&RigidbodyDesc {
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let dynamic_body = backend.create_body(&RigidbodyDesc::default()).unwrap();
    backend
        .add_collider(static_body, &ColliderDesc::default())
        .unwrap();
    backend
        .add_collider(dynamic_body, &ColliderDesc::default())
        .unwrap();
    backend
        .create_joint(&JointDesc::pin(
            static_body,
            dynamic_body,
            Vec3::ZERO,
            Vec3::ZERO,
        ))
        .unwrap();

    backend.fixed_update(1.0 / 60.0);
    let stats = backend.stats();

    assert_eq!(stats.body_count, 2);
    assert_eq!(stats.collider_count, 2);
    assert_eq!(stats.joint_count, 1);
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_vehicle_creation_update_and_destroy_updates_stats() {
    let mut backend = RapierPhysicsBackend::new();
    let ground = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, -0.5, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            ground,
            &ColliderDesc {
                shape: ColliderShape::Box {
                    half_extents: Vec3::new(10.0, 0.5, 10.0),
                },
                ..ColliderDesc::default()
            },
        )
        .unwrap();
    let chassis = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, 1.0, 0.0),
                ..Transform::IDENTITY
            },
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            chassis,
            &ColliderDesc {
                shape: ColliderShape::Box {
                    half_extents: Vec3::new(0.8, 0.25, 1.2),
                },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    let vehicle = backend
        .create_vehicle(&VehicleDesc {
            chassis,
            wheels: vec![
                WheelDesc {
                    chassis_connection: Vec3::new(-0.5, 0.0, 0.7),
                    is_steered: true,
                    ..WheelDesc::default()
                },
                WheelDesc {
                    chassis_connection: Vec3::new(0.5, 0.0, 0.7),
                    is_steered: true,
                    ..WheelDesc::default()
                },
                WheelDesc {
                    chassis_connection: Vec3::new(-0.5, 0.0, -0.7),
                    is_steered: false,
                    ..WheelDesc::default()
                },
                WheelDesc {
                    chassis_connection: Vec3::new(0.5, 0.0, -0.7),
                    is_steered: false,
                    ..WheelDesc::default()
                },
            ],
            tuning: VehicleTuning::default(),
        })
        .unwrap();

    backend.fixed_update(1.0 / 60.0);
    let state = backend
        .update_vehicle(
            vehicle,
            VehicleInput {
                throttle: 1.0,
                steering: 0.25,
                ..VehicleInput::default()
            },
        )
        .unwrap();

    assert_eq!(state.wheel_transforms.len(), 4);
    assert_eq!(state.suspension_displacements.len(), 4);
    assert_eq!(backend.stats().vehicle_count, 1);

    backend.destroy_vehicle(vehicle).unwrap();
    backend.fixed_update(1.0 / 60.0);
    assert_eq!(backend.stats().vehicle_count, 0);
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_raycast_returns_closest_of_two_spheres() {
    let mut backend = RapierPhysicsBackend::new();

    // Near sphere at z=3
    let near = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, 0.0, 3.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            near,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Far sphere at z=6
    let far = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, 0.0, 6.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            far,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    backend.fixed_update(1.0 / 60.0);

    // Raycast along +Z from origin
    let hit = backend
        .raycast(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 1.0),
            100.0,
            QueryFilter::default(),
        )
        .expect("should hit something");

    // Closest hit should be the near sphere
    assert_eq!(hit.body, near);
    assert!(hit.distance > 2.0 && hit.distance < 4.0);
    assert!(hit.point.z > 2.0 && hit.point.z < 4.0);
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_overlap_sphere_returns_intersecting_bodies() {
    let mut backend = RapierPhysicsBackend::new();

    // Body at origin
    let center_body = backend
        .create_body(&RigidbodyDesc {
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            center_body,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Nearby body at (1, 0, 0)
    let nearby_body = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(1.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            nearby_body,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Distant body at (10, 0, 0)
    let distant_body = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(10.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    backend
        .add_collider(
            distant_body,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    backend.fixed_update(1.0 / 60.0);

    // Overlap sphere at origin with radius 3 — should find center and nearby bodies
    let results = backend.overlap_sphere(Vec3::ZERO, 3.0, QueryFilter::default());
    let found_bodies: Vec<BodyHandle> = results.iter().map(|r| r.body).collect();
    assert!(
        found_bodies.contains(&center_body),
        "should find center body"
    );
    assert!(
        found_bodies.contains(&nearby_body),
        "should find nearby body"
    );
    assert!(
        !found_bodies.contains(&distant_body),
        "should not find distant body"
    );
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_contact_event_on_cube_landing_on_floor() {
    let mut backend = RapierPhysicsBackend::new();

    // Ground plane (static)
    let floor = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, -0.5, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let floor_collider = backend
        .add_collider(
            floor,
            &ColliderDesc {
                shape: ColliderShape::Box {
                    half_extents: Vec3::new(5.0, 0.5, 5.0),
                },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Falling cube (dynamic)
    let cube = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(0.0, 2.0, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Dynamic,
            gravity_scale: 1.0,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let cube_collider = backend
        .add_collider(
            cube,
            &ColliderDesc {
                shape: ColliderShape::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Step physics for several frames so cube falls and hits floor
    let mut found_enter = false;
    for _ in 0..120 {
        backend.fixed_update(1.0 / 60.0);
        let contacts = backend.drain_contacts();
        for event in contacts {
            if event.entered {
                found_enter = true;
                // Verify both bodies/colliders are present
                assert!(
                    (event.body_a == floor && event.body_b == cube)
                        || (event.body_a == cube && event.body_b == floor),
                    "contact should involve floor and cube"
                );
                assert!(
                    (event.collider_a == floor_collider && event.collider_b == cube_collider)
                        || (event.collider_a == cube_collider
                            && event.collider_b == floor_collider),
                    "contact should involve floor_collider and cube_collider"
                );
                assert!(!event.is_trigger, "contact event should not be trigger");
            }
        }
        if found_enter {
            break;
        }
    }

    assert!(
        found_enter,
        "should have received at least one entered contact event"
    );
}

#[cfg(feature = "rapier")]
#[test]
fn rapier_backend_trigger_enter_and_exit_events() {
    let mut backend = RapierPhysicsBackend::new();

    // Trigger zone (static, is_trigger=true) at origin
    let trigger_body = backend
        .create_body(&RigidbodyDesc {
            kind: BodyKind::Static,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let trigger_collider = backend
        .add_collider(
            trigger_body,
            &ColliderDesc {
                shape: ColliderShape::Box {
                    half_extents: Vec3::new(1.0, 1.0, 1.0),
                },
                is_trigger: true,
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Dynamic body that will pass through the trigger zone
    let mover = backend
        .create_body(&RigidbodyDesc {
            transform: Transform {
                translation: Vec3::new(-3.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
            kind: BodyKind::Dynamic,
            gravity_scale: 0.0,
            ..RigidbodyDesc::default()
        })
        .unwrap();
    let mover_collider = backend
        .add_collider(
            mover,
            &ColliderDesc {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..ColliderDesc::default()
            },
        )
        .unwrap();

    // Step: mover is far away — no events expected
    backend.fixed_update(1.0 / 60.0);
    let far_contacts = backend.drain_contacts();
    let far_enter = far_contacts.iter().any(|e| e.entered && e.is_trigger);
    assert!(!far_enter, "no trigger enter event when far away");

    // Move mover into the trigger zone
    backend
        .set_body_transform(
            mover,
            Transform {
                translation: Vec3::new(0.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        )
        .unwrap();
    backend.fixed_update(1.0 / 60.0);
    let enter_contacts = backend.drain_contacts();
    let enter_event = enter_contacts.iter().find(|e| e.entered && e.is_trigger);
    assert!(enter_event.is_some(), "should receive trigger enter event");
    let enter = enter_event.unwrap();
    assert!(
        (enter.collider_a == trigger_collider && enter.collider_b == mover_collider)
            || (enter.collider_a == mover_collider && enter.collider_b == trigger_collider),
        "enter event should involve trigger and mover colliders"
    );

    // Move mover out of the trigger zone
    backend
        .set_body_transform(
            mover,
            Transform {
                translation: Vec3::new(3.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        )
        .unwrap();
    backend.fixed_update(1.0 / 60.0);
    let exit_contacts = backend.drain_contacts();
    let exit_event = exit_contacts.iter().find(|e| !e.entered && e.is_trigger);
    assert!(exit_event.is_some(), "should receive trigger exit event");
}
