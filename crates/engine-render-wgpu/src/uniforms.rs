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
pub(crate) struct TemporalUniform {
    pub(crate) previous_view_projection: [[f32; 4]; 4],
    pub(crate) current_view_projection: [[f32; 4]; 4],
    pub(crate) jitter_reset: [f32; 4],
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
    pub(crate) quality: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct LightingUniform {
    pub(crate) ambient: [f32; 4],
    pub(crate) params: [u32; 4],
    pub(crate) lights: [ForwardLightUniform; MAX_FORWARD_LIGHTS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GiProbeUniform {
    pub(crate) center: [f32; 4],
    pub(crate) extent: [f32; 4],
    pub(crate) counts_intensity: [f32; 4],
    pub(crate) params: [u32; 4],
}

impl Default for GiProbeUniform {
    fn default() -> Self {
        Self {
            center: [0.0; 4],
            extent: [1.0, 1.0, 1.0, 0.0],
            counts_intensity: [1.0, 1.0, 1.0, 0.0],
            params: [0, 0, 0, 0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GiProbe {
    pub(crate) irradiance_pos_x: [f32; 4],
    pub(crate) irradiance_neg_x: [f32; 4],
    pub(crate) irradiance_pos_y: [f32; 4],
    pub(crate) irradiance_neg_y: [f32; 4],
    pub(crate) irradiance_pos_z: [f32; 4],
    pub(crate) irradiance_neg_z: [f32; 4],
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
    /// x: cascade fade range, y: shadow map texel size, z: constant depth bias, w: slope bias.
    pub(crate) params: [f32; 4],
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
    pub(crate) history_blend: f32,
    pub(crate) reset_history: f32,
    pub(crate) _pad: f32,
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
    pub(crate) inv_view_projection: [[f32; 4]; 4],
    pub(crate) view_projection: [[f32; 4]; 4],
    pub(crate) camera_position: [f32; 4],
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
    pub(crate) ssr_enabled: f32,
    pub(crate) ssr_intensity: f32,
    pub(crate) taa_reset: f32,
    pub(crate) taa_history_weight: f32,
    pub(crate) taa_enabled: f32,
    pub(crate) _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuGuiVertex {
    pub(crate) position: [f32; 2],
    pub(crate) uv: [f32; 2],
    pub(crate) color: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GuiUniform {
    pub(crate) screen_size: [f32; 2],
    pub(crate) _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SkinnedVertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) uv: [f32; 2],
    pub(crate) joints: [u32; 4],
    pub(crate) weights: [f32; 4],
}
