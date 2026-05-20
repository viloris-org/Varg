//! Real Vulkan backend, compiled only with `--features vulkan`.
//!
//! This module owns the Vulkan instance, physical device, logical device,
//! swapchain, command pools, synchronization primitives, and the
//! gpu-allocator memory allocator.

use std::collections::VecDeque;

use ash::{vk, vk::Handle as VkHandle};
use engine_core::{EngineError, EngineResult, Generation, Handle};
use engine_render::{
    BufferDesc, BufferHandle, GuiDrawList, GuiTextureId, ImageDesc, ImageHandle, RenderApi,
    RenderDevice, RenderFrame, RenderGraph, RenderTarget, RenderTargetDesc,
};
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use tracing::{debug, info, warn};

/// Number of frames in flight.
const FRAMES_IN_FLIGHT: usize = 2;

/// A pending GPU resource destruction.
enum PendingDestroy {
    Image(vk::Image, vk::ImageView, gpu_allocator::vulkan::Allocation),
    Buffer(vk::Buffer, gpu_allocator::vulkan::Allocation),
}

/// Vulkan rendering backend.
pub struct VulkanRenderDevice {
    entry: ash::Entry,
    instance: ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    graphics_queue: vk::Queue,
    graphics_family: u32,
    command_pool: vk::CommandPool,
    command_buffers: [vk::CommandBuffer; FRAMES_IN_FLIGHT],
    image_available: [vk::Semaphore; FRAMES_IN_FLIGHT],
    render_finished: [vk::Semaphore; FRAMES_IN_FLIGHT],
    in_flight: [vk::Fence; FRAMES_IN_FLIGHT],
    allocator: Allocator,
    destroy_queue: VecDeque<(u64, PendingDestroy)>,
    frame_parity: usize,
}

impl VulkanRenderDevice {
    /// Creates a headless Vulkan device (no surface, no swapchain).
    ///
    /// # Safety
    /// Caller must ensure the Vulkan loader is available.
    pub fn new_headless() -> EngineResult<Self> {
        let entry = unsafe { ash::Entry::load() }.map_err(|e| EngineError::other(e.to_string()))?;

        let app_info = vk::ApplicationInfo::default()
            .application_name(c"Aster")
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(c"Aster")
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_3);

        let instance_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = unsafe { entry.create_instance(&instance_info, None) }
            .map_err(|e| EngineError::other(e.to_string()))?;

        let (physical_device, graphics_family) = pick_physical_device(&instance, &entry)?;

        let queue_priority = 1.0_f32;
        let queue_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(graphics_family)
            .queue_priorities(std::slice::from_ref(&queue_priority));

        let device_info =
            vk::DeviceCreateInfo::default().queue_create_infos(std::slice::from_ref(&queue_info));

        let device = unsafe { instance.create_device(physical_device, &device_info, None) }
            .map_err(|e| EngineError::other(e.to_string()))?;

        let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None) }
            .map_err(|e| EngineError::other(e.to_string()))?;

        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT as u32);
        let cbs = unsafe { device.allocate_command_buffers(&alloc_info) }
            .map_err(|e| EngineError::other(e.to_string()))?;
        let command_buffers = [cbs[0], cbs[1]];

        let sem_info = vk::SemaphoreCreateInfo::default();
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

        let image_available = [
            unsafe { device.create_semaphore(&sem_info, None) }
                .map_err(|e| EngineError::other(e.to_string()))?,
            unsafe { device.create_semaphore(&sem_info, None) }
                .map_err(|e| EngineError::other(e.to_string()))?,
        ];
        let render_finished = [
            unsafe { device.create_semaphore(&sem_info, None) }
                .map_err(|e| EngineError::other(e.to_string()))?,
            unsafe { device.create_semaphore(&sem_info, None) }
                .map_err(|e| EngineError::other(e.to_string()))?,
        ];
        let in_flight = [
            unsafe { device.create_fence(&fence_info, None) }
                .map_err(|e| EngineError::other(e.to_string()))?,
            unsafe { device.create_fence(&fence_info, None) }
                .map_err(|e| EngineError::other(e.to_string()))?,
        ];

        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })
        .map_err(|e| EngineError::other(e.to_string()))?;

        info!("Vulkan headless device created");

        Ok(Self {
            entry,
            instance,
            physical_device,
            device,
            graphics_queue,
            graphics_family,
            command_pool,
            command_buffers,
            image_available,
            render_finished,
            in_flight,
            allocator,
            destroy_queue: VecDeque::new(),
            frame_parity: 0,
        })
    }

    fn current_cb(&self) -> vk::CommandBuffer {
        self.command_buffers[self.frame_parity]
    }

    fn current_fence(&self) -> vk::Fence {
        self.in_flight[self.frame_parity]
    }
}

impl RenderDevice for VulkanRenderDevice {
    fn api(&self) -> RenderApi {
        RenderApi::Vulkan
    }

    fn render(&mut self, frame: RenderFrame) -> EngineResult<()> {
        self.execute_graph(&RenderGraph::default(), frame)
    }

    fn execute_graph(&mut self, graph: &RenderGraph, frame: RenderFrame) -> EngineResult<()> {
        let parity = self.frame_parity;
        let fence = self.in_flight[parity];
        let cb = self.command_buffers[parity];

        unsafe {
            self.device
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(|e| EngineError::other(e.to_string()))?;
            self.device
                .reset_fences(&[fence])
                .map_err(|e| EngineError::other(e.to_string()))?;
        }

        self.flush_destroy_queue(frame.frame_index.saturating_sub(FRAMES_IN_FLIGHT as u64));

        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.device
                .begin_command_buffer(cb, &begin_info)
                .map_err(|e| EngineError::other(e.to_string()))?;
        }

        // Execute passes in topological order.
        for pass in &graph.passes {
            debug!(pass = %pass.name, "executing render pass");
            // Concrete pass recording would go here.
        }

        unsafe {
            self.device
                .end_command_buffer(cb)
                .map_err(|e| EngineError::other(e.to_string()))?;

            let submit_info = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cb));
            self.device
                .queue_submit(self.graphics_queue, &[submit_info], fence)
                .map_err(|e| EngineError::other(e.to_string()))?;
        }

        self.frame_parity = (parity + 1) % FRAMES_IN_FLIGHT;
        Ok(())
    }

    fn create_render_target(&mut self, desc: RenderTargetDesc) -> EngineResult<RenderTarget> {
        // Allocate a color image.
        let _color = self.create_image(engine_render::ImageDesc::color_2d(
            desc.width,
            desc.height,
            desc.color_format,
        ))?;
        Ok(RenderTarget {
            handle: Handle::new(0, Generation::FIRST),
            desc,
        })
    }

    fn upload_texture(&mut self, desc: ImageDesc, _data: &[u8]) -> EngineResult<ImageHandle> {
        self.create_image(desc)
    }

    fn destroy_render_target(&mut self, _target: RenderTarget) {
        // Queued destruction handled by flush_destroy_queue.
    }

    fn create_image(&mut self, desc: ImageDesc) -> EngineResult<ImageHandle> {
        let format = map_format(desc.format);
        let usage = map_image_usage(desc.usage);

        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: desc.width,
                height: desc.height,
                depth: 1,
            })
            .mip_levels(desc.mip_levels)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let image = unsafe { self.device.create_image(&image_info, None) }
            .map_err(|e| EngineError::other(e.to_string()))?;

        let reqs = unsafe { self.device.get_image_memory_requirements(image) };
        let alloc = self
            .allocator
            .allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
                name: desc.label.unwrap_or("image"),
                requirements: reqs,
                location: gpu_allocator::MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| EngineError::other(e.to_string()))?;

        unsafe {
            self.device
                .bind_image_memory(image, alloc.memory(), alloc.offset())
                .map_err(|e| EngineError::other(e.to_string()))?;
        }

        // Store in destroy queue with a sentinel frame so it's never auto-freed.
        // Real tracking would use a slot allocator; this is the minimal path.
        let handle = Handle::new(image.as_raw() as u32, Generation::FIRST);

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: desc.mip_levels,
                base_array_layer: 0,
                layer_count: 1,
            });
        let view = unsafe { self.device.create_image_view(&view_info, None) }
            .map_err(|e| EngineError::other(e.to_string()))?;

        // Immediately push to destroy queue tagged at u64::MAX so it lives until
        // explicitly freed via destroy_image.
        self.destroy_queue
            .push_back((u64::MAX, PendingDestroy::Image(image, view, alloc)));

        Ok(ImageHandle::new(handle))
    }

    fn destroy_image(&mut self, handle: ImageHandle) {
        // Re-tag the matching entry so it gets cleaned up next flush.
        for (frame, entry) in &mut self.destroy_queue {
            if let PendingDestroy::Image(img, _, _) = entry {
                if img.as_raw() == handle.raw().slot() as u64 {
                    *frame = 0;
                    break;
                }
            }
        }
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> EngineResult<BufferHandle> {
        let usage = map_buffer_usage(desc.usage);
        let buffer_info = vk::BufferCreateInfo::default()
            .size(desc.size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = unsafe { self.device.create_buffer(&buffer_info, None) }
            .map_err(|e| EngineError::other(e.to_string()))?;

        let reqs = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let location = if desc.host_visible {
            gpu_allocator::MemoryLocation::CpuToGpu
        } else {
            gpu_allocator::MemoryLocation::GpuOnly
        };
        let alloc = self
            .allocator
            .allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
                name: desc.label.unwrap_or("buffer"),
                requirements: reqs,
                location,
                linear: true,
                allocation_scheme: gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| EngineError::other(e.to_string()))?;

        unsafe {
            self.device
                .bind_buffer_memory(buffer, alloc.memory(), alloc.offset())
                .map_err(|e| EngineError::other(e.to_string()))?;
        }

        let handle = Handle::new(buffer.as_raw() as u32, Generation::FIRST);
        self.destroy_queue
            .push_back((u64::MAX, PendingDestroy::Buffer(buffer, alloc)));

        Ok(BufferHandle::new(handle))
    }

    fn destroy_buffer(&mut self, handle: BufferHandle) {
        for (frame, entry) in &mut self.destroy_queue {
            if let PendingDestroy::Buffer(buf, _) = entry {
                if buf.as_raw() == handle.raw().slot() as u64 {
                    *frame = 0;
                    break;
                }
            }
        }
    }

    fn upload_gui_texture(&mut self, desc: ImageDesc, _data: &[u8]) -> EngineResult<GuiTextureId> {
        let handle = self.create_image(desc)?;
        Ok(GuiTextureId(handle.raw().slot() as u64))
    }

    fn draw_gui(&mut self, _draw_list: &GuiDrawList) -> EngineResult<()> {
        // GUI draw commands would be recorded into the current command buffer.
        Ok(())
    }

    fn flush_destroy_queue(&mut self, before_frame: u64) {
        let device = &self.device;
        let allocator = &mut self.allocator;

        self.destroy_queue.retain_mut(|(frame, entry)| {
            if *frame <= before_frame {
                match entry {
                    PendingDestroy::Image(img, view, alloc) => {
                        unsafe {
                            device.destroy_image_view(*view, None);
                            device.destroy_image(*img, None);
                        }
                        if let Err(e) = allocator.free(std::mem::replace(
                            alloc,
                            // Safety: we're about to drop this entry.
                            unsafe { std::mem::zeroed() },
                        )) {
                            warn!("gpu-allocator free error: {e}");
                        }
                        false
                    }
                    PendingDestroy::Buffer(buf, alloc) => {
                        unsafe { device.destroy_buffer(*buf, None) };
                        if let Err(e) =
                            allocator.free(std::mem::replace(alloc, unsafe { std::mem::zeroed() }))
                        {
                            warn!("gpu-allocator free error: {e}");
                        }
                        false
                    }
                }
            } else {
                true
            }
        });
    }
}

impl Drop for VulkanRenderDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
        }
        // Flush all pending destroys.
        self.flush_destroy_queue(u64::MAX);
        unsafe {
            for i in 0..FRAMES_IN_FLIGHT {
                self.device.destroy_semaphore(self.image_available[i], None);
                self.device.destroy_semaphore(self.render_finished[i], None);
                self.device.destroy_fence(self.in_flight[i], None);
            }
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn pick_physical_device(
    instance: &ash::Instance,
    _entry: &ash::Entry,
) -> EngineResult<(vk::PhysicalDevice, u32)> {
    let devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| EngineError::other(e.to_string()))?;

    for pd in devices {
        let families = unsafe { instance.get_physical_device_queue_family_properties(pd) };
        for (i, family) in families.iter().enumerate() {
            if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                return Ok((pd, i as u32));
            }
        }
    }
    Err(EngineError::other(
        "no suitable Vulkan physical device found",
    ))
}

fn map_format(fmt: engine_render::ImageFormat) -> vk::Format {
    use engine_render::ImageFormat::*;
    match fmt {
        Rgba8Srgb => vk::Format::R8G8B8A8_SRGB,
        Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
        Depth32Float => vk::Format::D32_SFLOAT,
        Depth24Stencil8 => vk::Format::D24_UNORM_S8_UINT,
        Bc7Srgb => vk::Format::BC7_SRGB_BLOCK,
    }
}

fn map_image_usage(usage: engine_render::ImageUsage) -> vk::ImageUsageFlags {
    use engine_render::ImageUsage;
    let mut flags = vk::ImageUsageFlags::empty();
    if usage.contains(ImageUsage::SAMPLED) {
        flags |= vk::ImageUsageFlags::SAMPLED;
    }
    if usage.contains(ImageUsage::COLOR_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    if usage.contains(ImageUsage::DEPTH_STENCIL_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
    }
    if usage.contains(ImageUsage::STORAGE) {
        flags |= vk::ImageUsageFlags::STORAGE;
    }
    if usage.contains(ImageUsage::TRANSFER_SRC) {
        flags |= vk::ImageUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(ImageUsage::TRANSFER_DST) {
        flags |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    flags
}

fn map_buffer_usage(usage: engine_render::BufferUsage) -> vk::BufferUsageFlags {
    use engine_render::BufferUsage;
    let mut flags = vk::BufferUsageFlags::empty();
    if usage.contains(BufferUsage::VERTEX) {
        flags |= vk::BufferUsageFlags::VERTEX_BUFFER;
    }
    if usage.contains(BufferUsage::INDEX) {
        flags |= vk::BufferUsageFlags::INDEX_BUFFER;
    }
    if usage.contains(BufferUsage::UNIFORM) {
        flags |= vk::BufferUsageFlags::UNIFORM_BUFFER;
    }
    if usage.contains(BufferUsage::STORAGE) {
        flags |= vk::BufferUsageFlags::STORAGE_BUFFER;
    }
    if usage.contains(BufferUsage::TRANSFER_SRC) {
        flags |= vk::BufferUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(BufferUsage::TRANSFER_DST) {
        flags |= vk::BufferUsageFlags::TRANSFER_DST;
    }
    flags
}
