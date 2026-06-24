use engine_core::{EngineConfig, math::Transform, math::Vec3};
use engine_ecs::{
    BuoyancyProbeSetComponentData, ColliderComponentData, ComponentData, FluidVolumeComponentData,
    RigidbodyComponentData, Scene, WindZoneComponentData,
};
use runtime_min::headless_services_from_scene;
use std::time::Duration;

/// Test that a dynamic rigidbody with gravity falls over 60 fixed timesteps.
/// This verifies the full game loop with physics integration works correctly.
#[test]
#[cfg(feature = "physics")]
fn windowless_scene_simulation_with_physics() {
    // Create a scene with a ground plane and a falling cube
    let mut scene = Scene::default();

    // Ground plane (static rigidbody at y = -0.5)
    let ground = scene.create_object("Ground").unwrap();
    scene
        .upsert_component(
            ground,
            ComponentData::Rigidbody(RigidbodyComponentData {
                body_type: "static".to_string(),
                mass: 1.0,
                use_gravity: false,
                linear_damping: 0.0,
                angular_damping: 0.05,
                lock_position: [false; 3],
                lock_rotation: [false; 3],
            }),
        )
        .unwrap();
    scene
        .upsert_component(
            ground,
            ComponentData::Collider(ColliderComponentData {
                shape: "box".to_string(),
                size: Vec3::new(10.0, 0.1, 10.0), // Large flat plane
                is_trigger: false,
                mask: !0,
                physics_material: "default".to_string(),
            }),
        )
        .unwrap();
    // Set ground position
    scene.transforms_mut().set_local(
        ground,
        Transform {
            translation: Vec3::new(0.0, -0.5, 0.0),
            ..Transform::IDENTITY
        },
    );

    // Falling cube (dynamic rigidbody at y = 5.0)
    let cube = scene.create_object("FallingCube").unwrap();
    scene
        .upsert_component(
            cube,
            ComponentData::Rigidbody(RigidbodyComponentData {
                body_type: "dynamic".to_string(),
                mass: 1.0,
                use_gravity: true,
                linear_damping: 0.0,
                angular_damping: 0.05,
                lock_position: [false; 3],
                lock_rotation: [false; 3],
            }),
        )
        .unwrap();
    scene
        .upsert_component(
            cube,
            ComponentData::Collider(ColliderComponentData {
                shape: "box".to_string(),
                size: Vec3::ONE,
                is_trigger: false,
                mask: !0,
                physics_material: "default".to_string(),
            }),
        )
        .unwrap();
    // Set cube initial position high above ground
    scene.transforms_mut().set_local(
        cube,
        Transform {
            translation: Vec3::new(0.0, 5.0, 0.0),
            ..Transform::IDENTITY
        },
    );

    // Record initial position
    let initial_y = scene.transforms().local(cube).unwrap().translation.y;
    assert_eq!(initial_y, 5.0, "Initial position should be y=5.0");

    // Create headless runtime services with physics
    let mut services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &scene,
    )
    .unwrap();

    // Run 60 fixed timesteps (1 second of simulated time at 60 Hz)
    let fixed_dt = Duration::from_secs_f32(1.0 / 60.0);
    for _ in 0..60 {
        services.run_frame(fixed_dt, false).unwrap();
    }

    // Verify the cube has fallen due to gravity
    let final_transform = services.scene.transforms().local(cube).unwrap();
    let final_y = final_transform.translation.y;

    // The cube should have fallen significantly (gravity pulls it down)
    assert!(
        final_y < initial_y,
        "Cube should have fallen: initial_y={}, final_y={}",
        initial_y,
        final_y
    );

    // The cube should have fallen at least 1 meter (conservative check)
    assert!(
        final_y < 4.0,
        "Cube should have fallen at least 1 meter: final_y={}",
        final_y
    );

    // Verify no NaN or infinity in transform components
    assert!(
        final_transform.translation.x.is_finite(),
        "Translation X should be finite"
    );
    assert!(
        final_transform.translation.y.is_finite(),
        "Translation Y should be finite"
    );
    assert!(
        final_transform.translation.z.is_finite(),
        "Translation Z should be finite"
    );
    assert!(
        final_transform.rotation.x.is_finite(),
        "Rotation X should be finite"
    );
    assert!(
        final_transform.rotation.y.is_finite(),
        "Rotation Y should be finite"
    );
    assert!(
        final_transform.rotation.z.is_finite(),
        "Rotation Z should be finite"
    );
    assert!(
        final_transform.rotation.w.is_finite(),
        "Rotation W should be finite"
    );
    assert!(
        final_transform.scale.x.is_finite(),
        "Scale X should be finite"
    );
    assert!(
        final_transform.scale.y.is_finite(),
        "Scale Y should be finite"
    );
    assert!(
        final_transform.scale.z.is_finite(),
        "Scale Z should be finite"
    );

    // Verify frame counter advanced
    assert_eq!(
        services.frame_index(),
        60,
        "Frame index should be 60 after 60 frames"
    );

    // Verify physics steps were executed
    assert!(
        services.stats.physics_steps > 0,
        "Physics steps should have been executed"
    );
}

#[test]
#[cfg(feature = "physics")]
fn fluid_volume_applies_buoyancy_and_drag_to_dynamic_body() {
    fn scene_with_optional_water(include_water: bool) -> (Scene, engine_core::EntityId) {
        let mut scene = Scene::default();

        let cube = scene.create_object("FloatingCube").unwrap();
        let cube_id = scene.object(cube).unwrap().id;
        scene
            .upsert_component(
                cube,
                ComponentData::Rigidbody(RigidbodyComponentData {
                    body_type: "dynamic".to_string(),
                    mass: 1.0,
                    use_gravity: true,
                    linear_damping: 0.0,
                    angular_damping: 0.05,
                    lock_position: [false; 3],
                    lock_rotation: [false; 3],
                }),
            )
            .unwrap();
        scene
            .upsert_component(
                cube,
                ComponentData::Collider(ColliderComponentData {
                    shape: "box".to_string(),
                    size: Vec3::ONE,
                    is_trigger: false,
                    mask: !0,
                    physics_material: "default".to_string(),
                }),
            )
            .unwrap();
        scene.transforms_mut().set_local(
            cube,
            Transform {
                translation: Vec3::new(0.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        );

        if include_water {
            let water = scene.create_object("WaterVolume").unwrap();
            scene
                .upsert_component(
                    water,
                    ComponentData::FluidVolume(FluidVolumeComponentData {
                        size: Vec3::new(10.0, 4.0, 10.0),
                        buoyancy_scale: 1.25,
                        linear_drag: 6.0,
                        ..FluidVolumeComponentData::default()
                    }),
                )
                .unwrap();
            scene.transforms_mut().set_local(
                water,
                Transform {
                    translation: Vec3::new(0.0, 0.0, 0.0),
                    ..Transform::IDENTITY
                },
            );
        }

        (scene, cube_id)
    }

    let (dry_scene, dry_cube_id) = scene_with_optional_water(false);
    let (wet_scene, wet_cube_id) = scene_with_optional_water(true);

    let mut dry_services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &dry_scene,
    )
    .unwrap();
    let mut wet_services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &wet_scene,
    )
    .unwrap();

    let fixed_dt = Duration::from_secs_f32(1.0 / 60.0);
    for _ in 0..60 {
        dry_services.run_frame(fixed_dt, false).unwrap();
        wet_services.run_frame(fixed_dt, false).unwrap();
    }

    let dry_cube = dry_services.scene.find_by_id(dry_cube_id).unwrap();
    let wet_cube = wet_services.scene.find_by_id(wet_cube_id).unwrap();
    let dry_y = dry_services
        .scene
        .transforms()
        .local(dry_cube)
        .unwrap()
        .translation
        .y;
    let wet_y = wet_services
        .scene
        .transforms()
        .local(wet_cube)
        .unwrap()
        .translation
        .y;

    assert!(
        wet_y > dry_y,
        "fluid volume should slow or reverse falling motion: wet_y={wet_y}, dry_y={dry_y}"
    );
}

#[test]
#[cfg(feature = "physics")]
fn ocean_wave_surface_changes_buoyancy_strength() {
    fn scene_at_x(x: f32) -> (Scene, engine_core::EntityId) {
        let mut scene = Scene::default();

        let ocean = scene.create_object("Ocean").unwrap();
        scene
            .upsert_component(
                ocean,
                ComponentData::FluidVolume(FluidVolumeComponentData {
                    size: Vec3::new(20.0, 4.0, 20.0),
                    surface_profile: "ocean".to_string(),
                    wave_direction: Vec3::new(1.0, 0.0, 0.0),
                    wave_amplitude: 0.5,
                    wave_length: 8.0,
                    wave_speed: 0.0,
                    linear_drag: 0.0,
                    ..FluidVolumeComponentData::default()
                }),
            )
            .unwrap();

        let cube = scene.create_object("WaveCube").unwrap();
        let cube_id = scene.object(cube).unwrap().id;
        scene
            .upsert_component(
                cube,
                ComponentData::Rigidbody(RigidbodyComponentData {
                    body_type: "dynamic".to_string(),
                    mass: 1.0,
                    use_gravity: false,
                    linear_damping: 0.0,
                    angular_damping: 0.05,
                    lock_position: [false; 3],
                    lock_rotation: [false; 3],
                }),
            )
            .unwrap();
        scene
            .upsert_component(
                cube,
                ComponentData::Collider(ColliderComponentData {
                    shape: "box".to_string(),
                    size: Vec3::ONE,
                    is_trigger: false,
                    mask: !0,
                    physics_material: "default".to_string(),
                }),
            )
            .unwrap();
        scene.transforms_mut().set_local(
            cube,
            Transform {
                translation: Vec3::new(x, 2.0, 0.0),
                ..Transform::IDENTITY
            },
        );

        (scene, cube_id)
    }

    let (crest_scene, crest_id) = scene_at_x(2.0);
    let (trough_scene, trough_id) = scene_at_x(6.0);
    let mut crest_services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &crest_scene,
    )
    .unwrap();
    let mut trough_services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &trough_scene,
    )
    .unwrap();

    let fixed_dt = Duration::from_secs_f32(1.0 / 60.0);
    for _ in 0..8 {
        crest_services.run_frame(fixed_dt, false).unwrap();
        trough_services.run_frame(fixed_dt, false).unwrap();
    }

    let crest = crest_services.scene.find_by_id(crest_id).unwrap();
    let trough = trough_services.scene.find_by_id(trough_id).unwrap();
    let crest_y = crest_services
        .scene
        .transforms()
        .local(crest)
        .unwrap()
        .translation
        .y;
    let trough_y = trough_services
        .scene
        .transforms()
        .local(trough)
        .unwrap()
        .translation
        .y;

    assert!(
        crest_y > trough_y,
        "crest should produce stronger buoyancy than trough: crest_y={crest_y}, trough_y={trough_y}"
    );
}

#[test]
#[cfg(feature = "physics")]
fn buoyancy_probe_set_applies_wave_sampled_lift() {
    let mut scene = Scene::default();

    let ocean = scene.create_object("Ocean").unwrap();
    scene
        .upsert_component(
            ocean,
            ComponentData::FluidVolume(FluidVolumeComponentData {
                size: Vec3::new(20.0, 4.0, 20.0),
                surface_profile: "ocean".to_string(),
                wave_direction: Vec3::new(1.0, 0.0, 0.0),
                wave_amplitude: 0.5,
                wave_length: 8.0,
                wave_speed: 0.0,
                linear_drag: 0.0,
                ..FluidVolumeComponentData::default()
            }),
        )
        .unwrap();

    let boat = scene.create_object("ProbeBoat").unwrap();
    let boat_id = scene.object(boat).unwrap().id;
    scene
        .upsert_component(
            boat,
            ComponentData::Rigidbody(RigidbodyComponentData {
                body_type: "dynamic".to_string(),
                mass: 1.0,
                use_gravity: false,
                linear_damping: 0.0,
                angular_damping: 0.05,
                lock_position: [false; 3],
                lock_rotation: [false; 3],
            }),
        )
        .unwrap();
    scene
        .upsert_component(
            boat,
            ComponentData::Collider(ColliderComponentData {
                shape: "box".to_string(),
                size: Vec3::new(2.0, 1.0, 1.0),
                is_trigger: false,
                mask: !0,
                physics_material: "default".to_string(),
            }),
        )
        .unwrap();
    scene
        .upsert_component(
            boat,
            ComponentData::BuoyancyProbeSet(BuoyancyProbeSetComponentData {
                probes: vec![Vec3::new(-0.75, -0.5, 0.0), Vec3::new(0.75, -0.5, 0.0)],
                buoyancy: 4.0,
                damping: 0.0,
                angular_response: 1.0,
            }),
        )
        .unwrap();
    scene.transforms_mut().set_local(
        boat,
        Transform {
            translation: Vec3::new(2.0, 2.0, 0.0),
            ..Transform::IDENTITY
        },
    );

    let mut services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &scene,
    )
    .unwrap();
    let initial_y = 2.0;
    let fixed_dt = Duration::from_secs_f32(1.0 / 60.0);
    for _ in 0..8 {
        services.run_frame(fixed_dt, false).unwrap();
    }

    let boat = services.scene.find_by_id(boat_id).unwrap();
    let y = services
        .scene
        .transforms()
        .local(boat)
        .unwrap()
        .translation
        .y;
    assert!(
        y > initial_y,
        "probe buoyancy should lift the boat on an ocean crest: y={y}"
    );
}

#[test]
#[cfg(feature = "physics")]
fn wind_zone_pushes_dynamic_body_toward_air_velocity() {
    fn scene_with_optional_wind(include_wind: bool) -> (Scene, engine_core::EntityId) {
        let mut scene = Scene::default();

        let cube = scene.create_object("WindCube").unwrap();
        let cube_id = scene.object(cube).unwrap().id;
        scene
            .upsert_component(
                cube,
                ComponentData::Rigidbody(RigidbodyComponentData {
                    body_type: "dynamic".to_string(),
                    mass: 1.0,
                    use_gravity: false,
                    linear_damping: 0.0,
                    angular_damping: 0.05,
                    lock_position: [false; 3],
                    lock_rotation: [false; 3],
                }),
            )
            .unwrap();
        scene
            .upsert_component(
                cube,
                ComponentData::Collider(ColliderComponentData {
                    shape: "box".to_string(),
                    size: Vec3::ONE,
                    is_trigger: false,
                    mask: !0,
                    physics_material: "default".to_string(),
                }),
            )
            .unwrap();

        if include_wind {
            let wind = scene.create_object("WindZone").unwrap();
            scene
                .upsert_component(
                    wind,
                    ComponentData::WindZone(WindZoneComponentData {
                        size: Vec3::new(20.0, 20.0, 20.0),
                        wind_velocity: Vec3::new(8.0, 0.0, 0.0),
                        strength: 2.0,
                        linear_drag: 1.0,
                    }),
                )
                .unwrap();
        }

        (scene, cube_id)
    }

    let (calm_scene, calm_cube_id) = scene_with_optional_wind(false);
    let (windy_scene, windy_cube_id) = scene_with_optional_wind(true);

    let mut calm_services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &calm_scene,
    )
    .unwrap();
    let mut windy_services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &windy_scene,
    )
    .unwrap();

    let fixed_dt = Duration::from_secs_f32(1.0 / 60.0);
    for _ in 0..60 {
        calm_services.run_frame(fixed_dt, false).unwrap();
        windy_services.run_frame(fixed_dt, false).unwrap();
    }

    let calm_cube = calm_services.scene.find_by_id(calm_cube_id).unwrap();
    let windy_cube = windy_services.scene.find_by_id(windy_cube_id).unwrap();
    let calm_x = calm_services
        .scene
        .transforms()
        .local(calm_cube)
        .unwrap()
        .translation
        .x;
    let windy_x = windy_services
        .scene
        .transforms()
        .local(windy_cube)
        .unwrap()
        .translation
        .x;

    assert!(
        windy_x > calm_x + 0.1,
        "wind zone should push body along wind velocity: windy_x={windy_x}, calm_x={calm_x}"
    );
}

#[test]
#[cfg(feature = "physics")]
fn fluid_volume_uses_displaced_volume_for_light_and_heavy_bodies() {
    let mut scene = Scene::default();

    let water = scene.create_object("WaterVolume").unwrap();
    scene
        .upsert_component(
            water,
            ComponentData::FluidVolume(FluidVolumeComponentData {
                size: Vec3::new(12.0, 6.0, 12.0),
                linear_drag: 8.0,
                ..FluidVolumeComponentData::default()
            }),
        )
        .unwrap();

    fn cube(scene: &mut Scene, name: &str, x: f32, mass: f32) -> engine_core::EntityId {
        let entity = scene.create_object(name).unwrap();
        let id = scene.object(entity).unwrap().id;
        scene
            .upsert_component(
                entity,
                ComponentData::Rigidbody(RigidbodyComponentData {
                    body_type: "dynamic".to_string(),
                    mass,
                    use_gravity: true,
                    linear_damping: 0.0,
                    angular_damping: 0.05,
                    lock_position: [false; 3],
                    lock_rotation: [false; 3],
                }),
            )
            .unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Collider(ColliderComponentData {
                    shape: "box".to_string(),
                    size: Vec3::ONE,
                    is_trigger: false,
                    mask: !0,
                    physics_material: "default".to_string(),
                }),
            )
            .unwrap();
        scene.transforms_mut().set_local(
            entity,
            Transform {
                translation: Vec3::new(x, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        );
        id
    }

    let light_id = cube(&mut scene, "LightCube", -1.5, 0.5);
    let heavy_id = cube(&mut scene, "HeavyCube", 1.5, 2000.0);

    let mut services = headless_services_from_scene(
        EngineConfig::default(),
        std::env::current_dir().unwrap(),
        &scene,
    )
    .unwrap();

    let fixed_dt = Duration::from_secs_f32(1.0 / 60.0);
    for _ in 0..60 {
        services.run_frame(fixed_dt, false).unwrap();
    }

    let light = services.scene.find_by_id(light_id).unwrap();
    let heavy = services.scene.find_by_id(heavy_id).unwrap();
    let light_y = services
        .scene
        .transforms()
        .local(light)
        .unwrap()
        .translation
        .y;
    let heavy_y = services
        .scene
        .transforms()
        .local(heavy)
        .unwrap()
        .translation
        .y;

    assert!(
        light_y > heavy_y,
        "lighter body should displace enough water to ride higher: light_y={light_y}, heavy_y={heavy_y}"
    );
}
