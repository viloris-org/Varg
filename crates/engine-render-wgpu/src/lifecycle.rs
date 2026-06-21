use crate::{device::*, format::*, render::*};
use engine_core::{EngineError, EngineResult, Handle};
use engine_render::{RenderPerformanceConfig, RenderTarget, RenderTargetDesc};

impl WgpuRenderDevice {
    pub(crate) fn update_offscreen_internal_sizes(&mut self, scale: f32) {
        for target in [
            &mut self.default_target,
            &mut self.game_target,
            &mut self.preview_target,
        ] {
            let (width, height) = scaled_render_size(target.desc.width, target.desc.height, scale);
            target.desc.internal_width = width;
            target.desc.internal_height = height;
            if let Some(gpu_target) = self.targets.get_mut(&target.handle) {
                gpu_target._desc.internal_width = width;
                gpu_target._desc.internal_height = height;
            }
        }
    }

    /// Number of render worlds submitted to this backend.
    pub fn submitted_worlds(&self) -> u64 {
        self.submitted_worlds
    }

    /// Returns the most recent temporal camera metadata and history reset flag.
    pub fn latest_temporal_camera(&self) -> (engine_render::TemporalCameraData, bool) {
        (self.latest_temporal_camera, self.reset_temporal_history)
    }

    /// Returns adapter and native output capability information.
    pub fn output_capabilities(&self) -> WgpuOutputCapabilities {
        let limits = self.device.limits();
        WgpuOutputCapabilities {
            adapter_name: self.adapter.get_info().name,
            max_texture_dimension_2d: limits.max_texture_dimension_2d,
            supports_4k_render_targets: limits.max_texture_dimension_2d >= 4096,
            supports_timestamp_queries: self.device.features().contains(
                wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
            ),
            present_mode: self
                .surface_config
                .as_ref()
                .map(|config| format!("{:?}", config.present_mode)),
        }
    }

    /// Blocks until all previously submitted GPU work has completed.
    ///
    /// Intended for benchmarks, capture tools, and controlled shutdown.
    pub fn wait_idle(&self) -> EngineResult<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|error| EngineError::other(format!("wait for GPU idle failed: {error}")))
    }

    /// Applies a native runtime performance policy and reconfigures the surface.
    pub fn configure_performance(&mut self, performance: RenderPerformanceConfig) {
        self.apply_performance_config(performance);
    }

    pub(crate) fn apply_performance_config(&mut self, performance: RenderPerformanceConfig) {
        self.performance_config = performance;
        self.dynamic_resolution = engine_render::DynamicResolutionController::new(
            performance.dynamic_resolution,
            performance.render_scale,
        );
        self.performance_metrics.render_scale = self.dynamic_resolution.scale();
        self.active_upscaler = if self.dynamic_resolution.scale() < 1.0 {
            engine_render::UpscalerKind::BuiltInSpatial
        } else {
            engine_render::UpscalerKind::Native
        };
        self.performance_metrics.upscaler = self.active_upscaler;
        self.update_offscreen_internal_sizes(self.dynamic_resolution.scale());
        if let (Some(surface), Some(config)) = (self.surface.as_ref(), self.surface_config.as_mut())
        {
            let caps = surface.get_capabilities(&self.adapter);
            config.present_mode =
                select_present_mode(&caps.present_modes, performance.present_strategy);
            config.desired_maximum_frame_latency = performance.maximum_frame_latency.max(1);
            surface.configure(&self.device, config);
        }
    }

    /// Resizes the default offscreen render target (scene view).
    ///
    /// No-op when the target already matches the requested dimensions.
    pub fn resize_default_target(&mut self, width: u32, height: u32) -> EngineResult<()> {
        let w = width.max(1);
        let h = height.max(1);
        if self.default_target.desc.width == w && self.default_target.desc.height == h {
            return Ok(());
        }
        let old_handle = self.default_target.handle;
        let (internal_width, internal_height) =
            scaled_render_size(w, h, self.dynamic_resolution.scale());
        let desc = RenderTargetDesc {
            width: w,
            height: h,
            internal_width,
            internal_height,
            ui_width: w,
            ui_height: h,
            ..self.default_target.desc.clone()
        };
        self.default_target = self.create_resized_target(old_handle, desc)?;
        Ok(())
    }

    /// Resizes the game offscreen render target.
    ///
    /// No-op when the target already matches the requested dimensions.
    pub fn resize_game_target(&mut self, width: u32, height: u32) -> EngineResult<()> {
        let w = width.max(1);
        let h = height.max(1);
        if self.game_target.desc.width == w && self.game_target.desc.height == h {
            return Ok(());
        }
        let old_handle = self.game_target.handle;
        let (internal_width, internal_height) =
            scaled_render_size(w, h, self.dynamic_resolution.scale());
        let desc = RenderTargetDesc {
            width: w,
            height: h,
            internal_width,
            internal_height,
            ui_width: w,
            ui_height: h,
            ..self.game_target.desc.clone()
        };
        self.game_target = self.create_resized_target(old_handle, desc)?;
        Ok(())
    }

    /// Resizes the preview offscreen render target.
    ///
    /// No-op when the target already matches the requested dimensions.
    pub fn resize_preview_target(&mut self, width: u32, height: u32) -> EngineResult<()> {
        let w = width.max(1);
        let h = height.max(1);
        if self.preview_target.desc.width == w && self.preview_target.desc.height == h {
            return Ok(());
        }
        let old_handle = self.preview_target.handle;
        let (internal_width, internal_height) =
            scaled_render_size(w, h, self.dynamic_resolution.scale());
        let desc = RenderTargetDesc {
            width: w,
            height: h,
            internal_width,
            internal_height,
            ui_width: w,
            ui_height: h,
            ..self.preview_target.desc.clone()
        };
        self.preview_target = self.create_resized_target(old_handle, desc)?;
        Ok(())
    }

    pub(crate) fn create_resized_target(
        &mut self,
        old_handle: Handle,
        desc: RenderTargetDesc,
    ) -> EngineResult<RenderTarget> {
        let CreatedTarget(color, color_view, depth, depth_view, new_target) =
            create_target(&self.device, &mut self.target_allocator, desc)?;
        if let Some(old_target) = self.targets.remove(&old_handle) {
            let frame = self.submitted_worlds;
            self.destroy_queue
                .push((frame, DestroyResource::Target(old_target)));
        }
        self.targets.insert(
            new_target.handle,
            GpuTarget {
                _color: color,
                color_view,
                _depth: depth,
                depth_view,
                _desc: new_target.desc.clone(),
            },
        );
        Ok(new_target)
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
}
