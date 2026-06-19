use crate::uniforms::Instance;
use engine_core::{EngineResult, HandleAllocator};
use engine_render::{
    BufferUsage, ImageDesc, ImageFormat, ImageUsage, RenderDevice, RenderTarget, RenderTargetDesc,
};

pub(crate) struct CreatedTarget(
    pub(crate) wgpu::Texture,
    pub(crate) wgpu::TextureView,
    pub(crate) Option<wgpu::Texture>,
    pub(crate) Option<wgpu::TextureView>,
    pub(crate) RenderTarget,
);

pub(crate) fn create_target(
    device: &wgpu::Device,
    allocator: &mut HandleAllocator,
    desc: RenderTargetDesc,
) -> EngineResult<CreatedTarget> {
    let handle = allocator.allocate()?;
    let color_desc = ImageDesc {
        width: desc.width.max(1),
        height: desc.height.max(1),
        mip_levels: 1,
        samples: desc.samples.max(1),
        format: desc.color_format,
        usage: ImageUsage::COLOR_ATTACHMENT
            .or(ImageUsage::SAMPLED)
            .or(ImageUsage::TRANSFER_SRC),
        label: desc.label,
    };
    let color = device.create_texture(&texture_desc(&color_desc));
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let (depth, depth_view) = if desc.with_depth {
        let depth_desc = ImageDesc::depth_2d(desc.width.max(1), desc.height.max(1));
        let depth = device.create_texture(&texture_desc(&depth_desc));
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        (Some(depth), Some(depth_view))
    } else {
        (None, None)
    };
    let target = RenderTarget { handle, desc };
    Ok(CreatedTarget(color, color_view, depth, depth_view, target))
}

pub(crate) fn texture_desc(desc: &ImageDesc) -> wgpu::TextureDescriptor<'_> {
    wgpu::TextureDescriptor {
        label: desc.label,
        size: wgpu::Extent3d {
            width: desc.width.max(1),
            height: desc.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: desc.mip_levels.max(1),
        sample_count: desc.samples.max(1),
        dimension: wgpu::TextureDimension::D2,
        format: to_wgpu_format(desc.format),
        usage: to_wgpu_texture_usage(desc.usage),
        view_formats: &[],
    }
}

pub(crate) fn to_wgpu_format(format: ImageFormat) -> wgpu::TextureFormat {
    match format {
        ImageFormat::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        ImageFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        ImageFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        ImageFormat::Rgba32Float => wgpu::TextureFormat::Rgba32Float,
        ImageFormat::Depth32Float => wgpu::TextureFormat::Depth32Float,
        ImageFormat::Depth24Stencil8 => wgpu::TextureFormat::Depth24PlusStencil8,
        ImageFormat::Bc7Srgb => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
    }
}

pub(crate) fn from_wgpu_format(format: wgpu::TextureFormat) -> Option<ImageFormat> {
    match format {
        wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Bgra8UnormSrgb => {
            Some(ImageFormat::Rgba8Srgb)
        }
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm => {
            Some(ImageFormat::Rgba8Unorm)
        }
        wgpu::TextureFormat::Rgba16Float => Some(ImageFormat::Rgba16Float),
        wgpu::TextureFormat::Rgba32Float => Some(ImageFormat::Rgba32Float),
        _ => None,
    }
}
pub(crate) fn to_wgpu_texture_usage(usage: ImageUsage) -> wgpu::TextureUsages {
    let mut out = wgpu::TextureUsages::empty();
    if usage.contains(ImageUsage::SAMPLED) {
        out |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    if usage.contains(ImageUsage::COLOR_ATTACHMENT) {
        out |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    if usage.contains(ImageUsage::DEPTH_STENCIL_ATTACHMENT) {
        out |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    if usage.contains(ImageUsage::STORAGE) {
        out |= wgpu::TextureUsages::STORAGE_BINDING;
    }
    if usage.contains(ImageUsage::TRANSFER_SRC) {
        out |= wgpu::TextureUsages::COPY_SRC;
    }
    if usage.contains(ImageUsage::TRANSFER_DST) {
        out |= wgpu::TextureUsages::COPY_DST;
    }
    out
}

pub(crate) fn to_wgpu_buffer_usage(usage: BufferUsage, host_visible: bool) -> wgpu::BufferUsages {
    let mut out = wgpu::BufferUsages::empty();
    if usage.contains(BufferUsage::VERTEX) {
        out |= wgpu::BufferUsages::VERTEX;
    }
    if usage.contains(BufferUsage::INDEX) {
        out |= wgpu::BufferUsages::INDEX;
    }
    if usage.contains(BufferUsage::UNIFORM) {
        out |= wgpu::BufferUsages::UNIFORM;
    }
    if usage.contains(BufferUsage::STORAGE) {
        out |= wgpu::BufferUsages::STORAGE;
    }
    if usage.contains(BufferUsage::TRANSFER_SRC) || host_visible {
        out |= wgpu::BufferUsages::COPY_SRC;
    }
    if usage.contains(BufferUsage::TRANSFER_DST) || host_visible {
        out |= wgpu::BufferUsages::COPY_DST;
    }
    out
}

pub(crate) fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aster instance buffer"),
        size: (capacity.max(1) * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
