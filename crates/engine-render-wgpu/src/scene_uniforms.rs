use crate::{math::*, uniforms::*};
use engine_render::RenderWorld;

pub(crate) fn skybox_uniform_from_world(world: &RenderWorld, use_cubemap: bool) -> SkyboxUniform {
    let sky = world.environment.as_ref().and_then(|environment| {
        environment.sky_enabled.then_some((
            environment.sky_zenith_color,
            environment.sky_horizon_color,
            environment.sky_rotation_degrees,
            environment.sky_intensity,
        ))
    });
    let sky = sky.or_else(|| {
        world.skybox.as_ref().map(|skybox| {
            (
                skybox.zenith_color,
                skybox.horizon_color,
                skybox.rotation_degrees,
                skybox.intensity,
            )
        })
    });
    let Some((zenith_color, horizon_color, rotation_degrees, intensity)) = sky else {
        return SkyboxUniform {
            view_rotation_only: IDENTITY_MAT4,
            zenith_color: [0.15, 0.35, 0.65, 1.0],
            horizon_color: [0.55, 0.7, 0.85, 1.0],
            rotation_intensity: [0.0, 1.0, 0.0, 0.0],
            use_cubemap: [0, 0, 0, 0],
        };
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
        zenith_color: [zenith_color[0], zenith_color[1], zenith_color[2], 1.0],
        horizon_color: [horizon_color[0], horizon_color[1], horizon_color[2], 1.0],
        rotation_intensity: [rotation_degrees, intensity, 0.0, 0.0],
        use_cubemap: [u32::from(use_cubemap), 0, 0, 0],
    }
}

pub(crate) fn fog_uniform_from_world(world: &RenderWorld) -> FogUniform {
    match world
        .environment
        .as_ref()
        .map(|environment| &environment.fog)
        .or(world.fog.as_ref())
    {
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
