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

pub(crate) fn csm_uniform_from_world(world: &RenderWorld, aspect: f32) -> CsmUniform {
    let light_dir = primary_directional_light(world)
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
        let near = if i == 0 {
            cam.near
        } else {
            CSM_CASCADE_SPLITS[i - 1]
        };
        let far = CSM_CASCADE_SPLITS[i];
        if i < 4 {
            cascade_splits[i] = far;
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
        params: default_csm_params(),
    }
}

pub(crate) fn default_csm_params() -> [f32; 4] {
    [
        CSM_CASCADE_FADE_RANGE,
        1.0 / CSM_SHADOW_RESOLUTION as f32,
        0.0005,
        0.0015,
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
pub(crate) fn skybox_uniform_from_world(world: &RenderWorld, use_cubemap: bool) -> SkyboxUniform {
    let skybox = match &world.skybox {
        Some(s) => s,
        None => {
            return SkyboxUniform {
                view_rotation_only: IDENTITY_MAT4,
                zenith_color: [0.15, 0.35, 0.65, 1.0],
                horizon_color: [0.55, 0.7, 0.85, 1.0],
                rotation_intensity: [0.0, 1.0, 0.0, 0.0],
                use_cubemap: [0, 0, 0, 0],
            };
        }
    };

    let eye = world
        .camera
        .as_ref()
        .map(|c| c.transform.translation)
        .unwrap_or(engine_core::math::Vec3::new(0.0, 0.0, 5.0));
    let target = world
        .camera
        .as_ref()
        .and_then(|c| c.look_at_target)
        .unwrap_or_else(|| {
            let q = world
                .camera
                .as_ref()
                .map(|c| c.transform.rotation)
                .unwrap_or(engine_core::math::Quat::IDENTITY);
            let fwd = engine_core::math::Vec3::new(
                2.0 * (q.x * q.z + q.w * q.y),
                2.0 * (q.y * q.z - q.w * q.x),
                1.0 - 2.0 * (q.x * q.x + q.y * q.y),
            );
            engine_core::math::Vec3::new(eye.x - fwd.x, eye.y - fwd.y, eye.z - fwd.z)
        });
    let view = look_at_rh(eye, target, engine_core::math::Vec3::new(0.0, 1.0, 0.0));

    SkyboxUniform {
        view_rotation_only: view,
        zenith_color: [
            skybox.zenith_color[0],
            skybox.zenith_color[1],
            skybox.zenith_color[2],
            1.0,
        ],
        horizon_color: [
            skybox.horizon_color[0],
            skybox.horizon_color[1],
            skybox.horizon_color[2],
            1.0,
        ],
        rotation_intensity: [skybox.rotation_degrees, skybox.intensity, 0.0, 0.0],
        use_cubemap: [u32::from(use_cubemap), 0, 0, 0],
    }
}

pub(crate) fn fog_uniform_from_world(world: &RenderWorld) -> FogUniform {
    match &world.fog {
        Some(fog) => FogUniform {
            density: fog.density,
            _pad: [0.0; 3],
            color: fog.color,
            enabled: if fog.enabled { 1.0 } else { 0.0 },
        },
        None => FogUniform {
            density: 0.0,
            _pad: [0.0; 3],
            color: [0.6, 0.7, 0.85],
            enabled: 0.0,
        },
    }
}
pub(crate) fn camera_uniform_from_world(world: &RenderWorld, aspect: f32) -> CameraUniform {
    let (vp, eye, target, _) = camera_temporal_basis(world, aspect);
    CameraUniform {
        view_projection: vp,
        camera_position: [eye.x, eye.y, eye.z, 1.0],
        camera_forward: {
            let forward = (target - eye).normalized();
            [forward.x, forward.y, forward.z, 0.0]
        },
    }
}

pub(crate) fn camera_uniform_with_view_projection(
    world: &RenderWorld,
    aspect: f32,
    view_projection: [[f32; 4]; 4],
) -> CameraUniform {
    let (_, eye, target, _) = camera_temporal_basis(world, aspect);
    CameraUniform {
        view_projection,
        camera_position: [eye.x, eye.y, eye.z, 1.0],
        camera_forward: {
            let forward = (target - eye).normalized();
            [forward.x, forward.y, forward.z, 0.0]
        },
    }
}

pub(crate) fn temporal_camera_from_world(
    world: &RenderWorld,
    aspect: f32,
    render_size: (u32, u32),
    state: &mut engine_render::TemporalFrameState,
) -> (engine_render::TemporalCameraData, bool) {
    let (view_projection, _, _, (near, far)) = camera_temporal_basis(world, aspect);
    state.next_camera_data(flatten_mat4(view_projection), render_size, near, far)
}

fn camera_temporal_basis(
    world: &RenderWorld,
    aspect: f32,
) -> (
    [[f32; 4]; 4],
    engine_core::math::Vec3,
    engine_core::math::Vec3,
    (f32, f32),
) {
    let eye = world
        .camera
        .as_ref()
        .map(|camera| camera.transform.translation)
        .unwrap_or_else(|| engine_core::math::Vec3::new(0.0, 0.0, 5.0));

    // Use the explicit look-at pivot if provided (editor orbit camera sets this),
    // otherwise fall back to deriving the target from the camera's transform rotation.
    let target = world
        .camera
        .as_ref()
        .and_then(|camera| camera.look_at_target)
        .unwrap_or_else(|| {
            // Extract the local +Z axis from the rotation quaternion in world space.
            // q * (0,0,1) gives the camera's local +Z in world space.
            // Since the camera looks along local -Z, the view direction is
            // -(+Z) which is achieved by target = eye - fwd below.
            let q = world
                .camera
                .as_ref()
                .map(|c| c.transform.rotation)
                .unwrap_or(engine_core::math::Quat::IDENTITY);
            let fwd = engine_core::math::Vec3::new(
                2.0 * (q.x * q.z + q.w * q.y),
                2.0 * (q.y * q.z - q.w * q.x),
                1.0 - 2.0 * (q.x * q.x + q.y * q.y),
            );
            // Negate because camera looks along -Z in its local space.
            engine_core::math::Vec3::new(eye.x - fwd.x, eye.y - fwd.y, eye.z - fwd.z)
        });

    let view = look_at_rh(eye, target, engine_core::math::Vec3::new(0.0, 1.0, 0.0));
    let fov = world
        .camera
        .as_ref()
        .map(|camera| camera.vertical_fov_degrees)
        .unwrap_or(60.0);
    let near = world
        .camera
        .as_ref()
        .map(|camera| camera.near)
        .unwrap_or(0.1);
    let far = world
        .camera
        .as_ref()
        .map(|camera| camera.far)
        .unwrap_or(100.0);
    let proj = match world.camera.as_ref().map(|camera| camera.projection) {
        Some(engine_render::RenderProjection::Orthographic { vertical_size }) => {
            orthographic_rh(vertical_size.max(0.001), aspect, near, far)
        }
        _ => perspective_rh(fov.to_radians(), aspect, near, far),
    };
    let vp = mul_mat4(&proj, &view);
    (vp, eye, target, (near, far))
}

fn flatten_mat4(matrix: [[f32; 4]; 4]) -> [f32; 16] {
    [
        matrix[0][0],
        matrix[0][1],
        matrix[0][2],
        matrix[0][3],
        matrix[1][0],
        matrix[1][1],
        matrix[1][2],
        matrix[1][3],
        matrix[2][0],
        matrix[2][1],
        matrix[2][2],
        matrix[2][3],
        matrix[3][0],
        matrix[3][1],
        matrix[3][2],
        matrix[3][3],
    ]
}

pub(crate) fn lighting_uniform_from_world(world: &RenderWorld) -> LightingUniform {
    let mut uniform = LightingUniform::default();
    let mut count = 0usize;

    for light in select_forward_lights(world) {
        let mut packed = forward_light_uniform(light);
        if count == 0 && light.kind == RenderLightKind::Directional {
            packed.spot_angles[2] = 1.0;
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
        };
        count = 1;
    }

    uniform.params = [count as u32, 0, 0, 0];
    uniform
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
                probes.push(GiProbe {
                    irradiance: [
                        probe_irradiance_at(world, position).x,
                        probe_irradiance_at(world, position).y,
                        probe_irradiance_at(world, position).z,
                        1.0,
                    ],
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

pub(crate) fn probe_irradiance_at(
    world: &RenderWorld,
    position: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    let sky_visibility = sky_visibility_at(world, position);
    let mut irradiance = engine_core::math::Vec3::new(0.025, 0.028, 0.032) * sky_visibility;
    if let Some(skybox) = &world.skybox {
        let sky = engine_core::math::Vec3::new(
            (skybox.zenith_color[0] + skybox.horizon_color[0]) * 0.5,
            (skybox.zenith_color[1] + skybox.horizon_color[1]) * 0.5,
            (skybox.zenith_color[2] + skybox.horizon_color[2]) * 0.5,
        ) * (0.28 * skybox.intensity * sky_visibility);
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
                let visibility = ray_visibility(world, position, dir, 80.0);
                irradiance += color * (0.1 * visibility);
            }
            engine_render::RenderLightKind::Point | engine_render::RenderLightKind::Spot => {
                let to_probe = position - light.transform.translation;
                let distance = to_probe.length();
                let range = light.range.max(0.001);
                let attenuation = (1.0 - distance / range).max(0.0).powi(2);
                let visibility = ray_visibility(
                    world,
                    position,
                    (light.transform.translation - position).normalized(),
                    distance,
                );
                irradiance += color * attenuation * visibility * 0.26;
            }
        }
    }
    irradiance += local_bounce_irradiance(world, position);
    clamp_vec3(irradiance, 0.0, 6.0)
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
        let area = (radius * radius / distance_sq).clamp(0.0, 1.0);
        let non_metal = 1.0 - material.metallic.clamp(0.0, 1.0);
        bounce += (albedo * non_metal * 0.18 + emissive * 0.35) * area * visibility;
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
        .filter(|light| light.kind == RenderLightKind::Directional && light.intensity > 0.0)
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

pub(crate) fn local_light_score(world: &RenderWorld, light: &RenderLight) -> Option<f32> {
    if light.intensity <= 0.0 || light.range <= 0.0 {
        return None;
    }

    let range = light.range.max(0.001);
    let camera = world.camera.as_ref();
    let Some(camera) = camera else {
        return Some(light.intensity * range);
    };

    let to_light = light.transform.translation - camera.transform.translation;
    let distance = to_light.length();
    if distance - range > camera.far {
        return None;
    }

    let distance_sq = to_light.length_squared().max(1.0);
    Some(light.intensity * range * range / distance_sq)
}

pub(crate) fn forward_light_uniform(light: &RenderLight) -> ForwardLightUniform {
    let light_type = match light.kind {
        RenderLightKind::Point => 1.0,
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

    ForwardLightUniform {
        position_type: [
            light.transform.translation.x,
            light.transform.translation.y,
            light.transform.translation.z,
            light_type,
        ],
        direction_range: [direction.x, direction.y, direction.z, range],
        color_intensity: [
            light.color.x.clamp(0.0, 1.0),
            light.color.y.clamp(0.0, 1.0),
            light.color.z.clamp(0.0, 1.0),
            light.intensity.max(0.0),
        ],
        spot_angles: [inner_half_angle.cos(), outer_half_angle.cos(), 0.0, 0.0],
    }
}
