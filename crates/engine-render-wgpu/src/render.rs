use crate::{device::*, format::*, passes::*, scene_uniforms::*, uniforms::*};
use engine_core::{EngineError, EngineResult, Handle};
use engine_render::{RenderTargetDesc, RenderWorld};
use std::time::Instant;

impl WgpuRenderDevice {
    pub(crate) fn prepare_render_batches(
        &mut self,
        world: &RenderWorld,
    ) -> Vec<(String, u32, String, bool)> {
        let batches = self.mesh_batches_from_world(world);
        let total_instances: usize = batches.iter().map(|(_, inst, _, _)| inst.len()).sum();
        if total_instances > self.instance_capacity {
            self.instance_capacity = total_instances.next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        let mut all_instances = Vec::with_capacity(total_instances);
        for (_, instances, _, _) in &batches {
            all_instances.extend_from_slice(instances);
        }
        if !all_instances.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&all_instances),
            );
        }
        batches
            .into_iter()
            .map(|(name, instances, mat, casts_shadows)| {
                (name, instances.len() as u32, mat, casts_shadows)
            })
            .collect()
    }

    /// Renders a render world to the default offscreen target, bypassing any surface.
    ///
    /// Use this when the host composites the result into its own UI.
    pub fn render_world_offscreen(&mut self, world: &RenderWorld) -> EngineResult<()> {
        let handle = self.default_target.handle;
        let (tw, th) = self.default_target.size();
        self.render_world_to_target(
            world,
            handle,
            tw as f32 / th.max(1) as f32,
            "aster offscreen render world encoder",
            "default wgpu target is missing",
        )
    }

    /// Renders a render world to the game offscreen target, bypassing any surface.
    ///
    /// Use this when the host composites the game view result into its own UI.
    pub fn render_world_offscreen_game(&mut self, world: &RenderWorld) -> EngineResult<()> {
        let handle = self.game_target.handle;
        let (tw, th) = self.game_target.size();
        self.render_world_to_target(
            world,
            handle,
            tw as f32 / th.max(1) as f32,
            "aster game offscreen render world encoder",
            "game wgpu target is missing",
        )
    }

    /// Renders a render world to the preview offscreen target.
    pub fn render_world_offscreen_preview(&mut self, world: &RenderWorld) -> EngineResult<()> {
        let handle = self.preview_target.handle;
        let (tw, th) = self.preview_target.size();
        self.render_world_to_target(
            world,
            handle,
            tw as f32 / th.max(1) as f32,
            "aster preview offscreen render world encoder",
            "preview wgpu target is missing",
        )
    }

    /// Read back the game offscreen target as RGBA pixels.
    ///
    /// Returns `(width, height, rgba_bytes)`. Uses a staging buffer and GPU readback.
    /// This is a synchronous blocking call — it waits for the GPU to finish.
    pub fn readback_game_target(&mut self) -> EngineResult<(u32, u32, Vec<u8>)> {
        let (w, h) = self.game_target.size();
        let format = self.game_target.desc.color_format;
        self.readback_target(w, h, format, self.game_target.handle)
    }

    /// Read back the default offscreen target as RGBA pixels.
    ///
    /// Returns `(width, height, rgba_bytes)`. Uses a staging buffer and GPU readback.
    /// This is a synchronous blocking call — it waits for the GPU to finish.
    pub fn readback_default_target(&mut self) -> EngineResult<(u32, u32, Vec<u8>)> {
        let (w, h) = self.default_target.size();
        let format = self.default_target.desc.color_format;
        self.readback_target(w, h, format, self.default_target.handle)
    }

    pub(crate) fn readback_target(
        &mut self,
        w: u32,
        h: u32,
        format: engine_render::ImageFormat,
        handle: Handle,
    ) -> EngineResult<(u32, u32, Vec<u8>)> {
        let bytes_per_pixel = format.bytes_per_pixel() as u64;
        let unpadded = w as u64 * bytes_per_pixel;
        let padding = (256 - (unpadded % 256)) % 256;
        let bytes_per_row = unpadded + padding;
        let total_bytes = bytes_per_row * h as u64;

        let target = self
            .targets
            .get(&handle)
            .ok_or_else(|| EngineError::invalid_handle("readback target missing"))?;

        // Reuse pre-allocated staging buffer if dimensions match
        if self.readback_staging_dims != (w, h) {
            self.readback_staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aster viewport readback staging"),
                size: total_bytes,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.readback_staging_dims = (w, h);
        }
        let staging = self.readback_staging.as_ref().unwrap();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster viewport readback encoder"),
            });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target._color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row as u32),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));

        let buffer_slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        receiver
            .recv()
            .map_err(|_| EngineError::other("viewport readback channel closed"))?
            .map_err(|e| EngineError::other(format!("viewport readback map failed: {e}")))?;

        let mapped = buffer_slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((w * h * bytes_per_pixel as u32) as usize);
        for row in 0..h as usize {
            let start = row * bytes_per_row as usize;
            let end = start + w as usize * bytes_per_pixel as usize;
            pixels.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        staging.unmap();

        tracing::trace!(target: "engine", w, h, bytes = pixels.len(), "readback complete");
        Ok((w, h, pixels))
    }

    pub(crate) fn render_world_to_target(
        &mut self,
        world: &RenderWorld,
        target_handle: Handle,
        aspect: f32,
        encoder_label: &str,
        missing_error: &str,
    ) -> EngineResult<()> {
        let render_started = Instant::now();
        let batches = self.prepare_render_batches(world);
        let total_instances: u32 = batches.iter().map(|(_, count, _, _)| *count).sum();
        tracing::debug!(
            target: "engine",
            batch_count = batches.len(),
            total_instances,
            has_camera = world.camera.is_some(),
            skybox = world.skybox.is_some(),
            encoder_label,
            "render_world_to_target"
        );
        let uniform = camera_uniform_from_world(world, aspect);
        self.queue
            .write_buffer(&self.camera_uniform, 0, bytemuck::bytes_of(&uniform));
        let lighting = lighting_uniform_from_world(world);
        self.queue
            .write_buffer(&self.lighting_uniform, 0, bytemuck::bytes_of(&lighting));
        let csm = csm_uniform_from_world(world, aspect);
        self.queue
            .write_buffer(&self.csm_uniform, 0, bytemuck::bytes_of(&csm));
        let skybox = skybox_uniform_from_world(world);
        self.queue
            .write_buffer(&self.skybox_uniform, 0, bytemuck::bytes_of(&skybox));
        let fog = fog_uniform_from_world(world);
        self.queue
            .write_buffer(&self.fog_uniform, 0, bytemuck::bytes_of(&fog));

        // Resolve target dimensions before any mutable operations.
        let (tw, th) = {
            let target = self
                .targets
                .get(&target_handle)
                .ok_or_else(|| EngineError::invalid_handle(missing_error))?;
            target._desc.internal_size()
        };

        let (ow, oh) = {
            let target = self
                .targets
                .get(&target_handle)
                .ok_or_else(|| EngineError::invalid_handle(missing_error))?;
            target._desc.output_size()
        };
        let (temporal_camera, reset_history) =
            temporal_camera_from_world(world, aspect, (tw, th), &mut self.temporal_state);
        self.latest_temporal_camera = temporal_camera;
        self.reset_temporal_history = reset_history;
        let frame_res =
            self.encode_frame_passes(&batches, &csm, tw, th, ow, oh, true, encoder_label);

        // Build resources in mutable phase, then get output_view immutably.
        let target = self
            .targets
            .get(&target_handle)
            .ok_or_else(|| EngineError::invalid_handle(missing_error))?;
        let output_view = &target.color_view;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(encoder_label),
            });
        self.encode_csm_shadow_passes(&mut encoder, &csm, &batches);
        self.encode_hdr_forward_passes(&mut encoder, &batches);
        self.encode_ssao_pass(&mut encoder, &frame_res);
        self.encode_ssgi_pass(&mut encoder, &frame_res);
        let _bloom_view = self.encode_bloom_pass(&mut encoder, &frame_res);
        self.encode_post_pass(&mut encoder, &frame_res, output_view);

        self.queue.submit(std::iter::once(encoder.finish()));
        self.submitted_worlds = self.submitted_worlds.saturating_add(1);
        self.performance_metrics = engine_render::RenderPerformanceMetrics {
            render_cpu_ms: render_started.elapsed().as_secs_f32() * 1000.0,
            output_width: ow,
            output_height: oh,
            internal_width: tw,
            internal_height: th,
            render_scale: self.dynamic_resolution.scale(),
            upscaler: self.active_upscaler,
            frame_generation_multiplier: 1,
            ..Default::default()
        };
        tracing::trace!(target: "engine", submitted = self.submitted_worlds, "gpu submit ok");
        Ok(())
    }

    /// Ensures HDR target / bloom mips / SSAO output exist at the given size,
    /// uploads post/SSAO uniforms, and pre-allocates CSM cascade data.
    pub(crate) fn encode_frame_passes(
        &mut self,
        _batches: &[(String, u32, String, bool)],
        csm: &CsmUniform,
        tw: u32,
        th: u32,
        output_width: u32,
        output_height: u32,
        ssao_enabled: bool,
        _encoder_label: &str,
    ) -> FrameResources {
        self.queue.write_buffer(
            &self.post_uniform,
            0,
            bytemuck::bytes_of(&PostProcessUniform {
                render_width: tw as f32,
                render_height: th as f32,
                inv_render_width: 1.0 / tw.max(1) as f32,
                inv_render_height: 1.0 / th.max(1) as f32,
                output_width: output_width as f32,
                output_height: output_height as f32,
                inv_output_width: 1.0 / output_width.max(1) as f32,
                inv_output_height: 1.0 / output_height.max(1) as f32,
                exposure: 1.0,
                bloom_intensity: 0.04,
                ssao_enabled: if ssao_enabled { 1.0 } else { 0.0 },
                upscale_sharpness: if tw != output_width || th != output_height {
                    self.upscale_sharpness
                } else {
                    0.0
                },
                ssgi_enabled: 1.0,
                ssgi_intensity: SSGI_INTENSITY,
                _pad: [0.0; 2],
            }),
        );
        self.queue.write_buffer(
            &self.ssao_uniform,
            0,
            bytemuck::bytes_of(&SsaoUniform {
                radius: SSAO_RADIUS,
                bias: SSAO_BIAS,
                intensity: 1.0,
                _pad: 0.0,
                width: tw as f32,
                height: th as f32,
                inv_width: 1.0 / tw.max(1) as f32,
                inv_height: 1.0 / th.max(1) as f32,
            }),
        );
        self.queue.write_buffer(
            &self.ssgi_uniform,
            0,
            bytemuck::bytes_of(&SsgiUniform {
                width: tw as f32,
                height: th as f32,
                inv_width: 1.0 / tw.max(1) as f32,
                inv_height: 1.0 / th.max(1) as f32,
                radius: SSGI_RADIUS,
                intensity: SSGI_INTENSITY,
                thickness: 0.08,
                sample_count: 8.0,
                frame_index: self.submitted_worlds as f32,
                _pad: [0.0; 3],
            }),
        );

        self.ensure_hdr_target(tw, th);
        self.ensure_bloom_mips(tw, th);
        if ssao_enabled {
            self.ensure_ssao_output();
        }
        self.ensure_ssgi_output();

        // Pre-write cascade VP data into pre-allocated buffers.
        for cascade_idx in 0..CSM_CASCADE_COUNT {
            let cascade_vp = ShadowUniform {
                light_view_projection: csm.cascade_vps[cascade_idx],
            };
            self.queue.write_buffer(
                &self.csm_cascade_uniforms[cascade_idx],
                0,
                bytemuck::bytes_of(&cascade_vp),
            );
        }

        // Pre-build all cached bind groups so encoding is purely &self.
        let ssao_bg = if ssao_enabled && self.ssao_compute_pipeline.is_some() {
            Some(self.ensure_ssao_bind_group())
        } else {
            None
        };
        let ssao_view = self.ssao_output_view.clone();
        let ssgi_bg = if self.ssgi_compute_pipeline.is_some() {
            Some(self.ensure_ssgi_bind_group())
        } else {
            None
        };
        let ssgi_view = self.ssgi_output_view.clone();
        let (bloom_down_bgs, bloom_up_bgs) = self.ensure_bloom_bind_groups();
        let post_bg = Some(self.ensure_post_bind_group());

        FrameResources {
            ssao_bg,
            ssao_view,
            ssgi_bg,
            ssgi_view,
            bloom_down_bgs,
            bloom_up_bgs,
            post_bg,
        }
    }

    pub(crate) fn encode_csm_shadow_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        _csm: &CsmUniform,
        batches: &[(String, u32, String, bool)],
    ) {
        if !batches
            .iter()
            .any(|(_, count, _, casts_shadows)| *count > 0 && *casts_shadows)
        {
            return;
        }

        for cascade_idx in 0..CSM_CASCADE_COUNT {
            encode_shadow_pass(
                encoder,
                &self.csm_depth_views[cascade_idx],
                &self.shadow_pipeline,
                &self.csm_cascade_bind_groups[cascade_idx],
                &self.vertex_buffer,
                &self.index_buffer,
                &self.instance_buffer,
                batches,
                &self.mesh_cache,
            );
        }
    }

    pub(crate) fn encode_hdr_forward_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        batches: &[(String, u32, String, bool)],
    ) {
        let hdr = self.hdr_target.as_ref().unwrap();
        encode_skybox_pass(
            encoder,
            &hdr.color_view,
            hdr.depth_view.as_ref(),
            &self.skybox_pipeline,
            &self.skybox_bind_group,
        );
        encode_batched_forward_pass(
            encoder,
            &hdr.color_view,
            self.hdr_normal_view.as_ref().unwrap(),
            self.hdr_albedo_view.as_ref().unwrap(),
            hdr.depth_view.as_ref(),
            &self.pipeline,
            &self.camera_bind_group,
            &self.default_material_bind_group,
            &self.material_gpu,
            &self.mesh_cache,
            &self.vertex_buffer,
            &self.index_buffer,
            &self.instance_buffer,
            batches,
        );
        encode_grid_pass(
            encoder,
            &hdr.color_view,
            hdr.depth_view.as_ref(),
            &self.grid_pipeline,
            &self.grid_bind_group,
            &self.grid_vertex_buffer,
            self.grid_vertex_count,
        );
    }

    pub(crate) fn ensure_surface_depth(&mut self) {
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

    /// Groups render objects by mesh name for batched instanced rendering.
    pub(crate) fn ensure_hdr_target(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        let need_create = match &self.hdr_target {
            None => true,
            Some(t) => t._desc.width != w || t._desc.height != h,
        };
        if need_create {
            let color_format = wgpu::TextureFormat::Rgba16Float;
            let color = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aster hdr color"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: color_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
            let depth = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aster hdr depth"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
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
            let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
            self.hdr_target = Some(GpuTarget {
                _color: color,
                color_view,
                _depth: Some(depth),
                depth_view: Some(depth_view),
                _desc: RenderTargetDesc {
                    width: w,
                    height: h,
                    internal_width: w,
                    internal_height: h,
                    ui_width: w,
                    ui_height: h,
                    color_format: engine_render::ImageFormat::Rgba16Float,
                    with_depth: true,
                    samples: 1,
                    kind: engine_render::ViewKind::SceneView,
                    label: Some("hdr target"),
                },
            });
            let normal = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aster hdr normal roughness"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.hdr_normal_view =
                Some(normal.create_view(&wgpu::TextureViewDescriptor::default()));
            self.hdr_normal_texture = Some(normal);
            let albedo = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aster hdr albedo metallic"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.hdr_albedo_view =
                Some(albedo.create_view(&wgpu::TextureViewDescriptor::default()));
            self.hdr_albedo_texture = Some(albedo);
            self.post_target_width = w;
            self.post_target_height = h;
            // Invalidate cached bind groups that reference the old HDR target.
            self.post_cached_bg = None;
            self.ssao_cached_bg = None;
            self.ssgi_cached_bg = None;
        }
    }
}

pub(crate) fn select_present_mode(
    supported: &[wgpu::PresentMode],
    strategy: engine_render::PresentStrategy,
) -> wgpu::PresentMode {
    let preferences: &[wgpu::PresentMode] = match strategy {
        engine_render::PresentStrategy::LowLatency => &[
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Fifo,
        ],
        engine_render::PresentStrategy::Uncapped => &[
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Fifo,
        ],
        engine_render::PresentStrategy::VSync => &[wgpu::PresentMode::Fifo],
    };
    preferences
        .iter()
        .copied()
        .find(|mode| supported.contains(mode))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

pub(crate) fn scaled_render_size(width: u32, height: u32, scale: f32) -> (u32, u32) {
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}
