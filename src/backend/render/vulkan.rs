use std::borrow::Cow;
use std::error::Error;
use std::sync::Arc;

use gc_arena::MutationContext;
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
use rust_libretro_sys::{
    retro_environment_t, retro_game_geometry, retro_hw_render_interface_vulkan,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;

use crate::backend::render::vulkan::render_interface::VulkanRenderInterface;

use self::negotiation::INSTANCE;
use self::target::RetroTextureTarget;
use self::util::create_descriptors;

pub mod negotiation;
pub mod render_interface;
mod target;
mod util;

#[derive(ThisError, Debug)]
pub enum VulkanRenderBackendError {
    #[error("Vulkan error in {0}: {1}")]
    VulkanError(&'static str, ash::vk::Result),
}

pub struct VulkanWgpuRenderBackend {
    backend: WgpuRenderBackend<RetroTextureTarget>,
    interface: VulkanRenderInterface,
    descriptors: Arc<Descriptors>,
}

impl VulkanWgpuRenderBackend {
    pub fn new(
        environ_cb: retro_environment_t,
        geometry: &retro_game_geometry,
        hw_render: &retro_hw_render_interface_vulkan,
    ) -> Result<Self, Box<dyn Error>> {
        let interface = VulkanRenderInterface::new(hw_render)?;

        unsafe {
            let instance = INSTANCE.as_ref().unwrap();
            let descriptors = create_descriptors(instance, &interface)?;
            let (width, height) = (geometry.base_width, geometry.base_height);
            let target =
                RetroTextureTarget::new(&descriptors.device, (width, height), wgpu::TextureFormat::Rgba8Unorm)?;
            let descriptors = Arc::new(descriptors);
            let backend = WgpuRenderBackend::new(descriptors.clone(), target)?;

            Ok(Self {
                backend,
                interface,
                descriptors,
            })
        }
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
        quality: StageQuality,
    ) -> Option<Box<(dyn SyncHandle + 'static)>> {
        self.backend.render_offscreen(handle, width, height, commands, quality)
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
        self.backend.submit_frame(clear, commands);
        let target = self.backend.target();
        let queue_index = self.interface.queue_index();
        self.interface.set_image(target.get_retro_image(), &[], queue_index);
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
            self.interface.wait_sync_index();
            let device = &self.descriptors.device;
            let device = device.as_hal::<Vulkan, _, _>(|c| c.unwrap().raw_device().clone());
            device.destroy_image_view(self.backend.target().get_image_view(), None);
        } // Do *not* destroy the VkImage associated with this VkImageView; we didn't create it, wgpu did

        // Also, don't destroy self.device or self.instance;
        // we created them, but RetroArch took ownership of them,
        // so it's responsible for cleanup.
    }
}
