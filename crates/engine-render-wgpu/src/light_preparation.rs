use crate::{device::*, math::*, uniforms::*};
use engine_render::{RenderLight, RenderLightKind, RenderWorld};

const PROBE_SKY_DIRECTIONS: [engine_core::math::Vec3; 6] = [
    engine_core::math::Vec3::new(0.0, 1.0, 0.0),
    engine_core::math::Vec3::new(0.55, 0.78, 0.32),
    engine_core::math::Vec3::new(-0.55, 0.78, 0.32),
    engine_core::math::Vec3::new(0.32, 0.78, -0.55),
    engine_core::math::Vec3::new(-0.32, 0.78, -0.55),
    engine_core::math::Vec3::new(0.0, 0.5, -0.86),
];

const PROBE_IRRADIANCE_DIRECTIONS: [engine_core::math::Vec3; 6] = [
    engine_core::math::Vec3::new(1.0, 0.0, 0.0),
    engine_core::math::Vec3::new(-1.0, 0.0, 0.0),
    engine_core::math::Vec3::new(0.0, 1.0, 0.0),
    engine_core::math::Vec3::new(0.0, -1.0, 0.0),
    engine_core::math::Vec3::new(0.0, 0.0, 1.0),
    engine_core::math::Vec3::new(0.0, 0.0, -1.0),
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct DirectionalShadowPlan {
    splits: [f32; CSM_CASCADE_COUNT],
    cascade_count: usize,
    blend_splits: bool,
    bias: f32,
    normal_bias: f32,
    fade_start_fraction: f32,
}

impl DirectionalShadowPlan {
    fn from_light(light: Option<&RenderLight>) -> Self {
        let Some(light) = light else {
            return Self::default();
        };
        let settings = &light.settings;
        let max_distance = settings.shadow_max_distance.max(1.0);
        let cascade_count = match settings.directional_shadow_mode {
            engine_render::RenderDirectionalShadowMode::Orthogonal => 1,
            engine_render::RenderDirectionalShadowMode::Parallel2Splits => 2,
            engine_render::RenderDirectionalShadowMode::Parallel4Splits => 4,
        };
        let mut splits = [max_distance; CSM_CASCADE_COUNT];
        match cascade_count {
            1 => {}
            2 => {
                splits[0] = settings.directional_shadow_splits[0].clamp(0.01, 0.98) * max_distance;
            }
            _ => {
                splits[0] = settings.directional_shadow_splits[0].clamp(0.01, 0.98) * max_distance;
                splits[1] = settings.directional_shadow_splits[1].clamp(0.01, 0.98) * max_distance;
                splits[2] = settings.directional_shadow_splits[2].clamp(0.01, 0.98) * max_distance;
            }
        }
        for index in 1..cascade_count {
            splits[index] = splits[index].max(splits[index - 1] + 0.1);
        }
        let terminal = splits[cascade_count - 1];
        for split in splits.iter_mut().skip(cascade_count) {
            *split = terminal;
        }

        Self {
            splits,
            cascade_count,
            blend_splits: settings.directional_shadow_blend_splits,
            bias: settings.shadow_bias.max(0.0),
            normal_bias: settings.shadow_normal_bias.max(0.0),
            fade_start_fraction: settings.shadow_fade_start.clamp(0.0, 1.0),
        }
    }
}

impl Default for DirectionalShadowPlan {
    fn default() -> Self {
        Self {
            splits: [
                CSM_CASCADE_SPLITS[0],
                CSM_CASCADE_SPLITS[1],
                CSM_CASCADE_SPLITS[2],
                CSM_CASCADE_SPLITS[CSM_CASCADE_COUNT - 1],
                CSM_CASCADE_SPLITS[CSM_CASCADE_COUNT - 1],
            ],
            cascade_count: 4,
            blend_splits: true,
            bias: 0.002,
            normal_bias: 0.006,
            fade_start_fraction: 0.8,
        }
    }
}

pub(crate) fn csm_uniform_from_world(world: &RenderWorld, aspect: f32) -> CsmUniform {
    let shadow_light = primary_directional_light(world);
    let plan = DirectionalShadowPlan::from_light(shadow_light);
    let light_dir = shadow_light
        .map(|l| {
            l.transform
                .rotation
                .rotate(engine_core::math::Vec3::new(0.0, 0.0, -1.0))
                .normalized()
        })
        .unwrap_or_else(|| engine_core::math::Vec3::new(-0.5, -1.0, -0.25).normalized());

    let cam = match &world.camera {
        Some(c) => c,
        None => {
            return CsmUniform {
                cascade_vps: [IDENTITY_MAT4; CSM_CASCADE_COUNT],
                cascade_splits: [0.0; 4],
                params: default_csm_params(),
                fade_params: default_csm_fade_params(),
            };
        }
    };

    let cam_pos = cam.transform.translation;
    let cam_forward = cam
        .look_at_target
        .map(|target| (target - cam_pos).normalized())
        .unwrap_or_else(|| {
            cam.transform
                .rotation
                .rotate(engine_core::math::Vec3::new(0.0, 0.0, -1.0))
                .normalized()
        });

    let fov_rad = cam.vertical_fov_degrees.to_radians();
    let tan_half_fov = (fov_rad * 0.5).tan();

    let mut cascade_vps = [IDENTITY_MAT4; CSM_CASCADE_COUNT];
    let mut cascade_splits = [0.0f32; 4];

    for i in 0..CSM_CASCADE_COUNT {
        let (near, far) = csm_cascade_view_depth_bounds_for_plan(i, cam.near, &plan);
        if i < 4 {
            cascade_splits[i] = plan.splits[i];
        }

        let half_height_near = tan_half_fov * near;
        let half_width_near = half_height_near * aspect;
        let half_height_far = tan_half_fov * far;
        let half_width_far = half_height_far * aspect;

        let near_center = cam_pos + cam_forward * near;
        let far_center = cam_pos + cam_forward * far;

        let right = engine_core::math::Vec3::new(0.0, 1.0, 0.0)
            .cross(cam_forward)
            .normalized();
        let up = cam_forward.cross(right);

        let corners = [
            near_center + right * half_width_near + up * half_height_near,
            near_center - right * half_width_near + up * half_height_near,
            near_center - right * half_width_near - up * half_height_near,
            near_center + right * half_width_near - up * half_height_near,
            far_center + right * half_width_far + up * half_height_far,
            far_center - right * half_width_far + up * half_height_far,
            far_center - right * half_width_far - up * half_height_far,
            far_center + right * half_width_far - up * half_height_far,
        ];

        let mut center = engine_core::math::Vec3::ZERO;
        for corner in &corners {
            center = center + *corner;
        }
        center = center * (1.0 / 8.0);

        let up = if light_dir.x.abs() < 0.99 {
            engine_core::math::Vec3::new(0.0, 1.0, 0.0)
        } else {
            engine_core::math::Vec3::new(0.0, 0.0, 1.0)
        };
        let light_view = look_at_rh(center - light_dir * 50.0, center, up);

        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        let mut min_z = f32::MAX;
        let mut max_z = f32::MIN;
        for corner in &corners {
            let p = mul_mat4_vec3(&light_view, *corner);
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
            min_z = min_z.min(p.z);
            max_z = max_z.max(p.z);
        }

        let (snapped_min_x, snapped_max_x, snapped_min_y, snapped_max_y) =
            snap_csm_bounds_to_texel_grid(min_x, max_x, min_y, max_y);
        min_x = snapped_min_x;
        max_x = snapped_max_x;
        min_y = snapped_min_y;
        max_y = snapped_max_y;

        let z_padding = 10.0;
        let light_proj = orthographic_rh_custom(
            min_x,
            max_x,
            min_y,
            max_y,
            min_z - z_padding,
            max_z + z_padding,
        );
        cascade_vps[i] = mul_mat4(&light_proj, &light_view);
    }

    CsmUniform {
        cascade_vps,
        cascade_splits,
        params: csm_params_for_plan(&plan),
        fade_params: csm_fade_params_for_plan(&plan),
    }
}

pub(crate) fn default_csm_params() -> [f32; 4] {
    [
        CSM_CASCADE_FADE_RANGE,
        1.0 / CSM_SHADOW_RESOLUTION as f32,
        0.002,
        0.006,
    ]
}

fn csm_params_for_plan(plan: &DirectionalShadowPlan) -> [f32; 4] {
    [
        if plan.blend_splits && plan.cascade_count > 1 {
            CSM_CASCADE_FADE_RANGE
        } else {
            0.0
        },
        1.0 / CSM_SHADOW_RESOLUTION as f32,
        plan.bias,
        plan.normal_bias,
    ]
}

pub(crate) fn default_csm_fade_params() -> [f32; 4] {
    [
        CSM_CASCADE_SPLITS[CSM_CASCADE_COUNT - 1] * 0.8,
        CSM_CASCADE_SPLITS[CSM_CASCADE_COUNT - 1],
        0.0,
        0.0,
    ]
}

fn csm_fade_params_for_plan(plan: &DirectionalShadowPlan) -> [f32; 4] {
    let max_distance = plan.splits[3].max(0.0);
    [
        max_distance * plan.fade_start_fraction,
        max_distance,
        0.0,
        0.0,
    ]
}

pub(crate) fn snap_csm_bounds_to_texel_grid(
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
) -> (f32, f32, f32, f32) {
    let texel_size =
        ((max_x - min_x).max(max_y - min_y) / CSM_SHADOW_RESOLUTION as f32).max(f32::EPSILON);
    (
        (min_x / texel_size).floor() * texel_size,
        (max_x / texel_size).ceil() * texel_size,
        (min_y / texel_size).floor() * texel_size,
        (max_y / texel_size).ceil() * texel_size,
    )
}

#[allow(dead_code)]
pub(crate) fn csm_cascade_view_depth_bounds(cascade_idx: usize, camera_near: f32) -> (f32, f32) {
    csm_cascade_view_depth_bounds_for_plan(
        cascade_idx,
        camera_near,
        &DirectionalShadowPlan::default(),
    )
}

fn csm_cascade_view_depth_bounds_for_plan(
    cascade_idx: usize,
    camera_near: f32,
    plan: &DirectionalShadowPlan,
) -> (f32, f32) {
    let clamped_idx = cascade_idx.min(CSM_CASCADE_COUNT - 1);
    if clamped_idx >= plan.cascade_count {
        let terminal = plan.splits[plan.cascade_count - 1];
        return (terminal, terminal + 0.1);
    }
    let split_near = if clamped_idx == 0 {
        camera_near
    } else {
        plan.splits[clamped_idx - 1]
    };
    let split_far = plan.splits[clamped_idx];
    let near_overlap = if plan.blend_splits && clamped_idx > 0 {
        CSM_CASCADE_FADE_RANGE
    } else {
        0.0
    };
    let far_overlap = if plan.blend_splits && clamped_idx + 1 < plan.cascade_count {
        CSM_CASCADE_FADE_RANGE
    } else {
        0.0
    };
    let near = if clamped_idx == 0 {
        split_near
    } else {
        (split_near - near_overlap).max(camera_near)
    };
    let far = (split_far + far_overlap).min(plan.splits[plan.cascade_count - 1]);
    (near, far.max(near + 0.1))
}

pub(crate) fn lighting_uniform_from_world(world: &RenderWorld) -> LightingUniform {
    let mut uniform = LightingUniform::default();
    let mut count = 0usize;
    let local_shadow_lights = select_local_shadow_lights(world);

    for light in select_forward_lights(world) {
        let mut packed = forward_light_uniform(light);
        if count == 0 && light.kind == RenderLightKind::Directional {
            packed.spot_angles[2] = 1.0;
        }
        if light.kind == RenderLightKind::Spot {
            packed.quality[2] = local_shadow_lights
                .iter()
                .position(|shadow_light| shadow_light.object == light.object)
                .map(|slot| slot as f32)
                .unwrap_or(-1.0);
            packed.quality[3] = 1.0 / LOCAL_SHADOW_ATLAS_RESOLUTION as f32;
        }
        uniform.lights[count] = packed;
        count += 1;
    }

    if count == 0 {
        uniform.lights[0] = ForwardLightUniform {
            position_type: [0.0, 0.0, 0.0, 0.0],
            direction_range: [-0.5, -1.0, -0.25, 0.0],
            color_intensity: [1.0, 1.0, 1.0, 1.0],
            spot_angles: [1.0, 1.0, 1.0, 0.0],
            quality: [2.0, 1.0, 1.0, 0.0],
        };
        count = 1;
    }

    uniform.params = [count as u32, 0, 0, 0];
    uniform
}

pub(crate) fn cluster_lighting_data_from_world(
    world: &RenderWorld,
    view_projection: &[[f32; 4]; 4],
    width: u32,
    height: u32,
) -> (
    ClusterUniform,
    Vec<ForwardLightUniform>,
    Vec<ClusterRange>,
    Vec<u32>,
) {
    let selected = select_forward_lights(world);
    let local_shadow_lights = select_local_shadow_lights(world);
    let mut lights = Vec::with_capacity(selected.len().min(MAX_CLUSTERED_LIGHTS));
    for light in selected.into_iter().take(MAX_CLUSTERED_LIGHTS) {
        let mut packed = forward_light_uniform(light);
        if light.kind == RenderLightKind::Spot {
            packed.quality[2] = local_shadow_lights
                .iter()
                .position(|shadow_light| shadow_light.object == light.object)
                .map(|slot| slot as f32)
                .unwrap_or(-1.0);
            packed.quality[3] = 1.0 / LOCAL_SHADOW_ATLAS_RESOLUTION as f32;
        }
        lights.push(packed);
    }
    if lights.is_empty() {
        lights.push(ForwardLightUniform {
            position_type: [0.0, 0.0, 0.0, 0.0],
            direction_range: [-0.5, -1.0, -0.25, 0.0],
            color_intensity: [1.0, 1.0, 1.0, 1.0],
            spot_angles: [1.0, 1.0, 0.0, 0.0],
            quality: [2.0, 1.0, -1.0, 0.0],
        });
    }

    let tile_width = (width.max(1) as f32 / CLUSTER_TILE_COLUMNS as f32).ceil();
    let tile_height = (height.max(1) as f32 / CLUSTER_TILE_ROWS as f32).ceil();
    let mut ranges = Vec::with_capacity(CLUSTER_TILE_COUNT);
    let mut indices = Vec::with_capacity(MAX_CLUSTER_LIGHT_INDICES);

    for tile_y in 0..CLUSTER_TILE_ROWS {
        for tile_x in 0..CLUSTER_TILE_COLUMNS {
            let offset = indices.len() as u32;
            for (light_index, light) in lights.iter().enumerate() {
                if indices.len() >= MAX_CLUSTER_LIGHT_INDICES {
                    break;
                }
                if light_intersects_tile(
                    light,
                    view_projection,
                    width.max(1),
                    height.max(1),
                    tile_x,
                    tile_y,
                    tile_width,
                    tile_height,
                ) {
                    let count = indices.len() as u32 - offset;
                    if count >= MAX_LIGHTS_PER_CLUSTER as u32 {
                        break;
                    }
                    indices.push(light_index as u32);
                }
            }
            ranges.push(ClusterRange {
                offset,
                count: indices.len() as u32 - offset,
                _pad: [0; 2],
            });
        }
    }

    (
        ClusterUniform {
            layout: [
                CLUSTER_TILE_COLUMNS as f32,
                CLUSTER_TILE_ROWS as f32,
                tile_width,
                tile_height,
            ],
            params: [lights.len() as u32, MAX_LIGHTS_PER_CLUSTER as u32, 0, 0],
        },
        lights,
        ranges,
        indices,
    )
}

pub(crate) fn local_shadow_uniform_from_world(world: &RenderWorld) -> LocalShadowUniform {
    let selected = select_local_shadow_lights(world);
    let mut uniform = LocalShadowUniform::default();
    uniform.params[0] = selected.len() as f32;
    uniform.params[1] = 1.0 / LOCAL_SHADOW_ATLAS_RESOLUTION as f32;

    for (slot, light) in selected.iter().enumerate() {
        uniform.light_view_projections[slot] = spot_light_view_projection(light);
        uniform.atlas_rects[slot] = local_shadow_atlas_rect(slot);
    }

    uniform
}

fn light_intersects_tile(
    light: &ForwardLightUniform,
    view_projection: &[[f32; 4]; 4],
    width: u32,
    height: u32,
    tile_x: u32,
    tile_y: u32,
    tile_width: f32,
    tile_height: f32,
) -> bool {
    if light.position_type[3] < 0.5 {
        return true;
    }
    let center = engine_core::math::Vec3::new(
        light.position_type[0],
        light.position_type[1],
        light.position_type[2],
    );
    let range = light.direction_range[3].max(0.001);
    let Some((sx, sy, depth)) = project_to_screen(view_projection, center, width, height) else {
        return true;
    };
    if depth < -range {
        return false;
    }
    let projected_radius =
        ((range / depth.max(0.5)) * height as f32 * 0.5).clamp(8.0, height as f32);
    let min_x = tile_x as f32 * tile_width;
    let min_y = tile_y as f32 * tile_height;
    let max_x = ((tile_x + 1) as f32 * tile_width).min(width as f32);
    let max_y = ((tile_y + 1) as f32 * tile_height).min(height as f32);
    sx + projected_radius >= min_x
        && sx - projected_radius <= max_x
        && sy + projected_radius >= min_y
        && sy - projected_radius <= max_y
}

fn project_to_screen(
    view_projection: &[[f32; 4]; 4],
    position: engine_core::math::Vec3,
    width: u32,
    height: u32,
) -> Option<(f32, f32, f32)> {
    let x = view_projection[0][0] * position.x
        + view_projection[1][0] * position.y
        + view_projection[2][0] * position.z
        + view_projection[3][0];
    let y = view_projection[0][1] * position.x
        + view_projection[1][1] * position.y
        + view_projection[2][1] * position.z
        + view_projection[3][1];
    let z = view_projection[0][2] * position.x
        + view_projection[1][2] * position.y
        + view_projection[2][2] * position.z
        + view_projection[3][2];
    let w = view_projection[0][3] * position.x
        + view_projection[1][3] * position.y
        + view_projection[2][3] * position.z
        + view_projection[3][3];
    if w.abs() <= f32::EPSILON {
        return None;
    }
    let ndc_x = x / w;
    let ndc_y = y / w;
    Some((
        (ndc_x * 0.5 + 0.5) * width as f32,
        (0.5 - ndc_y * 0.5) * height as f32,
        w.abs().max(z.abs()),
    ))
}

pub(crate) fn gi_probe_uniform_and_data(world: &RenderWorld) -> (GiProbeUniform, Vec<GiProbe>) {
    let engine_render::RenderGlobalIllumination::ProbeVolume(volume) = &world.global_illumination
    else {
        return (GiProbeUniform::default(), Vec::new());
    };

    let (center, extent) = resolved_probe_volume_bounds(world, volume);
    let counts = [
        volume.counts[0].clamp(1, 16),
        volume.counts[1].clamp(1, 16),
        volume.counts[2].clamp(1, 16),
    ];
    let total = counts
        .iter()
        .product::<u32>()
        .min(crate::device::MAX_GI_PROBES as u32);
    let mut probes = Vec::with_capacity(total as usize);
    let min = center - extent * 0.5;
    let step = engine_core::math::Vec3::new(
        if counts[0] > 1 {
            extent.x / (counts[0] - 1) as f32
        } else {
            0.0
        },
        if counts[1] > 1 {
            extent.y / (counts[1] - 1) as f32
        } else {
            0.0
        },
        if counts[2] > 1 {
            extent.z / (counts[2] - 1) as f32
        } else {
            0.0
        },
    );

    'z: for z in 0..counts[2] {
        for y in 0..counts[1] {
            for x in 0..counts[0] {
                if probes.len() >= crate::device::MAX_GI_PROBES {
                    break 'z;
                }
                let position = engine_core::math::Vec3::new(
                    min.x + step.x * x as f32,
                    min.y + step.y * y as f32,
                    min.z + step.z * z as f32,
                );
                let irradiance = directional_probe_irradiance_at(world, position);
                probes.push(GiProbe {
                    irradiance_pos_x: vec3_to_probe_slot(irradiance[0]),
                    irradiance_neg_x: vec3_to_probe_slot(irradiance[1]),
                    irradiance_pos_y: vec3_to_probe_slot(irradiance[2]),
                    irradiance_neg_y: vec3_to_probe_slot(irradiance[3]),
                    irradiance_pos_z: vec3_to_probe_slot(irradiance[4]),
                    irradiance_neg_z: vec3_to_probe_slot(irradiance[5]),
                });
            }
        }
    }

    (
        GiProbeUniform {
            center: [center.x, center.y, center.z, 0.0],
            extent: [extent.x, extent.y, extent.z, 0.0],
            counts_intensity: [
                counts[0] as f32,
                counts[1] as f32,
                counts[2] as f32,
                volume.intensity,
            ],
            params: [1, probes.len() as u32, counts[0], counts[1]],
        },
        probes,
    )
}

fn resolved_probe_volume_bounds(
    world: &RenderWorld,
    volume: &engine_render::RenderProbeVolume,
) -> (engine_core::math::Vec3, engine_core::math::Vec3) {
    let mut min = volume.center - volume.extent * 0.5;
    let mut max = volume.center + volume.extent * 0.5;
    let mut has_bounds = false;

    if let Some(camera) = &world.camera {
        let camera_extent = engine_core::math::Vec3::new(18.0, 7.0, 18.0);
        min = min.min(camera.transform.translation - camera_extent);
        max = max.max(camera.transform.translation + camera_extent);
        has_bounds = true;
    }

    for object in &world.objects {
        let radius = object.bounds.radius.max(1.0)
            * object
                .transform
                .scale
                .x
                .abs()
                .max(object.transform.scale.y.abs())
                .max(object.transform.scale.z.abs())
                .max(0.001);
        let local_center = object.bounds.center * object.transform.scale;
        let center = object.transform.translation + object.transform.rotation.rotate(local_center);
        let pad = engine_core::math::Vec3::new(radius, radius, radius);
        min = min.min(center - pad);
        max = max.max(center + pad);
        has_bounds = true;
    }

    if !has_bounds {
        return (volume.center, volume.extent);
    }

    min -= engine_core::math::Vec3::new(3.0, 2.0, 3.0);
    max += engine_core::math::Vec3::new(3.0, 3.0, 3.0);
    let center = (min + max) * 0.5;
    let extent = (max - min).max(engine_core::math::Vec3::new(6.0, 4.0, 6.0));
    (center, extent)
}

#[cfg(test)]
pub(crate) fn probe_irradiance_at(
    world: &RenderWorld,
    position: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    directional_probe_irradiance_at(world, position)
        .into_iter()
        .fold(engine_core::math::Vec3::ZERO, |sum, value| sum + value)
        / 6.0
}

fn directional_probe_irradiance_at(
    world: &RenderWorld,
    position: engine_core::math::Vec3,
) -> [engine_core::math::Vec3; 6] {
    PROBE_IRRADIANCE_DIRECTIONS.map(|normal| probe_irradiance_for_normal(world, position, normal))
}

fn probe_irradiance_for_normal(
    world: &RenderWorld,
    position: engine_core::math::Vec3,
    normal: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    let sky_visibility = sky_visibility_at(world, position);
    let sky_direction = normal.y.clamp(0.0, 1.0);
    let ground_direction = (-normal.y).clamp(0.0, 1.0);
    let mut irradiance = engine_core::math::Vec3::new(0.018, 0.02, 0.024)
        * sky_visibility
        * (0.35 + sky_direction * 0.65);
    if let Some(skybox) = &world.skybox {
        let sky_mix = (normal.y * 0.5 + 0.5).clamp(0.0, 1.0);
        let horizon = engine_core::math::Vec3::new(
            skybox.horizon_color[0],
            skybox.horizon_color[1],
            skybox.horizon_color[2],
        );
        let zenith = engine_core::math::Vec3::new(
            skybox.zenith_color[0],
            skybox.zenith_color[1],
            skybox.zenith_color[2],
        );
        let ground = horizon * 0.18;
        let sky_color = horizon * (1.0 - sky_mix) + zenith * sky_mix;
        let sky = (sky_color * (1.0 - ground_direction) + ground * ground_direction)
            * (0.18 * skybox.intensity * sky_visibility);
        irradiance += sky;
    }
    for light in &world.lights {
        let color = light.color * light.intensity;
        match light.kind {
            engine_render::RenderLightKind::Directional => {
                let dir = -light
                    .transform
                    .rotation
                    .rotate(engine_core::math::Vec3::new(0.0, 0.0, -1.0))
                    .normalized();
                let facing = normal.dot(-dir).clamp(0.0, 1.0);
                let visibility = ray_visibility(world, position, dir, 80.0);
                irradiance += color * (0.08 * facing * visibility);
            }
            engine_render::RenderLightKind::Point
            | engine_render::RenderLightKind::Spot
            | engine_render::RenderLightKind::Area => {
                let to_light = light.transform.translation - position;
                let distance = to_light.length();
                let range = light.range.max(0.001);
                let attenuation = (1.0 - distance / range)
                    .max(0.0)
                    .powf(light.settings.attenuation.max(0.01));
                let light_dir = to_light.normalized();
                let facing = normal.dot(light_dir).clamp(0.0, 1.0);
                let visibility = ray_visibility(world, position, light_dir, distance);
                irradiance += color
                    * light.settings.indirect_energy.max(0.0)
                    * attenuation
                    * facing
                    * visibility
                    * 0.22;
            }
        }
    }
    irradiance += local_bounce_irradiance(world, position, normal);
    clamp_vec3(irradiance, 0.0, 6.0)
}

fn vec3_to_probe_slot(value: engine_core::math::Vec3) -> [f32; 4] {
    [value.x, value.y, value.z, 1.0]
}

fn sky_visibility_at(world: &RenderWorld, position: engine_core::math::Vec3) -> f32 {
    let mut visibility = 0.0;
    for dir in PROBE_SKY_DIRECTIONS {
        visibility += ray_visibility(world, position, dir.normalized(), 36.0);
    }
    (visibility / PROBE_SKY_DIRECTIONS.len() as f32).clamp(0.12, 1.0)
}

fn local_bounce_irradiance(
    world: &RenderWorld,
    position: engine_core::math::Vec3,
    normal: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    let mut bounce = engine_core::math::Vec3::ZERO;
    for object in &world.objects {
        let radius = object.bounds.radius.max(0.5)
            * object
                .transform
                .scale
                .x
                .abs()
                .max(object.transform.scale.y.abs())
                .max(object.transform.scale.z.abs())
                .max(0.001);
        let local_center = object.bounds.center * object.transform.scale;
        let center = object.transform.translation + object.transform.rotation.rotate(local_center);
        let to_object = center - position;
        let distance_sq = to_object.length_squared().max(0.25);
        let distance = distance_sq.sqrt();
        if distance > 18.0 + radius {
            continue;
        }
        let material = world
            .material_params
            .get(&object.material)
            .copied()
            .unwrap_or(engine_render::RenderMaterialParams {
                base_color: [0.72, 0.72, 0.72, 1.0],
                metallic: 0.0,
                roughness: 0.65,
                emissive: [0.0, 0.0, 0.0],
            });
        let albedo = engine_core::math::Vec3::new(
            material.base_color[0],
            material.base_color[1],
            material.base_color[2],
        );
        let emissive = engine_core::math::Vec3::new(
            material.emissive[0],
            material.emissive[1],
            material.emissive[2],
        );
        let visibility = ray_visibility(world, position, to_object.normalized(), distance);
        let facing = normal.dot(to_object.normalized()).clamp(0.0, 1.0);
        let area = (radius * radius / distance_sq).clamp(0.0, 1.0);
        let non_metal = 1.0 - material.metallic.clamp(0.0, 1.0);
        bounce += (albedo * non_metal * 0.14 + emissive * 0.32) * area * facing * visibility;
    }
    bounce
}

fn ray_visibility(
    world: &RenderWorld,
    origin: engine_core::math::Vec3,
    direction: engine_core::math::Vec3,
    max_distance: f32,
) -> f32 {
    if direction.length_squared() <= 1e-8 {
        return 1.0;
    }
    let mut transmittance: f32 = 1.0;
    for object in &world.objects {
        if !object.casts_shadows && !object.receive_shadows {
            continue;
        }
        let radius = object.bounds.radius.max(0.5)
            * object
                .transform
                .scale
                .x
                .abs()
                .max(object.transform.scale.y.abs())
                .max(object.transform.scale.z.abs())
                .max(0.001);
        let local_center = object.bounds.center * object.transform.scale;
        let center = object.transform.translation + object.transform.rotation.rotate(local_center);
        if ray_sphere_hit(origin, direction, center, radius * 0.85, max_distance) {
            transmittance *= 0.42;
            if transmittance < 0.18 {
                return transmittance;
            }
        }
    }
    transmittance
}

fn ray_sphere_hit(
    origin: engine_core::math::Vec3,
    direction: engine_core::math::Vec3,
    center: engine_core::math::Vec3,
    radius: f32,
    max_distance: f32,
) -> bool {
    let oc = origin - center;
    let b = oc.dot(direction);
    let c = oc.length_squared() - radius * radius;
    let h = b * b - c;
    if h < 0.0 {
        return false;
    }
    let t = -b - h.sqrt();
    t > 0.05 && t < max_distance
}

fn clamp_vec3(value: engine_core::math::Vec3, min: f32, max: f32) -> engine_core::math::Vec3 {
    engine_core::math::Vec3::new(
        value.x.clamp(min, max),
        value.y.clamp(min, max),
        value.z.clamp(min, max),
    )
}

pub(crate) fn primary_directional_light(world: &RenderWorld) -> Option<&RenderLight> {
    world
        .lights
        .iter()
        .filter(|light| {
            light.kind == RenderLightKind::Directional
                && light.intensity > 0.0
                && light.settings.casts_shadow
        })
        .max_by(|a, b| a.intensity.total_cmp(&b.intensity))
}

pub(crate) fn select_forward_lights(world: &RenderWorld) -> Vec<&RenderLight> {
    let mut selected = Vec::with_capacity(MAX_FORWARD_LIGHTS);
    let mut directional: Vec<&RenderLight> = world
        .lights
        .iter()
        .filter(|light| light.kind == RenderLightKind::Directional && light.intensity > 0.0)
        .collect();
    directional.sort_by(|a, b| b.intensity.total_cmp(&a.intensity));

    selected.extend(directional.into_iter().take(MAX_DIRECTIONAL_LIGHTS));

    let remaining = MAX_FORWARD_LIGHTS.saturating_sub(selected.len());
    if remaining == 0 {
        return selected;
    }

    let mut local: Vec<(&RenderLight, f32)> = world
        .lights
        .iter()
        .filter(|light| light.kind != RenderLightKind::Directional)
        .filter_map(|light| local_light_score(world, light).map(|score| (light, score)))
        .collect();
    local.sort_by(|(_, a), (_, b)| b.total_cmp(a));

    selected.extend(local.into_iter().take(remaining).map(|(light, _)| light));
    selected
}

pub(crate) fn select_local_shadow_lights(world: &RenderWorld) -> Vec<&RenderLight> {
    let selected = select_forward_lights(world);
    let mut spot_lights: Vec<&RenderLight> = selected
        .into_iter()
        .filter(|light| {
            light.kind == RenderLightKind::Spot && light.settings.casts_shadow && light.range > 0.0
        })
        .collect();
    spot_lights.truncate(MAX_LOCAL_SHADOWS);
    spot_lights
}

pub(crate) fn local_light_score(_world: &RenderWorld, light: &RenderLight) -> Option<f32> {
    if light.intensity <= 0.0 || light.range <= 0.0 {
        return None;
    }

    let range = light.range.max(0.001);
    Some(light.intensity * range * range)
}

pub(crate) fn forward_light_uniform(light: &RenderLight) -> ForwardLightUniform {
    let light_type = match light.kind {
        RenderLightKind::Point | RenderLightKind::Area => 1.0,
        RenderLightKind::Spot => 2.0,
        RenderLightKind::Directional => 0.0,
    };
    let direction = rotate_vec3(
        light.transform.rotation,
        engine_core::math::Vec3::new(0.0, 0.0, -1.0),
    )
    .normalized();
    let direction = if direction.length_squared() <= f32::EPSILON {
        engine_core::math::Vec3::new(0.0, -1.0, 0.0)
    } else {
        direction
    };
    let range = light.range.max(0.001);
    let outer_half_angle = (light.spot_angle.clamp(1.0, 179.0) * 0.5).to_radians();
    let inner_half_angle = outer_half_angle * 0.75;
    let shaded_color = light_color_with_temperature(light);

    ForwardLightUniform {
        position_type: [
            light.transform.translation.x,
            light.transform.translation.y,
            light.transform.translation.z,
            light_type,
        ],
        direction_range: [direction.x, direction.y, direction.z, range],
        color_intensity: [
            shaded_color.x.clamp(0.0, 1.0),
            shaded_color.y.clamp(0.0, 1.0),
            shaded_color.z.clamp(0.0, 1.0),
            light.intensity.max(0.0),
        ],
        spot_angles: [
            inner_half_angle.cos(),
            outer_half_angle.cos(),
            if light.settings.casts_shadow {
                1.0
            } else {
                0.0
            },
            light.settings.source_radius.max(0.0),
        ],
        quality: [
            light.settings.attenuation.max(0.01),
            light.settings.specular.max(0.0),
            light.settings.indirect_energy.max(0.0),
            light.settings.contact_shadow_strength.max(0.0),
        ],
    }
}

pub(crate) fn spot_light_view_projection(light: &RenderLight) -> [[f32; 4]; 4] {
    let position = light.transform.translation;
    let direction = rotate_vec3(
        light.transform.rotation,
        engine_core::math::Vec3::new(0.0, 0.0, -1.0),
    )
    .normalized();
    let direction = if direction.length_squared() <= f32::EPSILON {
        engine_core::math::Vec3::new(0.0, -1.0, 0.0)
    } else {
        direction
    };
    let up = if direction.y.abs() > 0.95 {
        engine_core::math::Vec3::new(0.0, 0.0, 1.0)
    } else {
        engine_core::math::Vec3::new(0.0, 1.0, 0.0)
    };
    let view = look_at_rh(position, position + direction, up);
    let fov = light.spot_angle.clamp(1.0, 175.0).to_radians();
    let projection = perspective_rh(fov, 1.0, 0.05, light.range.max(0.1));
    mul_mat4(&projection, &view)
}

pub(crate) fn local_shadow_atlas_rect(slot: usize) -> [f32; 4] {
    let slot = slot as u32;
    let column = slot % LOCAL_SHADOW_ATLAS_COLUMNS;
    let row = slot / LOCAL_SHADOW_ATLAS_COLUMNS;
    let scale = LOCAL_SHADOW_TILE_RESOLUTION as f32 / LOCAL_SHADOW_ATLAS_RESOLUTION as f32;
    [column as f32 * scale, row as f32 * scale, scale, scale]
}

pub(crate) fn light_color_with_temperature(light: &RenderLight) -> engine_core::math::Vec3 {
    let kelvin = light.settings.temperature_kelvin;
    if kelvin <= 0.0 {
        return light.color;
    }
    let tint = blackbody_temperature_to_rgb(kelvin);
    engine_core::math::Vec3::new(
        light.color.x * tint.x,
        light.color.y * tint.y,
        light.color.z * tint.z,
    )
}

pub(crate) fn blackbody_temperature_to_rgb(kelvin: f32) -> engine_core::math::Vec3 {
    let temp = (kelvin.clamp(1_000.0, 40_000.0) / 100.0).max(1.0);
    let red = if temp <= 66.0 {
        1.0
    } else {
        (329.69873 * (temp - 60.0).powf(-0.133_204_76) / 255.0).clamp(0.0, 1.0)
    };
    let green = if temp <= 66.0 {
        (99.470_8 * temp.ln() - 161.119_57) / 255.0
    } else {
        288.12216 * (temp - 60.0).powf(-0.075_514_846) / 255.0
    }
    .clamp(0.0, 1.0);
    let blue = if temp >= 66.0 {
        1.0
    } else if temp <= 19.0 {
        0.0
    } else {
        (138.517_73 * (temp - 10.0).ln() - 305.044_8) / 255.0
    }
    .clamp(0.0, 1.0);
    engine_core::math::Vec3::new(red, green, blue)
}
