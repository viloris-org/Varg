use std::sync::Arc;

use crate::device::*;

impl WgpuRenderDevice {
    pub(crate) fn ensure_ssao_bind_group(&mut self) -> Arc<wgpu::BindGroup> {
        if self.ssao_cached_bg.is_none() {
            let bgl = self.ssao_compute_bgl.as_ref().unwrap();
            let hdr = self.hdr_target.as_ref().unwrap();
            let depth_view = hdr.depth_view.as_ref().unwrap();
            let bg = Arc::new(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aster ssao compute bg"),
                layout: bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.ssao_noise_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.ssao_uniform,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.ssao_samples_buffer,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(
                            self.ssao_output_view.as_ref().unwrap(),
                        ),
                    },
                ],
            }));
            self.ssao_cached_bg = Some(Arc::clone(&bg));
            bg
        } else {
            Arc::clone(self.ssao_cached_bg.as_ref().unwrap())
        }
    }

    pub(crate) fn encode_ssao_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        res: &FrameResources,
    ) {
        let Some(ssao_bg) = &res.ssao_bg else { return };
        let Some(_ssao_view) = &res.ssao_view else {
            return;
        };
        let Some(pipeline) = self.ssao_compute_pipeline.as_ref() else {
            return;
        };
        let w = self.post_target_width.max(1);
        let h = self.post_target_height.max(1);
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aster ssao compute"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(pipeline);
        cpass.set_bind_group(0, &**ssao_bg, &[]);
        let wx = (w + 7) / 8;
        let wy = (h + 7) / 8;
        cpass.dispatch_workgroups(wx, wy, 1);
        drop(cpass);
    }

    pub(crate) fn ensure_ssgi_bind_group(&mut self) -> Arc<wgpu::BindGroup> {
        if self.ssgi_cached_bg.is_none() {
            let bgl = self.ssgi_compute_bgl.as_ref().unwrap();
            let hdr = self.hdr_target.as_ref().unwrap();
            let depth_view = hdr.depth_view.as_ref().unwrap();
            let bg = Arc::new(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aster ssgi compute bg"),
                layout: bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&hdr.color_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(
                            self.hdr_normal_view.as_ref().unwrap(),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(
                            self.hdr_albedo_view.as_ref().unwrap(),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(
                            self.ssgi_output_view.as_ref().unwrap(),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.ssgi_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&self.bloom_sampler),
                    },
                ],
            }));
            self.ssgi_cached_bg = Some(Arc::clone(&bg));
            bg
        } else {
            Arc::clone(self.ssgi_cached_bg.as_ref().unwrap())
        }
    }

    pub(crate) fn encode_ssgi_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        res: &FrameResources,
    ) {
        let Some(ssgi_bg) = &res.ssgi_bg else { return };
        let Some(_ssgi_view) = &res.ssgi_view else {
            return;
        };
        let Some(pipeline) = self.ssgi_compute_pipeline.as_ref() else {
            return;
        };
        let w = self.post_target_width.max(1);
        let h = self.post_target_height.max(1);
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aster ssgi compute"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(pipeline);
        cpass.set_bind_group(0, &**ssgi_bg, &[]);
        cpass.dispatch_workgroups((w + 7) / 8, (h + 7) / 8, 1);
    }

    pub(crate) fn ensure_bloom_bind_groups(
        &mut self,
    ) -> (Vec<Arc<wgpu::BindGroup>>, Vec<Arc<wgpu::BindGroup>>) {
        if self.bloom_cached_down_bgs.is_empty() {
            let bcl = self.bloom_compute_bgl.as_ref().unwrap();
            let hdr_cv = &self.hdr_target.as_ref().unwrap().color_view;
            for i in 0..self.bloom_mip_views.len() {
                let src = if i == 0 {
                    hdr_cv
                } else {
                    &self.bloom_mip_views[i - 1]
                };
                let dst = &self.bloom_mip_views[i];
                let bg = Arc::new(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("aster bloom down bg"),
                    layout: bcl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(src),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(dst),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.bloom_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: self.bloom_uniform.as_entire_binding(),
                        },
                    ],
                }));
                self.bloom_cached_down_bgs.push(Arc::clone(&bg));
            }
            for i in (1..self.bloom_mip_views.len()).rev() {
                let src = &self.bloom_mip_views[i];
                let dst = &self.bloom_mip_views[i - 1];
                let bg = Arc::new(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("aster bloom up bg"),
                    layout: bcl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(src),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(dst),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.bloom_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: self.bloom_uniform.as_entire_binding(),
                        },
                    ],
                }));
                self.bloom_cached_up_bgs.push(Arc::clone(&bg));
            }
        }
        (
            self.bloom_cached_down_bgs.iter().map(Arc::clone).collect(),
            self.bloom_cached_up_bgs.iter().map(Arc::clone).collect(),
        )
    }

    pub(crate) fn encode_bloom_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        res: &FrameResources,
    ) -> &wgpu::TextureView {
        // Downsample
        for i in 0..self.bloom_mip_views.len() {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("aster bloom down"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(self.bloom_compute_down.as_ref().unwrap());
            cpass.set_bind_group(0, &*res.bloom_down_bgs[i], &[]);
            let dst_size = self.bloom_mip_textures[i].size();
            let wx = (dst_size.width + 7) / 8;
            let wy = (dst_size.height + 7) / 8;
            cpass.dispatch_workgroups(wx, wy, 1);
        }
        // Upsample
        for i in (1..self.bloom_mip_views.len()).rev() {
            let up_idx = i - 1;
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("aster bloom up"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(self.bloom_compute_up.as_ref().unwrap());
            cpass.set_bind_group(0, &*res.bloom_up_bgs[up_idx], &[]);
            let dst_size = self.bloom_mip_textures[i - 1].size();
            let wx = (dst_size.width + 7) / 8;
            let wy = (dst_size.height + 7) / 8;
            cpass.dispatch_workgroups(wx, wy, 1);
        }
        &self.bloom_mip_views[0]
    }

    pub(crate) fn ensure_post_bind_group(&mut self) -> Arc<wgpu::BindGroup> {
        if self.post_cached_bg.is_none() {
            let hdr_cv = &self.hdr_target.as_ref().unwrap().color_view;
            let ssao_view = self
                .ssao_output_view
                .as_ref()
                .map(|v| v as &wgpu::TextureView)
                .unwrap_or(&self.ssao_noise_view);
            let bloom_view = &self.bloom_mip_views[0];
            let ssgi_view = self
                .ssgi_output_view
                .as_ref()
                .map(|v| v as &wgpu::TextureView)
                .unwrap_or(&self.ssao_noise_view);
            let bg = Arc::new(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aster post bind group frame"),
                layout: &self.post_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(hdr_cv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(bloom_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(ssao_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.post_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.bloom_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(ssgi_view),
                    },
                ],
            }));
            self.post_cached_bg = Some(Arc::clone(&bg));
            bg
        } else {
            Arc::clone(self.post_cached_bg.as_ref().unwrap())
        }
    }

    pub(crate) fn encode_post_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        res: &FrameResources,
        output_view: &wgpu::TextureView,
    ) {
        let post_bg = res.post_bg.as_ref().unwrap();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aster post composite pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.post_pipeline);
        pass.set_bind_group(0, &**post_bg, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(crate) fn ensure_bloom_mips(&mut self, w: u32, h: u32) {
        if !self.bloom_mip_textures.is_empty()
            && bloom_mips_match(
                self.bloom_mip_textures[0].width(),
                self.bloom_mip_textures[0].height(),
                w,
                h,
            )
        {
            return;
        }
        self.bloom_mip_views.clear();
        self.bloom_mip_textures.clear();
        let mut mw = w;
        let mut mh = h;
        for _ in 0..MAX_BLOOM_MIPS {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aster bloom mip"),
                size: wgpu::Extent3d {
                    width: mw.max(1),
                    height: mh.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.bloom_mip_views.push(view);
            self.bloom_mip_textures.push(tex);
            mw /= 2;
            mh /= 2;
        }
        // Invalidate cached bloom bind groups.
        self.bloom_cached_down_bgs.clear();
        self.bloom_cached_up_bgs.clear();
        self.post_cached_bg = None;
        self.ssgi_cached_bg = None;
    }

    pub(crate) fn ensure_ssao_output(&mut self) {
        let w = self.post_target_width.max(1);
        let h = self.post_target_height.max(1);
        let need_create = match &self.ssao_output_texture {
            None => true,
            Some(t) => t.width() != w || t.height() != h,
        };
        if need_create {
            self.ssao_output_view = None;
            self.ssao_output_texture = None;
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aster ssao output"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.ssao_output_texture = Some(tex);
            self.ssao_output_view = Some(view);
            self.ssao_cached_bg = None;
            self.post_cached_bg = None;
        }
    }

    pub(crate) fn ensure_ssgi_output(&mut self) {
        let w = self.post_target_width.max(1);
        let h = self.post_target_height.max(1);
        let need_create = match &self.ssgi_output_texture {
            None => true,
            Some(t) => t.width() != w || t.height() != h,
        };
        if need_create {
            self.ssgi_output_view = None;
            self.ssgi_output_texture = None;
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aster ssgi output"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.ssgi_output_texture = Some(tex);
            self.ssgi_output_view = Some(view);
            self.ssgi_cached_bg = None;
            self.post_cached_bg = None;
        }
    }
}
pub(crate) fn bloom_mips_match(
    current_width: u32,
    current_height: u32,
    width: u32,
    height: u32,
) -> bool {
    current_width == width && current_height == height
}
