use std::time::Instant;

use crate::{
    device::*,
    format::*,
    meshes::{SkinnedMeshBuffers, with_generated_tangents},
    render::*,
    scene_uniforms::*,
    uniforms::*,
};
use engine_core::{EngineError, EngineResult};
use engine_render::{
    BufferDesc, BufferHandle, GuiDrawList, GuiTextureId, ImageDesc, ImageHandle, RenderApi,
    RenderDevice, RenderFrame, RenderGraph, RenderMaterialTextures, RenderPerformanceMetrics,
    RenderTarget, RenderTargetDesc, RenderWorld,
};
use wgpu::util::DeviceExt;

impl RenderDevice for WgpuRenderDevice {
    fn api(&self) -> RenderApi {
        RenderApi::WebGpu
    }

    fn render(&mut self, frame: RenderFrame) -> EngineResult<()> {
        self.submit_render_world(&RenderWorld::default(), frame)
    }

    fn submit_render_world(&mut self, world: &RenderWorld, frame: RenderFrame) -> EngineResult<()> {
        self.collect_gpu_frame_time();
        let render_started = Instant::now();
        let previous_gpu_frame_ms = self.performance_metrics.gpu_frame_ms;
        let aspect = self
            .surface_config
            .as_ref()
            .map(|cfg| cfg.width as f32 / cfg.height.max(1) as f32)
            .unwrap_or(16.0 / 9.0);
        let batches = self.prepare_render_batches(world, aspect);
        self.prepare_gpu_particles(world);
        let plan = self.active_frame_plan.clone().unwrap_or_default();
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
            if plan.temporal_inputs {
                let (temporal_camera, reset_history) =
                    temporal_camera_from_world(world, aspect, (rw, rh), &mut self.temporal_state);
                self.latest_temporal_camera = temporal_camera;
                self.reset_temporal_history = reset_history || frame.frame_index == 0;
            }

            let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let enable_ssao = self.ssao_compute_pipeline.is_some();
            let enable_ssgi = self.ssgi_compute_pipeline.is_some();
            let enable_bloom = self.bloom_compute_down.is_some() && self.bloom_compute_up.is_some();
            let frame_res = self.encode_frame_passes(
                &batches,
                &csm,
                rw,
                rh,
                sw,
                sh,
                enable_ssao,
                enable_ssgi,
                enable_bloom,
                "aster surface",
            );
            self.update_render_submission_stats(
                &batches,
                &plan,
                enable_ssao,
                enable_ssgi,
                enable_bloom,
            );

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("aster surface render world encoder"),
                });
            let gpu_timestamps = self.encode_gpu_timestamps_begin(&mut encoder);
            self.encode_frame_pipeline(
                &mut encoder,
                &plan,
                &csm,
                &batches,
                &frame_res,
                &output_view,
                enable_ssao,
                enable_ssgi,
                enable_bloom,
            );
            self.encode_gpu_timestamps_end(&mut encoder, gpu_timestamps);

            self.queue.submit(std::iter::once(encoder.finish()));
            self.schedule_gpu_timestamp_readback(gpu_timestamps);
            self.finish_validation_scope(validation, "aster surface")?;
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
                draw_calls: self.latest_draw_calls,
                triangles: self.latest_triangles,
                submitted_objects: self.latest_submitted_objects,
                visible_objects: self.latest_visible_objects,
                culled_objects: self.latest_culled_objects,
                pipeline_passes: plan.pass_count,
                gpu_frame_ms: previous_gpu_frame_ms,
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

        let enable_ssao = false;
        let enable_ssgi = self.ssgi_compute_pipeline.is_some();
        let enable_bloom = self.bloom_compute_down.is_some() && self.bloom_compute_up.is_some();
        let frame_res = self.encode_frame_passes(
            &batches,
            &csm,
            tw,
            th,
            ow,
            oh,
            enable_ssao,
            enable_ssgi,
            enable_bloom,
            "aster offscreen",
        );
        self.update_render_submission_stats(
            &batches,
            &plan,
            enable_ssao,
            enable_ssgi,
            enable_bloom,
        );
        if plan.temporal_inputs {
            let (temporal_camera, reset_history) =
                temporal_camera_from_world(world, aspect, (tw, th), &mut self.temporal_state);
            self.latest_temporal_camera = temporal_camera;
            self.reset_temporal_history = reset_history || frame.frame_index == 0;
        }

        let target = self
            .targets
            .get(&target_handle)
            .ok_or_else(|| EngineError::invalid_handle("default wgpu target is missing"))?;
        let output_view = &target.color_view;

        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster render world encoder"),
            });
        let gpu_timestamps = self.encode_gpu_timestamps_begin(&mut encoder);
        self.encode_frame_pipeline(
            &mut encoder,
            &plan,
            &csm,
            &batches,
            &frame_res,
            output_view,
            enable_ssao,
            enable_ssgi,
            enable_bloom,
        );
        self.encode_gpu_timestamps_end(&mut encoder, gpu_timestamps);

        self.queue.submit(std::iter::once(encoder.finish()));
        self.schedule_gpu_timestamp_readback(gpu_timestamps);
        self.finish_validation_scope(validation, "aster offscreen")?;
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
            draw_calls: self.latest_draw_calls,
            triangles: self.latest_triangles,
            submitted_objects: self.latest_submitted_objects,
            visible_objects: self.latest_visible_objects,
            culled_objects: self.latest_culled_objects,
            pipeline_passes: plan.pass_count,
            gpu_frame_ms: previous_gpu_frame_ms,
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
        let capabilities = if context.platform.is_mobile() {
            engine_render::RenderScalingCapabilities::mobile_prototype(context)
        } else {
            self.render_scaling_capabilities()
        };
        let selection = engine_render::negotiate_render_scaling(&settings, &capabilities, context);
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

    fn submit_render_world_with_graph(
        &mut self,
        world: &RenderWorld,
        graph: &RenderGraph,
        frame: RenderFrame,
    ) -> EngineResult<()> {
        self.execute_graph(graph, frame)?;
        self.active_frame_plan = Some(FramePipelinePlan::from_graph(graph));
        let result = self.submit_render_world(world, frame);
        self.active_frame_plan = None;
        result
    }

    fn record_frame_time(&mut self, frame_ms: f32) {
        let feedback_ms = self.performance_metrics.gpu_frame_ms.unwrap_or(frame_ms);
        if let Some(scale) = self.dynamic_resolution.record_frame(feedback_ms) {
            self.performance_metrics.render_scale = scale;
            self.update_offscreen_internal_sizes(scale);
        }
    }

    fn execute_graph(&mut self, graph: &RenderGraph, _frame: RenderFrame) -> EngineResult<()> {
        for pass in &graph.passes {
            let supported = matches!(
                pass.name.as_str(),
                "shadow"
                    | "forward"
                    | "temporal-inputs"
                    | "upscale"
                    | "post"
                    | "ui"
                    | "gui"
                    | "outline"
            );
            if !supported {
                return Err(EngineError::other(format!(
                    "wgpu Frame Pipeline contains unsupported pass: {}",
                    pass.name
                )));
            }
        }
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
        if let Some(gpu_target) = self.targets.remove(&target.handle) {
            let frame = self.submitted_worlds;
            self.destroy_queue
                .push((frame, DestroyResource::Target(gpu_target)));
        }
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

    fn upload_texture(&mut self, mut desc: ImageDesc, data: &[u8]) -> EngineResult<ImageHandle> {
        desc.usage = desc.usage.or(engine_render::ImageUsage::TRANSFER_DST);
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
            self.bone_palette_counts.remove(&handle.raw());
            let _ = self.buffer_allocator.free(handle.raw());
            let frame = self.submitted_worlds;
            self.destroy_queue
                .push((frame, DestroyResource::Buffer(buffer)));
        }
    }

    fn upload_gui_texture(
        &mut self,
        mut desc: ImageDesc,
        data: &[u8],
    ) -> EngineResult<GuiTextureId> {
        desc.usage = desc.usage.or(engine_render::ImageUsage::TRANSFER_DST);
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
        self.gui_textures.insert(id.0, handle);
        Ok(id)
    }

    fn draw_gui(&mut self, draw_list: &GuiDrawList) -> EngineResult<()> {
        if draw_list.vertices.is_empty()
            || draw_list.indices.is_empty()
            || draw_list.commands.is_empty()
        {
            return Ok(());
        }
        if draw_list.vertices.len() > self.gui_vertex_capacity {
            self.gui_vertex_capacity = draw_list.vertices.len().next_power_of_two();
            self.gui_vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aster gui vertices"),
                size: (self.gui_vertex_capacity * std::mem::size_of::<GpuGuiVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if draw_list.indices.len() > self.gui_index_capacity {
            self.gui_index_capacity = draw_list.indices.len().next_power_of_two();
            self.gui_index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aster gui indices"),
                size: (self.gui_index_capacity * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let vertices: Vec<GpuGuiVertex> = draw_list
            .vertices
            .iter()
            .map(|vertex| GpuGuiVertex {
                position: vertex.pos,
                uv: vertex.uv,
                color: vertex.color,
            })
            .collect();
        self.queue
            .write_buffer(&self.gui_vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        self.queue.write_buffer(
            &self.gui_index_buffer,
            0,
            bytemuck::cast_slice(&draw_list.indices),
        );

        let (width, height) = self.default_target.size();
        self.queue.write_buffer(
            &self.gui_uniform,
            0,
            bytemuck::bytes_of(&GuiUniform {
                screen_size: [width as f32, height as f32],
                _pad: [0.0; 2],
            }),
        );
        let target = self
            .targets
            .get(&self.default_target.handle)
            .ok_or_else(|| EngineError::invalid_handle("default GUI target is missing"))?;
        let mut bind_groups = Vec::with_capacity(draw_list.commands.len());
        for command in &draw_list.commands {
            let view = self
                .gui_textures
                .get(&command.texture.0)
                .and_then(|handle| self.images.get(handle))
                .map_or(&self.default_texture_view, |image| &image.view);
            bind_groups.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aster gui draw bind group"),
                layout: &self.gui_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.gui_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.gui_sampler),
                    },
                ],
            }));
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster gui encoder"),
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aster gui pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.gui_pipeline);
        pass.set_vertex_buffer(0, self.gui_vertex_buffer.slice(..));
        pass.set_index_buffer(self.gui_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for (command, bind_group) in draw_list.commands.iter().zip(&bind_groups) {
            let [x, y, command_width, command_height] = command.scissor;
            let x = x.min(width);
            let y = y.min(height);
            let scissor_width = command_width.min(width.saturating_sub(x));
            let scissor_height = command_height.min(height.saturating_sub(y));
            let end = command.index_offset.saturating_add(command.index_count);
            if scissor_width == 0 || scissor_height == 0 || end as usize > draw_list.indices.len() {
                continue;
            }
            pass.set_scissor_rect(x, y, scissor_width, scissor_height);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw_indexed(command.index_offset..end, 0, 0..1);
        }
        drop(pass);
        self.queue.submit(Some(encoder.finish()));
        self.latest_draw_calls = self
            .latest_draw_calls
            .saturating_add(draw_list.commands.len() as u32);
        self.latest_triangles = self
            .latest_triangles
            .saturating_add(draw_list.indices.len() as u64 / 3);
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

    fn upload_skinned_mesh_data(
        &mut self,
        mesh_name: &str,
        positions: &[[f32; 3]],
        normals: &[[f32; 3]],
        texcoords: &[[f32; 2]],
        joint_indices: &[[u16; 4]],
        joint_weights: &[[f32; 4]],
        indices: &[u32],
    ) -> EngineResult<()> {
        let vertex_count = positions
            .len()
            .min(normals.len())
            .min(texcoords.len())
            .min(joint_indices.len())
            .min(joint_weights.len());
        if vertex_count == 0 || indices.is_empty() {
            return Err(EngineError::other("skinned mesh data must not be empty"));
        }
        if indices.iter().any(|index| *index as usize >= vertex_count) {
            return Err(EngineError::other(
                "skinned mesh index references a missing vertex",
            ));
        }
        let vertices: Vec<SkinnedVertex> = (0..vertex_count)
            .map(|index| {
                let mut weights = joint_weights[index];
                let total = weights.iter().copied().sum::<f32>();
                if total > 0.0 && total.is_finite() {
                    for weight in &mut weights {
                        *weight /= total;
                    }
                } else {
                    weights = [1.0, 0.0, 0.0, 0.0];
                }
                SkinnedVertex {
                    position: positions[index],
                    normal: normals[index],
                    uv: texcoords[index],
                    joints: joint_indices[index].map(u32::from),
                    weights,
                }
            })
            .collect();
        let max_joint_index = vertices
            .iter()
            .flat_map(|vertex| vertex.joints)
            .max()
            .unwrap_or(0);
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aster skinned mesh vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aster skinned mesh indices"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.skinned_mesh_cache.insert(
            mesh_name.to_owned(),
            SkinnedMeshBuffers {
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                max_joint_index,
            },
        );
        Ok(())
    }

    fn upload_bone_matrices(&mut self, matrices: &[[f32; 16]]) -> EngineResult<BufferHandle> {
        if matrices.is_empty() {
            return Err(EngineError::other("bone palette must not be empty"));
        }
        let handle = self.buffer_allocator.allocate()?;
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aster bone palette"),
                contents: bytemuck::cast_slice(matrices),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            });
        self.buffers.insert(handle, buffer);
        self.bone_palette_counts
            .insert(handle, matrices.len().min(u32::MAX as usize) as u32);
        Ok(BufferHandle::new(handle))
    }

    fn draw_skinned_mesh(
        &mut self,
        mesh_name: &str,
        _material_name: &str,
        bone_buffer: BufferHandle,
        bone_count: u32,
    ) -> EngineResult<()> {
        if bone_count == 0 {
            return Err(EngineError::other("bone count must be non-zero"));
        }
        let mesh = self.skinned_mesh_cache.get(mesh_name).ok_or_else(|| {
            EngineError::other(format!("skinned mesh is not uploaded: {mesh_name}"))
        })?;
        if mesh.max_joint_index >= bone_count {
            return Err(EngineError::other(format!(
                "skinned mesh references joint {} but palette contains {bone_count} bones",
                mesh.max_joint_index
            )));
        }
        let bones = self
            .buffers
            .get(&bone_buffer.raw())
            .ok_or_else(|| EngineError::invalid_handle("bone palette buffer is missing"))?;
        let actual_bone_count = self
            .bone_palette_counts
            .get(&bone_buffer.raw())
            .copied()
            .ok_or_else(|| EngineError::invalid_handle("bone palette metadata is missing"))?;
        if bone_count > actual_bone_count {
            return Err(EngineError::other(format!(
                "bone count {bone_count} exceeds uploaded palette size {actual_bone_count}"
            )));
        }
        let bone_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster bone palette bind group"),
            layout: &self.skinned_bone_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: bones.as_entire_binding(),
            }],
        });
        let target = self
            .targets
            .get(&self.default_target.handle)
            .ok_or_else(|| EngineError::invalid_handle("default skinned target is missing"))?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster skinned mesh encoder"),
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aster skinned mesh pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.skinned_pipeline);
        pass.set_bind_group(0, &self.skinned_camera_bind_group, &[]);
        pass.set_bind_group(1, &bone_bind_group, &[]);
        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        drop(pass);
        self.queue.submit(Some(encoder.finish()));
        self.latest_draw_calls = self.latest_draw_calls.saturating_add(1);
        self.latest_triangles = self
            .latest_triangles
            .saturating_add(u64::from(mesh.index_count / 3));
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
