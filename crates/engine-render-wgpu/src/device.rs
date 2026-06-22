use std::{collections::HashMap, sync::Arc};

use crate::meshes::{MeshBuffers, SkinnedMeshBuffers};
use engine_core::{Handle, HandleAllocator};
use engine_render::{
    ImageDesc, RenderGraph, RenderPerformanceConfig, RenderPerformanceMetrics, RenderStage,
    RenderTarget, RenderTargetDesc, TemporalCameraData, TemporalFrameState,
};

/// Native output capabilities exposed by the selected graphics adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WgpuOutputCapabilities {
    /// Adapter name reported by the graphics backend.
    pub adapter_name: String,
    /// Maximum supported 2D texture dimension.
    pub max_texture_dimension_2d: u32,
    /// Whether native 4K render targets fit within adapter limits.
    pub supports_4k_render_targets: bool,
    /// Whether GPU timestamp queries are supported.
    pub supports_timestamp_queries: bool,
    /// Active presentation mode, when a surface is configured.
    pub present_mode: Option<String>,
}
pub(crate) const DEFAULT_WIDTH: u32 = 960;
pub(crate) const DEFAULT_HEIGHT: u32 = 540;
pub(crate) const CUBE_INDEX_COUNT: u32 = 36;
pub(crate) const MAX_FORWARD_LIGHTS: usize = 32;
pub(crate) const MAX_DIRECTIONAL_LIGHTS: usize = 2;
pub(crate) const DEFAULT_AMBIENT_LIGHT: [f32; 4] = [0.16, 0.16, 0.16, 1.0];
pub(crate) const CSM_CASCADE_COUNT: usize = 5;
pub(crate) const CSM_SHADOW_RESOLUTION: u32 = 4096;
pub(crate) const CSM_CASCADE_SPLITS: [f32; CSM_CASCADE_COUNT] = [8.0, 20.0, 55.0, 120.0, 200.0];
pub(crate) const CSM_CASCADE_FADE_RANGE: f32 = 4.0;
pub(crate) const MAX_BLOOM_MIPS: u32 = 5;
pub(crate) const IBL_IRRADIANCE_RES: u32 = 32;
pub(crate) const IBL_PREFILTER_RES: u32 = 512;
pub(crate) const IBL_BRDF_LUT_RES: u32 = 256;
pub(crate) const SSAO_KERNEL_SIZE: u32 = 32;
pub(crate) const SSAO_NOISE_RES: u32 = 4;
pub(crate) const SSAO_RADIUS: f32 = 0.5;
pub(crate) const SSAO_BIAS: f32 = 0.025;
pub(crate) const SSGI_RADIUS: f32 = 2.5;
pub(crate) const SSGI_INTENSITY: f32 = 0.65;
pub(crate) const INTERMEDIATE_WIDTH: u32 = 1920;
pub(crate) const INTERMEDIATE_HEIGHT: u32 = 1080;

/// Rectangle on a configured native surface where a render result is presented.
///
/// Hosts can keep a full-window swapchain while asking the Frame Pipeline to
/// composite only into the editor Scene View aperture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceViewportRect {
    /// Left edge in surface pixels.
    pub x: u32,
    /// Top edge in surface pixels.
    pub y: u32,
    /// Width in surface pixels.
    pub width: u32,
    /// Height in surface pixels.
    pub height: u32,
}

impl SurfaceViewportRect {
    /// Creates a surface viewport rectangle, clamping zero dimensions to one.
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// Returns this rectangle clamped to a surface of the given size.
    pub fn clamped_to(self, surface_width: u32, surface_height: u32) -> Self {
        self.clamp_to(surface_width, surface_height)
    }

    pub(crate) fn clamp_to(self, surface_width: u32, surface_height: u32) -> Self {
        let surface_width = surface_width.max(1);
        let surface_height = surface_height.max(1);
        let x = self.x.min(surface_width.saturating_sub(1));
        let y = self.y.min(surface_height.saturating_sub(1));
        Self {
            x,
            y,
            width: self.width.min(surface_width - x).max(1),
            height: self.height.min(surface_height - y).max(1),
        }
    }
}

pub(crate) struct GpuImage {
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) _desc: ImageDesc,
}

pub(crate) struct MaterialGpuData {
    pub(crate) bind_group: wgpu::BindGroup,
}

pub(crate) struct GpuTarget {
    pub(crate) _color: wgpu::Texture,
    pub(crate) color_view: wgpu::TextureView,
    pub(crate) _depth: Option<wgpu::Texture>,
    pub(crate) depth_view: Option<wgpu::TextureView>,
    pub(crate) _desc: RenderTargetDesc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FramePipelineStep {
    Shadow,
    GBuffer,
    DeferredLighting,
    Forward,
    TemporalInputs,
    Upscale,
    Post,
    Ui,
    Outline,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FramePipelinePlan {
    pub(crate) shadow: bool,
    pub(crate) gbuffer: bool,
    pub(crate) deferred_lighting: bool,
    pub(crate) forward: bool,
    pub(crate) temporal_inputs: bool,
    pub(crate) upscale: bool,
    pub(crate) post: bool,
    pub(crate) ui: bool,
    pub(crate) pass_count: u32,
    pub(crate) steps: Vec<FramePipelineStep>,
}

impl Default for FramePipelinePlan {
    fn default() -> Self {
        Self {
            shadow: true,
            gbuffer: false,
            deferred_lighting: false,
            forward: true,
            temporal_inputs: true,
            upscale: true,
            post: true,
            ui: true,
            pass_count: 6,
            steps: vec![
                FramePipelineStep::Shadow,
                FramePipelineStep::Forward,
                FramePipelineStep::TemporalInputs,
                FramePipelineStep::Upscale,
                FramePipelineStep::Post,
                FramePipelineStep::Ui,
            ],
        }
    }
}

impl FramePipelinePlan {
    pub(crate) fn from_graph(graph: &RenderGraph) -> Self {
        let steps = graph
            .passes
            .iter()
            .filter_map(|pass| match pass.name.as_str() {
                "shadow" => Some(FramePipelineStep::Shadow),
                "gbuffer" => Some(FramePipelineStep::GBuffer),
                "deferred-lighting" | "deferred_lighting" => {
                    Some(FramePipelineStep::DeferredLighting)
                }
                "forward" => Some(FramePipelineStep::Forward),
                "temporal-inputs" => Some(FramePipelineStep::TemporalInputs),
                "upscale" => Some(FramePipelineStep::Upscale),
                "post" => Some(FramePipelineStep::Post),
                "ui" | "gui" => Some(FramePipelineStep::Ui),
                "outline" => Some(FramePipelineStep::Outline),
                _ => match pass.stage {
                    RenderStage::TemporalInputs => Some(FramePipelineStep::TemporalInputs),
                    RenderStage::Upscale => Some(FramePipelineStep::Upscale),
                    RenderStage::PostUpscale => Some(FramePipelineStep::Post),
                    RenderStage::UiComposition => Some(FramePipelineStep::Ui),
                    RenderStage::PreUpscale => None,
                },
            })
            .collect();
        Self {
            shadow: graph.contains_pass("shadow"),
            gbuffer: graph.contains_pass("gbuffer"),
            deferred_lighting: graph.contains_pass("deferred-lighting")
                || graph.contains_pass("deferred_lighting"),
            forward: graph.contains_pass("forward"),
            temporal_inputs: graph.contains_stage(RenderStage::TemporalInputs),
            upscale: graph.contains_stage(RenderStage::Upscale),
            post: graph.contains_pass("post") || graph.contains_stage(RenderStage::PostUpscale),
            ui: graph.contains_pass("ui")
                || graph.contains_pass("gui")
                || graph.contains_stage(RenderStage::UiComposition),
            pass_count: graph.pass_count() as u32,
            steps,
        }
    }
}

/// Real wgpu render device with an offscreen default target.
pub struct WgpuRenderDevice {
    pub(crate) _instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) image_allocator: HandleAllocator,
    pub(crate) buffer_allocator: HandleAllocator,
    pub(crate) target_allocator: HandleAllocator,
    pub(crate) images: HashMap<Handle, GpuImage>,
    pub(crate) buffers: HashMap<Handle, wgpu::Buffer>,
    pub(crate) bone_palette_counts: HashMap<Handle, u32>,
    pub(crate) targets: HashMap<Handle, GpuTarget>,
    pub(crate) default_target: RenderTarget,
    pub(crate) game_target: RenderTarget,
    pub(crate) preview_target: RenderTarget,
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) transparent_pipeline: wgpu::RenderPipeline,
    pub(crate) camera_bind_group: wgpu::BindGroup,
    pub(crate) camera_uniform: wgpu::Buffer,
    pub(crate) temporal_uniform: wgpu::Buffer,
    pub(crate) lighting_uniform: wgpu::Buffer,
    pub(crate) _default_texture: wgpu::Texture,
    pub(crate) default_texture_view: wgpu::TextureView,
    pub(crate) _default_normal_texture: wgpu::Texture,
    pub(crate) default_normal_texture_view: wgpu::TextureView,
    pub(crate) _default_mra_texture: wgpu::Texture,
    pub(crate) default_mra_texture_view: wgpu::TextureView,
    pub(crate) _default_sampler: wgpu::Sampler,
    pub(crate) material_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) default_material_bind_group: wgpu::BindGroup,
    pub(crate) material_gpu: HashMap<String, MaterialGpuData>,
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    pub(crate) instance_buffer: wgpu::Buffer,
    pub(crate) instance_capacity: usize,
    pub(crate) mesh_cache: HashMap<String, MeshBuffers>,
    pub(crate) skinned_mesh_cache: HashMap<String, SkinnedMeshBuffers>,
    pub(crate) skinned_pipeline: wgpu::RenderPipeline,
    pub(crate) skinned_camera_bind_group: wgpu::BindGroup,
    pub(crate) skinned_bone_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) surface: Option<wgpu::Surface<'static>>,
    pub(crate) surface_config: Option<wgpu::SurfaceConfiguration>,
    pub(crate) surface_depth: Option<wgpu::Texture>,
    pub(crate) surface_depth_view: Option<wgpu::TextureView>,
    pub(crate) surface_suspended: bool,
    pub(crate) surface_viewport: Option<SurfaceViewportRect>,
    pub(crate) next_gui_texture: u64,
    pub(crate) gui_textures: HashMap<u64, Handle>,
    pub(crate) gui_pipeline: wgpu::RenderPipeline,
    pub(crate) gui_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) gui_sampler: wgpu::Sampler,
    pub(crate) gui_uniform: wgpu::Buffer,
    pub(crate) gui_vertex_buffer: wgpu::Buffer,
    pub(crate) gui_index_buffer: wgpu::Buffer,
    pub(crate) gui_vertex_capacity: usize,
    pub(crate) gui_index_capacity: usize,
    pub(crate) submitted_worlds: u64,
    pub(crate) grid_pipeline: wgpu::RenderPipeline,
    pub(crate) grid_bind_group: wgpu::BindGroup,
    pub(crate) grid_vertex_buffer: wgpu::Buffer,
    pub(crate) grid_vertex_count: u32,
    pub(crate) csm_depth_views: [wgpu::TextureView; CSM_CASCADE_COUNT],
    pub(crate) _csm_depth_textures: [wgpu::Texture; CSM_CASCADE_COUNT],
    pub(crate) _csm_sampler: wgpu::Sampler,
    pub(crate) csm_uniform: wgpu::Buffer,
    /// Pre-allocated per-cascade uniform buffer (updated via write_buffer each frame).
    pub(crate) csm_cascade_uniforms: [wgpu::Buffer; CSM_CASCADE_COUNT],
    /// Pre-allocated per-cascade bind group.
    pub(crate) csm_cascade_bind_groups: [wgpu::BindGroup; CSM_CASCADE_COUNT],
    pub(crate) shadow_pipeline: wgpu::RenderPipeline,
    pub(crate) shadow_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) material_cache: HashMap<String, ([f32; 4], f32, f32, [f32; 3])>,
    pub(crate) skybox_pipeline: wgpu::RenderPipeline,
    pub(crate) skybox_bind_group: wgpu::BindGroup,
    pub(crate) skybox_uniform: wgpu::Buffer,
    pub(crate) fog_uniform: wgpu::Buffer,
    pub(crate) _skybox_default_cubemap: wgpu::Texture,
    pub(crate) _skybox_default_cubemap_view: wgpu::TextureView,
    pub(crate) _skybox_sampler: wgpu::Sampler,
    /// Frame-lagged destruction queue: (frame_index, resource).
    pub(crate) destroy_queue: Vec<(u64, DestroyResource)>,
    // IBL resources
    pub(crate) ibl_irradiance_map: wgpu::Texture,
    pub(crate) ibl_irradiance_view: wgpu::TextureView,
    pub(crate) ibl_prefilter_map: wgpu::Texture,
    pub(crate) ibl_prefilter_views: Vec<wgpu::TextureView>,
    pub(crate) ibl_brdf_lut: wgpu::Texture,
    pub(crate) ibl_brdf_lut_view: wgpu::TextureView,
    pub(crate) ibl_sampler: wgpu::Sampler,
    pub(crate) ibl_enabled: bool,
    pub(crate) ibl_scratch_tex: Option<wgpu::Texture>,
    pub(crate) ibl_scratch_view: Option<wgpu::TextureView>,
    pub(crate) ibl_bake_bgl: Option<wgpu::BindGroupLayout>,
    // Post-processing pipeline
    pub(crate) post_pipeline: wgpu::RenderPipeline,
    pub(crate) post_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) post_bind_group: wgpu::BindGroup,
    /// Frame-level post bind group, cached per output resolution.
    pub(crate) post_cached_bg: Option<Arc<wgpu::BindGroup>>,
    pub(crate) post_cached_dims: (u32, u32),
    pub(crate) post_uniform: wgpu::Buffer,
    // Bloom resources
    pub(crate) bloom_pipeline_downsample: Option<wgpu::RenderPipeline>,
    pub(crate) bloom_pipeline_upsample: Option<wgpu::RenderPipeline>,
    pub(crate) bloom_bind_group_layout: Option<wgpu::BindGroupLayout>,
    pub(crate) bloom_mip_views: Vec<wgpu::TextureView>,
    pub(crate) bloom_mip_textures: Vec<wgpu::Texture>,
    /// Cached bloom compute bind groups per mip, regenerated on resize.
    pub(crate) bloom_cached_down_bgs: Vec<Arc<wgpu::BindGroup>>,
    pub(crate) bloom_cached_up_bgs: Vec<Arc<wgpu::BindGroup>>,
    pub(crate) bloom_cached_dims: (u32, u32),
    pub(crate) bloom_sampler: wgpu::Sampler,
    pub(crate) bloom_uniform: wgpu::Buffer,
    // SSAO resources
    pub(crate) ssao_pipeline: Option<wgpu::RenderPipeline>,
    pub(crate) ssao_bind_group_layout: Option<wgpu::BindGroupLayout>,
    pub(crate) ssao_bind_group: Option<wgpu::BindGroup>,
    pub(crate) ssao_cached_bg: Option<Arc<wgpu::BindGroup>>,
    pub(crate) ssao_cached_dims: (u32, u32),
    pub(crate) ssao_noise_texture: wgpu::Texture,
    pub(crate) ssao_noise_view: wgpu::TextureView,
    pub(crate) ssao_samples_buffer: wgpu::Buffer,
    pub(crate) ssao_linear_sampler: wgpu::Sampler,
    pub(crate) ssao_output_texture: Option<wgpu::Texture>,
    pub(crate) ssao_output_view: Option<wgpu::TextureView>,
    pub(crate) ssao_uniform: wgpu::Buffer,
    // Real-time screen-space global illumination resources
    pub(crate) ssgi_cached_bg: Option<Arc<wgpu::BindGroup>>,
    pub(crate) ssgi_output_texture: Option<wgpu::Texture>,
    pub(crate) ssgi_output_view: Option<wgpu::TextureView>,
    pub(crate) ssgi_history_texture: Option<wgpu::Texture>,
    pub(crate) ssgi_history_view: Option<wgpu::TextureView>,
    pub(crate) ssgi_uniform: wgpu::Buffer,
    pub(crate) ssgi_compute_pipeline: Option<wgpu::ComputePipeline>,
    pub(crate) ssgi_compute_bgl: Option<wgpu::BindGroupLayout>,
    // HDR intermediate target
    pub(crate) hdr_target: Option<GpuTarget>,
    pub(crate) hdr_normal_texture: Option<wgpu::Texture>,
    pub(crate) hdr_normal_view: Option<wgpu::TextureView>,
    pub(crate) hdr_albedo_texture: Option<wgpu::Texture>,
    pub(crate) hdr_albedo_view: Option<wgpu::TextureView>,
    pub(crate) hdr_motion_texture: Option<wgpu::Texture>,
    pub(crate) hdr_motion_view: Option<wgpu::TextureView>,
    pub(crate) post_target_width: u32,
    pub(crate) post_target_height: u32,
    pub(crate) ibl_irradiance_compute: Option<wgpu::ComputePipeline>,
    pub(crate) ibl_prefilter_compute: Option<wgpu::ComputePipeline>,
    pub(crate) ibl_brdf_compute: Option<wgpu::ComputePipeline>,
    pub(crate) ibl_brdf_bgl: Option<wgpu::BindGroupLayout>,
    pub(crate) ssao_compute_pipeline: Option<wgpu::ComputePipeline>,
    pub(crate) ssao_compute_bgl: Option<wgpu::BindGroupLayout>,
    pub(crate) bloom_compute_down: Option<wgpu::ComputePipeline>,
    pub(crate) bloom_compute_up: Option<wgpu::ComputePipeline>,
    pub(crate) bloom_compute_bgl: Option<wgpu::BindGroupLayout>,
    /// Pre-allocated staging buffer for readback (avoids per-frame allocation).
    pub(crate) readback_staging: Option<wgpu::Buffer>,
    /// Current staging buffer dimensions (width, height).
    pub(crate) readback_staging_dims: (u32, u32),
    pub(crate) performance_config: RenderPerformanceConfig,
    pub(crate) dynamic_resolution: engine_render::DynamicResolutionController,
    pub(crate) performance_metrics: RenderPerformanceMetrics,
    pub(crate) active_upscaler: engine_render::UpscalerKind,
    pub(crate) upscale_sharpness: f32,
    pub(crate) temporal_state: TemporalFrameState,
    pub(crate) latest_temporal_camera: TemporalCameraData,
    pub(crate) reset_temporal_history: bool,
    pub(crate) active_frame_plan: Option<FramePipelinePlan>,
    pub(crate) latest_submitted_objects: u32,
    pub(crate) latest_visible_objects: u32,
    pub(crate) latest_culled_objects: u32,
    pub(crate) latest_submitted_lights: u32,
    pub(crate) latest_visible_lights: u32,
    pub(crate) latest_culled_lights: u32,
    pub(crate) latest_draw_calls: u32,
    pub(crate) latest_triangles: u64,
    pub(crate) gpu_particles: crate::particles::GpuParticlePipeline,
    pub(crate) gpu_timestamp_query: Option<wgpu::QuerySet>,
    pub(crate) gpu_timestamp_resolve: Option<wgpu::Buffer>,
    pub(crate) gpu_timestamp_readback: Option<wgpu::Buffer>,
    pub(crate) gpu_timestamp_receiver:
        Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
}
pub(crate) struct FrameResources {
    pub(crate) ssao_bg: Option<Arc<wgpu::BindGroup>>,
    pub(crate) ssao_view: Option<wgpu::TextureView>,
    pub(crate) ssgi_bg: Option<Arc<wgpu::BindGroup>>,
    pub(crate) ssgi_view: Option<wgpu::TextureView>,
    pub(crate) bloom_down_bgs: Vec<Arc<wgpu::BindGroup>>,
    pub(crate) bloom_up_bgs: Vec<Arc<wgpu::BindGroup>>,
    pub(crate) post_bg: Option<Arc<wgpu::BindGroup>>,
}

/// A GPU resource pending deferred destruction.
#[allow(dead_code)]
pub(crate) enum DestroyResource {
    /// Full render target bundle.
    Target(GpuTarget),
    /// wgpu Texture (dropped when all GPU command buffers referencing it have completed).
    Texture(wgpu::Texture),
    /// wgpu Buffer.
    Buffer(wgpu::Buffer),
    /// wgpu TextureView.
    TextureView(wgpu::TextureView),
}
impl std::fmt::Debug for WgpuRenderDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuRenderDevice")
            .field("adapter", &self.adapter.get_info().name)
            .field("default_target", &self.default_target)
            .field("game_target", &self.game_target)
            .field("preview_target", &self.preview_target)
            .field("image_count", &self.images.len())
            .field("buffer_count", &self.buffers.len())
            .field("target_count", &self.targets.len())
            .field("submitted_worlds", &self.submitted_worlds)
            .finish()
    }
}

impl WgpuRenderDevice {
    /// Returns a shared reference to the wgpu device.
    pub fn device_arc(&self) -> Arc<wgpu::Device> {
        Arc::clone(&self.device)
    }

    /// Returns a shared reference to the wgpu queue.
    pub fn queue_arc(&self) -> Arc<wgpu::Queue> {
        Arc::clone(&self.queue)
    }

    /// Returns a reference to the default offscreen render target's color texture view.
    pub fn default_target_view(&self) -> &wgpu::TextureView {
        self.targets
            .get(&self.default_target.handle)
            .map(|t| &t.color_view)
            .expect("default target exists")
    }

    /// Returns the pixel dimensions of the default offscreen render target.
    pub fn default_target_size(&self) -> (u32, u32) {
        self.default_target.size()
    }

    /// Returns a reference to the game offscreen render target's color texture view.
    pub fn game_target_view(&self) -> &wgpu::TextureView {
        self.targets
            .get(&self.game_target.handle)
            .map(|t| &t.color_view)
            .expect("game target exists")
    }

    /// Returns the pixel dimensions of the game offscreen render target.
    pub fn game_target_size(&self) -> (u32, u32) {
        self.game_target.size()
    }

    /// Returns a reference to the preview offscreen render target's color texture view.
    pub fn preview_target_view(&self) -> &wgpu::TextureView {
        self.targets
            .get(&self.preview_target.handle)
            .map(|t| &t.color_view)
            .expect("preview target exists")
    }

    /// Returns the pixel dimensions of the preview offscreen render target.
    pub fn preview_target_size(&self) -> (u32, u32) {
        self.preview_target.size()
    }
}
