use std::{collections::HashMap, sync::Arc};

use crate::{
    device::*, format::*, math::*, meshes::*, render::*, scene_uniforms::default_csm_params,
    shaders::*, uniforms::*,
};
use engine_core::{EngineError, EngineResult, HandleAllocator};
use engine_render::{
    ImageFormat, RenderPerformanceConfig, RenderPerformanceMetrics, RenderTargetDesc, ViewKind,
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
    fn requested_device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
        let mut descriptor = wgpu::DeviceDescriptor {
            label: Some("varg wgpu device"),
            ..Default::default()
        };
        let timestamp_features =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if adapter.features().contains(timestamp_features) {
            descriptor.required_features |= timestamp_features;
        }
        descriptor
    }

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
            .request_device(&Self::requested_device_descriptor(&adapter))
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

    /// Creates a surface renderer from raw platform handles.
    ///
    /// This is used by native editor hosts that present into a toolkit-owned
    /// child surface instead of a winit window.
    ///
    /// # Safety
    ///
    /// The raw display/window handles must remain valid for the lifetime of the
    /// returned renderer, and the host toolkit must keep the surface alive until
    /// the renderer is dropped.
    pub unsafe fn new_raw_surface_with_performance(
        raw_display_handle: Option<wgpu::rwh::RawDisplayHandle>,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        performance: RenderPerformanceConfig,
    ) -> EngineResult<Self> {
        pollster::block_on(unsafe {
            Self::new_raw_surface_async_with_performance(
                raw_display_handle,
                raw_window_handle,
                width,
                height,
                performance,
            )
        })
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
            .request_device(&Self::requested_device_descriptor(&adapter))
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

    /// Creates a surface renderer asynchronously from raw platform handles.
    ///
    /// # Safety
    ///
    /// The raw display/window handles must remain valid for the lifetime of the
    /// returned renderer.
    pub async unsafe fn new_raw_surface_async_with_performance(
        raw_display_handle: Option<wgpu::rwh::RawDisplayHandle>,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        performance: RenderPerformanceConfig,
    ) -> EngineResult<Self> {
        let instance = wgpu::Instance::default();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .map_err(|error| EngineError::other(format!("create wgpu surface failed: {error}")))?;
        // SAFETY: instance is moved into the returned struct and outlives the surface.
        let surface: wgpu::Surface<'static> = unsafe { std::mem::transmute(surface) };
        Self::new_surface_async_with_performance(instance, surface, width, height, performance)
            .await
    }

    /// Creates a wgpu device configured to present into a surface asynchronously.
    pub async fn new_surface_async(
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> EngineResult<Self> {
        let instance = wgpu::Instance::default();
        Self::new_surface_async_with_performance(
            instance,
            surface,
            width,
            height,
            RenderPerformanceConfig::default(),
        )
        .await
    }

    async fn new_surface_async_with_performance(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        performance: RenderPerformanceConfig,
    ) -> EngineResult<Self> {
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
            .request_device(&Self::requested_device_descriptor(&adapter))
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
        let timestamp_features =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let timestamp_resources = device.features().contains(timestamp_features).then(|| {
            let query = device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("varg frame timestamp query"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            });
            let resolve = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("varg frame timestamp resolve"),
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("varg frame timestamp readback"),
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            (query, resolve, readback)
        });
        let mut target_allocator = HandleAllocator::default();
        let default_target = create_target(
            &device,
            &mut target_allocator,
            RenderTargetDesc {
                width: width.max(1),
                height: height.max(1),
                internal_width: width.max(1),
                internal_height: height.max(1),
                ui_width: width.max(1),
                ui_height: height.max(1),
                color_format: format,
                with_depth: true,
                samples: 1,
                kind: ViewKind::SceneView,
                label: Some("varg default offscreen target"),
            },
        )?;
        let game_target = create_target(
            &device,
            &mut target_allocator,
            RenderTargetDesc {
                width: width.max(1),
                height: height.max(1),
                internal_width: width.max(1),
                internal_height: height.max(1),
                ui_width: width.max(1),
                ui_height: height.max(1),
                color_format: format,
                with_depth: true,
                samples: 1,
                kind: ViewKind::GameView,
                label: Some("varg game offscreen target"),
            },
        )?;
        let preview_target = create_target(
            &device,
            &mut target_allocator,
            RenderTargetDesc {
                width: 320,
                height: 180,
                internal_width: 320,
                internal_height: 180,
                ui_width: 320,
                ui_height: 180,
                color_format: format,
                with_depth: true,
                samples: 1,
                kind: ViewKind::Preview,
                label: Some("varg camera preview offscreen target"),
            },
        )?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg forward shader"),
            source: wgpu::ShaderSource::Wgsl(FORWARD_SHADER.into()),
        });
        let camera_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg camera uniform"),
            contents: bytemuck::bytes_of(&CameraUniform {
                view_projection: IDENTITY_MAT4,
                camera_position: [0.0, 0.0, 5.0, 1.0],
                camera_forward: [0.0, 0.0, -1.0, 0.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let temporal_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg temporal uniform"),
            contents: bytemuck::bytes_of(&TemporalUniform {
                previous_view_projection: IDENTITY_MAT4,
                current_view_projection: IDENTITY_MAT4,
                jitter_reset: [0.0, 0.0, 1.0, 0.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let gpu_particles = crate::particles::GpuParticlePipeline::new(&device, &camera_uniform);
        let lighting_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg lighting uniform"),
            contents: bytemuck::bytes_of(&LightingUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let gi_probe_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg gi probe uniform"),
            contents: bytemuck::bytes_of(&GiProbeUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let gi_probe_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg gi probe buffer"),
            contents: bytemuck::cast_slice(
                &[GiProbe {
                    irradiance_pos_x: [0.0; 4],
                    irradiance_neg_x: [0.0; 4],
                    irradiance_pos_y: [0.0; 4],
                    irradiance_neg_y: [0.0; 4],
                    irradiance_pos_z: [0.0; 4],
                    irradiance_neg_z: [0.0; 4],
                }; MAX_GI_PROBES],
            ),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        let default_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg default white texture"),
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
            label: Some("varg default sampler"),
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
                label: Some(&format!("varg csm cascade {i} depth")),
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
            label: Some("varg csm sampler"),
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
            label: Some("varg csm uniform"),
            contents: bytemuck::bytes_of(&CsmUniform {
                cascade_vps: [IDENTITY_MAT4; CSM_CASCADE_COUNT],
                cascade_splits: [0.0; 4],
                params: default_csm_params(),
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        // Pre-allocate cascade uniform buffers (updated via write_buffer per frame).
        let mut csm_cascade_uniforms = Vec::with_capacity(CSM_CASCADE_COUNT);
        for i in 0..CSM_CASCADE_COUNT {
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("varg csm cascade {i} uniform")),
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
                label: Some("varg scene bind group layout"),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 14,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 15,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 16,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        // IBL resources
        let ibl_irradiance_map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg ibl irradiance map"),
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
            label: Some("varg ibl irradiance cube view"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        let ibl_prefilter_map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg ibl prefilter map"),
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
                label: Some("varg ibl prefilter cube view"),
                dimension: Some(wgpu::TextureViewDimension::Cube),
                base_mip_level: 0,
                mip_level_count: Some(5),
                ..Default::default()
            })];
        let ibl_brdf_lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg ibl brdf lut"),
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
            label: Some("varg ibl sampler"),
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
            label: Some("varg ibl scratch"),
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
            label: Some("varg ssao noise"),
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
            label: Some("varg ssao linear sampler"),
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
            label: Some("varg ssao samples"),
            contents: bytemuck::cast_slice(&ssao_kernel),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bloom_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("varg bloom sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let fog_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg fog uniform"),
            contents: bytemuck::bytes_of(&FogUniform {
                density: 0.0,
                _pad: [0.0; 3],
                color: [0.6, 0.7, 0.85],
                enabled: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varg scene bind group"),
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
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: temporal_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 15,
                    resource: gi_probe_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 16,
                    resource: gi_probe_buffer.as_entire_binding(),
                },
            ],
        });

        // Group 1: material textures
        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg material bind group layout"),
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
            label: Some("varg default normal texture"),
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
            label: Some("varg default MRA texture"),
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
            label: Some("varg default material bind group"),
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
            label: Some("varg forward pipeline layout"),
            bind_group_layouts: &[
                Some(&scene_bind_group_layout),
                Some(&material_bind_group_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg forward pipeline"),
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
                            2 => Float32x2,
                            3 => Float32x4
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            4 => Float32x3,
                            5 => Float32x3,
                            6 => Float32x4,
                            7 => Float32x4,
                            8 => Float32,
                            9 => Float32,
                            10 => Float32x3,
                            11 => Float32
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
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rg16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let transparent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg transparent pipeline"),
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
                            2 => Float32x2,
                            3 => Float32x4
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            4 => Float32x3,
                            5 => Float32x3,
                            6 => Float32x4,
                            7 => Float32x4,
                            8 => Float32,
                            9 => Float32,
                            10 => Float32x3,
                            11 => Float32
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
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rg16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let skinned_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg skinned mesh shader"),
            source: wgpu::ShaderSource::Wgsl(SKINNED_SHADER.into()),
        });
        let skinned_camera_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg skinned camera layout"),
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
        let skinned_bone_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg skinned bone layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let skinned_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varg skinned camera bind group"),
            layout: &skinned_camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform.as_entire_binding(),
            }],
        });
        let skinned_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("varg skinned pipeline layout"),
                bind_group_layouts: &[
                    Some(&skinned_camera_layout),
                    Some(&skinned_bone_bind_group_layout),
                ],
                immediate_size: 0,
            });
        let skinned_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg skinned mesh pipeline"),
            layout: Some(&skinned_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &skinned_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SkinnedVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3,
                        1 => Float32x3,
                        2 => Float32x2,
                        3 => Uint32x4,
                        4 => Float32x4
                    ],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &skinned_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: to_wgpu_format(format),
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Grid pipeline ---
        let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg grid shader"),
            source: wgpu::ShaderSource::Wgsl(GRID_SHADER.into()),
        });
        let grid_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg grid bind group layout"),
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
            label: Some("varg grid bind group"),
            layout: &grid_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform.as_entire_binding(),
            }],
        });
        let grid_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varg grid pipeline layout"),
            bind_group_layouts: &[Some(&grid_bind_group_layout)],
            immediate_size: 0,
        });
        let grid_color_format = wgpu::TextureFormat::Rgba16Float;
        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg grid pipeline"),
            layout: Some(&grid_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &grid_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4],
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
            label: Some("varg grid vertices"),
            contents: bytemuck::cast_slice(&grid_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grid_vertex_count = grid_vertices.len() as u32;

        // Shadow pipeline
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg shadow shader"),
            source: wgpu::ShaderSource::Wgsl(SHADOW_SHADER.into()),
        });
        let shadow_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg shadow bind group layout"),
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
                label: Some("varg shadow pipeline layout"),
                bind_group_layouts: &[Some(&shadow_bind_group_layout)],
                immediate_size: 0,
            });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg shadow pipeline"),
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
                            2 => Float32x2,
                            3 => Float32x4
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            4 => Float32x3,
                            5 => Float32x3,
                            6 => Float32x4,
                            7 => Float32x4,
                            8 => Float32,
                            9 => Float32,
                            10 => Float32x3,
                            11 => Float32
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
                label: Some(&format!("varg csm cascade {i} bind group")),
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
            label: Some("varg skybox shader"),
            source: wgpu::ShaderSource::Wgsl(SKYBOX_SHADER.into()),
        });
        let skybox_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg skybox uniform"),
            contents: bytemuck::bytes_of(&SkyboxUniform {
                view_rotation_only: IDENTITY_MAT4,
                zenith_color: [0.09, 0.11, 0.14, 1.0],
                horizon_color: [0.18, 0.2, 0.22, 1.0],
                rotation_intensity: [0.0, 0.35, 0.0, 0.0],
                use_cubemap: [0, 0, 0, 0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let skybox_default_cubemap = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg skybox default cubemap"),
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
                &[42, 46, 52, 255],
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
                label: Some("varg skybox default cubemap view"),
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            });
        let skybox_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("varg skybox sampler"),
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
                label: Some("varg skybox bind group layout"),
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
            label: Some("varg skybox bind group"),
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
                label: Some("varg skybox pipeline layout"),
                bind_group_layouts: &[Some(&skybox_bind_group_layout)],
                immediate_size: 0,
            });
        let skybox_color_format = wgpu::TextureFormat::Rgba16Float;
        let skybox_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg skybox pipeline"),
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
            label: Some("varg post shader"),
            source: wgpu::ShaderSource::Wgsl(POST_SHADER.into()),
        });
        let post_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg post bind group layout"),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let post_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg post uniform"),
            contents: bytemuck::bytes_of(&PostProcessUniform {
                inv_view_projection: IDENTITY_MAT4,
                view_projection: IDENTITY_MAT4,
                camera_position: [0.0, 0.0, 0.0, 1.0],
                render_width: width as f32,
                render_height: height as f32,
                inv_render_width: 1.0 / width as f32,
                inv_render_height: 1.0 / height as f32,
                output_width: width as f32,
                output_height: height as f32,
                inv_output_width: 1.0 / width as f32,
                inv_output_height: 1.0 / height as f32,
                exposure: 1.0,
                bloom_intensity: 0.04,
                ssao_enabled: 0.0,
                upscale_sharpness: 0.35,
                ssgi_enabled: 1.0,
                ssgi_intensity: SSGI_INTENSITY,
                ssr_enabled: 1.0,
                ssr_intensity: 0.35,
                taa_reset: 1.0,
                taa_history_weight: 0.72,
                taa_enabled: 1.0,
                _pad: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let taa_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg taa resolve shader"),
            source: wgpu::ShaderSource::Wgsl(TAA_SHADER.into()),
        });
        let taa_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg taa bind group layout"),
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba16Float,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let taa_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varg taa pipeline layout"),
            bind_group_layouts: &[Some(&taa_bind_group_layout)],
            immediate_size: 0,
        });
        let taa_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("varg taa resolve pipeline"),
            layout: Some(&taa_pipeline_layout),
            module: &taa_shader,
            entry_point: Some("cs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varg post pipeline layout"),
            bind_group_layouts: &[Some(&post_bind_group_layout)],
            immediate_size: 0,
        });
        let post_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg post pipeline"),
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
            label: Some("varg ibl brdf bgl"),
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
            label: Some("varg ibl brdf shader"),
            source: wgpu::ShaderSource::Wgsl(IBL_BRDF_LUT_SHADER.into()),
        });
        let ibl_brdf_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("varg ibl brdf pipeline layout"),
                bind_group_layouts: &[Some(&ibl_brdf_bgl)],
                immediate_size: 0,
            });
        let ibl_brdf_compute = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("varg ibl brdf compute"),
                layout: Some(&ibl_brdf_pipeline_layout),
                module: &ibl_brdf_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        // IBL baking: bind group layout shared by irradiance and prefilter compute pipelines
        let ibl_bake_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varg ibl bake bgl"),
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
            label: Some("varg ibl bake pipeline layout"),
            bind_group_layouts: &[Some(&ibl_bake_bgl)],
            immediate_size: 0,
        });
        let ibl_irradiance_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg ibl irradiance shader"),
            source: wgpu::ShaderSource::Wgsl(IBL_IRRADIANCE_SHADER.into()),
        });
        let ibl_irradiance_compute = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("varg ibl irradiance compute"),
                layout: Some(&ibl_bake_pl_layout),
                module: &ibl_irradiance_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        let ibl_prefilter_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg ibl prefilter shader"),
            source: wgpu::ShaderSource::Wgsl(IBL_PREFILTER_SHADER.into()),
        });
        let ibl_prefilter_compute = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("varg ibl prefilter compute"),
                layout: Some(&ibl_bake_pl_layout),
                module: &ibl_prefilter_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        // SSAO compute pipeline
        let ssao_compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varg ssao compute bgl"),
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
            label: Some("varg ssao shader"),
            source: wgpu::ShaderSource::Wgsl(SSAO_SHADER.into()),
        });
        let ssao_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varg ssao pipeline layout"),
            bind_group_layouts: &[Some(&ssao_compute_bgl)],
            immediate_size: 0,
        });
        let ssao_compute_pipeline = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("varg ssao compute"),
                layout: Some(&ssao_pipeline_layout),
                module: &ssao_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        // SSGI compute pipeline
        let ssgi_compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varg ssgi compute bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let ssgi_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg ssgi shader"),
            source: wgpu::ShaderSource::Wgsl(SSGI_SHADER.into()),
        });
        let ssgi_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varg ssgi pipeline layout"),
            bind_group_layouts: &[Some(&ssgi_compute_bgl)],
            immediate_size: 0,
        });
        let ssgi_compute_pipeline = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("varg ssgi compute"),
                layout: Some(&ssgi_pipeline_layout),
                module: &ssgi_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        // Bloom compute pipelines
        let bloom_compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varg bloom compute bgl"),
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
            label: Some("varg bloom downsample shader"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_DOWNSAMPLE_SHADER.into()),
        });
        let bloom_up_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg bloom upsample shader"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_UPSAMPLE_SHADER.into()),
        });
        let bloom_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("varg bloom pipeline layout"),
                bind_group_layouts: &[Some(&bloom_compute_bgl)],
                immediate_size: 0,
            });
        let bloom_compute_down = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("varg bloom downsample compute"),
                layout: Some(&bloom_pipeline_layout),
                module: &bloom_down_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        let bloom_compute_up = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("varg bloom upsample compute"),
                layout: Some(&bloom_pipeline_layout),
                module: &bloom_up_shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            },
        ));
        let bloom_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg bloom uniform"),
            contents: bytemuck::bytes_of(&BloomUniform {
                intensity: 0.04,
                threshold: 1.0,
                knee: 0.5,
                _pad: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let ssao_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg ssao uniform"),
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
        let ssgi_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg ssgi uniform"),
            contents: bytemuck::bytes_of(&SsgiUniform {
                width: width as f32,
                height: height as f32,
                inv_width: 1.0 / width.max(1) as f32,
                inv_height: 1.0 / height.max(1) as f32,
                radius: SSGI_RADIUS,
                intensity: SSGI_INTENSITY,
                thickness: 0.08,
                sample_count: 8.0,
                frame_index: 0.0,
                history_blend: 0.0,
                reset_history: 1.0,
                _pad: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg cube vertices"),
            contents: bytemuck::cast_slice(CUBE_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg cube indices"),
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

        let surface_gui_format = surface_state
            .as_ref()
            .map(|(_, config)| config.format)
            .unwrap_or_else(|| to_wgpu_format(format));
        let (surface, surface_config) = surface_state
            .map(|(surface, config)| (Some(surface), Some(config)))
            .unwrap_or((None, None));

        // Dummy 1x1 textures for initial post bind group
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg dummy 1x1"),
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
        let dummy_depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg post dummy depth"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_depth_view = dummy_depth.create_view(&wgpu::TextureViewDescriptor::default());
        let post_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varg post bind group"),
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
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&dummy_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&dummy_depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&dummy_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&dummy_tex_view),
                },
            ],
        });

        let gui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varg gui shader"),
            source: wgpu::ShaderSource::Wgsl(GUI_SHADER.into()),
        });
        let gui_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("varg gui bind group layout"),
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
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let gui_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varg gui pipeline layout"),
            bind_group_layouts: &[Some(&gui_bind_group_layout)],
            immediate_size: 0,
        });
        let gui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg gui pipeline"),
            layout: Some(&gui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &gui_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuGuiVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Uint32
                    ],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &gui_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: to_wgpu_format(format),
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let surface_gui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("varg surface gui pipeline"),
            layout: Some(&gui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &gui_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuGuiVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Uint32
                    ],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &gui_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_gui_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let gui_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("varg gui sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let gui_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varg gui uniform"),
            contents: bytemuck::bytes_of(&GuiUniform {
                screen_size: [width.max(1) as f32, height.max(1) as f32],
                _pad: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let gui_vertex_capacity = 4;
        let gui_index_capacity = 6;
        let gui_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varg gui vertices"),
            size: (gui_vertex_capacity * std::mem::size_of::<GpuGuiVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gui_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varg gui indices"),
            size: (gui_index_capacity * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
            bone_palette_counts: HashMap::new(),
            targets,
            default_target,
            game_target,
            preview_target,
            pipeline,
            transparent_pipeline,
            camera_bind_group,
            camera_uniform,
            temporal_uniform,
            lighting_uniform,
            gi_probe_uniform,
            gi_probe_buffer,
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
            skinned_mesh_cache: HashMap::new(),
            skinned_pipeline,
            skinned_camera_bind_group,
            skinned_bone_bind_group_layout,
            surface,
            surface_config,
            surface_depth: None,
            surface_depth_view: None,
            surface_suspended: false,
            surface_viewport: None,
            next_gui_texture: 1,
            gui_textures: HashMap::new(),
            gui_pipeline,
            surface_gui_pipeline,
            gui_bind_group_layout,
            gui_sampler,
            gui_uniform,
            gui_vertex_buffer,
            gui_index_buffer,
            gui_vertex_capacity,
            gui_index_capacity,
            pending_surface_gui: None,
            submitted_worlds: 0,
            editor_grid_enabled: false,
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
            _shadow_bind_group_layout: shadow_bind_group_layout,
            material_cache: HashMap::new(),
            skybox_pipeline,
            skybox_bind_group_layout,
            skybox_bind_group,
            skybox_cubemaps: HashMap::new(),
            active_skybox_cubemap: None,
            active_ibl_cubemap: None,
            skybox_uniform,
            fog_uniform,
            _skybox_default_cubemap: skybox_default_cubemap,
            _skybox_default_cubemap_view: skybox_default_cubemap_view,
            _skybox_sampler: skybox_sampler,
            destroy_queue: Vec::new(),
            ibl_irradiance_map,
            _ibl_irradiance_view: ibl_irradiance_view,
            ibl_prefilter_map,
            _ibl_prefilter_views: ibl_prefilter_views,
            _ibl_brdf_lut: ibl_brdf_lut,
            ibl_brdf_lut_view,
            ibl_sampler,
            ibl_enabled: false,
            ibl_scratch_tex: Some(ibl_scratch_tex),
            ibl_scratch_view: Some(ibl_scratch_view),
            ibl_bake_bgl: Some(ibl_bake_bgl),
            post_pipeline,
            post_bind_group_layout,
            _post_bind_group: post_bg,
            taa_pipeline,
            taa_bind_group_layout,
            taa_bind_group: None,
            post_cached_bg: None,
            post_cached_uses_taa: false,
            _post_cached_dims: (0, 0),
            post_uniform,
            _bloom_pipeline_downsample: None,
            _bloom_pipeline_upsample: None,
            _bloom_bind_group_layout: None,
            bloom_mip_views: Vec::new(),
            bloom_mip_textures: Vec::new(),
            bloom_cached_down_bgs: Vec::new(),
            bloom_cached_up_bgs: Vec::new(),
            _bloom_cached_dims: (0, 0),
            bloom_sampler,
            bloom_uniform,
            _ssao_pipeline: None,
            _ssao_bind_group_layout: None,
            _ssao_bind_group: None,
            ssao_cached_bg: None,
            _ssao_cached_dims: (0, 0),
            _ssao_noise_texture: ssao_noise_texture,
            ssao_noise_view,
            ssao_samples_buffer,
            _ssao_linear_sampler: ssao_linear_sampler,
            ssao_output_texture: None,
            ssao_output_view: None,
            ssao_uniform,
            ssgi_cached_bg: None,
            ssgi_output_texture: None,
            ssgi_output_view: None,
            ssgi_history_texture: None,
            ssgi_history_view: None,
            ssgi_uniform,
            ssgi_compute_pipeline,
            ssgi_compute_bgl: Some(ssgi_compute_bgl),
            hdr_target: None,
            hdr_normal_texture: None,
            hdr_normal_view: None,
            hdr_albedo_texture: None,
            hdr_albedo_view: None,
            hdr_motion_texture: None,
            hdr_motion_view: None,
            taa_resolved_texture: None,
            taa_resolved_view: None,
            taa_history_texture: None,
            taa_history_view: None,
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
                frame_generation_multiplier: 1,
                ..RenderPerformanceMetrics::default()
            },
            active_upscaler: engine_render::UpscalerKind::Native,
            anti_aliasing: engine_render::AntiAliasingMode::Taa,
            upscale_sharpness: 0.35,
            temporal_state: engine_render::TemporalFrameState::default(),
            latest_temporal_camera: engine_render::TemporalCameraData::default(),
            reset_temporal_history: true,
            active_frame_plan: None,
            latest_submitted_objects: 0,
            latest_visible_objects: 0,
            latest_culled_objects: 0,
            latest_submitted_lights: 0,
            latest_visible_lights: 0,
            latest_culled_lights: 0,
            latest_shadowed_lights: 0,
            latest_shadow_caster_batches: 0,
            latest_directional_shadow_cascades: 0,
            latest_draw_calls: 0,
            latest_triangles: 0,
            gpu_particles,
            gpu_timestamp_query: timestamp_resources
                .as_ref()
                .map(|resources| resources.0.clone()),
            gpu_timestamp_resolve: timestamp_resources
                .as_ref()
                .map(|resources| resources.1.clone()),
            gpu_timestamp_readback: timestamp_resources.map(|resources| resources.2),
            gpu_timestamp_receiver: None,
        };
        renderer.upload_debug_meshes();
        renderer.bake_ibl();
        Ok(renderer)
    }
}
