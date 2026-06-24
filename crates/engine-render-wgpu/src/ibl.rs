use crate::device::*;
use wgpu::util::DeviceExt;

impl WgpuRenderDevice {
    pub(crate) fn bake_ibl(&mut self) {
        self.bake_ibl_from_view(&self._skybox_default_cubemap_view);
        self.active_ibl_cubemap = None;
    }

    pub(crate) fn bake_ibl_for_cubemap(&mut self, handle: engine_core::Handle) {
        let view = match self
            .images
            .get(&handle)
            .and_then(|image| image.cube_view.as_ref())
        {
            Some(view) => view,
            None => {
                return;
            }
        };
        self.bake_ibl_from_view(view);
        self.active_ibl_cubemap = Some(handle);
    }

    pub(crate) fn bake_ibl_from_view(&self, skybox_view: &wgpu::TextureView) {
        let scratch_view = self.ibl_scratch_view.as_ref().unwrap();
        let scratch_tex = self.ibl_scratch_tex.as_ref().unwrap();
        let bake_bgl = self.ibl_bake_bgl.as_ref().unwrap();
        let sampl = &self.ibl_sampler;

        let irradiance_pipeline = match self.ibl_irradiance_compute.as_ref() {
            Some(pipeline) => pipeline,
            None => {
                return;
            }
        };
        let prefilter_pipeline = match self.ibl_prefilter_compute.as_ref() {
            Some(pipeline) => pipeline,
            None => {
                return;
            }
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aster ibl bake encoder"),
            });

        let brdf_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aster ibl brdf bg"),
            layout: self.ibl_brdf_bgl.as_ref().unwrap(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&self.ibl_brdf_lut_view),
            }],
        });
        {
            let Some(brdf_pipeline) = self.ibl_brdf_compute.as_ref() else {
                return;
            };
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("aster ibl brdf pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(brdf_pipeline);
            cpass.set_bind_group(0, &brdf_bg, &[]);
            cpass.dispatch_workgroups((IBL_BRDF_LUT_RES + 7) / 8, (IBL_BRDF_LUT_RES + 7) / 8, 1);
        }

        let create_bake_resources = |label: &str, params: [u32; 4]| {
            let params_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: bake_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(skybox_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampl),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(scratch_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: params_buf.as_entire_binding(),
                    },
                ],
            });
            (params_buf, bind_group)
        };
        let mut bake_resources = Vec::with_capacity(36);

        // Bake irradiance cubemap (32x32, 6 faces)
        for face in 0u32..6 {
            let resources = create_bake_resources("aster ibl irradiance params", [face, 0, 0, 0]);
            bake_resources.push(resources);
            let (_, bg) = bake_resources.last().unwrap();
            {
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("aster ibl irradiance pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(irradiance_pipeline);
                cpass.set_bind_group(0, bg, &[]);
                cpass.dispatch_workgroups(
                    (IBL_IRRADIANCE_RES + 7) / 8,
                    (IBL_IRRADIANCE_RES + 7) / 8,
                    1,
                );
            }
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: scratch_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.ibl_irradiance_map,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: face,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: IBL_IRRADIANCE_RES,
                    height: IBL_IRRADIANCE_RES,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Bake prefiltered environment map (512x512 base, 5 mips, 6 faces each)
        for mip in 0u32..5 {
            let res = IBL_PREFILTER_RES >> mip;
            let roughness = mip as f32 / 4.0;
            for face in 0u32..6 {
                let resources = create_bake_resources(
                    "aster ibl prefilter params",
                    [face, roughness.to_bits(), res, 0],
                );
                bake_resources.push(resources);
                let (_, bg) = bake_resources.last().unwrap();
                {
                    let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("aster ibl prefilter pass"),
                        timestamp_writes: None,
                    });
                    cpass.set_pipeline(prefilter_pipeline);
                    cpass.set_bind_group(0, bg, &[]);
                    cpass.dispatch_workgroups((res + 7) / 8, (res + 7) / 8, 1);
                }
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: scratch_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.ibl_prefilter_map,
                        mip_level: mip,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: face,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: res,
                        height: res,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }

        self.queue.submit(Some(encoder.finish()));
    }
}
