//! GPU resource descriptors and typed handles.

use engine_core::Handle;

/// Typed handle for a GPU image.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImageHandle(pub(crate) Handle);

impl ImageHandle {
    /// Creates an image handle from the shared generational handle type.
    pub const fn new(handle: Handle) -> Self {
        Self(handle)
    }

    /// Returns the shared generational handle value.
    pub const fn raw(self) -> Handle {
        self.0
    }
}

/// Typed handle for a GPU buffer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BufferHandle(pub(crate) Handle);

impl BufferHandle {
    /// Creates a buffer handle from the shared generational handle type.
    pub const fn new(handle: Handle) -> Self {
        Self(handle)
    }

    /// Returns the shared generational handle value.
    pub const fn raw(self) -> Handle {
        self.0
    }
}

/// Typed handle for a GPU sampler.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SamplerHandle(pub(crate) Handle);

impl SamplerHandle {
    /// Creates a sampler handle from the shared generational handle type.
    pub const fn new(handle: Handle) -> Self {
        Self(handle)
    }

    /// Returns the shared generational handle value.
    pub const fn raw(self) -> Handle {
        self.0
    }
}

/// Image pixel format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageFormat {
    /// 8-bit RGBA sRGB.
    Rgba8Srgb,
    /// 8-bit RGBA linear.
    Rgba8Unorm,
    /// 16-bit float RGBA.
    Rgba16Float,
    /// 16-bit float RG for motion vectors and compact velocity fields.
    Rg16Float,
    /// 32-bit float RGBA.
    Rgba32Float,
    /// 32-bit depth.
    Depth32Float,
    /// 24-bit depth + 8-bit stencil.
    Depth24Stencil8,
    /// BC7 compressed sRGB.
    Bc7Srgb,
}

/// Image usage flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageUsage(u32);

impl ImageUsage {
    /// Sampled in shaders.
    pub const SAMPLED: Self = Self(1 << 0);
    /// Used as a color attachment.
    pub const COLOR_ATTACHMENT: Self = Self(1 << 1);
    /// Used as a depth/stencil attachment.
    pub const DEPTH_STENCIL_ATTACHMENT: Self = Self(1 << 2);
    /// Used as a storage image.
    pub const STORAGE: Self = Self(1 << 3);
    /// Transfer source.
    pub const TRANSFER_SRC: Self = Self(1 << 4);
    /// Transfer destination.
    pub const TRANSFER_DST: Self = Self(1 << 5);

    /// Combines two usage flags.
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns whether a flag is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl ImageFormat {
    /// Returns the bytes per pixel (or per block for compressed formats) for row-pitch calculations.
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            ImageFormat::Rgba8Srgb => 4,
            ImageFormat::Rgba8Unorm => 4,
            ImageFormat::Rgba16Float => 8,
            ImageFormat::Rg16Float => 4,
            ImageFormat::Rgba32Float => 16,
            ImageFormat::Depth32Float => 4,
            ImageFormat::Depth24Stencil8 => 4,
            // BC7: 4x4 pixel blocks, 16 bytes per block → 4 bytes per pixel equivalent stride
            ImageFormat::Bc7Srgb => 4,
        }
    }
}

/// Image creation descriptor.
#[derive(Clone, Debug)]
pub struct ImageDesc {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Mip level count.
    pub mip_levels: u32,
    /// Sample count for MSAA.
    pub samples: u32,
    /// Pixel format.
    pub format: ImageFormat,
    /// Usage flags.
    pub usage: ImageUsage,
    /// Debug label.
    pub label: Option<&'static str>,
}

impl ImageDesc {
    /// Creates a simple 2D color image descriptor.
    pub fn color_2d(width: u32, height: u32, format: ImageFormat) -> Self {
        Self {
            width,
            height,
            mip_levels: 1,
            samples: 1,
            format,
            usage: ImageUsage::SAMPLED.or(ImageUsage::COLOR_ATTACHMENT),
            label: None,
        }
    }

    /// Creates a depth image descriptor.
    pub fn depth_2d(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            mip_levels: 1,
            samples: 1,
            format: ImageFormat::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
            label: None,
        }
    }

    /// Creates a motion-vector image descriptor at internal render resolution.
    pub fn motion_vectors_2d(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            mip_levels: 1,
            samples: 1,
            format: ImageFormat::Rg16Float,
            usage: ImageUsage::SAMPLED.or(ImageUsage::COLOR_ATTACHMENT),
            label: Some("aster motion vectors"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_vector_descriptor_uses_compact_sampled_render_target() {
        let desc = ImageDesc::motion_vectors_2d(1280, 720);
        assert_eq!(desc.width, 1280);
        assert_eq!(desc.height, 720);
        assert_eq!(desc.format, ImageFormat::Rg16Float);
        assert!(desc.usage.contains(ImageUsage::SAMPLED));
        assert!(desc.usage.contains(ImageUsage::COLOR_ATTACHMENT));
    }
}

/// Buffer usage flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferUsage(u32);

impl BufferUsage {
    /// Vertex buffer.
    pub const VERTEX: Self = Self(1 << 0);
    /// Index buffer.
    pub const INDEX: Self = Self(1 << 1);
    /// Uniform buffer.
    pub const UNIFORM: Self = Self(1 << 2);
    /// Storage buffer.
    pub const STORAGE: Self = Self(1 << 3);
    /// Transfer source.
    pub const TRANSFER_SRC: Self = Self(1 << 4);
    /// Transfer destination.
    pub const TRANSFER_DST: Self = Self(1 << 5);

    /// Combines two usage flags.
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns whether a flag is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Buffer creation descriptor.
#[derive(Clone, Debug)]
pub struct BufferDesc {
    /// Size in bytes.
    pub size: u64,
    /// Usage flags.
    pub usage: BufferUsage,
    /// Whether the buffer is CPU-visible.
    pub host_visible: bool,
    /// Debug label.
    pub label: Option<&'static str>,
}

/// Sampler filter mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterMode {
    /// Nearest-neighbor.
    Nearest,
    /// Bilinear.
    Linear,
}

/// Sampler address mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressMode {
    /// Clamp to edge.
    ClampToEdge,
    /// Repeat.
    Repeat,
    /// Mirrored repeat.
    MirroredRepeat,
}

/// Sampler creation descriptor.
#[derive(Clone, Debug)]
pub struct SamplerDesc {
    /// Minification filter.
    pub min_filter: FilterMode,
    /// Magnification filter.
    pub mag_filter: FilterMode,
    /// Mip filter.
    pub mip_filter: FilterMode,
    /// Address mode for U/V/W.
    pub address_mode: AddressMode,
    /// Anisotropy level (1 = disabled).
    pub anisotropy: u32,
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            mip_filter: FilterMode::Linear,
            address_mode: AddressMode::Repeat,
            anisotropy: 1,
        }
    }
}

/// Simple texture cache tracking upload state.
#[derive(Debug, Default)]
pub struct TextureCache {
    entries: Vec<(String, ImageHandle)>,
}

impl TextureCache {
    /// Inserts or replaces a cached texture.
    pub fn insert(&mut self, key: impl Into<String>, handle: ImageHandle) {
        let key = key.into();
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k == &key) {
            entry.1 = handle;
        } else {
            self.entries.push((key, handle));
        }
    }

    /// Looks up a cached texture.
    pub fn get(&self, key: &str) -> Option<ImageHandle> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, h)| *h)
    }

    /// Removes a cached texture, returning the handle for destruction.
    pub fn remove(&mut self, key: &str) -> Option<ImageHandle> {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == key) {
            Some(self.entries.swap_remove(pos).1)
        } else {
            None
        }
    }
}
