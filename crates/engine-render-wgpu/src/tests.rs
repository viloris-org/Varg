use crate::{device::*, meshes::*, post::*, render::*, scene_uniforms::*, shaders::*, uniforms::*};
use engine_render::{RenderLight, RenderLightKind, RenderWorld};

#[test]
fn bloom_mips_are_recreated_when_either_dimension_changes() {
    assert!(bloom_mips_match(1280, 720, 1280, 720));
    assert!(!bloom_mips_match(1280, 720, 1280, 900));
    assert!(!bloom_mips_match(1280, 720, 1440, 720));
}

#[test]
fn low_latency_present_mode_prefers_mailbox_then_immediate() {
    assert_eq!(
        select_present_mode(
            &[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox],
            engine_render::PresentStrategy::LowLatency,
        ),
        wgpu::PresentMode::Mailbox
    );
    assert_eq!(
        select_present_mode(
            &[wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate],
            engine_render::PresentStrategy::LowLatency,
        ),
        wgpu::PresentMode::Immediate
    );
}

#[test]
fn frame_pipeline_plan_preserves_compiled_pass_order() {
    let mut builder = engine_render::RenderGraphBuilder::new();
    let shadow = builder.add_pass("shadow");
    let forward = builder.add_pass("forward");
    let post = builder.add_pass("post");
    builder.order_before(shadow, forward);
    builder.order_before(forward, post);
    let plan = FramePipelinePlan::from_graph(&builder.build());
    assert_eq!(
        plan.steps,
        vec![
            FramePipelineStep::Shadow,
            FramePipelineStep::Forward,
            FramePipelineStep::Post,
        ]
    );
}

#[test]
fn frame_pipeline_plan_recognizes_hybrid_deferred_boundaries() {
    let mut builder = engine_render::RenderGraphBuilder::new();
    let shadow = builder.add_pass("shadow");
    let gbuffer = builder.add_pass("gbuffer");
    let deferred = builder.add_pass("deferred-lighting");
    let post = builder.add_pass("post");
    builder.order_before(shadow, gbuffer);
    builder.order_before(gbuffer, deferred);
    builder.order_before(deferred, post);
    let plan = FramePipelinePlan::from_graph(&builder.build());
    assert_eq!(
        plan.steps,
        vec![
            FramePipelineStep::Shadow,
            FramePipelineStep::GBuffer,
            FramePipelineStep::DeferredLighting,
            FramePipelineStep::Post,
        ]
    );
    assert!(plan.gbuffer);
    assert!(plan.deferred_lighting);
    assert!(!plan.forward);
}

#[test]
fn frame_resources_report_taa_only_when_resolve_bind_group_exists() {
    let resources = FrameResources {
        ssao_bg: None,
        ssao_view: None,
        ssgi_bg: None,
        ssgi_view: None,
        bloom_down_bgs: Vec::new(),
        bloom_up_bgs: Vec::new(),
        taa_bg: None,
        post_bg: None,
    };

    assert!(!resources.taa_enabled());
}

#[test]
fn dynamic_resolution_preserves_4k_output_with_scaled_internal_target() {
    assert_eq!(scaled_render_size(3840, 2160, 1.0), (3840, 2160));
    assert_eq!(scaled_render_size(3840, 2160, 0.75), (2880, 1620));
    assert_eq!(scaled_render_size(3840, 2160, 0.5), (1920, 1080));
}

#[test]
fn surface_viewport_rect_clamps_to_swapchain_bounds() {
    assert_eq!(
        SurfaceViewportRect::new(100, 50, 640, 360).clamped_to(1920, 1080),
        SurfaceViewportRect::new(100, 50, 640, 360)
    );
    assert_eq!(
        SurfaceViewportRect::new(1900, 1070, 640, 360).clamped_to(1920, 1080),
        SurfaceViewportRect::new(1900, 1070, 20, 10)
    );
    assert_eq!(
        SurfaceViewportRect::new(0, 0, 0, 0).clamped_to(0, 0),
        SurfaceViewportRect::new(0, 0, 1, 1)
    );
}

#[test]
fn surface_viewport_rect_reclamps_after_surface_resize() {
    let viewport = SurfaceViewportRect::new(1000, 700, 640, 360).clamped_to(1920, 1080);

    assert_eq!(
        viewport.clamped_to(1280, 720),
        SurfaceViewportRect::new(1000, 700, 280, 20)
    );
}

#[test]
fn fullscreen_shaders_use_oversized_triangle() {
    for shader in [SKYBOX_SHADER, POST_SHADER] {
        assert!(shader.contains("f32((vertex_index << 1u) & 2u)"));
        assert!(shader.contains("f32(vertex_index & 2u)"));
        assert!(shader.contains("uv * 2.0 - vec2<f32>(1.0)"));
        assert!(!shader.contains("f32(i32(vertex_index) - 1)"));
    }
}

#[test]
fn forward_shader_binds_and_samples_all_csm_cascades() {
    for cascade in 0..CSM_CASCADE_COUNT {
        assert!(FORWARD_SHADER.contains(&format!("var csm_shadow_{cascade}:")));
        assert!(FORWARD_SHADER.contains(&format!("textureSampleCompare(csm_shadow_{cascade}")));
    }
    assert!(FORWARD_SHADER.contains("params: vec4<f32>"));
    assert!(FORWARD_SHADER.contains("let texel = csm.params.y"));
    assert!(FORWARD_SHADER.contains("blocker_ratio"));
    assert!(FORWARD_SHADER.contains("let penumbra = mix(1.0, 6.0, blocker_ratio)"));
    assert!(!FORWARD_SHADER.contains("let texel = 1.0 / 4096.0"));
    assert!(!FORWARD_SHADER.contains("- 4.0"));
}

#[test]
fn forward_shader_selects_cascades_with_linear_camera_depth() {
    assert!(FORWARD_SHADER.contains("camera_forward: vec4<f32>"));
    assert!(FORWARD_SHADER.contains(
        "dot(input.world_position - camera.camera_position.xyz, camera.camera_forward.xyz)"
    ));
    assert!(
        !FORWARD_SHADER.contains(
            "let view_pos = camera.view_projection * vec4<f32>(input.world_position, 1.0)"
        )
    );
    assert!(FORWARD_SHADER.contains("light.spot_angles.z > 0.5"));
}

#[test]
fn gpu_uniform_structs_match_wgsl_alignment() {
    assert_eq!(std::mem::size_of::<CameraUniform>(), 96);
    assert_eq!(std::mem::size_of::<PostProcessUniform>(), 80);
    assert_eq!(
        std::mem::size_of::<LightingUniform>(),
        32 + MAX_FORWARD_LIGHTS * std::mem::size_of::<ForwardLightUniform>()
    );
    assert_eq!(std::mem::size_of::<CsmUniform>(), 352);
    assert_eq!(std::mem::size_of::<FogUniform>(), 32);
}

#[test]
fn forward_and_ssgi_shaders_expose_p1_p2_temporal_lighting_inputs() {
    assert!(FORWARD_SHADER.contains("lights: array<ForwardLight, 32>"));
    assert!(SSGI_SHADER.contains("var motion_tex: texture_2d<f32>"));
    assert!(SSGI_SHADER.contains("var history_tex: texture_2d<f32>"));
    assert!(SSGI_SHADER.contains("history_uv = clamp(uv - motion"));
    assert!(SSGI_SHADER.contains("ssgi.reset_history > 0.5"));
    assert!(POST_SHADER.contains("var depth_tex: texture_depth_2d"));
    assert!(POST_SHADER.contains("fn screen_space_reflection"));
    assert!(POST_SHADER.contains("post.ssr_intensity"));
}

#[test]
fn csm_uniform_exposes_shadow_sampling_params() {
    let params = default_csm_params();

    assert_eq!(params[0], CSM_CASCADE_FADE_RANGE);
    assert_eq!(params[1], 1.0 / CSM_SHADOW_RESOLUTION as f32);
    assert!(params[2] > 0.0);
    assert!(params[3] > params[2]);
}

#[test]
fn csm_bounds_snap_to_shadow_texel_grid() {
    let (min_x, max_x, min_y, max_y) = snap_csm_bounds_to_texel_grid(-3.217, 8.911, -2.603, 4.119);
    let texel_size =
        ((8.911_f32 + 3.217).max(4.119 + 2.603) / CSM_SHADOW_RESOLUTION as f32).max(f32::EPSILON);

    for value in [min_x, max_x, min_y, max_y] {
        let snapped = value / texel_size;
        assert!((snapped - snapped.round()).abs() < 0.001);
    }
    assert!(min_x <= -3.217);
    assert!(max_x >= 8.911);
    assert!(min_y <= -2.603);
    assert!(max_y >= 4.119);
}

#[test]
fn compute_ibl_sampling_uses_explicit_lod_and_dynamic_mip_resolution() {
    assert!(!IBL_IRRADIANCE_SHADER.contains("textureSample(env_map"));
    assert!(!IBL_PREFILTER_SHADER.contains("textureSample(env_map"));
    assert!(IBL_IRRADIANCE_SHADER.contains("textureSampleLevel(env_map"));
    assert!(IBL_PREFILTER_SHADER.contains("textureSampleLevel(env_map"));
    assert!(IBL_PREFILTER_SHADER.contains("let res = params.resolution"));
}

#[test]
fn post_shader_leaves_srgb_encoding_to_the_output_attachment() {
    assert!(!POST_SHADER.contains("gamma_correct"));
    assert!(!POST_SHADER.contains("1.0 / 2.2"));
}

#[test]
fn post_shader_uses_cubic_reconstruction_and_bounded_sharpening() {
    assert!(POST_SHADER.contains("fn reconstruct_hdr"));
    assert!(POST_SHADER.contains("fn cubic_weight"));
    assert!(POST_SHADER.contains("fn sharpen_hdr"));
    assert!(POST_SHADER.contains("textureLoad(hdr_tex"));
    assert!(POST_SHADER.contains("clamp(center + detail"));
}

#[test]
fn taa_shader_reprojects_and_clamps_history_for_antialiasing() {
    assert!(TAA_SHADER.contains("var history_tex: texture_2d<f32>"));
    assert!(TAA_SHADER.contains("var motion_tex: texture_2d<f32>"));
    assert!(TAA_SHADER.contains("history_uv = uv - motion"));
    assert!(TAA_SHADER.contains("post.taa_enabled < 0.5"));
    assert!(TAA_SHADER.contains("clamp(history, neighborhood_min, neighborhood_max)"));
    assert!(TAA_SHADER.contains("post.taa_history_weight"));
    assert!(POST_SHADER.contains("let hdr = resolve_hdr(input.uv)"));
    assert!(!POST_SHADER.contains("fn fxaa_hdr"));
}

#[test]
fn cube_has_24_vertices_and_36_indices() {
    let (verts, indices) = generate_cube();
    assert_eq!(
        verts.len(),
        24,
        "cube must have 24 vertices with hard normals"
    );
    assert_eq!(
        indices.len(),
        36,
        "cube must have 36 indices (6 faces × 2 triangles × 3)"
    );
}

#[test]
fn cube_vertices_have_correct_data() {
    let (verts, _indices) = generate_cube();
    // Front face vertices should have normal +Z
    for v in &verts[0..4] {
        assert!(
            (v.normal[2] - 1.0).abs() < 0.001,
            "front face normal should be +Z"
        );
    }
    // Back face vertices should have normal -Z
    for v in &verts[4..8] {
        assert!(
            (v.normal[2] + 1.0).abs() < 0.001,
            "back face normal should be -Z"
        );
    }
}

#[test]
fn sphere_generates_expected_counts() {
    let (verts, indices) = generate_sphere(8);
    let expected_verts = (8 + 1) * (16 + 1); // lat+1 × lon+1
    let expected_indices = 8 * 16 * 6; // lat × lon × 6
    assert_eq!(verts.len(), expected_verts as usize);
    assert_eq!(indices.len(), expected_indices as usize);
}

#[test]
fn sphere_min_segments_clamped() {
    let (verts, _) = generate_sphere(1);
    // Min segments is 3, so (3+1)*(6+1) = 28
    assert_eq!(verts.len(), 28);
}

#[test]
fn plane_has_4_vertices_and_6_indices() {
    let (verts, indices) = generate_plane();
    assert_eq!(verts.len(), 4);
    assert_eq!(indices.len(), 6);
    // All normals point up
    for v in &verts {
        assert!((v.normal[2] - 1.0).abs() < 0.001);
    }
}

#[test]
fn debug_mesh_enum_variants() {
    // Verify the enum can be constructed and matched
    let cube = DebugMesh::Cube;
    let sphere = DebugMesh::Sphere(8);
    let plane = DebugMesh::Plane;
    assert_eq!(cube, DebugMesh::Cube);
    assert_eq!(sphere, DebugMesh::Sphere(8));
    assert_eq!(plane, DebugMesh::Plane);
}

#[test]
fn packs_scene_lights_into_forward_uniform() {
    let light = RenderLight {
        object: engine_core::EntityId::from_u128(7),
        transform: engine_core::math::Transform {
            translation: engine_core::math::Vec3::new(1.0, 2.0, 3.0),
            rotation: engine_core::math::Quat::IDENTITY,
            scale: engine_core::math::Vec3::ONE,
        },
        kind: RenderLightKind::Point,
        color: engine_core::math::Vec3::new(0.5, 0.75, 1.0),
        intensity: 3.0,
        range: 12.0,
        spot_angle: 45.0,
    };
    let world = RenderWorld {
        camera: None,
        objects: Vec::new(),
        sprites: Vec::new(),
        lights: vec![light],
        particles: vec![],
        particle_emitters: vec![],
        skybox: None,
        fog: None,
        lighting_mode: Default::default(),
        global_illumination: Default::default(),
        shadow_virtualization: Default::default(),
        ..RenderWorld::default()
    };

    let uniform = lighting_uniform_from_world(&world);

    assert_eq!(uniform.params[0], 1);
    assert_eq!(uniform.lights[0].position_type, [1.0, 2.0, 3.0, 1.0]);
    assert_eq!(uniform.lights[0].color_intensity, [0.5, 0.75, 1.0, 3.0]);
    assert_eq!(uniform.lights[0].direction_range[3], 12.0);
}

#[test]
fn mesh_batches_group_objects_without_per_object_mesh_names() {
    let world = RenderWorld {
        camera: None,
        objects: vec![
            test_render_object(1, "debug/cube"),
            test_render_object(2, "debug/cube"),
            test_render_object(3, "debug/sphere"),
        ],
        sprites: Vec::new(),
        lights: Vec::new(),
        particles: Vec::new(),
        particle_emitters: Vec::new(),
        skybox: None,
        fog: None,
        lighting_mode: Default::default(),
        global_illumination: Default::default(),
        shadow_virtualization: Default::default(),
        ..RenderWorld::default()
    };

    let batches = test_mesh_batches(&world);

    assert_eq!(batch_len(&batches, "debug/cube"), Some(2));
    assert_eq!(batch_len(&batches, "debug/sphere"), Some(1));
    assert_eq!(batches.len(), 2);
}

#[test]
fn mesh_batches_merge_particles_with_plane_objects() {
    let world = RenderWorld {
        camera: None,
        objects: vec![test_render_object(1, "debug/plane")],
        sprites: Vec::new(),
        lights: Vec::new(),
        particles: vec![engine_render::RenderParticle {
            object: engine_core::EntityId::from_u128(2),
            transform: engine_core::math::Transform::IDENTITY,
            color: [1.0, 1.0, 1.0, 1.0],
            age_fraction: 0.5,
        }],
        particle_emitters: Vec::new(),
        skybox: None,
        fog: None,
        lighting_mode: Default::default(),
        global_illumination: Default::default(),
        shadow_virtualization: Default::default(),
        ..RenderWorld::default()
    };

    let batches = test_mesh_batches(&world);

    assert_eq!(batch_len(&batches, "debug/plane"), Some(2));
}

#[test]
fn mesh_batches_render_sprites_as_colored_planes() {
    let mut transform = engine_core::math::Transform::IDENTITY;
    transform.rotation = engine_core::math::Quat::from_euler_deg(0.0, 0.0, 90.0);
    let world = RenderWorld {
        sprites: vec![engine_render::RenderSprite {
            object: engine_core::EntityId::from_u128(2),
            transform,
            texture: None,
            color: [0.2, 0.4, 0.6, 0.5],
            order_in_layer: 7,
            layer: "Default".to_string(),
            flip_h: true,
            flip_v: false,
        }],
        ..RenderWorld::default()
    };

    let batches = test_mesh_batches(&world);
    let instances = &batches
        .iter()
        .find(|(mesh, _)| mesh == "debug/plane")
        .unwrap()
        .1;

    assert_eq!(instances.len(), 1);
    assert!(instances[0].scale[0] < 0.0);
    assert_eq!(instances[0].color, [0.2, 0.4, 0.6, 0.5]);
    assert_ne!(instances[0].rotation, [0.0, 0.0, 0.0, 1.0]);
}

fn batch_len(batches: &[(String, Vec<Instance>)], mesh: &str) -> Option<usize> {
    batches
        .iter()
        .find(|(name, _)| name == mesh)
        .map(|(_, instances)| instances.len())
}

fn test_mesh_batches(world: &RenderWorld) -> Vec<(String, Vec<Instance>)> {
    use std::collections::HashMap;
    let batch_capacity = (world.objects.len()
        + usize::from(!world.sprites.is_empty())
        + usize::from(!world.particles.is_empty()))
    .min(32);
    let mut batches: HashMap<&str, Vec<Instance>> = HashMap::with_capacity(batch_capacity);
    for object in &world.objects {
        let (color, metallic, roughness, emissive) = test_pbr(&object.material);
        let t = object.transform;
        let mesh = if object.mesh.is_empty() {
            "debug/cube"
        } else {
            object.mesh.as_str()
        };
        batches.entry(mesh).or_default().push(Instance {
            offset: [t.translation.x, t.translation.y, t.translation.z],
            scale: [
                t.scale.x.max(0.05),
                t.scale.y.max(0.05),
                t.scale.z.max(0.05),
            ],
            color,
            rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
            metallic,
            roughness,
            emissive,
            receive_shadows: 1.0,
        });
    }
    if !world.sprites.is_empty() {
        let mut sprites = world.sprites.iter().collect::<Vec<_>>();
        sprites.sort_by(|left, right| {
            left.layer
                .cmp(&right.layer)
                .then(left.order_in_layer.cmp(&right.order_in_layer))
        });
        let sprite_instances = sprites.into_iter().map(|sprite| {
            let t = sprite.transform;
            let x = t.scale.x.abs().max(0.01) * if sprite.flip_h { -1.0 } else { 1.0 };
            let y = t.scale.y.abs().max(0.01) * if sprite.flip_v { -1.0 } else { 1.0 };
            Instance {
                offset: [
                    t.translation.x,
                    t.translation.y,
                    t.translation.z + sprite.order_in_layer as f32 * 0.0001,
                ],
                scale: [x, y, t.scale.z.abs().max(0.01)],
                color: sprite.color,
                rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                metallic: 0.0,
                roughness: 0.5,
                emissive: [0.0; 3],
                receive_shadows: 1.0,
            }
        });
        batches
            .entry("debug/plane")
            .or_default()
            .extend(sprite_instances);
    }
    if !world.particles.is_empty() {
        let particle_instances: Vec<Instance> = world
            .particles
            .iter()
            .map(|particle| {
                let t = particle.transform;
                Instance {
                    offset: [t.translation.x, t.translation.y, t.translation.z],
                    scale: [
                        t.scale.x.max(0.01),
                        t.scale.y.max(0.01),
                        t.scale.z.max(0.01),
                    ],
                    color: particle.color,
                    rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                    metallic: 0.0,
                    roughness: 0.5,
                    emissive: [0.0; 3],
                    receive_shadows: 1.0,
                }
            })
            .collect();
        batches
            .entry("debug/plane")
            .or_default()
            .extend(particle_instances);
    }
    batches
        .into_iter()
        .map(|(mesh, instances)| (mesh.to_owned(), instances))
        .collect()
}

fn test_pbr(material: &str) -> ([f32; 4], f32, f32, [f32; 3]) {
    if material.contains("debug") {
        ([0.2, 0.65, 1.0, 1.0], 0.0, 0.5, [0.0, 0.0, 0.0])
    } else if material.contains("error") {
        ([1.0, 0.2, 0.45, 1.0], 0.0, 0.5, [0.0, 0.0, 0.0])
    } else {
        ([0.82, 0.86, 0.72, 1.0], 0.0, 0.5, [0.0, 0.0, 0.0])
    }
}

fn test_render_object(id: u128, mesh: &str) -> engine_render::RenderObject {
    engine_render::RenderObject {
        object: engine_core::EntityId::from_u128(id),
        transform: engine_core::math::Transform::IDENTITY,
        mesh: mesh.to_owned(),
        material: "debug/material".to_owned(),
        casts_shadows: true,
        receive_shadows: true,
        bounds: engine_render::RenderBounds::default(),
        lods: Vec::new(),
    }
}

#[test]
fn uses_fallback_directional_light_when_scene_has_no_lights() {
    let uniform = lighting_uniform_from_world(&RenderWorld::default());

    assert_eq!(uniform.params[0], 1);
    assert_eq!(uniform.lights[0].position_type[3], 0.0);
    assert_eq!(uniform.lights[0].color_intensity[3], 1.0);
    assert_eq!(uniform.lights[0].spot_angles[2], 1.0);
}

#[test]
fn selects_directional_budget_then_highest_scored_local_lights() {
    let camera = engine_render::RenderCamera {
        object: engine_core::EntityId::from_u128(1),
        transform: engine_core::math::Transform::IDENTITY,
        projection: engine_render::RenderProjection::Perspective,
        vertical_fov_degrees: 60.0,
        near: 0.1,
        far: 50.0,
        look_at_target: None,
    };
    let mut lights = vec![
        test_light(
            2,
            RenderLightKind::Directional,
            engine_core::math::Vec3::ZERO,
            1.0,
            1.0,
        ),
        test_light(
            3,
            RenderLightKind::Directional,
            engine_core::math::Vec3::ZERO,
            5.0,
            1.0,
        ),
        test_light(
            4,
            RenderLightKind::Directional,
            engine_core::math::Vec3::ZERO,
            3.0,
            1.0,
        ),
        test_light(
            5,
            RenderLightKind::Point,
            engine_core::math::Vec3::new(100.0, 0.0, 0.0),
            100.0,
            4.0,
        ),
    ];
    for index in 0..48 {
        lights.push(test_light(
            10 + index,
            RenderLightKind::Point,
            engine_core::math::Vec3::new(2.0 + index as f32, 0.0, 0.0),
            1.0,
            5.0,
        ));
    }
    let world = RenderWorld {
        camera: Some(camera),
        objects: Vec::new(),
        sprites: Vec::new(),
        lights,
        particles: Vec::new(),
        particle_emitters: Vec::new(),
        skybox: None,
        fog: None,
        lighting_mode: Default::default(),
        global_illumination: Default::default(),
        shadow_virtualization: Default::default(),
        ..RenderWorld::default()
    };

    let selected = select_forward_lights(&world);

    assert_eq!(selected.len(), MAX_FORWARD_LIGHTS);
    assert_eq!(selected[0].object, engine_core::EntityId::from_u128(3));
    assert_eq!(selected[1].object, engine_core::EntityId::from_u128(4));
    assert!(
        selected
            .iter()
            .all(|light| light.object != engine_core::EntityId::from_u128(5))
    );
    assert!(
        selected
            .iter()
            .any(|light| light.object == engine_core::EntityId::from_u128(10))
    );

    let uniform = lighting_uniform_from_world(&world);
    assert_eq!(uniform.lights[0].spot_angles[2], 1.0);
    assert_eq!(uniform.lights[1].spot_angles[2], 0.0);
    assert_eq!(
        primary_directional_light(&world).unwrap().object,
        engine_core::EntityId::from_u128(3)
    );
}

fn test_light(
    id: u128,
    kind: RenderLightKind,
    translation: engine_core::math::Vec3,
    intensity: f32,
    range: f32,
) -> RenderLight {
    RenderLight {
        object: engine_core::EntityId::from_u128(id),
        transform: engine_core::math::Transform {
            translation,
            rotation: engine_core::math::Quat::IDENTITY,
            scale: engine_core::math::Vec3::ONE,
        },
        kind,
        color: engine_core::math::Vec3::ONE,
        intensity,
        range,
        spot_angle: 45.0,
    }
}

#[test]
fn grid_generates_404_vertices() {
    let vertices = generate_grid();
    assert_eq!(
        vertices.len(),
        404,
        "grid must have 404 vertices (202 lines × 2)"
    );
}

#[test]
fn grid_vertices_lie_on_y_zero() {
    let vertices = generate_grid();
    for v in &vertices {
        assert!(
            (v.position[1] - 0.0).abs() < f32::EPSILON,
            "every grid vertex must lie on Y=0"
        );
    }
}

#[test]
fn grid_major_lines_have_alpha_0_35() {
    let vertices = generate_grid();
    assert!((vertices[0].uv[0] - 0.35).abs() < 0.001);
    assert!((vertices[202].uv[0] - 0.35).abs() < 0.001);
}

#[test]
fn grid_minor_lines_have_alpha_0_15() {
    let vertices = generate_grid();
    assert!((vertices[2].uv[0] - 0.15).abs() < 0.001);
}

#[test]
fn grid_vertices_within_extent() {
    let vertices = generate_grid();
    for v in &vertices {
        assert!(v.position[0].abs() <= 50.0 + f32::EPSILON);
        assert!(v.position[2].abs() <= 50.0 + f32::EPSILON);
    }
}

#[test]
fn probe_volume_generates_enabled_irradiance_grid() {
    let world = RenderWorld {
        global_illumination: engine_render::RenderGlobalIllumination::ProbeVolume(
            engine_render::RenderProbeVolume {
                counts: [2, 2, 2],
                extent: engine_core::math::Vec3::new(2.0, 2.0, 2.0),
                intensity: 1.5,
                ..engine_render::RenderProbeVolume::default()
            },
        ),
        lights: vec![RenderLight {
            object: engine_core::EntityId::from_u128(42),
            transform: engine_core::math::Transform {
                translation: engine_core::math::Vec3::new(0.0, 2.0, 0.0),
                rotation: engine_core::math::Quat::IDENTITY,
                scale: engine_core::math::Vec3::ONE,
            },
            kind: RenderLightKind::Point,
            color: engine_core::math::Vec3::new(1.0, 0.5, 0.25),
            intensity: 4.0,
            range: 10.0,
            spot_angle: 45.0,
        }],
        ..RenderWorld::default()
    };

    let (uniform, probes) = gi_probe_uniform_and_data(&world);

    assert_eq!(uniform.params[0], 1);
    assert_eq!(uniform.params[1], 8);
    assert_eq!(uniform.counts_intensity, [2.0, 2.0, 2.0, 1.5]);
    assert_eq!(probes.len(), 8);
    assert!(probes.iter().any(|probe| probe.irradiance[0] > 0.03));
}
