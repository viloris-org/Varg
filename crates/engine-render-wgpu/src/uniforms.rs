use crate::{device::*, math::IDENTITY_MAT4};
use bytemuck::{Pod, Zeroable};

/// GPU vertex layout: position (3×f32), normal (3×f32), UV (2×f32).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) uv: [f32; 2],
    pub(crate) tangent: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct Instance {
    pub(crate) offset: [f32; 3],
    pub(crate) scale: [f32; 3],
    pub(crate) color: [f32; 4],
    pub(crate) rotation: [f32; 4],
    pub(crate) metallic: f32,
    pub(crate) roughness: f32,
    pub(crate) emissive: [f32; 3],
    pub(crate) receive_shadows: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct CameraUniform {
    pub(crate) view_projection: [[f32; 4]; 4],
    pub(crate) camera_position: [f32; 4],
    pub(crate) camera_forward: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct ModelUniform {
    pub(crate) model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct ForwardLightUniform {
    pub(crate) position_type: [f32; 4],
    pub(crate) direction_range: [f32; 4],
    pub(crate) color_intensity: [f32; 4],
    pub(crate) spot_angles: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct LightingUniform {
    pub(crate) ambient: [f32; 4],
    pub(crate) params: [u32; 4],
    pub(crate) lights: [ForwardLightUniform; MAX_FORWARD_LIGHTS],
}

impl Default for LightingUniform {
    fn default() -> Self {
        Self {
            ambient: DEFAULT_AMBIENT_LIGHT,
            params: [0, 0, 0, 0],
            lights: [ForwardLightUniform::zeroed(); MAX_FORWARD_LIGHTS],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct ShadowUniform {
    pub(crate) light_view_projection: [[f32; 4]; 4],
}

impl ShadowUniform {
    #[allow(dead_code)]
    fn zeroed() -> Self {
        Self {
            light_view_projection: IDENTITY_MAT4,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct CsmUniform {
    /// Light-space VP matrix for each cascade.
    pub(crate) cascade_vps: [[[f32; 4]; 4]; CSM_CASCADE_COUNT],
    /// Split depth for each cascade in view space (vec4 for alignment, only first 4 used).
    pub(crate) cascade_splits: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SkyboxUniform {
    pub(crate) view_rotation_only: [[f32; 4]; 4],
    pub(crate) zenith_color: [f32; 4],
    pub(crate) horizon_color: [f32; 4],
    pub(crate) rotation_intensity: [f32; 4],
    pub(crate) use_cubemap: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct ExposureUniform {
    pub(crate) exposure: f32,
    pub(crate) _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct BloomUniform {
    pub(crate) intensity: f32,
    pub(crate) threshold: f32,
    pub(crate) knee: f32,
    pub(crate) _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SsaoUniform {
    pub(crate) radius: f32,
    pub(crate) bias: f32,
    pub(crate) intensity: f32,
    pub(crate) _pad: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) inv_width: f32,
    pub(crate) inv_height: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SsaoSample {
    pub(crate) dir: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SsgiUniform {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) inv_width: f32,
    pub(crate) inv_height: f32,
    pub(crate) radius: f32,
    pub(crate) intensity: f32,
    pub(crate) thickness: f32,
    pub(crate) sample_count: f32,
    pub(crate) frame_index: f32,
    pub(crate) _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct FogUniform {
    pub(crate) density: f32,
    pub(crate) _pad: [f32; 3],
    pub(crate) color: [f32; 3],
    pub(crate) enabled: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct PostProcessUniform {
    pub(crate) render_width: f32,
    pub(crate) render_height: f32,
    pub(crate) inv_render_width: f32,
    pub(crate) inv_render_height: f32,
    pub(crate) output_width: f32,
    pub(crate) output_height: f32,
    pub(crate) inv_output_width: f32,
    pub(crate) inv_output_height: f32,
    pub(crate) exposure: f32,
    pub(crate) bloom_intensity: f32,
    pub(crate) ssao_enabled: f32,
    pub(crate) upscale_sharpness: f32,
    pub(crate) ssgi_enabled: f32,
    pub(crate) ssgi_intensity: f32,
    pub(crate) _pad: [f32; 2],
}
