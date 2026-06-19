use std::{collections::HashMap, sync::Arc};

use crate::{device::*, format::*, math::*, meshes::*, render::*, shaders::*, uniforms::*};
use engine_core::{EngineError, EngineResult, HandleAllocator};
use engine_render::{
    ImageFormat, RenderDevice, RenderPerformanceConfig, RenderPerformanceMetrics, RenderTargetDesc,
    ViewKind,
};
use wgpu::util::DeviceExt;

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

    /// Creates a surface renderer using an explicit runtime performance policy.
    pub fn new_with_performance(
        window: &winit::window::Window,
        performance: RenderPerformanceConfig,
    ) -> EngineResult<Self> {
        pollster::block_on(Self::new_async_with_performance(window, performance))
    }

    /// Creates a wgpu device from a winit window asynchronously.
    pub async fn new_async(window: &winit::window::Window) -> EngineResult<Self> {
        Self::new_async_with_performance(window, RenderPerformanceConfig::default()).await
    }

    /// Creates a surface renderer asynchronously using an explicit performance policy.
    pub async fn new_async_with_performance(
        window: &winit::window::Window,
        performance: RenderPerformanceConfig,
    ) -> EngineResult<Self> {
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
            present_mode: select_present_mode(&caps.present_modes, performance.present_strategy),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: performance.maximum_frame_latency.max(1),
        };
        surface.configure(&device, &surface_config);
        let image_format = from_wgpu_format(format).unwrap_or(ImageFormat::Rgba8Srgb);

        let mut renderer = Self::from_device(
            instance,
            adapter,
            Arc::new(device),
            Arc::new(queue),
            width,
            height,
            image_format,
            Some((surface, surface_config)),
        )?;
        renderer.apply_performance_config(performance);
        Ok(renderer)
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
    /// that must be shared between the 3D renderer and the host compositor.
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
    pub(crate) fn from_device(
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
        let preview_target = create_target(
            &device,
            &mut target_allocator,
            RenderTargetDesc {
                width: 320,
                height: 180,
                color_format: format,
                with_depth: true,
                samples: 1,
                kind: ViewKind::Preview,
                label: Some("aster camera preview offscreen target"),
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
                camera_forward: [0.0, 0.0, -1.0, 0.0],
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
        // CSM cascade shadow maps
        let mut csm_depth_textures = Vec::with_capacity(CSM_CASCADE_COUNT);
        let mut csm_depth_views = Vec::with_capacity(CSM_CASCADE_COUNT);
        for i in 0..CSM_CASCADE_COUNT {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(&format!("aster csm cascade {i} depth")),
                size: wgpu::Extent3d {
                    width: CSM_SHADOW_RESOLUTION,
                    height: CSM_SHADOW_RESOLUTION,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            csm_depth_textures.push(tex);
            csm_depth_views.push(view);
        }
        let csm_depth_textures: [wgpu::Texture; CSM_CASCADE_COUNT] =
            csm_depth_textures.try_into().unwrap();
        let csm_depth_views: [wgpu::TextureView; CSM_CASCADE_COUNT] =
            csm_depth_views.try_into().unwrap();
        let csm_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aster csm sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let csm_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster csm uniform"),
            contents: bytemuck::bytes_of(&CsmUniform {
                cascade_vps: [IDENTITY_MAT4; CSM_CASCADE_COUNT],
                cascade_splits: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        // Pre-allocate cascade uniform buffers (updated via write_buffer per frame).
        let mut csm_cascade_uniforms = Vec::with_capacity(CSM_CASCADE_COUNT);
        for i in 0..CSM_CASCADE_COUNT {
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("aster csm cascade {i} uniform")),
                contents: bytemuck::bytes_of(&ShadowUniform {
                    light_view_projection: IDENTITY_MAT4,
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            csm_cascade_uniforms.push(buf);
        }
        let csm_cascade_uniforms: [wgpu::Buffer; CSM_CASCADE_COUNT] =
            csm_cascade_uniforms.try_into().unwrap();

        // Group 0: camera + lighting + CSM + IBL
        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aster scene bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
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
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::Cube,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 10,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::Cube,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 11,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 12,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 13,
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
        // IBL resources
        let ibl_irradiance_map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster ibl irradiance map"),
            size: wgpu::Extent3d {
                width: IBL_IRRADIANCE_RES,
                height: IBL_IRRADIANCE_RES,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let ibl_irradiance_view = ibl_irradiance_map.create_view(&wgpu::TextureViewDescriptor {
            label: Some("aster ibl irradiance cube view"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        let ibl_prefilter_map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster ibl prefilter map"),
            size: wgpu::Extent3d {
                width: IBL_PREFILTER_RES,
                height: IBL_PREFILTER_RES,
                depth_or_array_layers: 6,
            },
            mip_level_count: 5,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let ibl_prefilter_views =
            vec![ibl_prefilter_map.create_view(&wgpu::TextureViewDescriptor {
                label: Some("aster ibl prefilter cube view"),
                dimension: Some(wgpu::TextureViewDimension::Cube),
                base_mip_level: 0,
                mip_level_count: Some(5),
                ..Default::default()
            })];
        let ibl_brdf_lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster ibl brdf lut"),
            size: wgpu::Extent3d {
                width: IBL_BRDF_LUT_RES,
                height: IBL_BRDF_LUT_RES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let ibl_brdf_lut_view = ibl_brdf_lut.create_view(&wgpu::TextureViewDescriptor::default());
        let ibl_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aster ibl sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        // IBL baking scratch texture (max resolution shared by irradiance and prefilter)
        let ibl_scratch_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster ibl scratch"),
            size: wgpu::Extent3d {
                width: IBL_PREFILTER_RES,
                height: IBL_PREFILTER_RES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let ibl_scratch_view = ibl_scratch_tex.create_view(&wgpu::TextureViewDescriptor::default());
        // SSAO noise texture
        let mut ssao_noise = vec![0u8; (SSAO_NOISE_RES * SSAO_NOISE_RES * 4) as usize];
        for i in 0..(SSAO_NOISE_RES * SSAO_NOISE_RES) as usize {
            let x: f32 = (i as u32 % SSAO_NOISE_RES) as f32;
            let y: f32 = (i as u32 / SSAO_NOISE_RES) as f32;
            let angle = (x * 17.0 + y * 31.0) * std::f32::consts::PI * 2.0 / 16.0;
            let v: [f32; 2] = [angle.cos(), angle.sin()];
            ssao_noise[i * 4] = (v[0] * 127.5 + 127.5) as u8;
            ssao_noise[i * 4 + 1] = (v[1] * 127.5 + 127.5) as u8;
            ssao_noise[i * 4 + 2] = 255u8;
            ssao_noise[i * 4 + 3] = 255u8;
        }
        let ssao_noise_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster ssao noise"),
            size: wgpu::Extent3d {
                width: SSAO_NOISE_RES,
                height: SSAO_NOISE_RES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let ssao_noise_view =
            ssao_noise_texture.create_view(&wgpu::TextureViewDescriptor::default());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &ssao_noise_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &ssao_noise,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SSAO_NOISE_RES * 4),
                rows_per_image: Some(SSAO_NOISE_RES),
            },
            wgpu::Extent3d {
                width: SSAO_NOISE_RES,
                height: SSAO_NOISE_RES,
                depth_or_array_layers: 1,
            },
        );
        let ssao_linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aster ssao linear sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let mut ssao_kernel = vec![0f32; (SSAO_KERNEL_SIZE * 4) as usize];
        for i in 0..SSAO_KERNEL_SIZE as usize {
            let mut s: [f32; 3] = [
                (i as f32 * 1.6180339887).cos() * (1.0 - i as f32 / SSAO_KERNEL_SIZE as f32).sqrt(),
                (i as f32 * 1.6180339887).sin() * (1.0 - i as f32 / SSAO_KERNEL_SIZE as f32).sqrt(),
                i as f32 / SSAO_KERNEL_SIZE as f32,
            ];
            let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
            s[0] /= len;
            s[1] /= len;
            s[2] /= len;
            ssao_kernel[i * 4] = s[0];
            ssao_kernel[i * 4 + 1] = s[1];
            ssao_kernel[i * 4 + 2] = s[2];
            ssao_kernel[i * 4 + 3] = 0.0;
        }
        let ssao_samples_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster ssao samples"),
            contents: bytemuck::cast_slice(&ssao_kernel),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bloom_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aster bloom sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let fog_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster fog uniform"),
            contents: bytemuck::bytes_of(&FogUniform {
                density: 0.0,
                _pad: [0.0; 3],
                color: [0.6, 0.7, 0.85],
                enabled: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster scene bind group"),
            layout: &scene_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: lighting_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: csm_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&csm_depth_views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&csm_depth_views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&csm_depth_views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&csm_depth_views[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&csm_depth_views[4]),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::Sampler(&csm_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&ibl_irradiance_view),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(&ibl_prefilter_views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::TextureView(&ibl_brdf_lut_view),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: wgpu::BindingResource::Sampler(&ibl_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: fog_uniform.as_entire_binding(),
                },
            ],
        });

        // Group 1: material textures
        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aster material bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Default normal map (flat normal 128,128,255,255 = (0,0,1) in tangent space)
        let default_normal_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster default normal texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let default_normal_texture_view =
            default_normal_texture.create_view(&wgpu::TextureViewDescriptor::default());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &default_normal_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[128, 128, 255, 255],
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

        // Neutral packed material texture. Shader factors are multiplied by these
        // channels, so all channels must be one when no texture is supplied.
        let default_mra_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster default MRA texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let default_mra_texture_view =
            default_mra_texture.create_view(&wgpu::TextureViewDescriptor::default());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &default_mra_texture,
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

        let default_material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster default material bind group"),
            layout: &material_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&default_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&default_normal_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&default_mra_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&default_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&default_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&default_sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster forward pipeline layout"),
            bind_group_layouts: &[
                Some(&scene_bind_group_layout),
                Some(&material_bind_group_layout),
            ],
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
                            5 => Float32x4,
                            6 => Float32x4,
                            7 => Float32,
                            8 => Float32,
                            9 => Float32x3
                        ],
                    },
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
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
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        let grid_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster grid pipeline layout"),
            bind_group_layouts: &[Some(&grid_bind_group_layout)],
            immediate_size: 0,
        });
        let grid_color_format = wgpu::TextureFormat::Rgba16Float;
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

        // Shadow pipeline
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster shadow shader"),
            source: wgpu::ShaderSource::Wgsl(SHADOW_SHADER.into()),
        });
        let shadow_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aster shadow bind group layout"),
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
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aster shadow pipeline layout"),
                bind_group_layouts: &[Some(&shadow_bind_group_layout)],
                immediate_size: 0,
            });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aster shadow pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
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
                            5 => Float32x4,
                            6 => Float32x4,
                            7 => Float32,
                            8 => Float32,
                            9 => Float32x3
                        ],
                    },
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        // Pre-allocate cascade bind groups (one per cascade with shared layout).
        let mut csm_cascade_bind_groups = Vec::with_capacity(CSM_CASCADE_COUNT);
        for i in 0..CSM_CASCADE_COUNT {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("aster csm cascade {i} bind group")),
                layout: &shadow_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: csm_cascade_uniforms[i].as_entire_binding(),
                }],
            });
            csm_cascade_bind_groups.push(bg);
        }
        let csm_cascade_bind_groups: [wgpu::BindGroup; CSM_CASCADE_COUNT] =
            csm_cascade_bind_groups.try_into().unwrap();

        // Skybox pipeline
        let skybox_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster skybox shader"),
            source: wgpu::ShaderSource::Wgsl(SKYBOX_SHADER.into()),
        });
        let skybox_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster skybox uniform"),
            contents: bytemuck::bytes_of(&SkyboxUniform {
                view_rotation_only: IDENTITY_MAT4,
                zenith_color: [0.15, 0.35, 0.65, 1.0],
                horizon_color: [0.55, 0.7, 0.85, 1.0],
                rotation_intensity: [0.0, 1.0, 0.0, 0.0],
                use_cubemap: [0, 0, 0, 0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let skybox_default_cubemap = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster skybox default cubemap"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for face in 0..6u32 {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &skybox_default_cubemap,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: face,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &[128, 128, 200, 255],
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
        }
        let skybox_default_cubemap_view =
            skybox_default_cubemap.create_view(&wgpu::TextureViewDescriptor {
                label: Some("aster skybox default cubemap view"),
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            });
        let skybox_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aster skybox sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let skybox_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aster skybox bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::Cube,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let skybox_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster skybox bind group"),
            layout: &skybox_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: skybox_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&skybox_default_cubemap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&skybox_sampler),
                },
            ],
        });
        let skybox_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aster skybox pipeline layout"),
                bind_group_layouts: &[Some(&skybox_bind_group_layout)],
                immediate_size: 0,
            });
        let skybox_color_format = wgpu::TextureFormat::Rgba16Float;
        let skybox_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aster skybox pipeline"),
            layout: Some(&skybox_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &skybox_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &skybox_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: skybox_color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Post-process pipeline (fullscreen quad) ---
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster post shader"),
            source: wgpu::ShaderSource::Wgsl(POST_SHADER.into()),
        });
        let post_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aster post bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let post_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster post uniform"),
            contents: bytemuck::bytes_of(&PostProcessUniform {
                width: width as f32,
                height: height as f32,
                inv_width: 1.0 / width as f32,
                inv_height: 1.0 / height as f32,
                exposure: 1.0,
                bloom_intensity: 0.04,
                ssao_enabled: 0.0,
                time: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster post pipeline layout"),
            bind_group_layouts: &[Some(&post_bind_group_layout)],
            immediate_size: 0,
        });
        let post_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aster post pipeline"),
            layout: Some(&post_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &post_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_state
                        .as_ref()
                        .map(|(_, config)| config.format)
                        .unwrap_or_else(|| to_wgpu_format(format)),
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let ibl_brdf_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aster ibl brdf bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba16Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            }],
        });
        let ibl_brdf_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster ibl brdf shader"),
            source: wgpu::ShaderSource::Wgsl(IBL_BRDF_LUT_SHADER.into()),
        });
        let ibl_brdf_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aster ibl brdf pipeline layout"),
                bind_group_layouts: &[Some(&ibl_brdf_bgl)],
                immediate_size: 0,
            });
        let ibl_brdf_compute = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("aster ibl brdf compute"),
                layout: Some(&ibl_brdf_pipeline_layout),
                module: &ibl_brdf_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        // IBL baking: bind group layout shared by irradiance and prefilter compute pipelines
        let ibl_bake_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aster ibl bake bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let ibl_bake_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster ibl bake pipeline layout"),
            bind_group_layouts: &[Some(&ibl_bake_bgl)],
            immediate_size: 0,
        });
        let ibl_irradiance_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster ibl irradiance shader"),
            source: wgpu::ShaderSource::Wgsl(IBL_IRRADIANCE_SHADER.into()),
        });
        let ibl_irradiance_compute = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("aster ibl irradiance compute"),
                layout: Some(&ibl_bake_pl_layout),
                module: &ibl_irradiance_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        let ibl_prefilter_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster ibl prefilter shader"),
            source: wgpu::ShaderSource::Wgsl(IBL_PREFILTER_SHADER.into()),
        });
        let ibl_prefilter_compute = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("aster ibl prefilter compute"),
                layout: Some(&ibl_bake_pl_layout),
                module: &ibl_prefilter_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        // SSAO compute pipeline
        let ssao_compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aster ssao compute bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let ssao_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster ssao shader"),
            source: wgpu::ShaderSource::Wgsl(SSAO_SHADER.into()),
        });
        let ssao_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aster ssao pipeline layout"),
            bind_group_layouts: &[Some(&ssao_compute_bgl)],
            immediate_size: 0,
        });
        let ssao_compute_pipeline = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("aster ssao compute"),
                layout: Some(&ssao_pipeline_layout),
                module: &ssao_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        // Bloom compute pipelines
        let bloom_compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aster bloom compute bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bloom_down_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster bloom downsample shader"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_DOWNSAMPLE_SHADER.into()),
        });
        let bloom_up_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aster bloom upsample shader"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_UPSAMPLE_SHADER.into()),
        });
        let bloom_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aster bloom pipeline layout"),
                bind_group_layouts: &[Some(&bloom_compute_bgl)],
                immediate_size: 0,
            });
        let bloom_compute_down = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("aster bloom downsample compute"),
                layout: Some(&bloom_pipeline_layout),
                module: &bloom_down_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        let bloom_compute_up = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("aster bloom upsample compute"),
                layout: Some(&bloom_pipeline_layout),
                module: &bloom_up_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        let bloom_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster bloom uniform"),
            contents: bytemuck::bytes_of(&BloomUniform {
                intensity: 0.04,
                threshold: 1.0,
                knee: 0.5,
                _pad: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let ssao_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aster ssao uniform"),
            contents: bytemuck::bytes_of(&SsaoUniform {
                radius: SSAO_RADIUS,
                bias: SSAO_BIAS,
                intensity: 1.0,
                _pad: 0.0,
                width: width as f32,
                height: height as f32,
                inv_width: 1.0 / width.max(1) as f32,
                inv_height: 1.0 / height.max(1) as f32,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

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
        let CreatedTarget(
            preview_color,
            preview_color_view,
            preview_depth,
            preview_depth_view,
            preview_target,
        ) = preview_target;
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
        targets.insert(
            preview_target.handle,
            GpuTarget {
                _color: preview_color,
                color_view: preview_color_view,
                _depth: preview_depth,
                depth_view: preview_depth_view,
                _desc: preview_target.desc.clone(),
            },
        );

        let (surface, surface_config) = surface_state
            .map(|(surface, config)| (Some(surface), Some(config)))
            .unwrap_or((None, None));

        // Dummy 1x1 textures for initial post bind group
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aster dummy 1x1"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_tex_view = dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let post_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster post bind group"),
            layout: &post_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&dummy_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&dummy_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&dummy_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: post_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&bloom_sampler),
                },
            ],
        });

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
            preview_target,
            pipeline,
            camera_bind_group,
            camera_uniform,
            lighting_uniform,
            _default_texture: default_texture,
            default_texture_view,
            _default_normal_texture: default_normal_texture,
            default_normal_texture_view,
            _default_mra_texture: default_mra_texture,
            default_mra_texture_view,
            _default_sampler: default_sampler,
            material_bind_group_layout,
            default_material_bind_group,
            material_gpu: HashMap::new(),
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
            csm_depth_views,
            _csm_depth_textures: csm_depth_textures,
            _csm_sampler: csm_sampler,
            csm_uniform,
            csm_cascade_uniforms,
            csm_cascade_bind_groups,
            shadow_pipeline,
            shadow_bind_group_layout,
            material_cache: HashMap::new(),
            skybox_pipeline,
            skybox_bind_group,
            skybox_uniform,
            fog_uniform,
            _skybox_default_cubemap: skybox_default_cubemap,
            _skybox_default_cubemap_view: skybox_default_cubemap_view,
            _skybox_sampler: skybox_sampler,
            destroy_queue: Vec::new(),
            ibl_irradiance_map,
            ibl_irradiance_view,
            ibl_prefilter_map,
            ibl_prefilter_views,
            ibl_brdf_lut,
            ibl_brdf_lut_view,
            ibl_sampler,
            ibl_enabled: false,
            ibl_scratch_tex: Some(ibl_scratch_tex),
            ibl_scratch_view: Some(ibl_scratch_view),
            ibl_bake_bgl: Some(ibl_bake_bgl),
            post_pipeline,
            post_bind_group_layout,
            post_bind_group: post_bg,
            post_cached_bg: None,
            post_cached_dims: (0, 0),
            post_uniform,
            bloom_pipeline_downsample: None,
            bloom_pipeline_upsample: None,
            bloom_bind_group_layout: None,
            bloom_mip_views: Vec::new(),
            bloom_mip_textures: Vec::new(),
            bloom_cached_down_bgs: Vec::new(),
            bloom_cached_up_bgs: Vec::new(),
            bloom_cached_dims: (0, 0),
            bloom_sampler,
            bloom_uniform,
            ssao_pipeline: None,
            ssao_bind_group_layout: None,
            ssao_bind_group: None,
            ssao_cached_bg: None,
            ssao_cached_dims: (0, 0),
            ssao_noise_texture,
            ssao_noise_view,
            ssao_samples_buffer,
            ssao_linear_sampler,
            ssao_output_texture: None,
            ssao_output_view: None,
            ssao_uniform,
            hdr_target: None,
            post_target_width: 0,
            post_target_height: 0,
            ibl_irradiance_compute,
            ibl_prefilter_compute,
            ibl_brdf_compute,
            ibl_brdf_bgl: Some(ibl_brdf_bgl),
            ssao_compute_pipeline,
            ssao_compute_bgl: Some(ssao_compute_bgl),
            bloom_compute_down,
            bloom_compute_up,
            bloom_compute_bgl: Some(bloom_compute_bgl),
            readback_staging: None,
            readback_staging_dims: (0, 0),
            performance_config: RenderPerformanceConfig::default(),
            dynamic_resolution: engine_render::DynamicResolutionController::new(
                RenderPerformanceConfig::default().dynamic_resolution,
                1.0,
            ),
            performance_metrics: RenderPerformanceMetrics {
                render_scale: 1.0,
                ..RenderPerformanceMetrics::default()
            },
        };
        renderer.upload_debug_meshes();
        renderer.bake_ibl();
        Ok(renderer)
    }
}
