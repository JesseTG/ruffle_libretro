use std::error::Error;
use std::ffi::CStr;
use std::sync::Arc;

use ash::vk;
use ash::vk::{
    Format, Image, ImageAspectFlags, ImageLayout, ImageSubresourceRange, ImageViewCreateInfo,
    ImageViewType,
};
use gc_arena::MutationContext;
use ruffle_core::Color;
use ruffle_core::swf::Glyph;
use ruffle_render::backend::{Context3D, Context3DCommand, RenderBackend, ShapeHandle, ViewportDimensions};
use ruffle_render::bitmap::{Bitmap, BitmapHandle, BitmapSource};
use ruffle_render::commands::CommandList;
use ruffle_render::error::Error as RuffleError;
use ruffle_render::shape_utils::DistilledShape;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro_sys::{
    retro_game_geometry,
    retro_vulkan_image,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan as VulkanApi;

use crate::backend::render::vulkan::render_interface::VulkanRenderInterface;
use crate::backend::render::vulkan::VulkanRenderError::VulkanError;

pub mod negotiation;
pub mod render_interface;

#[derive(ThisError, Debug)]
pub enum VulkanRenderError {
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
    pub async fn new(interface: VulkanRenderInterface, geometry: &retro_game_geometry) -> Result<Self, Box<dyn Error>> {
        let device = interface.device();
        let entry = interface.entry();
        let mut extensions: Vec<&CStr> = match entry.enumerate_instance_extension_properties(None) {
            Ok(properties) => properties
                .iter()
                .map(|p| unsafe { CStr::from_ptr(p.extension_name.as_ptr()) })
                .collect(),
            Err(error) => Err(VulkanError("vkEnumerateDeviceExtensionProperties", error))?,
        };

        // TODO: Implement the context negotiation interface
        extensions.push(ash::extensions::khr::Swapchain::name());
        extensions.push(ash::extensions::khr::TimelineSemaphore::name());

        let descriptors = unsafe {
            WgpuRenderBackend::<TextureTarget>::build_descriptors_for_vulkan(
                interface.gpu(),
                device.clone(),
                false,
                extensions.as_slice(),                 // TODO: Populate this properly
                wgpu::Features::all_native_mask(),     // TODO: Populate this properly
                wgpu_hal::UpdateAfterBindTypes::all(), // TODO: Populate this properly
                interface.queue_index(),               // I think this field is misnamed
                0,
                None,
            )
        }
        .await?;

        let (width, height) = (geometry.base_width, geometry.base_height);
        let descriptors = Arc::new(descriptors);
        let target = TextureTarget::new(&descriptors.device, (width, height))?;

        let image = unsafe {
            let mut texture: Option<Image> = None;
            target.texture.as_hal::<VulkanApi, _>(|t| {
                texture = t.map(|t| t.raw_handle());
            });
            texture.ok_or("Texture must exist in Vulkan HAL")?
        }; // Don't free this, it belongs to wgpu

        let backend = WgpuRenderBackend::new(descriptors.clone(), target)?;
        let subresource_range = ImageSubresourceRange::builder()
            .aspect_mask(ImageAspectFlags::COLOR | ImageAspectFlags::COLOR)
            .level_count(vk::REMAINING_MIP_LEVELS)
            .layer_count(vk::REMAINING_ARRAY_LAYERS)
            .build();

        let create_info = ImageViewCreateInfo::builder()
            .image(image)
            .view_type(ImageViewType::TYPE_2D)
            .format(Format::R8G8B8A8_UNORM)
            .subresource_range(subresource_range)
            .build();

        let image_view = unsafe { device.create_image_view(&create_info, None)? };

        let image = retro_vulkan_image {
            image_view,
            image_layout: ImageLayout::GENERAL,
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
    ) -> Result<Bitmap, RuffleError> {
        self.backend.render_offscreen(handle, width, height, commands)
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
}

impl Drop for VulkanWgpuRenderBackend {
    fn drop(&mut self) {
        unsafe {
            self.interface.device().destroy_image_view(self.image.image_view, None);
        } // Do *not* destroy the VkImage associated with this VkImageView; we didn't create it, wgpu did.

        // Also, don't destroy self.device or self.instance;
        // we didn't create the underlying VkDevice or VkInstance, RetroArch did.
    }
}
