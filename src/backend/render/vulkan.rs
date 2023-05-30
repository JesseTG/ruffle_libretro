use std::borrow::Cow;
use std::error::Error;
use std::sync::Arc;

use gc_arena::MutationContext;
use log::{debug, warn};
use ruffle_core::swf::Glyph;
use ruffle_core::Color;
use ruffle_render::backend::{Context3D, Context3DCommand, RenderBackend, ShapeHandle, ViewportDimensions};
use ruffle_render::bitmap::{Bitmap, BitmapHandle, BitmapSource, SyncHandle, PixelRegion};
use ruffle_render::commands::CommandList;
use ruffle_render::error::Error as RuffleError;
use ruffle_render::filters::Filter;
use ruffle_render::quality::StageQuality;
use ruffle_render::shape_utils::DistilledShape;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use rust_libretro_sys::{retro_game_geometry, retro_hw_render_interface_vulkan};
use wgpu_hal::api::Vulkan;

use crate::backend::render::vulkan::render_interface::VulkanRenderInterface;

use self::target::RetroTextureTarget;
use self::util::create_descriptors;

mod global;
pub mod negotiation;
pub mod render_interface;
mod target;
mod util;

pub struct VulkanWgpuRenderBackend {
    backend: WgpuRenderBackend<RetroTextureTarget>,
    interface: VulkanRenderInterface,
    descriptors: Arc<Descriptors>,
}

impl VulkanWgpuRenderBackend {
    pub fn new(
        geometry: &retro_game_geometry,
        hw_render: &retro_hw_render_interface_vulkan,
    ) -> Result<Self, Box<dyn Error>> {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::new");
        let interface = VulkanRenderInterface::new(hw_render)?;

        unsafe {
            let instance = global::INSTANCE.as_ref().unwrap();
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
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::set_viewport_dimensions");
        self.backend.set_viewport_dimensions(dimensions)
    }

    fn register_shape(&mut self, shape: DistilledShape, bitmap_source: &dyn BitmapSource) -> ShapeHandle {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::register_shape");
        self.backend.register_shape(shape, bitmap_source)
    }

    fn render_offscreen(
        &mut self,
        handle: BitmapHandle,
        commands: CommandList,
        quality: StageQuality,
        bounds: PixelRegion,
    ) -> Option<Box<(dyn SyncHandle)>> {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::render_offscreen");
        self.backend.render_offscreen(handle, commands, quality, bounds)
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
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::apply_filter");
        self.backend
            .apply_filter(source, source_point, source_size, destination, dest_point, filter)
    }

    fn is_filter_supported(&self, _filter: &Filter) -> bool {
        false
    }

    fn submit_frame(&mut self, clear: Color, commands: CommandList) {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::submit_frame");
        self.backend.submit_frame(clear, commands);
        let target = self.backend.target();
        let queue_index = self.interface.queue_index();
        self.interface.set_image(target.get_retro_image(), &[], queue_index);
    }

    fn register_bitmap(&mut self, bitmap: Bitmap) -> Result<BitmapHandle, RuffleError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::register_bitmap");
        self.backend.register_bitmap(bitmap)
    }

    fn update_texture(
        &mut self,
        handle: &BitmapHandle,
        bitmap: Bitmap,
        region: PixelRegion
    ) -> Result<(), RuffleError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::update_texture");
        self.backend.update_texture(handle, bitmap, region)
    }

    fn create_context3d(&mut self) -> Result<Box<dyn Context3D>, RuffleError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::create_context3d");
        self.backend.create_context3d()
    }

    fn context3d_present<'gc>(
        &mut self,
        context: &mut dyn Context3D
    ) -> Result<(), RuffleError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::context3d_present");
        self.backend.context3d_present(context)
    }

    fn debug_info(&self) -> Cow<'static, str> {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::debug_info");
        self.backend.debug_info()
    }

    fn set_quality(&mut self, quality: StageQuality) {
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::set_quality");
        self.backend.set_quality(quality)
    }

    fn name(&self) -> &'static str {
        "Vulkan (wgpu)"
    }
}

impl Drop for VulkanWgpuRenderBackend {
    fn drop(&mut self) {
        debug!("VulkanWgpuRenderBackend::drop");
        #[cfg(feature = "profiler")]
        profiling::scope!("VulkanWgpuRenderBackend::drop");
        unsafe {
            if global::DEVICE.is_none() {
                return;
            }

            {
                let device = global::DEVICE.as_ref().unwrap();
                {
                    #[cfg(feature = "profiler")]
                    profiling::scope!("vkDeviceWaitIdle");
                    if let Err(e) = device.device_wait_idle() {
                        warn!("vkDeviceWaitIdle({:?}) failed with {e}", device.handle());
                    }
                }

                self.interface.wait_sync_index();
                let device = &self.descriptors.device;
                let device = device.as_hal::<Vulkan, _, _>(|c| c.unwrap().raw_device().clone());
                device.destroy_image_view(self.backend.target().get_image_view(), None);
                // Do *not* destroy the VkImage associated with this VkImageView; we didn't create it, wgpu did
            } // Scoped to prevent misuse after being dropped

            // Also, don't destroy the underlying VkInstance or VkDevice.
            // We created them, but RetroArch took ownership of them,
            // so it's responsible for cleanup.

            global::DEVICE = None;
            global::INSTANCE = None;
            global::ENTRY = None;

            #[cfg(debug_assertions)]
            {
                global::DEBUG_UTILS = None;
            }
        }
    }
}
