use crate::math::{
    IDENTITY_MAT4, mul_mat4_vec3, orthographic_rh, orthographic_rh_custom, perspective_rh,
};
use crate::{
    batches::{
        RenderBatchInstance, active_csm_cascade_count, csm_cascade_depth_range,
        shadow_ranges_for_depth_bounds, shadow_ranges_for_instances,
        sphere_intersects_cascade_clip,
    },
    device::*,
    light_preparation::*,
    meshes::*,
    post::*,
    render::*,
    shaders::*,
    uniforms::*,
};
use engine_render::{
    RenderGlobalIllumination, RenderLight, RenderLightKind, RenderObject, RenderWorld,
};

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
fn skybox_uses_camera_rotation_without_translation() {
    let world = RenderWorld {
        camera: Some(engine_render::RenderCamera {
            object: engine_core::EntityId::from_u128(42),
            transform: engine_core::math::Transform {
                translation: engine_core::math::Vec3::new(12.0, 5.0, -8.0),
                ..engine_core::math::Transform::IDENTITY
            },
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 100.0,
            look_at_target: Some(engine_core::math::Vec3::new(12.0, 5.0, -9.0)),
        }),
        ..RenderWorld::default()
    };

    let skybox = crate::scene_uniforms::skybox_uniform_from_world(&world, false);

    assert_eq!(skybox.view_rotation_only[3], [0.0, 0.0, 0.0, 1.0]);
    assert!(SKYBOX_SHADER.contains("vec4<f32>(view_ray, 0.0)"));
}

#[test]
fn forward_shader_binds_and_samples_all_csm_cascades() {
    for cascade in 0..CSM_CASCADE_COUNT {
        assert!(FORWARD_SHADER.contains(&format!("var csm_shadow_{cascade}:")));
        assert!(FORWARD_SHADER.contains(&format!("textureSampleCompare(csm_shadow_{cascade}")));
    }
    assert!(FORWARD_SHADER.contains("params: vec4<f32>"));
    assert!(FORWARD_SHADER.contains("let texel = csm.params.y"));
    assert!(FORWARD_SHADER.contains("let filter_radius = texel * mix(1.15, 2.35"));
    assert!(FORWARD_SHADER.contains("corner_weight"));
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
fn forward_shader_uses_godot_style_local_light_falloff() {
    assert!(FORWARD_SHADER.contains("fn distance_attenuation"));
    assert!(
        FORWARD_SHADER.contains("normalized_distance = normalized_distance * normalized_distance")
    );
    assert!(FORWARD_SHADER.contains("pow(effective_distance, -max(decay, 0.01))"));
    assert!(FORWARD_SHADER.contains("fn spot_cone_attenuation"));
    assert!(FORWARD_SHADER.contains("let spot_rim = max(0.0001"));
    assert!(!FORWARD_SHADER.contains("let falloff = max(1.0 - effective_distance / range"));
    assert!(!FORWARD_SHADER.contains("smoothstep(light.spot_angles.y, light.spot_angles.x"));
}

#[test]
fn forward_shader_uses_burley_diffuse_and_roughness_fresnel() {
    assert!(FORWARD_SHADER.contains("fn diffuse_burley"));
    assert!(FORWARD_SHADER.contains("fd90_minus_1"));
    assert!(FORWARD_SHADER.contains("fresnel_schlick_roughness"));
    assert!(FORWARD_SHADER.contains("fresnel_schlick_f90"));
    assert!(FORWARD_SHADER.contains("let f90 = clamp(dot(f0, vec3<f32>(16.5)), metallic, 1.0)"));
    assert!(FORWARD_SHADER.contains("kd * base_color * diffuse_burley"));
    assert!(!FORWARD_SHADER.contains("let diffuse = kd * base_color / PI;"));
    assert!(
        !FORWARD_SHADER
            .contains("var radiance = (diffuse + specular) * light_color * intensity * ndotl")
    );
}

#[test]
fn gpu_uniform_structs_match_wgsl_alignment() {
    assert_eq!(std::mem::size_of::<CameraUniform>(), 96);
    assert_eq!(std::mem::size_of::<PostProcessUniform>(), 224);
    assert_eq!(
        std::mem::size_of::<LightingUniform>(),
        32 + MAX_FORWARD_LIGHTS * std::mem::size_of::<ForwardLightUniform>()
    );
    assert_eq!(std::mem::size_of::<CsmUniform>(), 368);
    assert_eq!(
        std::mem::size_of::<LocalShadowUniform>(),
        64 * MAX_LOCAL_SHADOWS + 16 * MAX_LOCAL_SHADOWS + 16
    );
    assert_eq!(std::mem::size_of::<ClusterUniform>(), 32);
    assert_eq!(std::mem::size_of::<ClusterRange>(), 16);
    assert_eq!(std::mem::size_of::<FogUniform>(), 32);
}

#[test]
fn forward_shader_samples_spot_shadow_atlas() {
    assert!(FORWARD_SHADER.contains("struct LocalShadowUniform"));
    assert!(FORWARD_SHADER.contains("@group(0) @binding(17) var<uniform> local_shadows"));
    assert!(FORWARD_SHADER.contains("@group(0) @binding(18) var local_shadow_atlas"));
    assert!(FORWARD_SHADER.contains("fn sample_spot_shadow"));
    assert!(FORWARD_SHADER.contains("textureSampleCompare(local_shadow_atlas"));
    assert!(FORWARD_SHADER.contains("light_type > 1.5 && light.quality.z >= 0.0"));
}

#[test]
fn spot_shadow_lights_receive_stable_atlas_slots() {
    let mut lights = Vec::new();
    for index in 0..6 {
        let mut light = RenderLight {
            object: engine_core::EntityId::from_u128(index + 1),
            transform: engine_core::math::Transform::IDENTITY,
            kind: RenderLightKind::Spot,
            color: engine_core::math::Vec3::ONE,
            intensity: 1.0,
            range: 10.0,
            spot_angle: 40.0,
            settings: Default::default(),
        };
        light.settings.casts_shadow = index != 1;
        lights.push(light);
    }
    let world = RenderWorld {
        lights,
        ..RenderWorld::default()
    };
    let selected = select_local_shadow_lights(&world);
    let local_shadow = local_shadow_uniform_from_world(&world);
    let packed = lighting_uniform_from_world(&world);

    assert_eq!(selected.len(), MAX_LOCAL_SHADOWS);
    assert_eq!(local_shadow.params[0], MAX_LOCAL_SHADOWS as f32);
    assert_eq!(local_shadow.atlas_rects[0], [0.0, 0.0, 0.5, 0.5]);
    assert_eq!(local_shadow.atlas_rects[1], [0.5, 0.0, 0.5, 0.5]);
    assert_eq!(packed.lights[0].quality[2], 0.0);
    assert_eq!(packed.lights[1].quality[2], -1.0);
    assert_eq!(packed.lights[2].quality[2], 1.0);
}

#[test]
fn clustered_lighting_builds_tile_ranges_for_selected_lights() {
    let lights = vec![
        RenderLight {
            object: engine_core::EntityId::from_u128(1),
            transform: engine_core::math::Transform::IDENTITY,
            kind: RenderLightKind::Directional,
            color: engine_core::math::Vec3::ONE,
            intensity: 1.0,
            range: 0.0,
            spot_angle: 30.0,
            settings: Default::default(),
        },
        RenderLight {
            object: engine_core::EntityId::from_u128(2),
            transform: engine_core::math::Transform::IDENTITY,
            kind: RenderLightKind::Point,
            color: engine_core::math::Vec3::ONE,
            intensity: 2.0,
            range: 8.0,
            spot_angle: 30.0,
            settings: Default::default(),
        },
    ];
    let world = RenderWorld {
        lights,
        ..RenderWorld::default()
    };
    let (uniform, clustered_lights, ranges, indices) =
        cluster_lighting_data_from_world(&world, &IDENTITY_MAT4, 1600, 900);

    assert_eq!(uniform.layout, [16.0, 9.0, 100.0, 100.0]);
    assert_eq!(clustered_lights.len(), 2);
    assert_eq!(ranges.len(), CLUSTER_TILE_COUNT);
    assert!(ranges.iter().any(|range| range.count > 0));
    assert!(!indices.is_empty());
    assert!(
        indices
            .iter()
            .all(|index| (*index as usize) < clustered_lights.len())
    );
}

#[test]
fn clustered_lighting_scores_and_culls_spotlights_by_cone_influence() {
    let mut point = test_light(
        1,
        RenderLightKind::Point,
        engine_core::math::Vec3::ZERO,
        1.0,
        10.0,
    );
    point.settings.source_radius = 0.0;
    let mut spot = test_light(
        2,
        RenderLightKind::Spot,
        engine_core::math::Vec3::ZERO,
        1.0,
        10.0,
    );
    spot.spot_angle = 30.0;
    spot.settings.source_radius = 0.0;

    let point_score = local_light_score(&RenderWorld::default(), &point).unwrap();
    let spot_score = local_light_score(&RenderWorld::default(), &spot).unwrap();

    assert!(spot_score < point_score * 0.1);
    assert!(local_light_distance_attenuation(9.5, 10.0, 2.0, 0.0) < 0.02);
    assert!(
        local_light_spot_attenuation(&spot, engine_core::math::Vec3::new(0.0, 0.0, -1.0)) > 0.99
    );
    assert!(
        local_light_spot_attenuation(&spot, engine_core::math::Vec3::new(1.0, 0.0, 0.0)) <= 0.001
    );
}

#[test]
fn forward_and_ssgi_shaders_expose_p1_p2_temporal_lighting_inputs() {
    assert!(FORWARD_SHADER.contains("lights: array<ForwardLight, 32>"));
    assert!(FORWARD_SHADER.contains("var<storage, read> cluster_lights"));
    assert!(FORWARD_SHADER.contains("var<storage, read> cluster_ranges"));
    assert!(FORWARD_SHADER.contains("cluster_light_indices"));
    assert!(FORWARD_SHADER.contains("cluster_index_for_fragment(input.position)"));
    assert!(!FORWARD_SHADER.contains("for (var i: u32 = 0u; i < lighting.params.x"));
    assert!(SSGI_SHADER.contains("var motion_tex: texture_2d<f32>"));
    assert!(SSGI_SHADER.contains("var history_tex: texture_2d<f32>"));
    assert!(SSGI_SHADER.contains("history_uv = clamp(uv - motion"));
    assert!(SSGI_SHADER.contains("ssgi.reset_history > 0.5"));
    assert!(SSGI_SHADER.contains("clamped_history = clamp(history"));
    assert!(SSGI_SHADER.contains("min(sample_radiance, vec3<f32>(8.0))"));
    assert!(POST_SHADER.contains("var depth_tex: texture_depth_2d"));
    assert!(POST_SHADER.contains("fn screen_space_reflection"));
    assert!(POST_SHADER.contains("post.ssr_intensity"));
    assert!(POST_SHADER.contains("reconstruct_world_position"));
    assert!(POST_SHADER.contains("post.view_projection * vec4<f32>(ray_pos, 1.0)"));
}

#[test]
fn screen_space_reflections_are_enabled_with_camera_matrices() {
    let render_source = include_str!("render.rs");
    let constructor_source = include_str!("constructors.rs");
    let diagnostics_source = include_str!("diagnostics.rs");

    assert!(render_source.contains("inv_view_projection"));
    assert!(render_source.contains("view_projection: camera.view_projection"));
    assert!(render_source.contains("diagnostics::disable_ssr()"));
    assert!(render_source.contains("world.resolved_environment()"));
    assert!(render_source.contains("environment.ssr_intensity.max(0.0)"));
    assert!(render_source.contains("ssr_intensity,"));
    assert!(constructor_source.contains("ssr_enabled: 1.0"));
    assert!(constructor_source.contains("ssr_intensity: 0.22"));
    assert!(diagnostics_source.contains("VARG_RENDER_DISABLE_SSR"));
}

#[test]
fn render_diagnostics_expose_feature_disable_switches() {
    let render_source = include_str!("render.rs");
    let device_trait_source = include_str!("device_trait.rs");
    let diagnostics_source = include_str!("diagnostics.rs");

    assert!(render_source.contains("diagnostics::disable_grid()"));
    assert!(render_source.contains("diagnostics::disable_csm_shadows()"));
    assert!(render_source.contains("diagnostics::disable_ssao()"));
    assert!(device_trait_source.contains("diagnostics::disable_ssao()"));
    assert!(diagnostics_source.contains("VARG_RENDER_DISABLE_GRID"));
    assert!(diagnostics_source.contains("VARG_RENDER_DISABLE_CSM_SHADOWS"));
    assert!(diagnostics_source.contains("VARG_RENDER_DISABLE_SSAO"));
    assert!(diagnostics_source.contains("VARG_RENDER_DISABLE_SSR"));
}

#[test]
fn ssao_shader_uses_depth_aware_contact_occlusion() {
    assert!(SSAO_SHADER.contains("let view_scale = mix(18.0, 95.0, 1.0 - depth)"));
    assert!(SSAO_SHADER.contains("let radius_px = clamp(params.radius * view_scale"));
    assert!(SSAO_SHADER.contains("let horizon = smoothstep(params.bias"));
    assert!(!SSAO_SHADER.contains("let radius_px = params.radius * params.width"));
}

#[test]
fn csm_uniform_exposes_shadow_sampling_params() {
    let params = default_csm_params();

    assert_eq!(params[0], CSM_CASCADE_FADE_RANGE);
    assert_eq!(params[1], 1.0 / CSM_SHADOW_RESOLUTION as f32);
    assert!(params[2] >= 0.002);
    assert!(params[3] > params[2]);
}

#[test]
fn forward_shader_applies_shadows_to_direct_directional_light_only() {
    assert!(FORWARD_SHADER.contains("radiance = radiance * shadow_factor;"));
    assert!(!FORWARD_SHADER.contains("direct_shadow"));
}

#[test]
fn forward_shader_flips_csm_y_for_texture_sampling() {
    assert!(FORWARD_SHADER.contains("0.5 - ndc.y * 0.5"));
    assert!(FORWARD_SHADER.contains("0.5 - ndc2.y * 0.5"));
}

#[test]
fn forward_shader_rejects_invalid_next_cascade_before_boundary_blend() {
    assert!(FORWARD_SHADER.contains("uv2.x < 0.0"));
    assert!(FORWARD_SHADER.contains("return base_shadow;"));
    assert!(
        FORWARD_SHADER
            .contains("apply_csm_distance_fade(min(base_shadow, next_shadow), view_depth)")
    );
    assert!(FORWARD_SHADER.contains("view_depth >= csm.cascade_splits.w"));
    assert!(FORWARD_SHADER.contains("next_split_is_distinct"));
    assert!(!FORWARD_SHADER.contains("csm.cascade_vps[4] * vec4<f32>(shadow_pos, 1.0)"));
}

#[test]
fn forward_shader_fades_csm_to_lit_at_shadow_distance() {
    assert!(FORWARD_SHADER.contains("fade_params: vec4<f32>"));
    assert!(FORWARD_SHADER.contains("fn apply_csm_distance_fade"));
    assert!(FORWARD_SHADER.contains("let fade_start = min(csm.fade_params.x, csm.fade_params.y)"));
    assert!(FORWARD_SHADER.contains("return mix(1.0, shadow_factor, visibility_weight);"));
    assert!(FORWARD_SHADER.contains("apply_csm_distance_fade(base_shadow, view_depth)"));
}

#[test]
fn forward_shader_offsets_csm_receivers_to_reduce_self_shadowing() {
    assert!(FORWARD_SHADER.contains("let receiver_offset = n *"));
    assert!(FORWARD_SHADER.contains("let shadow_pos = world_pos + receiver_offset"));
    assert!(FORWARD_SHADER.contains("vec4<f32>(shadow_pos, 1.0)"));
    assert!(
        FORWARD_SHADER.contains(
            "sample_csm_shadow(input.world_position, view_depth, tbn_n, shadow_light_dir)"
        )
    );
}

#[test]
fn shadow_pipeline_uses_front_face_culling_and_depth_bias() {
    let constructor_source = include_str!("constructors.rs");

    assert!(constructor_source.contains("label: Some(\"varg shadow pipeline\")"));
    assert!(constructor_source.contains("cull_mode: Some(wgpu::Face::Front)"));
    assert!(constructor_source.contains("constant: 8"));
    assert!(constructor_source.contains("slope_scale: 4.0"));
}

#[test]
fn shadow_shader_skips_low_profile_horizontal_receivers_as_casters() {
    let constructor_source = include_str!("constructors.rs");

    assert!(SHADOW_SHADER.contains("let low_profile = input.scale.y < 0.08"));
    assert!(SHADOW_SHADER.contains("let horizontal_face = abs(world_normal.y) > 0.92"));
    assert!(SHADOW_SHADER.contains("discard;"));
    assert!(constructor_source.contains("entry_point: Some(\"fs_main\")"));
    assert!(constructor_source.contains("targets: &[]"));
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
fn shadow_caster_ranges_coalesce_contiguous_depth_overlaps() {
    let ranges = shadow_ranges_for_depth_bounds(
        &[(0.0, 3.0), (4.0, 7.0), (20.0, 25.0), (8.0, 9.0)],
        10,
        0.0,
        10.0,
    );

    assert_eq!(ranges, vec![10..12, 13..14]);
}

#[test]
fn shadow_caster_ranges_reject_instances_outside_cascade_clip() {
    let instances = [
        test_shadow_instance(engine_core::math::Vec3::ZERO, 0.25, 4.0, 6.0),
        test_shadow_instance(engine_core::math::Vec3::new(2.5, 0.0, 0.0), 0.1, 4.0, 6.0),
    ];
    let ranges = shadow_ranges_for_instances(&instances, 3, &IDENTITY_MAT4, 0.0, 10.0);

    assert_eq!(ranges, vec![3..4]);
}

#[test]
fn cascade_clip_intersection_keeps_spheres_touching_edges() {
    assert!(sphere_intersects_cascade_clip(
        &IDENTITY_MAT4,
        engine_core::math::Vec3::new(1.05, 0.0, 0.5),
        0.1,
    ));
    assert!(!sphere_intersects_cascade_clip(
        &IDENTITY_MAT4,
        engine_core::math::Vec3::new(1.2, 0.0, 0.5),
        0.1,
    ));
}

#[test]
fn csm_active_cascades_ignore_duplicate_terminal_splits() {
    let csm = CsmUniform {
        cascade_vps: [IDENTITY_MAT4; CSM_CASCADE_COUNT],
        cascade_splits: [24.0, 60.0, 60.0, 60.0],
        params: default_csm_params(),
        fade_params: default_csm_fade_params(),
    };

    assert_eq!(active_csm_cascade_count(&csm), 2);
    assert_eq!(csm_cascade_depth_range(&csm, 1), (20.0, 64.0));
}

#[test]
fn csm_cascades_overlap_across_shader_fade_range() {
    let camera_near = 0.1;
    for split_idx in 0..3 {
        let split_depth = CSM_CASCADE_SPLITS[split_idx];
        let (_, current_far) = csm_cascade_view_depth_bounds(split_idx, camera_near);
        let (next_near, _) = csm_cascade_view_depth_bounds(split_idx + 1, camera_near);

        assert!(current_far >= split_depth + CSM_CASCADE_FADE_RANGE);
        assert!(next_near <= split_depth - CSM_CASCADE_FADE_RANGE);
    }
}

#[test]
fn csm_uniform_uses_directional_light_shadow_settings() {
    let mut light = test_light(
        77,
        RenderLightKind::Directional,
        engine_core::math::Vec3::ZERO,
        3.0,
        1.0,
    );
    light.settings.shadow_max_distance = 80.0;
    light.settings.directional_shadow_splits = [0.2, 0.4, 0.7];
    light.settings.shadow_fade_start = 0.9;
    light.settings.shadow_bias = 0.004;
    light.settings.shadow_normal_bias = 0.011;
    let world = RenderWorld {
        camera: Some(engine_render::RenderCamera {
            object: engine_core::EntityId::from_u128(1),
            transform: engine_core::math::Transform {
                translation: engine_core::math::Vec3::new(0.0, 0.0, 8.0),
                rotation: engine_core::math::Quat::IDENTITY,
                scale: engine_core::math::Vec3::ONE,
            },
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 100.0,
            look_at_target: Some(engine_core::math::Vec3::ZERO),
        }),
        lights: vec![light],
        ..RenderWorld::default()
    };

    let csm = csm_uniform_from_world(&world, 16.0 / 9.0);

    assert_eq!(csm.cascade_splits, [16.0, 32.0, 56.0, 80.0]);
    assert_eq!(csm.fade_params[0], 72.0);
    assert_eq!(csm.fade_params[1], 80.0);
    assert_eq!(csm.params[2], 0.004);
    assert_eq!(csm.params[3], 0.011);
}

#[test]
fn csm_uniform_respects_directional_shadow_mode() {
    let camera = engine_render::RenderCamera {
        object: engine_core::EntityId::from_u128(1),
        transform: engine_core::math::Transform::IDENTITY,
        projection: engine_render::RenderProjection::Perspective,
        vertical_fov_degrees: 60.0,
        near: 0.1,
        far: 100.0,
        look_at_target: Some(engine_core::math::Vec3::new(0.0, 0.0, -1.0)),
    };
    let mut orthogonal = test_light(
        80,
        RenderLightKind::Directional,
        engine_core::math::Vec3::ZERO,
        3.0,
        1.0,
    );
    orthogonal.settings.shadow_max_distance = 64.0;
    orthogonal.settings.directional_shadow_mode =
        engine_render::RenderDirectionalShadowMode::Orthogonal;
    let mut parallel2 = orthogonal.clone();
    parallel2.object = engine_core::EntityId::from_u128(81);
    parallel2.settings.shadow_max_distance = 60.0;
    parallel2.settings.directional_shadow_splits = [0.4, 0.6, 0.8];
    parallel2.settings.directional_shadow_mode =
        engine_render::RenderDirectionalShadowMode::Parallel2Splits;

    let orthogonal_csm = csm_uniform_from_world(
        &RenderWorld {
            camera: Some(camera.clone()),
            lights: vec![orthogonal],
            ..RenderWorld::default()
        },
        1.0,
    );
    let parallel2_csm = csm_uniform_from_world(
        &RenderWorld {
            camera: Some(camera),
            lights: vec![parallel2],
            ..RenderWorld::default()
        },
        1.0,
    );

    assert_eq!(orthogonal_csm.cascade_splits, [64.0, 64.0, 64.0, 64.0]);
    assert_eq!(orthogonal_csm.params[0], 0.0);
    assert_eq!(parallel2_csm.cascade_splits, [24.0, 60.0, 60.0, 60.0]);
    assert_eq!(parallel2_csm.params[0], CSM_CASCADE_FADE_RANGE);
}

#[test]
fn csm_uniform_disables_split_fade_when_directional_blending_is_off() {
    let mut light = test_light(
        78,
        RenderLightKind::Directional,
        engine_core::math::Vec3::ZERO,
        3.0,
        1.0,
    );
    light.settings.directional_shadow_blend_splits = false;
    let world = RenderWorld {
        camera: Some(engine_render::RenderCamera {
            object: engine_core::EntityId::from_u128(1),
            transform: engine_core::math::Transform::IDENTITY,
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 100.0,
            look_at_target: Some(engine_core::math::Vec3::new(0.0, 0.0, -1.0)),
        }),
        lights: vec![light],
        ..RenderWorld::default()
    };

    let csm = csm_uniform_from_world(&world, 1.0);

    assert_eq!(csm.params[0], 0.0);
}

#[test]
fn shadow_orthographic_projection_maps_depth_into_compare_range() {
    let projection = orthographic_rh_custom(-2.0, 2.0, -1.0, 1.0, -60.0, 35.0);

    let near = mul_mat4_vec3(&projection, engine_core::math::Vec3::new(0.0, 0.0, -60.0));
    let mid = mul_mat4_vec3(&projection, engine_core::math::Vec3::new(0.0, 0.0, -12.5));
    let far = mul_mat4_vec3(&projection, engine_core::math::Vec3::new(0.0, 0.0, 35.0));

    assert!((near.z - 0.0).abs() < 0.0001);
    assert!((mid.z - 0.5).abs() < 0.0001);
    assert!((far.z - 1.0).abs() < 0.0001);
}

#[test]
fn camera_projections_map_depth_into_wgpu_clip_range() {
    let perspective = perspective_rh(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
    let perspective_near = mul_mat4_vec4(&perspective, [0.0, 0.0, -0.1, 1.0]);
    let perspective_far = mul_mat4_vec4(&perspective, [0.0, 0.0, -100.0, 1.0]);

    assert!((perspective_near[2] / perspective_near[3]).abs() < 0.0001);
    assert!((perspective_far[2] / perspective_far[3] - 1.0).abs() < 0.0001);

    let orthographic = orthographic_rh(8.0, 16.0 / 9.0, 0.1, 100.0);
    let near = mul_mat4_vec3(&orthographic, engine_core::math::Vec3::new(0.0, 0.0, -0.1));
    let far = mul_mat4_vec3(
        &orthographic,
        engine_core::math::Vec3::new(0.0, 0.0, -100.0),
    );

    assert!(near.z.abs() < 0.0001);
    assert!((far.z - 1.0).abs() < 0.0001);
}

fn mul_mat4_vec4(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2] + m[3][0] * v[3],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2] + m[3][1] * v[3],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2] + m[3][2] * v[3],
        m[0][3] * v[0] + m[1][3] * v[1] + m[2][3] * v[2] + m[3][3] * v[3],
    ]
}

#[test]
fn compute_ibl_sampling_uses_explicit_lod_and_dynamic_mip_resolution() {
    assert!(!IBL_IRRADIANCE_SHADER.contains("textureSample(env_map"));
    assert!(!IBL_PREFILTER_SHADER.contains("textureSample(env_map"));
    assert!(IBL_IRRADIANCE_SHADER.contains("textureSampleLevel(env_map"));
    assert!(IBL_PREFILTER_SHADER.contains("textureSampleLevel(env_map"));
    assert!(IBL_PREFILTER_SHADER.contains("let res = params.resolution"));
    assert!(IBL_IRRADIANCE_SHADER.contains("let right = normalize(cross(up, dir))"));
    assert!(IBL_IRRADIANCE_SHADER.contains("let tangent_up = cross(dir, right)"));
    assert!(!IBL_IRRADIANCE_SHADER.contains("tangent.y * vec3<f32>(0.0, 1.0, 0.0)"));
}

#[test]
fn bloom_downsample_uses_soft_knee_threshold() {
    assert!(BLOOM_DOWNSAMPLE_SHADER.contains("let knee = params.y"));
    assert!(BLOOM_DOWNSAMPLE_SHADER.contains("let knee_start = threshold - soft"));
    assert!(BLOOM_DOWNSAMPLE_SHADER.contains("soft_contribution"));
    assert!(!BLOOM_DOWNSAMPLE_SHADER.contains("color *= max(brightness - threshold"));
}

#[test]
fn post_shader_leaves_srgb_encoding_to_the_output_attachment() {
    assert!(!POST_SHADER.contains("gamma_correct"));
    assert!(!POST_SHADER.contains("1.0 / 2.2"));
}

#[test]
fn post_shader_uses_godot_aces_transform() {
    assert!(POST_SHADER.contains("let exposure_bias = 1.28"));
    assert!(POST_SHADER.contains("let rgb_to_rrt = mat3x3<f32>"));
    assert!(POST_SHADER.contains("let odt_to_rgb = mat3x3<f32>"));
    assert!(POST_SHADER.contains("0.0245786"));
    assert!(POST_SHADER.contains("0.000090537"));
    assert!(POST_SHADER.contains("return saturate(odt_to_rgb * tonemapped)"));
    assert!(!POST_SHADER.contains("let a = 2.51"));
}

#[test]
fn post_shader_uses_cubic_reconstruction_and_bounded_sharpening() {
    assert!(POST_SHADER.contains("fn reconstruct_hdr"));
    assert!(POST_SHADER.contains("fn cubic_weight"));
    assert!(POST_SHADER.contains("fn sharpen_hdr"));
    assert!(POST_SHADER.contains("textureLoad(hdr_tex"));
    assert!(POST_SHADER.contains("clamp(center + detail"));
    assert!(POST_SHADER.contains("return sharpen_hdr(textureLoad(hdr_tex, pixel, 0).rgb"));
}

#[test]
fn post_shader_samples_depth_for_glsl_backend_compatibility() {
    assert!(POST_SHADER.contains("return textureLoad(depth_tex, pixel, 0);"));
    assert!(!POST_SHADER.contains("textureSample(depth_tex"));
}

#[test]
fn taa_shader_reprojects_and_clamps_history_for_antialiasing() {
    assert!(TAA_SHADER.contains("var history_tex: texture_2d<f32>"));
    assert!(TAA_SHADER.contains("var motion_tex: texture_2d<f32>"));
    assert!(TAA_SHADER.contains("history_uv = uv - motion"));
    assert!(TAA_SHADER.contains("post.taa_enabled < 0.5"));
    assert!(TAA_SHADER.contains("clamp(history, neighborhood_min, neighborhood_max)"));
    assert!(TAA_SHADER.contains("post.taa_history_weight"));
    assert!(TAA_SHADER.contains("mix(post.taa_history_weight, 0.38"));
    assert!(TAA_SHADER.contains("0.0, 0.82"));
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
fn cylinder_generates_expected_counts() {
    let (verts, indices) = generate_cylinder(12);
    assert_eq!(verts.len(), 50);
    assert_eq!(indices.len(), 144);
}

#[test]
fn cone_generates_expected_counts() {
    let (verts, indices) = generate_cone(12);
    assert_eq!(verts.len(), 37);
    assert_eq!(indices.len(), 72);
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
    let cylinder = DebugMesh::Cylinder(16);
    let cone = DebugMesh::Cone(16);
    let plane = DebugMesh::Plane;
    assert_eq!(cube, DebugMesh::Cube);
    assert_eq!(sphere, DebugMesh::Sphere(8));
    assert_eq!(cylinder, DebugMesh::Cylinder(16));
    assert_eq!(cone, DebugMesh::Cone(16));
    assert_eq!(plane, DebugMesh::Plane);
    assert_eq!(mesh_name(&cylinder), "debug/cylinder");
    assert_eq!(mesh_name(&cone), "debug/cone");
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
        settings: Default::default(),
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
fn packs_light_quality_controls_into_forward_uniform() {
    let light = RenderLight {
        object: engine_core::EntityId::from_u128(8),
        transform: engine_core::math::Transform::default(),
        kind: RenderLightKind::Directional,
        color: engine_core::math::Vec3::ONE,
        intensity: 2.0,
        range: 100.0,
        spot_angle: 45.0,
        settings: engine_render::RenderLightSettings {
            casts_shadow: false,
            source_radius: 1.75,
            temperature_kelvin: 2_700.0,
            contact_shadow_strength: 0.5,
            attenuation: 1.4,
            specular: 0.65,
            ..Default::default()
        },
    };

    let packed = forward_light_uniform(&light);

    assert_eq!(packed.spot_angles[2], 0.0);
    assert_eq!(packed.spot_angles[3], 1.75);
    assert_eq!(packed.quality[0], 1.4);
    assert_eq!(packed.quality[1], 0.65);
    assert!(packed.color_intensity[0] >= packed.color_intensity[2]);
    assert_eq!(packed.color_intensity[3], 2.0);
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

fn test_shadow_instance(
    center: engine_core::math::Vec3,
    radius: f32,
    depth_min: f32,
    depth_max: f32,
) -> RenderBatchInstance {
    RenderBatchInstance {
        instance: Instance {
            offset: [center.x, center.y, center.z],
            scale: [radius, radius, radius],
            color: [1.0; 4],
            rotation: [0.0, 0.0, 0.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
            receive_shadows: 1.0,
        },
        shadow_center: center,
        shadow_radius: radius,
        shadow_depth_min: depth_min,
        shadow_depth_max: depth_max,
    }
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
    assert_eq!(uniform.lights[0].color_intensity[3], 1.35);
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
        test_light(
            6,
            RenderLightKind::Point,
            engine_core::math::Vec3::new(0.0, 0.0, 12.0),
            500.0,
            2.0,
        ),
    ];
    for index in 0..48 {
        lights.push(test_light(
            10 + index,
            RenderLightKind::Point,
            engine_core::math::Vec3::new(
                (index as f32 % 8.0) - 4.0,
                0.0,
                -6.0 - index as f32 * 0.2,
            ),
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
            .any(|light| light.object == engine_core::EntityId::from_u128(5))
    );
    assert!(
        selected
            .iter()
            .any(|light| light.object == engine_core::EntityId::from_u128(6))
    );
    assert!(
        selected
            .iter()
            .any(|light| light.object == engine_core::EntityId::from_u128(10))
    );

    let uniform = lighting_uniform_from_world(&world);
    assert_eq!(uniform.lights[0].spot_angles[2], 1.0);
    assert_eq!(uniform.lights[1].spot_angles[2], 1.0);
    assert_eq!(
        primary_directional_light(&world).unwrap().object,
        engine_core::EntityId::from_u128(3)
    );
}

#[test]
fn local_light_selection_is_stable_across_camera_views() {
    let lights = vec![
        test_light(
            1,
            RenderLightKind::Point,
            engine_core::math::Vec3::new(-12.0, 0.0, 0.0),
            4.0,
            8.0,
        ),
        test_light(
            2,
            RenderLightKind::Point,
            engine_core::math::Vec3::new(12.0, 0.0, 0.0),
            3.0,
            8.0,
        ),
        test_light(
            3,
            RenderLightKind::Point,
            engine_core::math::Vec3::new(0.0, 0.0, -20.0),
            2.0,
            12.0,
        ),
    ];
    let world_a = RenderWorld {
        camera: Some(engine_render::RenderCamera {
            object: engine_core::EntityId::from_u128(10),
            transform: engine_core::math::Transform::IDENTITY,
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 50.0,
            look_at_target: Some(engine_core::math::Vec3::new(0.0, 0.0, -1.0)),
        }),
        lights,
        ..RenderWorld::default()
    };
    let mut world_b = world_a.clone();
    world_b.camera.as_mut().unwrap().look_at_target =
        Some(engine_core::math::Vec3::new(0.0, 0.0, 1.0));

    let selected_a: Vec<_> = select_forward_lights(&world_a)
        .into_iter()
        .map(|light| light.object)
        .collect();
    let selected_b: Vec<_> = select_forward_lights(&world_b)
        .into_iter()
        .map(|light| light.object)
        .collect();

    assert_eq!(selected_a, selected_b);
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
        settings: Default::default(),
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
fn grid_vertices_are_slightly_above_y_zero_to_avoid_z_fighting() {
    let vertices = generate_grid();
    for v in &vertices {
        assert!(
            (v.position[1] - 0.01).abs() < f32::EPSILON,
            "editor grid must sit slightly above Y=0 geometry"
        );
    }
}

#[test]
fn grid_depth_test_uses_strict_less_to_avoid_equal_depth_fighting() {
    let constructor_source = include_str!("constructors.rs");

    assert!(constructor_source.contains("label: Some(\"varg grid pipeline\")"));
    assert!(constructor_source.contains("depth_compare: Some(wgpu::CompareFunction::Less)"));
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
            settings: Default::default(),
        }],
        ..RenderWorld::default()
    };

    let (uniform, probes) = gi_probe_uniform_and_data(&world);

    assert_eq!(uniform.params[0], 1);
    assert_eq!(uniform.params[1], 8);
    assert_eq!(uniform.counts_intensity, [2.0, 2.0, 2.0, 1.5]);
    assert_eq!(probes.len(), 8);
    assert!(probes.iter().any(|probe| probe.irradiance_pos_y[0] > 0.03));
    assert!(
        probes
            .iter()
            .any(|probe| probe.irradiance_pos_y[0] > probe.irradiance_neg_y[0])
    );
}

#[test]
fn render_world_defaults_to_conservative_screen_space_gi() {
    let world = RenderWorld::default();
    assert_eq!(
        world.global_illumination,
        RenderGlobalIllumination::ScreenSpace { intensity: 0.35 }
    );
}

#[test]
fn probe_volume_auto_bounds_include_scene_objects() {
    let world = RenderWorld {
        global_illumination: engine_render::RenderGlobalIllumination::ProbeVolume(
            engine_render::RenderProbeVolume::default(),
        ),
        objects: vec![RenderObject {
            object: engine_core::EntityId::from_u128(7),
            transform: engine_core::math::Transform {
                translation: engine_core::math::Vec3::new(16.0, 0.0, 0.0),
                rotation: engine_core::math::Quat::IDENTITY,
                scale: engine_core::math::Vec3::ONE,
            },
            mesh: "debug/cube".to_owned(),
            material: String::new(),
            casts_shadows: true,
            receive_shadows: true,
            bounds: engine_render::RenderBounds {
                center: engine_core::math::Vec3::ZERO,
                radius: 2.0,
            },
            lods: Vec::new(),
        }],
        ..RenderWorld::default()
    };

    let (uniform, probes) = gi_probe_uniform_and_data(&world);

    assert_eq!(uniform.params[0], 1);
    assert!(!probes.is_empty());
    assert!(uniform.center[0] > 2.0);
    assert!(uniform.extent[0] > 24.0);
}

#[test]
fn probe_irradiance_is_reduced_by_occluding_geometry() {
    let light = RenderLight {
        object: engine_core::EntityId::from_u128(42),
        transform: engine_core::math::Transform {
            translation: engine_core::math::Vec3::new(0.0, 4.0, 0.0),
            rotation: engine_core::math::Quat::IDENTITY,
            scale: engine_core::math::Vec3::ONE,
        },
        kind: RenderLightKind::Point,
        color: engine_core::math::Vec3::ONE,
        intensity: 8.0,
        range: 12.0,
        spot_angle: 45.0,
        settings: Default::default(),
    };
    let open_world = RenderWorld {
        lights: vec![light.clone()],
        ..RenderWorld::default()
    };
    let blocked_world = RenderWorld {
        lights: vec![light],
        objects: vec![RenderObject {
            object: engine_core::EntityId::from_u128(9),
            transform: engine_core::math::Transform {
                translation: engine_core::math::Vec3::new(0.0, 2.0, 0.0),
                rotation: engine_core::math::Quat::IDENTITY,
                scale: engine_core::math::Vec3::ONE,
            },
            mesh: "debug/sphere".to_owned(),
            material: String::new(),
            casts_shadows: true,
            receive_shadows: true,
            bounds: engine_render::RenderBounds {
                center: engine_core::math::Vec3::ZERO,
                radius: 1.4,
            },
            lods: Vec::new(),
        }],
        ..RenderWorld::default()
    };

    let open = probe_irradiance_at(&open_world, engine_core::math::Vec3::ZERO);
    let blocked = probe_irradiance_at(&blocked_world, engine_core::math::Vec3::ZERO);

    assert!(blocked.x < open.x);
}
