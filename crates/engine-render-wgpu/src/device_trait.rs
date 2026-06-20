use std::time::Instant;

use crate::{
    device::*, format::*, meshes::with_generated_tangents, render::*, scene_uniforms::*,
    uniforms::*,
};
use engine_core::{EngineError, EngineResult};
use engine_render::{
    BufferDesc, BufferHandle, GuiDrawList, GuiTextureId, ImageDesc, ImageHandle, RenderApi,
    RenderDevice, RenderFrame, RenderGraph, RenderMaterialTextures, RenderPerformanceMetrics,
    RenderTarget, RenderTargetDesc, RenderWorld,
};

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
        let render_started = Instant::now();
        let batches = self.prepare_render_batches(world);
        let aspect = self
            .surface_config
            .as_ref()
            .map(|cfg| cfg.width as f32 / cfg.height.max(1) as f32)
            .unwrap_or(16.0 / 9.0);
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

        if self.surface.is_some() {
            if self.surface_suspended {
                return Ok(());
            }
            self.ensure_surface_depth();
            let surface = self
                .surface
                .as_ref()
                .ok_or_else(|| EngineError::invalid_handle("wgpu surface is missing"))?;
            let surface_frame = match surface.get_current_texture() {
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
            let output_view = surface_frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let (sw, sh) = {
                let cfg = self.surface_config.as_ref().unwrap();
                (cfg.width, cfg.height)
            };
            let scale = self.dynamic_resolution.scale();
            let (rw, rh) = scaled_render_size(sw, sh, scale);

            let frame_res =
                self.encode_frame_passes(&batches, &csm, rw, rh, sw, sh, true, "aster surface");

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("aster surface render world encoder"),
                });
            self.encode_csm_shadow_passes(&mut encoder, &csm, &batches);
            self.encode_hdr_forward_passes(&mut encoder, &batches);
            self.encode_ssao_pass(&mut encoder, &frame_res);
            let _bloom_view = self.encode_bloom_pass(&mut encoder, &frame_res);
            self.encode_post_pass(&mut encoder, &frame_res, &output_view);

            self.queue.submit(std::iter::once(encoder.finish()));
            surface_frame.present();
            self.submitted_worlds = self.submitted_worlds.saturating_add(1);
            self.performance_metrics = RenderPerformanceMetrics {
                render_cpu_ms: render_started.elapsed().as_secs_f32() * 1000.0,
                output_width: sw,
                output_height: sh,
                internal_width: rw,
                internal_height: rh,
                render_scale: scale,
                upscaler: self.active_upscaler,
                frame_generation_multiplier: 1,
                ..RenderPerformanceMetrics::default()
            };
            return Ok(());
        }

        // Fallback: offscreen path
        let (tw, th, ow, oh, target_handle) = {
            let target = self
                .targets
                .get(&self.default_target.handle)
                .ok_or_else(|| EngineError::invalid_handle("default wgpu target is missing"))?;
            (
                target._desc.internal_width,
                target._desc.internal_height,
                target._desc.width,
                target._desc.height,
                self.default_target.handle,
            )
        };

        let frame_res =
            self.encode_frame_passes(&batches, &csm, tw, th, ow, oh, false, "aster offscreen");

        let target = self
            .targets
            .get(&target_handle)
            .ok_or_else(|| EngineError::invalid_handle("default wgpu target is missing"))?;
        let output_view = &target.color_view;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster render world encoder"),
            });
        self.encode_csm_shadow_passes(&mut encoder, &csm, &batches);
        self.encode_hdr_forward_passes(&mut encoder, &batches);
        self.encode_ssao_pass(&mut encoder, &frame_res);
        let _bloom_view = self.encode_bloom_pass(&mut encoder, &frame_res);
        self.encode_post_pass(&mut encoder, &frame_res, output_view);

        self.queue.submit(std::iter::once(encoder.finish()));
        self.submitted_worlds = self.submitted_worlds.saturating_add(1);
        self.performance_metrics = RenderPerformanceMetrics {
            render_cpu_ms: render_started.elapsed().as_secs_f32() * 1000.0,
            output_width: ow,
            output_height: oh,
            internal_width: tw,
            internal_height: th,
            render_scale: self.dynamic_resolution.scale(),
            upscaler: self.active_upscaler,
            frame_generation_multiplier: 1,
            ..RenderPerformanceMetrics::default()
        };
        Ok(())
    }

    fn submit_render_world_to_target(
        &mut self,
        world: &RenderWorld,
        target: &RenderTarget,
        _frame: RenderFrame,
    ) -> EngineResult<()> {
        let (width, height) = target.size();
        self.render_world_to_target(
            world,
            target.handle,
            width as f32 / height.max(1) as f32,
            "aster explicit target render world encoder",
            "explicit wgpu target is missing",
        )
    }

    fn set_render_scale(&mut self, scale: f32) {
        self.performance_config.render_scale = scale;
        self.dynamic_resolution = engine_render::DynamicResolutionController::new(
            self.performance_config.dynamic_resolution,
            scale,
        );
        self.active_upscaler = if self.dynamic_resolution.scale() < 1.0 {
            engine_render::UpscalerKind::BuiltInSpatial
        } else {
            engine_render::UpscalerKind::Native
        };
        self.performance_metrics.render_scale = self.dynamic_resolution.scale();
        self.performance_metrics.upscaler = self.active_upscaler;
        self.update_offscreen_internal_sizes(self.dynamic_resolution.scale());
    }

    fn configure_render_scaling(
        &mut self,
        settings: &engine_render::RenderScalingSettings,
        context: engine_render::RenderScalingContext,
    ) -> engine_render::RenderScalingSelection {
        let settings = settings.clone().normalized();
        let selection = engine_render::negotiate_render_scaling(
            &settings,
            &self.render_scaling_capabilities(),
            context,
        );
        self.performance_config.dynamic_resolution.enabled = settings.dynamic_resolution
            && selection.upscaler != engine_render::UpscalerKind::Native;
        self.performance_config.dynamic_resolution.target_fps = settings.target_fps;
        self.performance_config.dynamic_resolution.min_scale = settings.min_render_scale;
        self.performance_config.dynamic_resolution.max_scale = settings.max_render_scale;
        self.performance_config.render_scale = selection.render_scale;
        self.dynamic_resolution = engine_render::DynamicResolutionController::new(
            self.performance_config.dynamic_resolution,
            selection.render_scale,
        );
        self.active_upscaler = selection.upscaler;
        self.upscale_sharpness = settings.sharpness;
        self.performance_metrics.render_scale = self.dynamic_resolution.scale();
        self.performance_metrics.upscaler = self.active_upscaler;
        self.update_offscreen_internal_sizes(self.dynamic_resolution.scale());
        selection
    }

    fn performance_metrics(&self) -> RenderPerformanceMetrics {
        self.performance_metrics
    }

    fn record_frame_time(&mut self, frame_ms: f32) {
        if let Some(scale) = self.dynamic_resolution.record_frame(frame_ms) {
            self.performance_metrics.render_scale = scale;
            self.update_offscreen_internal_sizes(scale);
        }
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
                view: view,
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
                view: view,
                _desc: desc,
            },
        );
        Ok(ImageHandle::new(handle))
    }

    fn destroy_image(&mut self, handle: ImageHandle) {
        if let Some(image) = self.images.remove(&handle.raw()) {
            let _ = self.image_allocator.free(handle.raw());
            let frame = self.submitted_worlds;
            self.destroy_queue
                .push((frame, DestroyResource::Texture(image._texture)));
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
            self.destroy_queue
                .push((frame, DestroyResource::Buffer(buffer)));
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
                view: view,
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
                tangent: [1.0, 0.0, 0.0, 1.0],
            })
            .collect();
        let vertices = with_generated_tangents(vertices, indices);
        self.upload_mesh(mesh_name, &vertices, indices);
        Ok(())
    }

    fn flush_destroy_queue(&mut self, frame_index: u64) {
        // Poll the device to process completed GPU work callbacks before
        // deciding which resources are safe to drop.
        self.device.poll(wgpu::PollType::Poll).ok();
        // Retain resources submitted at or after (frame_index - max_in_flight).
        // max_in_flight = desired_maximum_frame_latency (2) + 1 = 3 frames.
        let threshold = frame_index.saturating_sub(3);
        self.destroy_queue
            .retain(|(idx, _resource)| *idx > threshold);
    }

    fn register_material_params(
        &mut self,
        name: &str,
        base_color: [f32; 4],
        metallic: f32,
        roughness: f32,
        emissive: [f32; 3],
    ) {
        WgpuRenderDevice::register_material_params(
            self, name, base_color, metallic, roughness, emissive,
        );
    }

    fn register_material_textures(&mut self, name: &str, textures: &RenderMaterialTextures) {
        let view_for = |handle: &Option<ImageHandle>| -> &wgpu::TextureView {
            match handle {
                Some(h) => match self.images.get(&h.raw()) {
                    Some(img) => &img.view,
                    None => &self.default_texture_view,
                },
                None => &self.default_texture_view,
            }
        };
        let base_color_view = view_for(&textures.base_color);
        let normal_view = if textures.normal.is_some() {
            view_for(&textures.normal)
        } else {
            &self.default_normal_texture_view
        };
        let mra_view = if textures.metallic_roughness.is_some() {
            view_for(&textures.metallic_roughness)
        } else {
            &self.default_mra_texture_view
        };
        let emissive_view = view_for(&textures.emissive);
        let occlusion_view = view_for(&textures.occlusion);

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("aster material bind group: {name}")),
            layout: &self.material_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(base_color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(mra_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(emissive_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(occlusion_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self._default_sampler),
                },
            ],
        });
        self.material_gpu
            .insert(name.to_owned(), MaterialGpuData { bind_group });
    }
}
