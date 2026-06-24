use engine_core::math::{Transform, Vec3};
use engine_ecs::{
    AudioSourceComponentData, BuoyancyProbeSetComponentData, CameraComponentData,
    ColliderComponentData, ComponentData, FluidVolumeComponentData, MaterialRef,
    MeshRendererComponentData, ParticleEmitterComponentData, RigidbodyComponentData, Scene,
};

/// Test that a scene with all component types can be serialized to JSON and
/// deserialized back to an identical scene.
#[test]
fn scene_with_all_components_round_trip() {
    let mut scene = Scene::default();

    // Create a camera object
    let camera = scene.create_object("MainCamera").unwrap();
    scene
        .upsert_component(
            camera,
            ComponentData::Camera(CameraComponentData {
                vertical_fov_degrees: 60.0,
                near: 0.1,
                far: 1000.0,
                aspect_ratio: None,
                primary: true,
                clear_color: Vec3::new(0.2, 0.3, 0.4),
            }),
        )
        .unwrap();

    // Create a mesh renderer object
    let mesh_obj = scene.create_object("Cube").unwrap();
    scene
        .upsert_component(
            mesh_obj,
            ComponentData::MeshRenderer(MeshRendererComponentData {
                mesh: None,
                builtin_mesh: Some("debug/cube".to_string()),
                material: MaterialRef {
                    asset: None,
                    builtin: Some("debug/default".to_string()),
                },
                casts_shadows: true,
                receive_shadows: true,
            }),
        )
        .unwrap();
    scene.transforms_mut().set_local(
        mesh_obj,
        Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            ..Transform::IDENTITY
        },
    );

    // Create a rigidbody object
    let physics_obj = scene.create_object("PhysicsObject").unwrap();
    scene
        .upsert_component(
            physics_obj,
            ComponentData::Rigidbody(RigidbodyComponentData {
                body_type: "dynamic".to_string(),
                mass: 2.5,
                use_gravity: true,
                linear_damping: 0.1,
                angular_damping: 0.05,
                lock_position: [false, true, false],
                lock_rotation: [true, false, false],
            }),
        )
        .unwrap();
    scene
        .upsert_component(
            physics_obj,
            ComponentData::Collider(ColliderComponentData {
                shape: "sphere".to_string(),
                size: Vec3::new(0.5, 0.5, 0.5),
                is_trigger: false,
                mask: 0xFF,
                physics_material: "metal".to_string(),
            }),
        )
        .unwrap();

    // Create an audio source object
    let audio_obj = scene.create_object("AudioSource").unwrap();
    scene
        .upsert_component(
            audio_obj,
            ComponentData::AudioSource(AudioSourceComponentData {
                clip: None,
                volume: 0.8,
                looping: true,
                play_on_start: true,
                spatial_blend: 0.5,
                ..AudioSourceComponentData::default()
            }),
        )
        .unwrap();

    // Create a particle emitter object
    let particles = scene.create_object("ParticleEmitter").unwrap();
    scene
        .upsert_component(
            particles,
            ComponentData::ParticleEmitter(ParticleEmitterComponentData {
                max_particles: 64,
                emission_rate: 16.0,
                lifetime: 1.5,
                elapsed: 0.25,
                ..ParticleEmitterComponentData::default()
            }),
        )
        .unwrap();

    // Create a fluid volume object
    let fluid = scene.create_object("WaterVolume").unwrap();
    scene
        .upsert_component(
            fluid,
            ComponentData::FluidVolume(FluidVolumeComponentData {
                size: Vec3::new(8.0, 2.0, 8.0),
                flow_velocity: Vec3::new(1.0, 0.0, 0.0),
                ..FluidVolumeComponentData::default()
            }),
        )
        .unwrap();

    let boat = scene.create_object("ProbeBoat").unwrap();
    scene
        .upsert_component(
            boat,
            ComponentData::BuoyancyProbeSet(BuoyancyProbeSetComponentData {
                probes: vec![Vec3::new(-1.0, -0.5, 0.0), Vec3::new(1.0, -0.5, 0.0)],
                buoyancy: 1.4,
                damping: 3.0,
                angular_response: 0.75,
            }),
        )
        .unwrap();

    // Serialize to JSON
    let scene_file = scene.to_scene_file("test_scene").unwrap();
    let json1 = serde_json::to_string_pretty(&scene_file).unwrap();

    // Deserialize back to Scene
    let scene_file_deserialized: engine_ecs::SceneFile =
        serde_json::from_str(&json1).expect("Failed to deserialize scene");
    let scene2 = Scene::from_scene_file(scene_file_deserialized).unwrap();

    // Verify object count matches
    assert_eq!(
        scene.object_count(),
        scene2.object_count(),
        "Object count should match after round-trip"
    );

    // Verify all objects exist with correct names by collecting into a sorted list
    let mut names1: Vec<_> = scene.iter_objects().map(|(_, obj)| &obj.name).collect();
    let mut names2: Vec<_> = scene2.iter_objects().map(|(_, obj)| &obj.name).collect();
    names1.sort();
    names2.sort();

    assert_eq!(names1, names2, "Object names should match");

    // Verify each object has the same component count
    for name in &names1 {
        let obj1 = scene
            .iter_objects()
            .find(|(_, obj)| &obj.name == *name)
            .map(|(_, obj)| obj)
            .unwrap();
        let obj2 = scene2
            .iter_objects()
            .find(|(_, obj)| &obj.name == *name)
            .map(|(_, obj)| obj)
            .unwrap();

        assert_eq!(
            obj1.components.len(),
            obj2.components.len(),
            "Component count should match for object {}",
            name
        );
    }

    // Serialize the deserialized scene again
    let scene_file2 = scene2.to_scene_file("test_scene").unwrap();
    let json2 = serde_json::to_string_pretty(&scene_file2).unwrap();

    // The two JSON strings should be identical (byte-for-byte)
    assert_eq!(
        json1, json2,
        "Second serialization should be identical to first"
    );
}

/// Test that an empty scene can be serialized and deserialized.
#[test]
fn empty_scene_round_trip() {
    let scene = Scene::default();

    // Serialize to JSON
    let scene_file = scene.to_scene_file("empty_scene").unwrap();
    let json = serde_json::to_string_pretty(&scene_file).unwrap();

    // Deserialize back
    let scene_file_deserialized: engine_ecs::SceneFile = serde_json::from_str(&json).unwrap();
    let scene2 = Scene::from_scene_file(scene_file_deserialized).unwrap();

    // Verify both scenes are empty
    assert_eq!(scene.object_count(), 0);
    assert_eq!(scene2.object_count(), 0);

    // Serialize again and verify identical
    let scene_file2 = scene2.to_scene_file("empty_scene").unwrap();
    let json2 = serde_json::to_string_pretty(&scene_file2).unwrap();
    assert_eq!(json, json2);
}

/// Test that nested GameObject hierarchy (parent-child relationships) are
/// preserved through serialization.
#[test]
fn nested_hierarchy_round_trip() {
    let mut scene = Scene::default();

    // Create parent
    let parent = scene.create_object("Parent").unwrap();
    scene.transforms_mut().set_local(
        parent,
        Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::IDENTITY
        },
    );

    // Create first child
    let child1 = scene.create_object("Child1").unwrap();
    scene.set_parent(child1, Some(parent)).unwrap();
    scene.transforms_mut().set_local(
        child1,
        Transform {
            translation: Vec3::new(0.0, 5.0, 0.0),
            ..Transform::IDENTITY
        },
    );

    // Create second child
    let child2 = scene.create_object("Child2").unwrap();
    scene.set_parent(child2, Some(parent)).unwrap();
    scene.transforms_mut().set_local(
        child2,
        Transform {
            translation: Vec3::new(0.0, -5.0, 0.0),
            ..Transform::IDENTITY
        },
    );

    // Create grandchild
    let grandchild = scene.create_object("Grandchild").unwrap();
    scene.set_parent(grandchild, Some(child1)).unwrap();
    scene.transforms_mut().set_local(
        grandchild,
        Transform {
            translation: Vec3::new(0.0, 0.0, 3.0),
            ..Transform::IDENTITY
        },
    );

    // Serialize
    let scene_file = scene.to_scene_file("hierarchy_scene").unwrap();
    let json = serde_json::to_string_pretty(&scene_file).unwrap();

    // Deserialize
    let scene_file_deserialized: engine_ecs::SceneFile = serde_json::from_str(&json).unwrap();
    let scene2 = Scene::from_scene_file(scene_file_deserialized).unwrap();

    // Verify object count
    assert_eq!(scene.object_count(), 4);
    assert_eq!(scene2.object_count(), 4);

    // Find objects by name in deserialized scene
    let parent2 = scene2
        .iter_objects()
        .find(|(_, obj)| obj.name == "Parent")
        .map(|(e, _)| e)
        .expect("Parent should exist");
    let child1_2 = scene2
        .iter_objects()
        .find(|(_, obj)| obj.name == "Child1")
        .map(|(e, _)| e)
        .expect("Child1 should exist");
    let child2_2 = scene2
        .iter_objects()
        .find(|(_, obj)| obj.name == "Child2")
        .map(|(e, _)| e)
        .expect("Child2 should exist");
    let grandchild2 = scene2
        .iter_objects()
        .find(|(_, obj)| obj.name == "Grandchild")
        .map(|(e, _)| e)
        .expect("Grandchild should exist");

    // Verify parent-child relationships
    assert_eq!(
        scene2.transforms().parent(child1_2),
        Some(parent2),
        "Child1 parent should be Parent"
    );
    assert_eq!(
        scene2.transforms().parent(child2_2),
        Some(parent2),
        "Child2 parent should be Parent"
    );
    assert_eq!(
        scene2.transforms().parent(grandchild2),
        Some(child1_2),
        "Grandchild parent should be Child1"
    );
    assert_eq!(
        scene2.transforms().parent(parent2),
        None,
        "Parent should have no parent"
    );

    // Verify local transforms are preserved
    let parent_transform = scene2.transforms().local(parent2).unwrap();
    assert_eq!(parent_transform.translation.x, 10.0);

    let child1_transform = scene2.transforms().local(child1_2).unwrap();
    assert_eq!(child1_transform.translation.y, 5.0);

    let grandchild_transform = scene2.transforms().local(grandchild2).unwrap();
    assert_eq!(grandchild_transform.translation.z, 3.0);

    // Serialize again and verify identical
    let scene_file2 = scene2.to_scene_file("hierarchy_scene").unwrap();
    let json2 = serde_json::to_string_pretty(&scene_file2).unwrap();
    assert_eq!(json, json2, "Second serialization should be identical");
}

#[test]
fn fluid_volume_samples_still_water_surface() {
    let fluid = FluidVolumeComponentData {
        size: Vec3::new(8.0, 4.0, 8.0),
        surface_offset: 0.25,
        ..FluidVolumeComponentData::default()
    };

    assert_approx_eq(fluid.surface_height_at(Vec3::ZERO, 0.0), 1.75);
    assert_approx_eq(fluid.depth_at(Vec3::new(0.0, 1.25, 0.0), 0.0), 0.5);
    assert!(fluid.contains_submerged_point(Vec3::new(0.0, 1.25, 0.0), 0.0));
    assert!(!fluid.contains_submerged_point(Vec3::new(0.0, 2.25, 0.0), 0.0));
}

#[test]
fn fluid_volume_samples_river_slope_along_flow() {
    let fluid = FluidVolumeComponentData {
        size: Vec3::new(20.0, 4.0, 8.0),
        surface_profile: "river".to_string(),
        wave_direction: Vec3::new(1.0, 0.0, 0.0),
        river_slope: -0.05,
        ..FluidVolumeComponentData::default()
    };

    assert_approx_eq(fluid.surface_height_at(Vec3::new(4.0, 0.0, 0.0), 0.0), 1.8);
    assert_approx_eq(fluid.surface_height_at(Vec3::new(-4.0, 0.0, 0.0), 0.0), 2.2);
}

#[test]
fn fluid_volume_samples_ocean_waves_and_tides() {
    let ocean = FluidVolumeComponentData {
        size: Vec3::new(20.0, 4.0, 20.0),
        surface_profile: "ocean".to_string(),
        wave_direction: Vec3::new(1.0, 0.0, 0.0),
        wave_amplitude: 0.5,
        wave_length: 8.0,
        wave_speed: 2.0,
        ..FluidVolumeComponentData::default()
    };

    assert_approx_eq(ocean.surface_height_at(Vec3::new(2.0, 0.0, 0.0), 0.0), 2.5);
    assert_approx_eq(ocean.surface_height_at(Vec3::new(4.0, 0.0, 0.0), 0.0), 2.0);

    let tidal = FluidVolumeComponentData {
        surface_profile: "tidal".to_string(),
        tide_amplitude: 1.25,
        tide_period_seconds: 12.0,
        ..FluidVolumeComponentData::default()
    };

    assert_approx_eq(tidal.surface_height_at(Vec3::ZERO, 3.0), 2.25);
    assert_approx_eq(tidal.surface_height_at(Vec3::ZERO, 9.0), -0.25);
}

#[test]
fn fluid_volume_deserializes_legacy_static_water_defaults() {
    let json = r#"{
        "size": { "x": 8.0, "y": 2.0, "z": 8.0 },
        "density": 1000.0,
        "buoyancy_scale": 1.0,
        "linear_drag": 2.0,
        "angular_drag": 0.5,
        "flow_velocity": { "x": 1.0, "y": 0.0, "z": 0.0 },
        "surface_offset": 0.0
    }"#;

    let fluid: FluidVolumeComponentData = serde_json::from_str(json).unwrap();

    assert_eq!(fluid.surface_profile, "still");
    assert_eq!(fluid.wave_direction, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(fluid.water_tint, Vec3::new(0.05, 0.32, 0.42));
    assert_approx_eq(fluid.reflection_strength, 0.85);
    assert_approx_eq(fluid.reflection_roughness, 0.06);
    assert_approx_eq(fluid.surface_height_at(Vec3::new(3.0, 0.0, 0.0), 99.0), 1.0);
}

#[test]
fn fluid_volume_exports_reflective_render_material_params() {
    let fluid = FluidVolumeComponentData {
        water_tint: Vec3::new(0.02, 0.24, 0.4),
        water_alpha: 0.6,
        reflection_strength: 0.9,
        reflection_roughness: 0.03,
        fresnel_power: 6.0,
        absorption_tint: Vec3::new(0.0, 0.1, 0.2),
        ..FluidVolumeComponentData::default()
    };

    let (base_color, metallic, roughness, emissive) = fluid.render_material_params();

    assert_approx_eq(base_color[3], 0.6);
    assert_approx_eq(metallic, 0.9);
    assert_approx_eq(roughness, 0.04);
    assert!(base_color[2] > base_color[0]);
    assert!(emissive[0] > 0.0);
}

#[test]
fn buoyancy_probe_set_defaults_to_four_corner_probes() {
    let probes = BuoyancyProbeSetComponentData::default();

    assert_eq!(probes.probes.len(), 4);
    assert!(probes.buoyancy > 0.0);
    assert!(probes.damping > 0.0);
    assert!(probes.angular_response > 0.0);
}

fn assert_approx_eq(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1e-4,
        "expected {expected}, got {actual}"
    );
}
