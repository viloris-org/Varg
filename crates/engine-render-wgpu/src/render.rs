use crate::{
    batches::{
        RenderBatchDraw, active_csm_cascade_count, csm_cascade_depth_range,
        shadow_ranges_for_instances,
    },
    device::*,
    diagnostics,
    format::*,
    light_preparation::{
        csm_uniform_from_world, gi_probe_uniform_and_data, lighting_uniform_from_world,
        select_forward_lights,
    },
    math::{IDENTITY_MAT4, inverse_mat4},
    passes::*,
    scene_uniforms::*,
    uniforms::*,
};
use engine_core::{EngineError, EngineResult, Handle};
use engine_render::{RenderTargetDesc, RenderWorld};
use std::time::Instant;

impl WgpuRenderDevice {
    pub(crate) fn screen_space_gi_enabled(&self, world: &RenderWorld) -> bool {
        self.ssgi_compute_pipeline.is_some()
            && matches!(
                world.global_illumination,
                engine_render::RenderGlobalIllumination::ScreenSpace { .. }
            )
    }

    pub(crate) fn screen_space_gi_intensity(world: &RenderWorld) -> f32 {
        match world.global_illumination {
            engine_render::RenderGlobalIllumination::ScreenSpace { intensity } => {
                intensity.max(0.0)
            }
            engine_render::RenderGlobalIllumination::ProbeVolume(_) => SSGI_INTENSITY,
        }
    }

    pub(crate) fn prepare_skybox_environment(&mut self, world: &RenderWorld) -> bool {
        let requested = world
            .skybox
            .as_ref()
            .and_then(|skybox| skybox.cubemap.as_deref())
            .and_then(|label| self.skybox_cubemaps.get(label).copied())
            .filter(|handle| {
                self.images
                    .get(handle)
                    .and_then(|image| image.cube_view.as_ref())
                    .is_some()
            });

        if self.active_skybox_cubemap != requested {
            let cube_view = requested
                .and_then(|handle| {
                    self.images
                        .get(&handle)
                        .and_then(|image| image.cube_view.as_ref())
                })
                .unwrap_or(&self._skybox_default_cubemap_view);
            self.skybox_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("varg skybox bind group"),
                layout: &self.skybox_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.skybox_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(cube_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self._skybox_sampler),
                    },
                ],
            });
            self.active_skybox_cubemap = requested;
        }

        match requested {
            Some(handle) if self.active_ibl_cubemap != Some(handle) => {
                self.bake_ibl_for_cubemap(handle);
                self.ibl_enabled = true;
            }
            None if self.active_ibl_cubemap.is_some() || !self.ibl_enabled => {
                self.bake_ibl();
                self.ibl_enabled = true;
            }
            _ => {}
        }

        requested.is_some()
    }

    pub(crate) fn upload_lighting_uniform(&mut self, world: &RenderWorld) {
        let disable_csm_shadows = diagnostics::disable_csm_shadows();
        let mut lighting = lighting_uniform_from_world(world);
        if disable_csm_shadows {
            for light in lighting.lights.iter_mut().take(lighting.params[0] as usize) {
                light.spot_angles[2] = 0.0;
            }
        }
        self.latest_submitted_lights = world.lights.len() as u32;
        self.latest_visible_lights = lighting.params[0];
        self.latest_culled_lights = self
            .latest_submitted_lights
            .saturating_sub(self.latest_visible_lights);
        let selected = select_forward_lights(world);
        self.latest_shadowed_lights = selected
            .iter()
            .filter(|light| !disable_csm_shadows && light.settings.casts_shadow)
            .count() as u32;
        self.latest_directional_shadow_cascades = selected
            .iter()
            .find(|light| {
                !disable_csm_shadows
                    && light.kind == engine_render::RenderLightKind::Directional
                    && light.settings.casts_shadow
            })
            .map(|light| match light.settings.directional_shadow_mode {
                engine_render::RenderDirectionalShadowMode::Orthogonal => 1,
                engine_render::RenderDirectionalShadowMode::Parallel2Splits => 2,
                engine_render::RenderDirectionalShadowMode::Parallel4Splits => 4,
            })
            .unwrap_or(0);
        self.queue
            .write_buffer(&self.lighting_uniform, 0, bytemuck::bytes_of(&lighting));
    }

    pub(crate) fn upload_gi_probes(&mut self, world: &RenderWorld) {
        let (uniform, probes) = gi_probe_uniform_and_data(world);
        self.queue
            .write_buffer(&self.gi_probe_uniform, 0, bytemuck::bytes_of(&uniform));
        if !probes.is_empty() {
            self.queue
                .write_buffer(&self.gi_probe_buffer, 0, bytemuck::cast_slice(&probes));
        }
    }

    pub(crate) fn lighting_mode_metrics(
        &self,
        world: &RenderWorld,
        plan: &FramePipelinePlan,
    ) -> (bool, u32, u32) {
        let hybrid_deferred = plan.gbuffer
            || world.lighting_mode == engine_render::RenderLightingMode::HybridDeferred;
        let active_gi_probes = match &world.global_illumination {
            engine_render::RenderGlobalIllumination::ProbeVolume(volume) => {
                volume.counts.iter().product()
            }
            engine_render::RenderGlobalIllumination::ScreenSpace { .. } => 0,
        };
        let virtual_shadow_pages = match world.shadow_virtualization {
            engine_render::RenderShadowVirtualization::VirtualPages { max_pages, .. } => max_pages,
            engine_render::RenderShadowVirtualization::Cascaded => 0,
        };
        (hybrid_deferred, active_gi_probes, virtual_shadow_pages)
    }

    pub(crate) fn upload_temporal_uniform(&self) {
        self.queue.write_buffer(
            &self.temporal_uniform,
            0,
            bytemuck::bytes_of(&TemporalUniform {
                previous_view_projection: mat4_from_flat(
                    self.latest_temporal_camera.previous_view_projection,
                ),
                current_view_projection: mat4_from_flat(
                    self.latest_temporal_camera.view_projection,
                ),
                jitter_reset: [
                    self.latest_temporal_camera.jitter[0],
                    self.latest_temporal_camera.jitter[1],
                    if self.reset_temporal_history {
                        1.0
                    } else {
                        0.0
                    },
                    0.0,
                ],
            }),
        );
    }

    pub(crate) fn collect_gpu_frame_time(&mut self) {
        let Some(receiver) = self.gpu_timestamp_receiver.as_ref() else {
            return;
        };
        self.device.poll(wgpu::PollType::Poll).ok();
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.gpu_timestamp_receiver = None;
        if result.is_err() {
            return;
        }
        let Some(readback) = self.gpu_timestamp_readback.as_ref() else {
            return;
        };
        let mapped = readback.slice(..).get_mapped_range();
        if mapped.len() >= 16 {
            let values = bytemuck::cast_slice::<u8, u64>(&mapped);
            let ticks = values[1].wrapping_sub(values[0]);
            let nanoseconds = ticks as f64 * f64::from(self.queue.get_timestamp_period());
            if nanoseconds.is_finite() && nanoseconds >= 0.0 {
                self.performance_metrics.gpu_frame_ms = Some((nanoseconds / 1_000_000.0) as f32);
            }
        }
        drop(mapped);
        readback.unmap();
    }

    pub(crate) fn encode_gpu_timestamps_begin(&self, encoder: &mut wgpu::CommandEncoder) -> bool {
        let Some(query) = self.gpu_timestamp_query.as_ref() else {
            return false;
        };
        if self.gpu_timestamp_receiver.is_some() {
            return false;
        }
        encoder.write_timestamp(query, 0);
        true
    }

    pub(crate) fn encode_gpu_timestamps_end(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        active: bool,
    ) {
        if !active {
            return;
        }
        let (Some(query), Some(resolve), Some(readback)) = (
            self.gpu_timestamp_query.as_ref(),
            self.gpu_timestamp_resolve.as_ref(),
            self.gpu_timestamp_readback.as_ref(),
        ) else {
            return;
        };
        encoder.write_timestamp(query, 1);
        encoder.resolve_query_set(query, 0..2, resolve, 0);
        encoder.copy_buffer_to_buffer(resolve, 0, readback, 0, 16);
    }

    pub(crate) fn schedule_gpu_timestamp_readback(&mut self, active: bool) {
        if !active {
            return;
        }
        let Some(readback) = self.gpu_timestamp_readback.as_ref() else {
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.gpu_timestamp_receiver = Some(receiver);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_frame_pipeline(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        plan: &FramePipelinePlan,
        csm: &CsmUniform,
        batches: &[RenderBatchDraw],
        resources: &FrameResources,
        output_view: &wgpu::TextureView,
        output_viewport: Option<SurfaceViewportRect>,
        ssao_enabled: bool,
        ssgi_enabled: bool,
        bloom_enabled: bool,
    ) {
        let mut composite_encoded = false;
        for step in &plan.steps {
            match step {
                FramePipelineStep::Shadow => {
                    self.encode_csm_shadow_passes(encoder, csm, batches);
                }
                FramePipelineStep::GBuffer => {
                    self.encode_gpu_particle_compute(encoder);
                    self.encode_hdr_forward_passes(encoder, batches);
                }
                FramePipelineStep::DeferredLighting if !composite_encoded => {
                    if ssao_enabled {
                        self.encode_ssao_pass(encoder, resources);
                    }
                    if ssgi_enabled {
                        self.encode_ssgi_pass(encoder, resources);
                    }
                    if bloom_enabled {
                        let _ = self.encode_bloom_pass(encoder, resources);
                    }
                    self.encode_post_pass(encoder, resources, output_view, output_viewport);
                    composite_encoded = true;
                }
                FramePipelineStep::DeferredLighting => {}
                FramePipelineStep::Forward => {
                    if plan.gbuffer {
                        continue;
                    }
                    self.encode_gpu_particle_compute(encoder);
                    self.encode_hdr_forward_passes(encoder, batches);
                }
                FramePipelineStep::TemporalInputs | FramePipelineStep::Ui => {}
                FramePipelineStep::Outline => {
                    tracing::debug!(
                        target: "engine",
                        "outline Frame Pipeline pass has no WGPU implementation"
                    );
                }
                FramePipelineStep::Upscale | FramePipelineStep::Post if !composite_encoded => {
                    if ssao_enabled {
                        self.encode_ssao_pass(encoder, resources);
                    }
                    if ssgi_enabled {
                        self.encode_ssgi_pass(encoder, resources);
                    }
                    if bloom_enabled {
                        let _ = self.encode_bloom_pass(encoder, resources);
                    }
                    self.encode_post_pass(encoder, resources, output_view, output_viewport);
                    composite_encoded = true;
                }
                FramePipelineStep::Upscale | FramePipelineStep::Post => {}
            }
        }
        if plan.forward && !composite_encoded {
            if ssao_enabled {
                self.encode_ssao_pass(encoder, resources);
            }
            if ssgi_enabled {
                self.encode_ssgi_pass(encoder, resources);
            }
            if bloom_enabled {
                let _ = self.encode_bloom_pass(encoder, resources);
            }
            self.encode_post_pass(encoder, resources, output_view, output_viewport);
        }
    }

    pub(crate) fn update_render_submission_stats(
        &mut self,
        batches: &[RenderBatchDraw],
        plan: &FramePipelinePlan,
        ssao_enabled: bool,
        ssgi_enabled: bool,
        bloom_enabled: bool,
    ) {
        let batch_triangles = |mesh_name: &str, count: u32| {
            let index_count = self
                .mesh_cache
                .get(mesh_name)
                .map_or(CUBE_INDEX_COUNT, |mesh| mesh.index_count);
            u64::from(index_count / 3) * u64::from(count)
        };
        let forward_triangles: u64 = batches
            .iter()
            .map(|batch| batch_triangles(&batch.mesh_name, batch.count))
            .sum();
        let shadow_triangles: u64 = batches
            .iter()
            .filter(|batch| batch.count > 0 && batch.casts_shadows)
            .map(|batch| {
                let shadow_instances = batch
                    .shadow_ranges
                    .iter()
                    .flat_map(|ranges| ranges.iter())
                    .map(|range| range.end.saturating_sub(range.start))
                    .sum();
                batch_triangles(&batch.mesh_name, shadow_instances)
            })
            .sum();

        let forward_draws = batches.iter().filter(|batch| batch.count > 0).count() as u32;
        let shadow_draws = batches
            .iter()
            .filter(|batch| batch.count > 0 && batch.casts_shadows)
            .flat_map(|batch| batch.shadow_ranges.iter())
            .map(|ranges| ranges.len() as u32)
            .sum::<u32>();
        self.latest_shadow_caster_batches = batches
            .iter()
            .filter(|batch| {
                batch.count > 0
                    && batch.casts_shadows
                    && batch.shadow_ranges.iter().any(|ranges| !ranges.is_empty())
            })
            .count() as u32;
        let bloom_draws = if bloom_enabled {
            self.bloom_mip_views
                .len()
                .saturating_mul(2)
                .saturating_sub(1) as u32
        } else {
            0
        };

        let geometry_passes = u32::from(plan.forward) + u32::from(plan.gbuffer);
        self.latest_draw_calls = geometry_passes * (forward_draws + 2)
            + u32::from(plan.shadow) * shadow_draws
            + u32::from(plan.deferred_lighting)
            + u32::from(ssao_enabled)
            + u32::from(ssgi_enabled)
            + bloom_draws
            + u32::from(plan.post || plan.upscale);
        self.latest_triangles = u64::from(geometry_passes) * (forward_triangles + 1)
            + u64::from(plan.shadow) * shadow_triangles
            + u64::from(plan.post || plan.upscale);
        if geometry_passes > 0 && self.gpu_particles.instance_count() > 0 {
            self.latest_draw_calls = self.latest_draw_calls.saturating_add(1);
            self.latest_triangles = self
                .latest_triangles
                .saturating_add(u64::from(self.gpu_particles.instance_count()) * 2);
        }
    }

    pub(crate) fn finish_validation_scope(
        &self,
        scope: wgpu::ErrorScopeGuard,
        context: &str,
    ) -> EngineResult<()> {
        finish_validation_scope(&self.device, scope, context)
    }

    pub(crate) fn prepare_render_batches(
        &mut self,
        world: &RenderWorld,
        aspect: f32,
        csm: &CsmUniform,
    ) -> Vec<RenderBatchDraw> {
        let (batches, visibility) = self.mesh_batches_from_world_visible(world, aspect);
        self.latest_submitted_objects = world.objects.len() as u32;
        self.latest_visible_objects = visibility.visible_indices.len() as u32;
        self.latest_culled_objects = visibility.culled_objects as u32;
        let total_instances: usize = batches.iter().map(|(_, inst, _, _)| inst.len()).sum();
        if total_instances > self.instance_capacity {
            self.instance_capacity = total_instances.next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        let mut all_instances = Vec::with_capacity(total_instances);
        for (_, instances, _, _) in &batches {
            all_instances.extend(instances.iter().map(|instance| instance.instance));
        }
        if !all_instances.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&all_instances),
            );
        }
        let active_cascades = active_csm_cascade_count(csm);
        let mut instance_start = 0u32;
        batches
            .into_iter()
            .map(|(name, instances, mat, casts_shadows)| {
                let count = instances.len() as u32;
                let mut shadow_ranges = std::array::from_fn(|_| Vec::new());
                if casts_shadows {
                    for (cascade_idx, ranges) in
                        shadow_ranges.iter_mut().enumerate().take(active_cascades)
                    {
                        let (cascade_near, cascade_far) = csm_cascade_depth_range(csm, cascade_idx);
                        *ranges = shadow_ranges_for_instances(
                            &instances,
                            instance_start,
                            &csm.cascade_vps[cascade_idx],
                            cascade_near,
                            cascade_far,
                        );
                    }
                }
                let draw = RenderBatchDraw {
                    mesh_name: name,
                    count,
                    material_name: mat,
                    casts_shadows,
                    instance_start,
                    shadow_ranges,
                };
                instance_start = instance_start.saturating_add(count);
                draw
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
            "varg offscreen render world encoder",
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
            "varg game offscreen render world encoder",
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
            "varg preview offscreen render world encoder",
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

        // Reuse pre-allocated staging buffer if dimensions match.
        if self.readback_staging_dims != (w, h) {
            let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            self.readback_staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("varg viewport readback staging"),
                size: total_bytes,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.finish_validation_scope(validation, "viewport readback staging creation")?;
            self.readback_staging_dims = (w, h);
        }

        let target = self
            .targets
            .get(&handle)
            .ok_or_else(|| EngineError::invalid_handle("readback target missing"))?;
        let staging = self.readback_staging.as_ref().unwrap();

        let copy_validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("varg viewport readback encoder"),
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

        let buffer_slice = staging.slice(..);
        self.queue.submit(Some(encoder.finish()));
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| {
                EngineError::other(format!(
                    "viewport readback copy: wait for GPU failed: {error}"
                ))
            })?;
        self.finish_validation_scope(copy_validation, "viewport readback copy")?;

        let map_validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| {
                EngineError::other(format!(
                    "viewport readback map: wait for GPU failed: {error}"
                ))
            })?;
        self.finish_validation_scope(map_validation, "viewport readback map")?;

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
        self.collect_gpu_frame_time();
        let render_started = Instant::now();
        let previous_gpu_frame_ms = self.performance_metrics.gpu_frame_ms;
        let csm = csm_uniform_from_world(world, aspect);
        let batches = self.prepare_render_batches(world, aspect, &csm);
        self.prepare_gpu_particles(world);
        let total_instances: u32 = batches.iter().map(|batch| batch.count).sum();
        tracing::debug!(
            target: "engine",
            batch_count = batches.len(),
            total_instances,
            has_camera = world.camera.is_some(),
            skybox = world.skybox.is_some(),
            encoder_label,
            "render_world_to_target"
        );
        self.upload_lighting_uniform(world);
        self.upload_gi_probes(world);
        self.queue
            .write_buffer(&self.csm_uniform, 0, bytemuck::bytes_of(&csm));
        let use_cubemap = self.prepare_skybox_environment(world);
        let skybox = skybox_uniform_from_world(world, use_cubemap);
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
        let plan = self.active_frame_plan.clone().unwrap_or_default();
        if plan.temporal_inputs {
            let (temporal_camera, reset_history) =
                temporal_camera_from_world(world, aspect, (tw, th), &mut self.temporal_state);
            self.latest_temporal_camera = temporal_camera;
            self.reset_temporal_history = reset_history;
        }
        self.upload_temporal_uniform();
        let uniform = if plan.temporal_inputs {
            camera_uniform_with_view_projection(
                world,
                aspect,
                mat4_from_flat(self.latest_temporal_camera.view_projection),
            )
        } else {
            camera_uniform_from_world(world, aspect)
        };
        self.queue
            .write_buffer(&self.camera_uniform, 0, bytemuck::bytes_of(&uniform));
        let enable_ssao = self.ssao_compute_pipeline.is_some() && !diagnostics::disable_ssao();
        let enable_ssgi = self.screen_space_gi_enabled(world);
        let ssgi_intensity = Self::screen_space_gi_intensity(world);
        let enable_bloom = self.bloom_compute_down.is_some() && self.bloom_compute_up.is_some();

        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let frame_res = self.encode_frame_passes(
            &batches,
            &csm,
            &uniform,
            tw,
            th,
            ow,
            oh,
            enable_ssao,
            enable_ssgi,
            ssgi_intensity,
            enable_bloom,
            encoder_label,
        );
        self.update_render_submission_stats(
            &batches,
            &plan,
            enable_ssao,
            enable_ssgi,
            enable_bloom,
        );

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
        let gpu_timestamps = self.encode_gpu_timestamps_begin(&mut encoder);
        self.encode_frame_pipeline(
            &mut encoder,
            &plan,
            &csm,
            &batches,
            &frame_res,
            output_view,
            None,
            enable_ssao,
            enable_ssgi,
            enable_bloom,
        );
        self.encode_gpu_timestamps_end(&mut encoder, gpu_timestamps);

        self.queue.submit(std::iter::once(encoder.finish()));
        self.schedule_gpu_timestamp_readback(gpu_timestamps);
        self.finish_validation_scope(validation, encoder_label)?;
        self.submitted_worlds = self.submitted_worlds.saturating_add(1);
        let (hybrid_deferred, active_gi_probes, virtual_shadow_pages) =
            self.lighting_mode_metrics(world, &plan);
        self.performance_metrics = engine_render::RenderPerformanceMetrics {
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
            submitted_lights: self.latest_submitted_lights,
            visible_lights: self.latest_visible_lights,
            culled_lights: self.latest_culled_lights,
            shadowed_lights: self.latest_shadowed_lights,
            shadow_caster_batches: self.latest_shadow_caster_batches,
            directional_shadow_cascades: self.latest_directional_shadow_cascades,
            hybrid_deferred,
            active_gi_probes,
            virtual_shadow_pages,
            gpu_frame_ms: previous_gpu_frame_ms,
            ..Default::default()
        };
        tracing::trace!(target: "engine", submitted = self.submitted_worlds, "gpu submit ok");
        Ok(())
    }

    /// Ensures HDR target / bloom mips / SSAO output exist at the given size,
    /// uploads post/SSAO uniforms, and pre-allocates CSM cascade data.
    pub(crate) fn encode_frame_passes(
        &mut self,
        _batches: &[RenderBatchDraw],
        csm: &CsmUniform,
        camera: &CameraUniform,
        tw: u32,
        th: u32,
        output_width: u32,
        output_height: u32,
        ssao_enabled: bool,
        ssgi_enabled: bool,
        ssgi_intensity: f32,
        bloom_enabled: bool,
        _encoder_label: &str,
    ) -> FrameResources {
        let taa_enabled = self.anti_aliasing == engine_render::AntiAliasingMode::Taa
            && self
                .active_frame_plan
                .as_ref()
                .map(|plan| plan.temporal_inputs)
                .unwrap_or(true);
        self.queue.write_buffer(
            &self.post_uniform,
            0,
            bytemuck::bytes_of(&PostProcessUniform {
                inv_view_projection: inverse_mat4(&camera.view_projection).unwrap_or(IDENTITY_MAT4),
                view_projection: camera.view_projection,
                camera_position: camera.camera_position,
                render_width: tw as f32,
                render_height: th as f32,
                inv_render_width: 1.0 / tw.max(1) as f32,
                inv_render_height: 1.0 / th.max(1) as f32,
                output_width: output_width as f32,
                output_height: output_height as f32,
                inv_output_width: 1.0 / output_width.max(1) as f32,
                inv_output_height: 1.0 / output_height.max(1) as f32,
                exposure: 1.0,
                bloom_intensity: if bloom_enabled { 0.04 } else { 0.0 },
                ssao_enabled: if ssao_enabled { 1.0 } else { 0.0 },
                upscale_sharpness: self.upscale_sharpness,
                ssgi_enabled: if ssgi_enabled { 1.0 } else { 0.0 },
                ssgi_intensity,
                ssr_enabled: if diagnostics::disable_ssr() { 0.0 } else { 1.0 },
                ssr_intensity: 0.35,
                taa_reset: if self.reset_temporal_history {
                    1.0
                } else {
                    0.0
                },
                taa_history_weight: 0.72,
                taa_enabled: if taa_enabled { 1.0 } else { 0.0 },
                _pad: 0.0,
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
                intensity: ssgi_intensity,
                thickness: 0.055,
                sample_count: 12.0,
                frame_index: self.submitted_worlds as f32,
                history_blend: 0.86,
                reset_history: if self.reset_temporal_history {
                    1.0
                } else {
                    0.0
                },
                _pad: 0.0,
            }),
        );

        self.ensure_hdr_target(tw, th);
        if taa_enabled {
            self.ensure_taa_targets();
        }
        self.ensure_bloom_mips(tw, th);
        if ssao_enabled {
            self.ensure_ssao_output();
        }
        if ssgi_enabled {
            self.ensure_ssgi_output();
        }

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
        let ssgi_bg = if ssgi_enabled && self.ssgi_compute_pipeline.is_some() {
            Some(self.ensure_ssgi_bind_group())
        } else {
            None
        };
        let ssgi_view = self.ssgi_output_view.clone();
        let (bloom_down_bgs, bloom_up_bgs) = self.ensure_bloom_bind_groups();
        let taa_bg = if taa_enabled {
            Some(self.ensure_taa_bind_group())
        } else {
            None
        };
        let post_bg = Some(self.ensure_post_bind_group(taa_enabled));

        FrameResources {
            ssao_bg,
            ssao_view,
            ssgi_bg,
            ssgi_view,
            bloom_down_bgs,
            bloom_up_bgs,
            taa_bg,
            post_bg,
        }
    }

    pub(crate) fn encode_csm_shadow_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        _csm: &CsmUniform,
        batches: &[RenderBatchDraw],
    ) {
        if diagnostics::disable_csm_shadows() {
            return;
        }
        if !batches
            .iter()
            .any(|batch| batch.count > 0 && batch.casts_shadows)
        {
            return;
        }

        for cascade_idx in 0..CSM_CASCADE_COUNT {
            if !batches.iter().any(|batch| {
                batch.shadow_ranges[cascade_idx]
                    .iter()
                    .any(|range| !range.is_empty())
            }) {
                continue;
            }
            encode_shadow_pass(
                encoder,
                &self.csm_depth_views[cascade_idx],
                &self.shadow_pipeline,
                &self.csm_cascade_bind_groups[cascade_idx],
                &self.vertex_buffer,
                &self.index_buffer,
                &self.instance_buffer,
                batches,
                cascade_idx,
                &self.mesh_cache,
            );
        }
    }

    pub(crate) fn encode_hdr_forward_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        batches: &[RenderBatchDraw],
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
            self.hdr_motion_view.as_ref().unwrap(),
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
            false,
        );
        encode_batched_forward_pass(
            encoder,
            &hdr.color_view,
            self.hdr_normal_view.as_ref().unwrap(),
            self.hdr_albedo_view.as_ref().unwrap(),
            self.hdr_motion_view.as_ref().unwrap(),
            hdr.depth_view.as_ref(),
            &self.transparent_pipeline,
            &self.camera_bind_group,
            &self.default_material_bind_group,
            &self.material_gpu,
            &self.mesh_cache,
            &self.vertex_buffer,
            &self.index_buffer,
            &self.instance_buffer,
            batches,
            true,
        );
        self.encode_gpu_particle_render(encoder);
        if self.editor_grid_enabled && !diagnostics::disable_grid() {
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
    }

    pub(crate) fn ensure_surface_depth(&mut self) {
        let Some(config) = self.surface_config.as_ref() else {
            return;
        };
        if self.surface_depth_view.is_some() {
            return;
        }
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varg surface depth"),
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
                label: Some("varg hdr color"),
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
                label: Some("varg hdr depth"),
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
                label: Some("varg hdr normal roughness"),
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
                label: Some("varg hdr albedo metallic"),
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
            let motion = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("varg hdr motion vectors"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.hdr_motion_view =
                Some(motion.create_view(&wgpu::TextureViewDescriptor::default()));
            self.hdr_motion_texture = Some(motion);
            self.post_target_width = w;
            self.post_target_height = h;
            // Invalidate cached bind groups that reference the old HDR target.
            self.post_cached_bg = None;
            self.taa_bind_group = None;
            self.ssao_cached_bg = None;
            self.ssgi_cached_bg = None;
        }
    }
}

pub(crate) fn mat4_from_flat(matrix: [f32; 16]) -> [[f32; 4]; 4] {
    [
        [matrix[0], matrix[1], matrix[2], matrix[3]],
        [matrix[4], matrix[5], matrix[6], matrix[7]],
        [matrix[8], matrix[9], matrix[10], matrix[11]],
        [matrix[12], matrix[13], matrix[14], matrix[15]],
    ]
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
