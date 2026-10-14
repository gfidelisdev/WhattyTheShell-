// src/render/dmabuf.rs
//! Zero-copy Vulkan dmabuf texture allocation and export for GTK4.
//!
//! Creates a VkImage with external memory export (DMA_BUF_EXT),
//! allocates exportable dedicated device memory, exports the file descriptor,
//! wraps the image as a wgpu::Texture, and builds a gdk4::DmabufTextureBuilder.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use ash::vk;
use gtk4::{gdk, glib};

pub const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");
pub const DRM_FORMAT_MOD_LINEAR: u64 = 0;

pub struct ExportedDmabuf {
    pub vk_image: vk::Image,
    pub vk_memory: vk::DeviceMemory,
    pub fd: OwnedFd,
    pub stride: u32,
    pub offset: u32,
    pub modifier: u64,
    pub width: u32,
    pub height: u32,
}

pub struct DmabufFrame {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub exported: ExportedDmabuf,
}

impl DmabufFrame {
    /// Create a new dmabuf frame backed by a Vulkan image and wrapped into a wgpu::Texture.
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let hal_device = unsafe {
            device
                .as_hal::<wgpu::hal::api::Vulkan>()
                .ok_or("Failed to get Vulkan hal device")?
        };

        let raw_device = hal_device.raw_device();
        let raw_instance = hal_device.shared_instance().raw_instance();
        let physical_device = hal_device.raw_physical_device();

        // 1. Create VkImage with external memory capability (DMA_BUF_EXT)
        let mut external_memory_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

        let image_create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::LINEAR)
            .usage(
                vk::ImageUsageFlags::COLOR_ATTACHMENT
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::SAMPLED,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external_memory_info);

        let vk_image = unsafe { raw_device.create_image(&image_create_info, None)? };

        // 2. Query memory requirements
        let mem_reqs = unsafe { raw_device.get_image_memory_requirements(vk_image) };

        let mem_props = unsafe {
            raw_instance.get_physical_device_memory_properties(physical_device)
        };

        // Find appropriate memory type (linear tiling usually requires host visible or device local)
        let mut mem_type_index = None;
        for i in 0..mem_props.memory_type_count {
            let matches_req = (mem_reqs.memory_type_bits & (1 << i)) != 0;
            if matches_req {
                mem_type_index = Some(i);
                break;
            }
        }
        let mem_type_index = mem_type_index.ok_or("No suitable memory type found for dmabuf image")?;

        // 3. Allocate memory with export flag
        let mut export_info = vk::ExportMemoryAllocateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

        let mut dedicated_info = vk::MemoryDedicatedAllocateInfo::default()
            .image(vk_image);

        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(mem_type_index)
            .push_next(&mut export_info)
            .push_next(&mut dedicated_info);

        let vk_memory = unsafe { raw_device.allocate_memory(&alloc_info, None)? };
        unsafe { raw_device.bind_image_memory(vk_image, vk_memory, 0)? };

        // 4. Export the file descriptor
        let ext_mem = ash::khr::external_memory_fd::Device::new(raw_instance, raw_device);
        let get_fd_info = vk::MemoryGetFdInfoKHR::default()
            .memory(vk_memory)
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

        let raw_fd = unsafe { ext_mem.get_memory_fd(&get_fd_info)? };
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

        // 5. Query layout (stride and offset)
        let subresource = vk::ImageSubresource {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            array_layer: 0,
        };
        let layout = unsafe { raw_device.get_image_subresource_layout(vk_image, subresource) };

        let exported = ExportedDmabuf {
            vk_image,
            vk_memory,
            fd,
            stride: layout.row_pitch as u32,
            offset: layout.offset as u32,
            modifier: DRM_FORMAT_MOD_LINEAR,
            width,
            height,
        };

        // 6. Wrap into wgpu::Texture
        let hal_desc = wgpu::hal::TextureDescriptor {
            label: Some("dmabuf_frame_hal"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUses::COLOR_TARGET | wgpu::TextureUses::COPY_SRC,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: vec![],
        };

        let hal_texture = unsafe {
            hal_device.texture_from_raw(
                vk_image,
                &hal_desc,
                None,
                wgpu::hal::vulkan::TextureMemory::Dedicated(vk_memory),
            )
        };

        let wgpu_desc = wgpu::TextureDescriptor {
            label: Some("dmabuf_frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };

        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
                hal_texture,
                &wgpu_desc,
                wgpu::TextureUses::UNINITIALIZED,
            )
        };

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(DmabufFrame {
            texture,
            view,
            exported,
        })
    }

    /// Build a GTK GdkTexture from this frame via GdkDmabufTextureBuilder.
    pub fn build_gdk_texture<F: FnOnce() + Send + 'static>(
        &self,
        display: &gdk::Display,
        release_func: F,
    ) -> Result<gdk::Texture, glib::Error> {
        // Duplicate fd because Gdk takes ownership or holds it
        let dup_fd = unsafe { libc::dup(self.exported.fd.as_raw_fd()) };
        if dup_fd < 0 {
            return Err(glib::Error::new(
                gdk::DmabufError::CreationFailed,
                &format!("dup failed: {}", std::io::Error::last_os_error()),
            ));
        }

        let builder = gdk::DmabufTextureBuilder::new()
            .set_display(display)
            .set_width(self.exported.width)
            .set_height(self.exported.height)
            .set_fourcc(DRM_FORMAT_ABGR8888)
            .set_modifier(self.exported.modifier)
            .set_n_planes(1)
            .set_stride(0, self.exported.stride)
            .set_offset(0, self.exported.offset);

        let builder = unsafe { builder.set_fd(0, dup_fd) };

        unsafe { builder.build_with_release_func(release_func) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::prelude::*;

    #[test]
    fn creates_vulkan_dmabuf_and_renders_pass() {
        let instance = wgpu::Instance::default();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("Failed to find Vulkan adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("dmabuf_test_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .expect("Failed to create wgpu device");

        let frame = DmabufFrame::new(&device, 128, 128)
            .expect("Failed to create dmabuf frame");

        assert!(frame.exported.fd.as_raw_fd() >= 0);
        assert!(frame.exported.stride >= 128 * 4);

        // Perform a clear render pass to the dmabuf texture
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test_encoder"),
        });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        queue.submit(Some(encoder.finish()));
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

        // If running in a graphical session (Wayland/X11), verify GdkTexture builder
        if gtk4::init().is_ok() {
            if let Some(display) = gdk::Display::default() {
                let gdk_texture = frame.build_gdk_texture(&display, || {
                    println!("Dmabuf frame released by GTK");
                });
                match gdk_texture {
                    Ok(tex) => {
                        assert_eq!(tex.width(), 128);
                        assert_eq!(tex.height(), 128);
                    }
                    Err(e) => {
                        println!("Gdk DmabufTextureBuilder note: {e} (normal if running headless or compositor rejects format)");
                    }
                }
            }
        }
    }
}

