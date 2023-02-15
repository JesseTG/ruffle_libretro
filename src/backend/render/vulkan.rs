use std::borrow::Cow;
use std::error::Error;
use std::ffi::CStr;
use std::sync::Arc;

use ash::vk;
use ash::vk::{
    Format, Image, ImageAspectFlags, ImageLayout, ImageSubresourceRange, ImageViewCreateInfo, ImageViewType,
};
use gc_arena::MutationContext;
use log::debug;
use ruffle_core::swf::Glyph;
use ruffle_core::Color;
use ruffle_render::backend::{Context3D, Context3DCommand, RenderBackend, ShapeHandle, ViewportDimensions};
use ruffle_render::bitmap::{Bitmap, BitmapHandle, BitmapSource, SyncHandle};
use ruffle_render::commands::CommandList;
use ruffle_render::error::Error as RuffleError;
use ruffle_render::filters::Filter;
use ruffle_render::quality::StageQuality;
use ruffle_render::shape_utils::DistilledShape;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro::anyhow;
use rust_libretro_sys::{
    retro_environment_t, retro_game_geometry, retro_hw_render_interface_vulkan, retro_vulkan_image,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;
use wgpu_hal::{Api, ExposedAdapter, InstanceFlags, OpenDevice};

type VulkanInstance = <Vulkan as Api>::Instance;
type VulkanDevice = <Vulkan as Api>::Device;
type VulkanPhysicalDevice = <Vulkan as Api>::Adapter;
type VulkanQueue = <Vulkan as Api>::Queue;
type VulkanPhysicalDeviceInfo = ExposedAdapter<Vulkan>;
type VulkanOpenDevice = OpenDevice<Vulkan>;
use crate::backend::render::required_limits;
use crate::backend::render::vulkan::render_interface::VulkanRenderInterface;

pub mod context;
pub mod render_interface;
mod util;

#[derive(ThisError, Debug)]
pub enum VulkanRenderBackendError {
    #[error("Vulkan error in {0}: {1}")]
    VulkanError(&'static str, ash::vk::Result),
}

pub struct VulkanWgpuRenderBackend {
    backend: WgpuRenderBackend<TextureTarget>,
    interface: VulkanRenderInterface,
    descriptors: Arc<Descriptors>,
    image: retro_vulkan_image,
}

impl VulkanWgpuRenderBackend {
    pub fn new(
        environ_cb: retro_environment_t,
        geometry: &retro_game_geometry,
        hw_render: &retro_hw_render_interface_vulkan,
    ) -> Result<Self, Box<dyn Error>> {
        let interface = unsafe { VulkanRenderInterface::new(hw_render)? };
        let descriptors = create_descriptors(&interface)?;
        let (width, height) = (geometry.base_width, geometry.base_height);
        let target = TextureTarget::new(&descriptors.device, (width, height))?;
        let descriptors = Arc::new(descriptors);
        // Create a VkImage that will be used to render the emulator's output.
        // Don't free it manually, it belongs to wgpu!
        let image = unsafe {
            let mut texture: Option<Image> = None;
            target.texture.as_hal::<Vulkan, _>(|t| {
                texture = t.map(|t| t.raw_handle());
            });
            texture.ok_or("Texture must exist in Vulkan HAL")?
        }; // Don't free this, it belongs to wgpu

        let backend = WgpuRenderBackend::new(descriptors.clone(), target)?;
        let subresource_range = ImageSubresourceRange::builder()
            .aspect_mask(ImageAspectFlags::COLOR)
            .level_count(vk::REMAINING_MIP_LEVELS)
            .layer_count(vk::REMAINING_ARRAY_LAYERS)
            .build();

        let create_info = ImageViewCreateInfo::builder()
            .image(image)
            .view_type(ImageViewType::TYPE_2D)
            .format(Format::R8G8B8A8_UNORM)
            .subresource_range(subresource_range)
            .build();

        let image_view = unsafe { interface.device().create_image_view(&create_info, None)? };

        let image = retro_vulkan_image {
            image_view,
            image_layout: ImageLayout::TRANSFER_DST_OPTIMAL,
            create_info,
        };

        Ok(Self {
            backend,
            interface,
            descriptors,
            image,
        })
    }
}

impl RenderBackend for VulkanWgpuRenderBackend {
    fn viewport_dimensions(&self) -> ViewportDimensions {
        self.backend.viewport_dimensions()
    }

    fn set_viewport_dimensions(&mut self, dimensions: ViewportDimensions) {
        self.backend.set_viewport_dimensions(dimensions)
    }

    fn register_shape(&mut self, shape: DistilledShape, bitmap_source: &dyn BitmapSource) -> ShapeHandle {
        self.backend.register_shape(shape, bitmap_source)
    }

    fn replace_shape(&mut self, shape: DistilledShape, bitmap_source: &dyn BitmapSource, handle: ShapeHandle) {
        self.backend.replace_shape(shape, bitmap_source, handle)
    }

    fn register_glyph_shape(&mut self, shape: &Glyph) -> ShapeHandle {
        self.backend.register_glyph_shape(shape)
    }

    fn render_offscreen(
        &mut self,
        handle: BitmapHandle,
        width: u32,
        height: u32,
        commands: CommandList,
    ) -> Option<Box<(dyn SyncHandle + 'static)>> {
        self.backend.render_offscreen(handle, width, height, commands)
    }

    fn apply_filter(
        &mut self,
        source: BitmapHandle,
        source_point: (u32, u32),
        source_size: (u32, u32),
        destination: BitmapHandle,
        dest_point: (u32, u32),
        filter: Filter,
    ) -> Option<Box<dyn SyncHandle>> {
        self.backend
            .apply_filter(source, source_point, source_size, destination, dest_point, filter)
    }

    fn submit_frame(&mut self, clear: Color, commands: CommandList) {
        self.backend.submit_frame(clear, commands)
    }

    fn register_bitmap(&mut self, bitmap: Bitmap) -> Result<BitmapHandle, RuffleError> {
        self.backend.register_bitmap(bitmap)
    }

    fn update_texture(
        &mut self,
        bitmap: &BitmapHandle,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), RuffleError> {
        self.backend.update_texture(bitmap, width, height, rgba)
    }

    fn create_context3d(&mut self) -> Result<Box<dyn Context3D>, RuffleError> {
        self.backend.create_context3d()
    }

    fn context3d_present<'gc>(
        &mut self,
        context: &mut dyn Context3D,
        commands: Vec<Context3DCommand<'gc>>,
        mc: MutationContext<'gc, '_>,
    ) -> Result<(), RuffleError> {
        self.backend.context3d_present(context, commands, mc)
    }

    fn debug_info(&self) -> Cow<'static, str> {
        self.backend.debug_info()
    }

    fn set_quality(&mut self, quality: StageQuality) {
        self.backend.set_quality(quality)
    }
}

impl Drop for VulkanWgpuRenderBackend {
    fn drop(&mut self) {
        unsafe {
            self.interface.device().destroy_image_view(self.image.image_view, None);
        } // Do *not* destroy the VkImage associated with this VkImageView; we didn't create it, wgpu did.

        // Also, don't destroy self.device or self.instance;
        // we created them, but RetroArch took ownership of them,
        // so it's responsible for cleanup.
    }
}

pub fn create_descriptors(interface: &VulkanRenderInterface) -> anyhow::Result<Descriptors> {
    let entry = interface.entry();
    let instance = interface.instance();
    let physical_device = interface.physical_device();
    let device = interface.device();
    
    let driver_api_version = entry
        .try_enumerate_instance_version()?
        .unwrap_or(vk::API_VERSION_1_0);
    // vkEnumerateInstanceVersion isn't available in Vulkan 1.0

    let flags = if cfg!(debug_assertions) {
        InstanceFlags::VALIDATION | InstanceFlags::DEBUG
    } else {
        InstanceFlags::empty()
    }; // Logic taken from `VulkanHalInstance::init`

    let instance_extensions = VulkanInstance::required_extensions(entry, 0, flags)?;
    debug!("Instance extensions required by wgpu: {instance_extensions:#?}");

    let has_nv_optimus = unsafe {
        let instance_layers = entry.enumerate_instance_layer_properties()?;
        let nv_optimus_layer = CStr::from_bytes_with_nul(b"VK_LAYER_NV_optimus\0")?;
        instance_layers
            .iter()
            .any(|inst_layer| CStr::from_ptr(inst_layer.layer_name.as_ptr()) == nv_optimus_layer)
    };

    let physical_device_properties = unsafe { instance.get_physical_device_properties(physical_device) };
    let instance = unsafe {
        VulkanInstance::from_raw(
            entry.clone(),
            instance.clone(),
            driver_api_version,
            util::get_android_sdk_version()?,
            instance_extensions,
            flags,
            has_nv_optimus,
            None,
            // None indicates that wgpu is *not* responsible for destroying the VkInstance
            // (in this case, that falls on the libretro frontend)
        )?
    };

    let adapter = instance
        .expose_adapter(physical_device)
        .ok_or(anyhow::anyhow!("Failed to expose physical device {physical_device:?}"))?;

    let open_device = unsafe {
        let device_extensions = adapter.adapter.required_device_extensions(adapter.features);
        adapter.adapter.device_from_raw(
            device.clone(),
            false,
            &device_extensions,
            adapter.features,
            interface.queue_family_index(),
            0, // wgpu assumes this to be 0
        )?
    };

    let instance = unsafe { wgpu::Instance::from_hal::<Vulkan>(instance) };
    let adapter = unsafe { instance.create_adapter_from_hal(adapter) };
    let (limits, features) = required_limits(&adapter);
    let (device, queue) = unsafe {
        adapter.create_device_from_hal(
            open_device,
            &wgpu::DeviceDescriptor {
                label: None,
                features,
                limits,
            },
            None,
        )?
    };

    Ok(Descriptors::new(adapter, device, queue))
}
