use std::error::Error;
use std::ffi::{c_void, CStr};
use std::mem::transmute;
use std::ptr;
use std::sync::Arc;

use ash::vk::{Image, ImageLayout, ImageViewCreateInfo, StaticFn};
use ash::{Device, Instance};
use futures::executor::block_on;
use log::debug;
use ruffle_core::Player;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro::environment;
use rust_libretro_sys::retro_hw_render_interface_type::RETRO_HW_RENDER_INTERFACE_VULKAN;
use rust_libretro_sys::{
    retro_environment_t, retro_hw_render_interface_type, retro_hw_render_interface_vulkan, retro_vulkan_image,
    RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan as VulkanApi;

use crate::core::render::vulkan::VulkanRenderStateError::*;
use crate::core::render::RenderState;

#[derive(ThisError, Debug)]
pub enum VulkanRenderStateError {
    #[error("Frontend does not recognize RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE environment callback")]
    UnknownEnvironmentCallback,

    #[error("Frontend provided a null pointer for the render interface")]
    RenderInterfaceWasNull,

    #[error("Failed to get retro_hw_render_interface_vulkan from libretro")]
    FailedToGetRenderInterface,

    #[error("Expected a render interface of type RETRO_HW_RENDER_INTERFACE_VULKAN, got {0:?}")]
    WrongInterfaceType(retro_hw_render_interface_type),

    #[error("Expected a render backend of type WgpuRenderBackend<TextureTarget>")]
    WrongRenderBackendType,

    #[error("Vulkan error in {0}: {1}")]
    VulkanError(&'static str, ash::vk::Result),
}

pub(crate) struct VulkanRenderState {
    interface: retro_hw_render_interface_vulkan,

    instance: Instance,
    device: Device,
    descriptors: Arc<Descriptors>,

    // Had to make this an option to break a dependency loop.
    // Originally this struct needed a target, which needed descriptors, which needed this struct...
    image: Option<retro_vulkan_image>,
}

impl VulkanRenderState {
    pub unsafe fn new(environ_cb: retro_environment_t) -> Result<Self, Box<dyn Error>> {
        let interface = environment::get_unchecked::<*mut retro_hw_render_interface_vulkan>(
            environ_cb,
            RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE,
        );

        let interface = match interface {
            Some((_, false)) => Err(UnknownEnvironmentCallback)?,
            Some((ptr, true)) if ptr.is_null() => Err(RenderInterfaceWasNull)?,
            Some((ptr, true)) if (*ptr).interface_type != RETRO_HW_RENDER_INTERFACE_VULKAN => {
                Err(WrongInterfaceType((*ptr).interface_type))?
            }
            Some((ptr, true)) => &*ptr,
            _ => Err(FailedToGetRenderInterface)?,
        };

        let static_fn = StaticFn::load(|sym| {
            let fun = (interface.get_instance_proc_addr)(interface.instance, sym.as_ptr());
            fun.unwrap_or(transmute::<*const c_void, unsafe extern "system" fn()>(ptr::null())) as *const c_void
        });
        let instance = Instance::load(&static_fn, interface.instance);
        let device = Device::load(instance.fp_v1_0(), interface.device);
        let extensions: Vec<&CStr> = match instance.enumerate_device_extension_properties(interface.gpu) {
            Ok(properties) => properties
                .iter()
                .map(|p| CStr::from_ptr(p.extension_name.as_ptr()))
                .collect(),
            Err(error) => Err(VulkanError("vkEnumerateDeviceExtensionProperties", error))?,
        };

        let descriptors = WgpuRenderBackend::<TextureTarget>::build_descriptors_for_vulkan(
            interface.gpu,
            device.clone(),
            false,
            extensions.as_slice(),
            wgpu::Features::all_native_mask(), // TODO: Populate this properly
            wgpu_hal::UpdateAfterBindTypes::all(),
            interface.queue_index, // I think this field is misnamed
            0,
            None,
        );

        let descriptors = match block_on(descriptors) {
            Ok(descriptors) => Arc::new(descriptors),
            Err(error) => Err(error)?,
        };

        Ok(Self {
            interface: *interface,
            instance,
            device,
            descriptors,
            image: None,
        })
    }
}

impl RenderState for VulkanRenderState {
    fn descriptors(&self) -> Arc<Descriptors> {
        self.descriptors.clone()
    }

    fn render(&mut self, player: &mut Player) -> Result<(), Box<dyn Error>> {
        player.render(); // First, render to the texture

        if let Some(image) = &self.image {
            unsafe {
                (self.interface.set_image.ok_or("Error")?)(self.interface.handle, image, 0, ptr::null(), 0);
            }
        }
        // Now render to the screen

        Ok(())
    }

    fn reset(&mut self) -> Result<(), Box<dyn Error>> {
        todo!()
    }

    fn set_target(&mut self, target: &TextureTarget) -> Result<(), Box<dyn Error>> {
        let image = unsafe {
            let mut texture: Option<Image> = None;
            target.texture.as_hal::<VulkanApi, _>(|t| {
                texture = t.map(|t| t.raw_handle());
            });
            texture.ok_or("Texture must exist in Vulkan HAL")?
        }; // Don't free this, it belongs to wgpu
        let create_info = ImageViewCreateInfo::builder().image(image).build();
        let image_view = unsafe { self.device.create_image_view(&create_info, None)? };

        self.image = Some(retro_vulkan_image {
            image_view,
            image_layout: ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            create_info,
        });

        Ok(())
    }
}

impl Drop for VulkanRenderState {
    fn drop(&mut self) {
        if let Some(image) = &self.image {
            unsafe {
                self.device.destroy_image_view(image.image_view, None);
            } // Do *not* destroy the image associated with the image view; we didn't create it, wgpu did.

            // Also, don't destroy self.device or self.instance; we didn't create them, RetroArch did.
        };
        debug!("Dropping VulkanRenderState");
    }
}
