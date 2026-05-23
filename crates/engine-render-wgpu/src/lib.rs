#![deny(missing_docs)]

//! WebGPU rendering backend for Aster.

use std::{collections::HashMap, sync::Arc};

use bytemuck::{Pod, Zeroable};
use engine_core::{EngineError, EngineResult, Handle, HandleAllocator};
use engine_render::{
    BufferDesc, BufferHandle, BufferUsage, GuiDrawList, GuiTextureId, ImageDesc, ImageFormat,
    ImageHandle, ImageUsage, RenderApi, RenderDevice, RenderFrame, RenderGraph, RenderLight,
    RenderTarget, RenderTargetDesc, RenderWorld, ViewKind,
};
use wgpu::util::DeviceExt;

/// Re-exported wgpu API used by window hosts that need to create surfaces.
pub use wgpu;

const DEFAULT_WIDTH: u32 = 960;
const DEFAULT_HEIGHT: u32 = 540;
const CUBE_INDEX_COUNT: u32 = 36;
const MAX_FORWARD_LIGHTS: usize = 8;
const DEFAULT_AMBIENT_LIGHT: [f32; 4] = [0.16, 0.16, 0.16, 1.0];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    offset: [f32; 3],
    scale: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_projection: [[f32; 4]; 4],
    camera_position: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ModelUniform {
    model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    metallic: f32,
    roughness: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ForwardLightUniform {
    position_type: [f32; 4],
    direction_range: [f32; 4],
    color_intensity: [f32; 4],
    spot_angles: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightingUniform {
    ambient: [f32; 4],
    params: [u32; 4],
    lights: [ForwardLightUniform; MAX_FORWARD_LIGHTS],
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

struct GpuImage {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    _desc: ImageDesc,
}

struct GpuTarget {
    _color: wgpu::Texture,
    color_view: wgpu::TextureView,
    _depth: Option<wgpu::Texture>,
    depth_view: Option<wgpu::TextureView>,
    _desc: RenderTargetDesc,
}

/// Configuration for creating an offscreen wgpu renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WgpuOffscreenConfig {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Color attachment format.
    pub format: ImageFormat,
}

impl Default for WgpuOffscreenConfig {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            format: ImageFormat::Rgba8Srgb,
        }
    }
}

/// Procedural debug mesh shapes for quick visualisation without external assets.
#[derive(Clone, Debug, PartialEq)]
pub enum DebugMesh {
    /// Unit cube centred at origin, edge length 1, hard normals.
    Cube,
    /// UV sphere with the given longitudinal/latitudinal segment count.
    Sphere(u32),
    /// Quad on the XY plane from (-0.5, -0.5, 0) to (0.5, 0.5, 0).
    Plane,
}

/// GPU buffers for a single indexed mesh, ready for drawing.
#[derive(Debug)]
pub struct MeshBuffers {
    /// Vertex buffer uploaded to the GPU.
    pub vertex_buffer: wgpu::Buffer,
    /// Index buffer uploaded to the GPU.
    pub index_buffer: wgpu::Buffer,
    /// Number of indices to draw.
    pub index_count: u32,
}

/// Real wgpu render device with an offscreen default target.
pub struct WgpuRenderDevice {
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    image_allocator: HandleAllocator,
    buffer_allocator: HandleAllocator,
    target_allocator: HandleAllocator,
    images: HashMap<Handle, GpuImage>,
    buffers: HashMap<Handle, wgpu::Buffer>,
    targets: HashMap<Handle, GpuTarget>,
    default_target: RenderTarget,
    game_target: RenderTarget,
    pipeline: wgpu::RenderPipeline,
    camera_bind_group: wgpu::BindGroup,
    camera_uniform: wgpu::Buffer,
    lighting_uniform: wgpu::Buffer,
    _default_texture: wgpu::Texture,
    _default_texture_view: wgpu::TextureView,
    _default_sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    mesh_cache: HashMap<String, MeshBuffers>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    surface_depth: Option<wgpu::Texture>,
    surface_depth_view: Option<wgpu::TextureView>,
    surface_suspended: bool,
    next_gui_texture: u64,
    submitted_worlds: u64,
    grid_pipeline: wgpu::RenderPipeline,
    grid_bind_group: wgpu::BindGroup,
    grid_vertex_buffer: wgpu::Buffer,
    grid_vertex_count: u32,
    /// Frame-lagged destruction queue: (frame_index, resource).
    destroy_queue: Vec<(u64, DestroyResource)>,
}

/// A GPU resource pending deferred destruction.
#[allow(dead_code)]
enum DestroyResource {
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
            .field("image_count", &self.images.len())
            .field("buffer_count", &self.buffers.len())
            .field("target_count", &self.targets.len())
            .field("submitted_worlds", &self.submitted_worlds)
            .finish()
    }
}

impl WgpuRenderDevice {
    /// Creates a wgpu device and an offscreen render target.
    pub fn new_offscreen(config: WgpuOffscreenConfig) -> EngineResult<Self> {
        pollster::block_on(Self::new_offscreen_async(config))
    }

    /// Creates a wgpu device and an offscreen render target asynchronously.
    pub async fn new_offscreen_async(config: WgpuOffscreenConfig) -> EngineResult<Self> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await;
        let adapter = match adapter {
            Ok(a) => a,
            Err(_) => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::None,
                    compatible_surface: None,
                    force_fallback_adapter: true,
                })
                .await
                .map_err(|error| {
                    EngineError::other(format!("no suitable wgpu adapter found: {error}"))
                })?,
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| EngineError::other(format!("request wgpu device failed: {error}")))?;

        Self::from_device(
            instance,
            adapter,
            Arc::new(device),
            Arc::new(queue),
            config.width,
            config.height,
            config.format,
            None,
        )
    }

    /// Creates a wgpu device configured to present into a surface.
    pub fn new_surface(
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> EngineResult<Self> {
        pollster::block_on(Self::new_surface_async(surface, width, height))
    }

    /// Creates a wgpu device from a winit window, creating a surface automatically.
    pub fn new(window: &winit::window::Window) -> EngineResult<Self> {
        pollster::block_on(Self::new_async(window))
    }

    /// Creates a wgpu device from a winit window asynchronously.
    pub async fn new_async(window: &winit::window::Window) -> EngineResult<Self> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|error| EngineError::other(format!("create wgpu surface failed: {error}")))?;
        // SAFETY: instance is moved into the returned struct and outlives the surface.
        let surface: wgpu::Surface<'static> = unsafe { std::mem::transmute(surface) };
        let width = window.inner_size().width;
        let height = window.inner_size().height;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await;
        let adapter = match adapter {
            Ok(a) => a,
            Err(_) => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::None,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: true,
                })
                .await
                .map_err(|error| {
                    EngineError::other(format!("no suitable wgpu adapter found: {error}"))
                })?,
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| EngineError::other(format!("request wgpu device failed: {error}")))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8UnormSrgb)
            .or_else(|| {
                caps.formats
                    .iter()
                    .copied()
                    .find(wgpu::TextureFormat::is_srgb)
            })
            .unwrap_or(caps.formats[0]);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);
        let image_format = from_wgpu_format(format).unwrap_or(ImageFormat::Rgba8Srgb);

        Self::from_device(
            instance,
            adapter,
            Arc::new(device),
            Arc::new(queue),
            width,
            height,
            image_format,
            Some((surface, surface_config)),
        )
    }

    /// Creates a wgpu device configured to present into a surface asynchronously.
    pub async fn new_surface_async(
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> EngineResult<Self> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await;
        let adapter = match adapter {
            Ok(a) => a,
            Err(_) => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::None,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: true,
                })
                .await
                .map_err(|error| {
                    EngineError::other(format!("no suitable wgpu adapter found: {error}"))
                })?,
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| EngineError::other(format!("request wgpu device failed: {error}")))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8UnormSrgb)
            .or_else(|| {
                caps.formats
                    .iter()
                    .copied()
                    .find(wgpu::TextureFormat::is_srgb)
            })
            .unwrap_or(caps.formats[0]);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);
        let image_format = from_wgpu_format(format).unwrap_or(ImageFormat::Rgba8Srgb);

        Self::from_device(
            instance,
            adapter,
            Arc::new(device),
            Arc::new(queue),
            width,
            height,
            image_format,
            Some((surface, surface_config)),
        )
    }

    /// Creates a wgpu render device from pre-created shared device and queue.
    ///
    /// Use this when the host (e.g. CLI editor) already owns a wgpu device/queue
    /// that must be shared between the 3D renderer and egui compositor.
    pub fn from_arc_device(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        width: u32,
        height: u32,
        format: ImageFormat,
        surface_state: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)>,
    ) -> EngineResult<Self> {
        Self::from_device(
            instance,
            adapter,
            device,
            queue,
            width,
            height,
            format,
            surface_state,
        )
    }

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

    /// Prepares instance buffer from mesh batches for rendering.
    fn prepare_render_batches(&mut self, world: &RenderWorld) -> Vec<(String, u32)> {
        let batches = mesh_batches_from_world(world);
        let total_instances: usize = batches.iter().map(|(_, inst)| inst.len()).sum();
        if total_instances > self.instance_capacity {
            self.instance_capacity = total_instances.next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        let all_instances: Vec<Instance> = batches
            .iter()
            .flat_map(|(_, instances)| instances.iter().copied())
            .collect();
        if !all_instances.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&all_instances),
            );
        }
        batches
            .into_iter()
            .map(|(name, instances)| (name, instances.len() as u32))
            .collect()
    }

    /// Renders a render world to the default offscreen target, bypassing any surface.
    ///
    /// Use this when the host composites the result into its own UI (e.g., egui).
    pub fn render_world_offscreen(&mut self, world: &RenderWorld) -> EngineResult<()> {
        let batches = self.prepare_render_batches(world);
        let uniform = camera_uniform_from_world(world);
        self.queue
            .write_buffer(&self.camera_uniform, 0, bytemuck::bytes_of(&uniform));
        let lighting = lighting_uniform_from_world(world);
        self.queue
            .write_buffer(&self.lighting_uniform, 0, bytemuck::bytes_of(&lighting));

        let target = self
            .targets
            .get(&self.default_target.handle)
            .ok_or_else(|| EngineError::invalid_handle("default wgpu target is missing"))?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster offscreen render world encoder"),
            });
        encode_batched_forward_pass(
            &mut encoder,
            &target.color_view,
            target.depth_view.as_ref(),
            &self.pipeline,
            &self.camera_bind_group,
            &self.mesh_cache,
            &self.vertex_buffer,
            &self.index_buffer,
            &self.instance_buffer,
            &batches,
        );
        encode_grid_pass(
            &mut encoder,
            &target.color_view,
            target.depth_view.as_ref(),
            &self.grid_pipeline,
            &self.grid_bind_group,
            &self.grid_vertex_buffer,
            self.grid_vertex_count,
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        self.submitted_worlds = self.submitted_worlds.saturating_add(1);
        Ok(())
    }

    /// Renders a render world to the game offscreen target, bypassing any surface.
    ///
    /// Use this when the host composites the game view result into its own UI (e.g., egui).
    pub fn render_world_offscreen_game(&mut self, world: &RenderWorld) -> EngineResult<()> {
        let batches = self.prepare_render_batches(world);
        let uniform = camera_uniform_from_world(world);
        self.queue
            .write_buffer(&self.camera_uniform, 0, bytemuck::bytes_of(&uniform));
        let lighting = lighting_uniform_from_world(world);
        self.queue
            .write_buffer(&self.lighting_uniform, 0, bytemuck::bytes_of(&lighting));

        let target = self
            .targets
            .get(&self.game_target.handle)
            .ok_or_else(|| EngineError::invalid_handle("game wgpu target is missing"))?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster game offscreen render world encoder"),
            });
        encode_batched_forward_pass(
            &mut encoder,
            &target.color_view,
            target.depth_view.as_ref(),
            &self.pipeline,
            &self.camera_bind_group,
            &self.mesh_cache,
            &self.vertex_buffer,
            &self.index_buffer,
            &self.instance_buffer,
            &batches,
        );
        encode_grid_pass(
            &mut encoder,
            &target.color_view,
            target.depth_view.as_ref(),
            &self.grid_pipeline,
            &self.grid_bind_group,
            &self.grid_vertex_buffer,
            self.grid_vertex_count,
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        self.submitted_worlds = self.submitted_worlds.saturating_add(1);
        Ok(())
    }

    fn from_device(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        width: u32,
        height: u32,
        format: ImageFormat,
        surface_state: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)>,
    ) -> EngineResult<Self> {
        let mut target_allocator = HandleAllocator::default();
        let default_target = create_target(
            &device,
            &mut target_allocator,
            RenderTargetDesc {
                width: width.max(1),
                height: height.max(1),
                color_format: format,
                with_depth: true,
                samples: 1,
                kind: ViewKind::SceneView,
                label: Some("aster default offscreen target"),
            },
        )?;
        let game_target = create_target(
            &device,
            &mut target_allocator,
            RenderTargetDesc {
                width: width.max(1),
                height: height.max(1),
                color_format: format,
                with_depth: true,
                samples: 1,
                kind: ViewKind::GameView,
                label: Some("aster game offscreen target"),
            },
        )?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster forward shader"),
            source: wgpu::ShaderSource::Wgsl(FORWARD_SHADER.into()),
        });
        let camera_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster camera uniform"),
            contents: bytemuck::bytes_of(&CameraUniform {
                view_projection: IDENTITY_MAT4,
                camera_position: [0.0, 0.0, 5.0, 1.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let model_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster model uniform"),
            contents: bytemuck::bytes_of(&ModelUniform {
                model: IDENTITY_MAT4,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let material_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster material uniform"),
            contents: bytemuck::bytes_of(&MaterialUniform {
                base_color: [1.0, 1.0, 1.0, 1.0],
                metallic: 0.0,
                roughness: 0.5,
                _pad: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let lighting_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster lighting uniform"),
            contents: bytemuck::bytes_of(&LightingUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let default_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster default white texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let default_texture_view =
            default_texture.create_view(&wgpu::TextureViewDescriptor::default());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &default_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let default_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aster default sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aster forward bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster forward bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: model_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: material_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&default_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&default_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: lighting_uniform.as_entire_binding(),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster forward pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aster forward pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3,
                            1 => Float32x3,
                            2 => Float32x2
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            3 => Float32x3,
                            4 => Float32x3,
                            5 => Float32x4
                        ],
                    },
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_state
                        .as_ref()
                        .map(|(_, config)| config.format)
                        .unwrap_or_else(|| to_wgpu_format(format)),
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Grid pipeline ---
        let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster grid shader"),
            source: wgpu::ShaderSource::Wgsl(GRID_SHADER.into()),
        });
        let grid_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aster grid bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let grid_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster grid bind group"),
            layout: &grid_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform.as_entire_binding(),
            }],
        });
        let grid_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aster grid pipeline layout"),
                bind_group_layouts: &[Some(&grid_bind_group_layout)],
                immediate_size: 0,
            });
        let grid_color_format = surface_state
            .as_ref()
            .map(|(_, config)| config.format)
            .unwrap_or_else(|| to_wgpu_format(format));
        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aster grid pipeline"),
            layout: Some(&grid_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &grid_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &grid_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: grid_color_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let grid_vertices = generate_grid();
        let grid_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster grid vertices"),
            contents: bytemuck::cast_slice(&grid_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grid_vertex_count = grid_vertices.len() as u32;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster cube vertices"),
            contents: bytemuck::cast_slice(CUBE_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster cube indices"),
            contents: bytemuck::cast_slice(CUBE_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_capacity = 1;
        let instance_buffer = create_instance_buffer(&device, instance_capacity);

        let CreatedTarget(color, color_view, depth, depth_view, default_target) = default_target;
        let CreatedTarget(game_color, game_color_view, game_depth, game_depth_view, game_target) =
            game_target;
        let mut targets = HashMap::new();
        targets.insert(
            default_target.handle,
            GpuTarget {
                _color: color,
                color_view,
                _depth: depth,
                depth_view,
                _desc: default_target.desc.clone(),
            },
        );
        targets.insert(
            game_target.handle,
            GpuTarget {
                _color: game_color,
                color_view: game_color_view,
                _depth: game_depth,
                depth_view: game_depth_view,
                _desc: game_target.desc.clone(),
            },
        );

        let (surface, surface_config) = surface_state
            .map(|(surface, config)| (Some(surface), Some(config)))
            .unwrap_or((None, None));

        let mut renderer = Self {
            _instance: instance,
            adapter,
            device,
            queue,
            image_allocator: HandleAllocator::default(),
            buffer_allocator: HandleAllocator::default(),
            target_allocator,
            images: HashMap::new(),
            buffers: HashMap::new(),
            targets,
            default_target,
            game_target,
            pipeline,
            camera_bind_group,
            camera_uniform,
            lighting_uniform,
            _default_texture: default_texture,
            _default_texture_view: default_texture_view,
            _default_sampler: default_sampler,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            instance_capacity,
            mesh_cache: HashMap::new(),
            surface,
            surface_config,
            surface_depth: None,
            surface_depth_view: None,
            surface_suspended: false,
            next_gui_texture: 1,
            submitted_worlds: 0,
            grid_pipeline,
            grid_bind_group,
            grid_vertex_buffer,
            grid_vertex_count,
            destroy_queue: Vec::new(),
        };
        renderer.upload_debug_meshes();
        Ok(renderer)
    }

    /// Number of render worlds submitted to this backend.
    pub fn submitted_worlds(&self) -> u64 {
        self.submitted_worlds
    }

    /// Resizes the configured presentation surface.
    ///
    /// When either dimension is zero, rendering is suspended until valid dimensions
    /// are provided. Old depth resources are dropped before reconfiguration.
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        let (Some(surface), Some(config)) = (self.surface.as_ref(), self.surface_config.as_mut())
        else {
            return;
        };
        if width == 0 || height == 0 {
            self.surface_suspended = true;
            return;
        }
        self.surface_depth = None;
        self.surface_depth_view = None;
        config.width = width;
        config.height = height;
        surface.configure(&self.device, config);
        self.surface_suspended = false;
    }

    /// Creates GPU vertex and index buffers from a procedural debug mesh.
    pub fn create_mesh_buffers(&self, mesh: &DebugMesh) -> MeshBuffers {
        let (vertices, indices) = generate_mesh(mesh);
        Self::buffers_from_data(&self.device, &vertices, &indices)
    }

    /// Uploads a mesh from vertex/index data into the mesh cache.
    pub fn upload_mesh(&mut self, name: &str, vertices: &[Vertex], indices: &[u32]) {
        let buffers = Self::buffers_from_data(&self.device, vertices, indices);
        self.mesh_cache.insert(name.to_string(), buffers);
    }

    /// Pre-loads procedural debug meshes into the cache.
    pub fn upload_debug_meshes(&mut self) {
        for mesh in &[DebugMesh::Cube, DebugMesh::Sphere(8), DebugMesh::Plane] {
            let name = mesh_name(mesh);
            let buffers = self.create_mesh_buffers(mesh);
            self.mesh_cache.insert(name, buffers);
        }
    }

    /// Returns true when a mesh is available in the cache.
    pub fn has_mesh(&self, name: &str) -> bool {
        self.mesh_cache.contains_key(name) || name == "debug/cube"
    }

    fn buffers_from_data(
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
    ) -> MeshBuffers {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster mesh vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster mesh indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        MeshBuffers {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
        }
    }

    fn ensure_surface_depth(&mut self) {
        let Some(config) = self.surface_config.as_ref() else {
            return;
        };
        if self.surface_depth_view.is_some() {
            return;
        }
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster surface depth"),
            size: wgpu::Extent3d {
                width: config.width.max(1),
                height: config.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        self.surface_depth = Some(depth);
        self.surface_depth_view = Some(view);
    }
}

impl RenderDevice for WgpuRenderDevice {
    fn api(&self) -> RenderApi {
        RenderApi::WebGpu
    }

    fn render(&mut self, frame: RenderFrame) -> EngineResult<()> {
        self.submit_render_world(&RenderWorld::default(), frame)
    }

    fn submit_render_world(
        &mut self,
        world: &RenderWorld,
        _frame: RenderFrame,
    ) -> EngineResult<()> {
        let batches = self.prepare_render_batches(world);
        let uniform = camera_uniform_from_world(world);
        self.queue
            .write_buffer(&self.camera_uniform, 0, bytemuck::bytes_of(&uniform));
        let lighting = lighting_uniform_from_world(world);
        self.queue
            .write_buffer(&self.lighting_uniform, 0, bytemuck::bytes_of(&lighting));

        if self.surface.is_some() {
            if self.surface_suspended {
                return Ok(());
            }
            self.ensure_surface_depth();
            let surface = self
                .surface
                .as_ref()
                .ok_or_else(|| EngineError::invalid_handle("wgpu surface is missing"))?;
            let frame = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    if let Some(config) = self.surface_config.as_ref() {
                        surface.configure(&self.device, config);
                    }
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Validation => return Ok(()),
            };
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("aster surface render world encoder"),
                });
            encode_batched_forward_pass(
                &mut encoder,
                &view,
                self.surface_depth_view.as_ref(),
                &self.pipeline,
                &self.camera_bind_group,
                &self.mesh_cache,
                &self.vertex_buffer,
                &self.index_buffer,
                &self.instance_buffer,
                &batches,
            );
            encode_grid_pass(
                &mut encoder,
                &view,
                self.surface_depth_view.as_ref(),
                &self.grid_pipeline,
                &self.grid_bind_group,
                &self.grid_vertex_buffer,
                self.grid_vertex_count,
            );
            self.queue.submit(std::iter::once(encoder.finish()));
            frame.present();
            self.submitted_worlds = self.submitted_worlds.saturating_add(1);
            return Ok(());
        }

        let target = self
            .targets
            .get(&self.default_target.handle)
            .ok_or_else(|| EngineError::invalid_handle("default wgpu target is missing"))?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster render world encoder"),
            });
        encode_batched_forward_pass(
            &mut encoder,
            &target.color_view,
            target.depth_view.as_ref(),
            &self.pipeline,
            &self.camera_bind_group,
            &self.mesh_cache,
            &self.vertex_buffer,
            &self.index_buffer,
            &self.instance_buffer,
            &batches,
        );
        encode_grid_pass(
            &mut encoder,
            &target.color_view,
            target.depth_view.as_ref(),
            &self.grid_pipeline,
            &self.grid_bind_group,
            &self.grid_vertex_buffer,
            self.grid_vertex_count,
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        self.submitted_worlds = self.submitted_worlds.saturating_add(1);
        Ok(())
    }

    /// Prepares instance buffer from mesh batches for rendering.
    fn execute_graph(&mut self, _graph: &RenderGraph, _frame: RenderFrame) -> EngineResult<()> {
        Ok(())
    }

    fn create_render_target(&mut self, desc: RenderTargetDesc) -> EngineResult<RenderTarget> {
        let created = create_target(&self.device, &mut self.target_allocator, desc)?;
        self.targets.insert(
            created.4.handle,
            GpuTarget {
                _color: created.0,
                color_view: created.1,
                _depth: created.2,
                depth_view: created.3,
                _desc: created.4.desc.clone(),
            },
        );
        Ok(created.4)
    }

    fn destroy_render_target(&mut self, target: RenderTarget) {
        self.targets.remove(&target.handle);
    }

    fn create_image(&mut self, desc: ImageDesc) -> EngineResult<ImageHandle> {
        let handle = self.image_allocator.allocate()?;
        let texture = self.device.create_texture(&texture_desc(&desc));
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.images.insert(
            handle,
            GpuImage {
                _texture: texture,
                _view: view,
                _desc: desc,
            },
        );
        Ok(ImageHandle::new(handle))
    }

    fn upload_texture(&mut self, desc: ImageDesc, data: &[u8]) -> EngineResult<ImageHandle> {
        let handle = self.image_allocator.allocate()?;
        let texture = self.device.create_texture(&texture_desc(&desc));
        if !data.is_empty() {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(desc.width.max(1) * desc.format.bytes_per_pixel()),
                    rows_per_image: Some(desc.height.max(1)),
                },
                wgpu::Extent3d {
                    width: desc.width.max(1),
                    height: desc.height.max(1),
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.images.insert(
            handle,
            GpuImage {
                _texture: texture,
                _view: view,
                _desc: desc,
            },
        );
        Ok(ImageHandle::new(handle))
    }

    fn destroy_image(&mut self, handle: ImageHandle) {
        if let Some(image) = self.images.remove(&handle.raw()) {
            let _ = self.image_allocator.free(handle.raw());
            let frame = self.submitted_worlds;
            self.destroy_queue.push((frame, DestroyResource::Texture(image._texture)));
        }
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> EngineResult<BufferHandle> {
        let handle = self.buffer_allocator.allocate()?;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: desc.label,
            size: desc.size.max(1),
            usage: to_wgpu_buffer_usage(desc.usage, desc.host_visible),
            mapped_at_creation: false,
        });
        self.buffers.insert(handle, buffer);
        Ok(BufferHandle::new(handle))
    }

    fn destroy_buffer(&mut self, handle: BufferHandle) {
        if let Some(buffer) = self.buffers.remove(&handle.raw()) {
            let _ = self.buffer_allocator.free(handle.raw());
            let frame = self.submitted_worlds;
            self.destroy_queue.push((frame, DestroyResource::Buffer(buffer)));
        }
    }

    fn upload_gui_texture(&mut self, desc: ImageDesc, data: &[u8]) -> EngineResult<GuiTextureId> {
        let texture = self.device.create_texture(&texture_desc(&desc));
        if !data.is_empty() {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(desc.width.max(1) * desc.format.bytes_per_pixel()),
                    rows_per_image: Some(desc.height.max(1)),
                },
                wgpu::Extent3d {
                    width: desc.width.max(1),
                    height: desc.height.max(1),
                    depth_or_array_layers: 1,
                },
            );
        }
        let handle = self.image_allocator.allocate()?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.images.insert(
            handle,
            GpuImage {
                _texture: texture,
                _view: view,
                _desc: desc,
            },
        );
        let id = GuiTextureId(self.next_gui_texture);
        self.next_gui_texture = self.next_gui_texture.saturating_add(1);
        Ok(id)
    }

    fn draw_gui(&mut self, _draw_list: &GuiDrawList) -> EngineResult<()> {
        Ok(())
    }

    fn upload_mesh_data(
        &mut self,
        mesh_name: &str,
        positions: &[[f32; 3]],
        normals: &[[f32; 3]],
        texcoords: &[[f32; 2]],
        indices: &[u32],
    ) -> EngineResult<()> {
        let vertex_count = positions.len().min(normals.len()).min(texcoords.len());
        let vertices: Vec<Vertex> = (0..vertex_count)
            .map(|i| Vertex {
                position: positions[i],
                normal: normals[i],
                uv: texcoords[i],
            })
            .collect();
        self.upload_mesh(mesh_name, &vertices, indices);
        Ok(())
    }

    fn flush_destroy_queue(&mut self, frame_index: u64) {
        // Drop resources whose frame index is at least 2 frames behind the
        // current frame, ensuring GPU command buffers referencing them have
        // completed.
        let threshold = frame_index.saturating_sub(2);
        self.destroy_queue.retain(|(idx, _resource)| {
            *idx > threshold
        });
    }
}

struct CreatedTarget(
    wgpu::Texture,
    wgpu::TextureView,
    Option<wgpu::Texture>,
    Option<wgpu::TextureView>,
    RenderTarget,
);

fn create_target(
    device: &wgpu::Device,
    allocator: &mut HandleAllocator,
    desc: RenderTargetDesc,
) -> EngineResult<CreatedTarget> {
    let handle = allocator.allocate()?;
    let color_desc = ImageDesc {
        width: desc.width.max(1),
        height: desc.height.max(1),
        mip_levels: 1,
        samples: desc.samples.max(1),
        format: desc.color_format,
        usage: ImageUsage::COLOR_ATTACHMENT
            .or(ImageUsage::SAMPLED)
            .or(ImageUsage::TRANSFER_SRC),
        label: desc.label,
    };
    let color = device.create_texture(&texture_desc(&color_desc));
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let (depth, depth_view) = if desc.with_depth {
        let depth_desc = ImageDesc::depth_2d(desc.width.max(1), desc.height.max(1));
        let depth = device.create_texture(&texture_desc(&depth_desc));
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        (Some(depth), Some(depth_view))
    } else {
        (None, None)
    };
    let target = RenderTarget { handle, desc };
    Ok(CreatedTarget(color, color_view, depth, depth_view, target))
}

fn texture_desc(desc: &ImageDesc) -> wgpu::TextureDescriptor<'_> {
    wgpu::TextureDescriptor {
        label: desc.label,
        size: wgpu::Extent3d {
            width: desc.width.max(1),
            height: desc.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: desc.mip_levels.max(1),
        sample_count: desc.samples.max(1),
        dimension: wgpu::TextureDimension::D2,
        format: to_wgpu_format(desc.format),
        usage: to_wgpu_texture_usage(desc.usage),
        view_formats: &[],
    }
}

fn to_wgpu_format(format: ImageFormat) -> wgpu::TextureFormat {
    match format {
        ImageFormat::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        ImageFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        ImageFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        ImageFormat::Rgba32Float => wgpu::TextureFormat::Rgba32Float,
        ImageFormat::Depth32Float => wgpu::TextureFormat::Depth32Float,
        ImageFormat::Depth24Stencil8 => wgpu::TextureFormat::Depth24PlusStencil8,
        ImageFormat::Bc7Srgb => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
    }
}

fn from_wgpu_format(format: wgpu::TextureFormat) -> Option<ImageFormat> {
    match format {
        wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Bgra8UnormSrgb => {
            Some(ImageFormat::Rgba8Srgb)
        }
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm => {
            Some(ImageFormat::Rgba8Unorm)
        }
        wgpu::TextureFormat::Rgba16Float => Some(ImageFormat::Rgba16Float),
        wgpu::TextureFormat::Rgba32Float => Some(ImageFormat::Rgba32Float),
        _ => None,
    }
}

fn encode_batched_forward_pass<'a>(
    encoder: &mut wgpu::CommandEncoder,
    color_view: &wgpu::TextureView,
    depth_view: Option<&wgpu::TextureView>,
    pipeline: &wgpu::RenderPipeline,
    camera_bind_group: &wgpu::BindGroup,
    mesh_cache: &'a HashMap<String, MeshBuffers>,
    default_vertex_buffer: &'a wgpu::Buffer,
    default_index_buffer: &'a wgpu::Buffer,
    instance_buffer: &wgpu::Buffer,
    batches: &[(String, u32)],
) {
    let color_attachment = Some(wgpu::RenderPassColorAttachment {
        view: color_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
                r: 0.1,
                g: 0.1,
                b: 0.1,
                a: 1.0,
            }),
            store: wgpu::StoreOp::Store,
        },
    });
    let depth_attachment = depth_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(1.0),
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("aster forward pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: depth_attachment,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, camera_bind_group, &[]);

    let mut instance_offset = 0u32;
    for (mesh_name, count) in batches {
        if *count == 0 {
            continue;
        }
        let buffers = mesh_cache.get(mesh_name);
        let (vertex_buf, index_buf, index_count) = match buffers {
            Some(b) => (&b.vertex_buffer, &b.index_buffer, b.index_count),
            None => (
                default_vertex_buffer,
                default_index_buffer,
                CUBE_INDEX_COUNT,
            ),
        };
        pass.set_vertex_buffer(0, vertex_buf.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..index_count, 0, instance_offset..instance_offset + count);
        instance_offset += count;
    }
}

fn encode_grid_pass(
    encoder: &mut wgpu::CommandEncoder,
    color_view: &wgpu::TextureView,
    depth_view: Option<&wgpu::TextureView>,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    vertex_buffer: &wgpu::Buffer,
    vertex_count: u32,
) {
    let color_attachment = Some(wgpu::RenderPassColorAttachment {
        view: color_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        },
    });
    let depth_attachment = depth_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("aster grid pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: depth_attachment,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.set_vertex_buffer(0, vertex_buffer.slice(..));
    pass.draw(0..vertex_count, 0..1);
}

fn to_wgpu_texture_usage(usage: ImageUsage) -> wgpu::TextureUsages {
    let mut out = wgpu::TextureUsages::empty();
    if usage.contains(ImageUsage::SAMPLED) {
        out |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    if usage.contains(ImageUsage::COLOR_ATTACHMENT) {
        out |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    if usage.contains(ImageUsage::DEPTH_STENCIL_ATTACHMENT) {
        out |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    if usage.contains(ImageUsage::STORAGE) {
        out |= wgpu::TextureUsages::STORAGE_BINDING;
    }
    if usage.contains(ImageUsage::TRANSFER_SRC) {
        out |= wgpu::TextureUsages::COPY_SRC;
    }
    if usage.contains(ImageUsage::TRANSFER_DST) {
        out |= wgpu::TextureUsages::COPY_DST;
    }
    out
}

fn to_wgpu_buffer_usage(usage: BufferUsage, host_visible: bool) -> wgpu::BufferUsages {
    let mut out = wgpu::BufferUsages::empty();
    if usage.contains(BufferUsage::VERTEX) {
        out |= wgpu::BufferUsages::VERTEX;
    }
    if usage.contains(BufferUsage::INDEX) {
        out |= wgpu::BufferUsages::INDEX;
    }
    if usage.contains(BufferUsage::UNIFORM) {
        out |= wgpu::BufferUsages::UNIFORM;
    }
    if usage.contains(BufferUsage::STORAGE) {
        out |= wgpu::BufferUsages::STORAGE;
    }
    if usage.contains(BufferUsage::TRANSFER_SRC) || host_visible {
        out |= wgpu::BufferUsages::COPY_SRC;
    }
    if usage.contains(BufferUsage::TRANSFER_DST) || host_visible {
        out |= wgpu::BufferUsages::COPY_DST;
    }
    out
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aster instance buffer"),
        size: (capacity.max(1) * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn camera_uniform_from_world(world: &RenderWorld) -> CameraUniform {
    let eye = world
        .camera
        .as_ref()
        .map(|camera| camera.transform.translation)
        .unwrap_or_else(|| engine_core::math::Vec3::new(0.0, 0.0, 5.0));
    let view = look_at_rh(
        eye,
        engine_core::math::Vec3::ZERO,
        engine_core::math::Vec3::new(0.0, 1.0, 0.0),
    );
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
    let aspect = 16.0 / 9.0;
    let proj = match world.camera.as_ref().map(|camera| camera.projection) {
        Some(engine_render::RenderProjection::Orthographic { vertical_size }) => {
            orthographic_rh(vertical_size.max(0.001), aspect, near, far)
        }
        _ => perspective_rh(fov.to_radians(), aspect, near, far),
    };
    let vp = mul_mat4(&proj, &view);
    CameraUniform {
        view_projection: vp,
        camera_position: [eye.x, eye.y, eye.z, 1.0],
    }
}

fn lighting_uniform_from_world(world: &RenderWorld) -> LightingUniform {
    let mut uniform = LightingUniform::default();
    let mut count = 0usize;

    for light in &world.lights {
        if light.intensity <= 0.0 {
            continue;
        }
        uniform.lights[count] = forward_light_uniform(light);
        count += 1;
        if count == MAX_FORWARD_LIGHTS {
            break;
        }
    }

    if count == 0 {
        uniform.lights[0] = ForwardLightUniform {
            position_type: [0.0, 0.0, 0.0, 0.0],
            direction_range: [-0.5, -1.0, -0.25, 0.0],
            color_intensity: [1.0, 1.0, 1.0, 1.0],
            spot_angles: [1.0, 1.0, 0.0, 0.0],
        };
        count = 1;
    }

    uniform.params = [count as u32, 0, 0, 0];
    uniform
}

fn forward_light_uniform(light: &RenderLight) -> ForwardLightUniform {
    let light_type = match light.kind.as_str() {
        "point" => 1.0,
        "spot" => 2.0,
        _ => 0.0,
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

fn rotate_vec3(
    rotation: engine_core::math::Quat,
    vector: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    let q = engine_core::math::Vec3::new(rotation.x, rotation.y, rotation.z);
    let t = cross(q, vector) * 2.0;
    vector + t * rotation.w + cross(q, t)
}

const IDENTITY_MAT4: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn look_at_rh(
    eye: engine_core::math::Vec3,
    target: engine_core::math::Vec3,
    up: engine_core::math::Vec3,
) -> [[f32; 4]; 4] {
    let f = (target - eye).normalized();
    let r = cross(f, up).normalized();
    let u = cross(r, f);
    [
        [r.x, u.x, -f.x, 0.0],
        [r.y, u.y, -f.y, 0.0],
        [r.z, u.z, -f.z, 0.0],
        [-r.dot(eye), -u.dot(eye), f.dot(eye), 1.0],
    ]
}

fn cross(a: engine_core::math::Vec3, b: engine_core::math::Vec3) -> engine_core::math::Vec3 {
    engine_core::math::Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn perspective_rh(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let range_inv = 1.0 / (near - far);
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, (far + near) * range_inv, -1.0],
        [0.0, 0.0, 2.0 * far * near * range_inv, 0.0],
    ]
}

fn orthographic_rh(vertical_size: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let top = vertical_size * 0.5;
    let bottom = -top;
    let right = top * aspect;
    let left = -right;
    let range_inv = 1.0 / (near - far);
    [
        [2.0 / (right - left), 0.0, 0.0, 0.0],
        [0.0, 2.0 / (top - bottom), 0.0, 0.0],
        [0.0, 0.0, 2.0 * range_inv, 0.0],
        [
            -(right + left) / (right - left),
            -(top + bottom) / (top - bottom),
            (far + near) * range_inv,
            1.0,
        ],
    ]
}

fn mul_mat4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            result[col][row] = a[0][row] * b[col][0]
                + a[1][row] * b[col][1]
                + a[2][row] * b[col][2]
                + a[3][row] * b[col][3];
        }
    }
    result
}

fn mesh_name(mesh: &DebugMesh) -> String {
    match mesh {
        DebugMesh::Cube => "debug/cube".to_string(),
        DebugMesh::Sphere(_) => "debug/sphere".to_string(),
        DebugMesh::Plane => "debug/plane".to_string(),
    }
}

/// Groups render objects by mesh name for batched instanced rendering.
fn mesh_batches_from_world(world: &RenderWorld) -> Vec<(String, Vec<Instance>)> {
    let mut batches: HashMap<String, Vec<Instance>> = HashMap::new();
    for object in &world.objects {
        let color = color_for_material(&object.material);
        let t = object.transform;
        let mesh = if object.mesh.is_empty() {
            "debug/cube".to_string()
        } else {
            object.mesh.clone()
        };
        batches.entry(mesh).or_default().push(Instance {
            offset: [t.translation.x, t.translation.y, t.translation.z],
            scale: [
                t.scale.x.max(0.05),
                t.scale.y.max(0.05),
                t.scale.z.max(0.05),
            ],
            color,
        });
    }
    // Particles go into their own particle mesh batch
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
                }
            })
            .collect();
        batches.insert("debug/plane".to_string(), particle_instances);
    }
    batches.into_iter().collect()
}

fn color_for_material(material: &str) -> [f32; 4] {
    if material.contains("debug") {
        [0.2, 0.65, 1.0, 1.0]
    } else if material.contains("error") {
        [1.0, 0.2, 0.45, 1.0]
    } else {
        [0.82, 0.86, 0.72, 1.0]
    }
}

// Cube vertices with hard normals (24 vertices, 4 per face × 6 faces).
const CUBE_VERTICES: &[Vertex] = &[
    // Front face (+Z)
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 1.0],
    },
    // Back face (-Z)
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 1.0],
    },
    // Right face (+X)
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 1.0],
    },
    // Left face (-X)
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [0.0, 1.0],
    },
    // Top face (+Y)
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [0.0, 1.0],
    },
    // Bottom face (-Y)
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [0.0, 1.0],
    },
];

const CUBE_INDICES: &[u32] = &[
    0, 1, 2, 2, 3, 0, // front
    4, 5, 6, 6, 7, 4, // back
    8, 9, 10, 10, 11, 8, // right
    12, 13, 14, 14, 15, 12, // left
    16, 17, 18, 18, 19, 16, // top
    20, 21, 22, 22, 23, 20, // bottom
];

fn generate_mesh(mesh: &DebugMesh) -> (Vec<Vertex>, Vec<u32>) {
    match mesh {
        DebugMesh::Cube => generate_cube(),
        DebugMesh::Sphere(segments) => generate_sphere(*segments),
        DebugMesh::Plane => generate_plane(),
    }
}

fn generate_cube() -> (Vec<Vertex>, Vec<u32>) {
    (CUBE_VERTICES.to_vec(), CUBE_INDICES.to_vec())
}

fn generate_sphere(segments: u32) -> (Vec<Vertex>, Vec<u32>) {
    let segs = segments.max(3);
    let lat = segs;
    let lon = segs * 2;

    let mut vertices = Vec::with_capacity(((lat + 1) * (lon + 1)) as usize);
    let mut indices = Vec::with_capacity((lat * lon * 6) as usize);

    for i in 0..=lat {
        let v = i as f32 / lat as f32;
        let theta = v * std::f32::consts::PI;
        let y = theta.cos();
        let r = theta.sin();

        for j in 0..=lon {
            let u = j as f32 / lon as f32;
            let phi = u * 2.0 * std::f32::consts::PI;
            let x = r * phi.cos();
            let z = r * phi.sin();

            vertices.push(Vertex {
                position: [x * 0.5, y * 0.5, z * 0.5],
                normal: [x, y, z],
                uv: [u, v],
            });
        }
    }

    for i in 0..lat {
        for j in 0..lon {
            let a = i * (lon + 1) + j;
            let b = a + lon + 1;
            let c = a + 1;
            let d = b + 1;
            indices.push(a);
            indices.push(b);
            indices.push(c);
            indices.push(c);
            indices.push(b);
            indices.push(d);
        }
    }

    (vertices, indices)
}

fn generate_plane() -> (Vec<Vertex>, Vec<u32>) {
    let vertices = vec![
        Vertex {
            position: [-0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
        Vertex {
            position: [0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
        Vertex {
            position: [0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            position: [-0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 1.0],
        },
    ];
    let indices = vec![0, 1, 2, 2, 3, 0];
    (vertices, indices)
}

fn generate_grid() -> Vec<Vertex> {
    let half = 50.0;
    let mut vertices = Vec::with_capacity(404);

    for i in -50..=50 {
        let x = i as f32;
        let alpha = if i % 5 == 0 { 0.35 } else { 0.15 };
        vertices.push(Vertex {
            position: [x, 0.0, -half],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
        });
        vertices.push(Vertex {
            position: [x, 0.0, half],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
        });
    }
    for i in -50..=50 {
        let z = i as f32;
        let alpha = if i % 5 == 0 { 0.35 } else { 0.15 };
        vertices.push(Vertex {
            position: [-half, 0.0, z],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
        });
        vertices.push(Vertex {
            position: [half, 0.0, z],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
        });
    }

    vertices
}

const FORWARD_SHADER: &str = r#"
struct CameraUniform {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
};

struct ModelUniform {
    model: mat4x4<f32>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    metallic: f32,
    roughness: f32,
};

struct ForwardLight {
    position_type: vec4<f32>,
    direction_range: vec4<f32>,
    color_intensity: vec4<f32>,
    spot_angles: vec4<f32>,
};

struct LightingUniform {
    ambient: vec4<f32>,
    params: vec4<u32>,
    lights: array<ForwardLight, 8>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(0) @binding(1) var<uniform> model: ModelUniform;
@group(0) @binding(2) var<uniform> material: MaterialUniform;
@group(0) @binding(3) var material_texture: texture_2d<f32>;
@group(0) @binding(4) var material_sampler: sampler;
@group(0) @binding(5) var<uniform> lighting: LightingUniform;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) offset: vec3<f32>,
    @location(4) scale: vec3<f32>,
    @location(5) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world_position: vec3<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    let world_pos = input.position * input.scale + input.offset;
    let world_pos4 = model.model * vec4<f32>(world_pos, 1.0);
    out.position = camera.view_projection * world_pos4;
    let normal_mat = mat3x3<f32>(
        model.model[0].xyz,
        model.model[1].xyz,
        model.model[2].xyz,
    );
    out.world_normal = normalize(normal_mat * input.normal);
    out.uv = input.uv;
    out.color = input.color;
    out.world_position = world_pos4.xyz;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(input.world_normal);
    let tex_color = textureSample(material_texture, material_sampler, input.uv);
    let base = material.base_color.rgb * input.color.rgb * tex_color.rgb;
    var radiance = lighting.ambient.rgb;

    for (var i: u32 = 0u; i < lighting.params.x; i = i + 1u) {
        let light = lighting.lights[i];
        let light_type = light.position_type.w;
        let light_color = light.color_intensity.rgb;
        let intensity = light.color_intensity.w;
        var light_dir = vec3<f32>(0.0, 1.0, 0.0);
        var attenuation = 1.0;
        var spot = 1.0;

        if (light_type < 0.5) {
            light_dir = normalize(-light.direction_range.xyz);
        } else {
            let to_light = light.position_type.xyz - input.world_position;
            let distance = length(to_light);
            light_dir = to_light / max(distance, 0.001);
            let range = max(light.direction_range.w, 0.001);
            let falloff = max(1.0 - distance / range, 0.0);
            attenuation = falloff * falloff;

            if (light_type > 1.5) {
                let spot_alignment = dot(normalize(-light_dir), normalize(light.direction_range.xyz));
                spot = smoothstep(light.spot_angles.y, light.spot_angles.x, spot_alignment);
            }
        }

        let ndotl = max(dot(n, light_dir), 0.0);
        radiance = radiance + light_color * intensity * ndotl * attenuation * spot;
    }

    let lit = base * min(radiance, vec3<f32>(3.0, 3.0, 3.0));
    let alpha = material.base_color.a * input.color.a * tex_color.a;
    return vec4<f32>(lit, alpha);
}
"#;

const GRID_SHADER: &str = r#"
struct CameraUniform {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) alpha_factor: f32,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.position = camera.view_projection * vec4<f32>(input.position, 1.0);
    out.world_pos = input.position;
    out.alpha_factor = input.uv.x;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let half_extent = 50.0;
    let fade_start = half_extent * 0.7;
    let dist = length(input.world_pos.xz);
    let fade = 1.0 - smoothstep(fade_start, half_extent, dist);
    let alpha = input.alpha_factor * fade;
    return vec4<f32>(vec3<f32>(0.6), alpha);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

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
            kind: "point".to_string(),
            color: engine_core::math::Vec3::new(0.5, 0.75, 1.0),
            intensity: 3.0,
            range: 12.0,
            spot_angle: 45.0,
        };
        let world = RenderWorld {
            camera: None,
            objects: Vec::new(),
            lights: vec![light],
            particles: vec![],
        };

        let uniform = lighting_uniform_from_world(&world);

        assert_eq!(uniform.params[0], 1);
        assert_eq!(uniform.lights[0].position_type, [1.0, 2.0, 3.0, 1.0]);
        assert_eq!(uniform.lights[0].color_intensity, [0.5, 0.75, 1.0, 3.0]);
        assert_eq!(uniform.lights[0].direction_range[3], 12.0);
    }

    #[test]
    fn uses_fallback_directional_light_when_scene_has_no_lights() {
        let uniform = lighting_uniform_from_world(&RenderWorld::default());

        assert_eq!(uniform.params[0], 1);
        assert_eq!(uniform.lights[0].position_type[3], 0.0);
        assert_eq!(uniform.lights[0].color_intensity[3], 1.0);
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
}
