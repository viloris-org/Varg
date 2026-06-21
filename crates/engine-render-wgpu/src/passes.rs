use std::collections::HashMap;

use crate::{device::*, meshes::*};

pub(crate) fn encode_batched_forward_pass<'a>(
    encoder: &mut wgpu::CommandEncoder,
    color_view: &wgpu::TextureView,
    normal_view: &wgpu::TextureView,
    albedo_view: &wgpu::TextureView,
    depth_view: Option<&wgpu::TextureView>,
    pipeline: &wgpu::RenderPipeline,
    camera_bind_group: &wgpu::BindGroup,
    default_material_bind_group: &wgpu::BindGroup,
    material_gpu: &HashMap<String, MaterialGpuData>,
    mesh_cache: &'a HashMap<String, MeshBuffers>,
    default_vertex_buffer: &'a wgpu::Buffer,
    default_index_buffer: &'a wgpu::Buffer,
    instance_buffer: &wgpu::Buffer,
    batches: &[(String, u32, String, bool)],
    transparent: bool,
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
    let normal_attachment = Some(wgpu::RenderPassColorAttachment {
        view: normal_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
                r: 0.5,
                g: 0.5,
                b: 1.0,
                a: 1.0,
            }),
            store: wgpu::StoreOp::Store,
        },
    });
    let albedo_attachment = Some(wgpu::RenderPassColorAttachment {
        view: albedo_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }),
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
        label: Some("aster forward pass"),
        color_attachments: &[color_attachment, normal_attachment, albedo_attachment],
        depth_stencil_attachment: depth_attachment,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, camera_bind_group, &[]);

    let mut instance_offset = 0u32;
    for (mesh_name, count, material_name, _) in batches {
        if *count == 0 {
            continue;
        }
        let is_transparent = material_name.starts_with("@sprite:") || material_name == "@particle";
        if is_transparent != transparent {
            instance_offset += count;
            continue;
        }
        let material_lookup = material_name
            .strip_prefix("@sprite:")
            .unwrap_or(material_name);
        let mat_bg = material_gpu
            .get(material_lookup)
            .map(|m| &m.bind_group)
            .unwrap_or(default_material_bind_group);
        pass.set_bind_group(1, mat_bg, &[]);
        let buffers = mesh_cache.get(mesh_name);
        let (vertex_buf, index_buf, index_count) = match buffers {
            Some(b) => (&b.vertex_buffer, &b.index_buffer, b.index_count),
            None => {
                tracing::warn!(target: "engine", mesh = %mesh_name, "mesh cache miss, using fallback cube");
                (
                    default_vertex_buffer,
                    default_index_buffer,
                    CUBE_INDEX_COUNT,
                )
            }
        };
        pass.set_vertex_buffer(0, vertex_buf.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..index_count, 0, instance_offset..instance_offset + count);
        instance_offset += count;
    }
}
pub(crate) fn encode_shadow_pass(
    encoder: &mut wgpu::CommandEncoder,
    depth_view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    vertex_buffer: &wgpu::Buffer,
    index_buffer: &wgpu::Buffer,
    instance_buffer: &wgpu::Buffer,
    batches: &[(String, u32, String, bool)],
    mesh_cache: &HashMap<String, MeshBuffers>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("aster shadow pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);

    let mut instance_offset = 0u32;
    for (mesh_name, count, _, casts_shadows) in batches {
        if *count == 0 {
            continue;
        }
        if !*casts_shadows {
            instance_offset += count;
            continue;
        }
        let buffers = mesh_cache.get(mesh_name);
        let (vertex_buf, index_buf, index_count) = match buffers {
            Some(b) => (&b.vertex_buffer, &b.index_buffer, b.index_count),
            None => (vertex_buffer, index_buffer, CUBE_INDEX_COUNT),
        };
        pass.set_vertex_buffer(0, vertex_buf.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..index_count, 0, instance_offset..instance_offset + count);
        instance_offset += count;
    }
}

pub(crate) fn encode_grid_pass(
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
pub(crate) fn encode_skybox_pass(
    encoder: &mut wgpu::CommandEncoder,
    color_view: &wgpu::TextureView,
    depth_view: Option<&wgpu::TextureView>,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let color_attachment = Some(wgpu::RenderPassColorAttachment {
        view: color_view,
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
        label: Some("aster skybox pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: depth_attachment,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}
