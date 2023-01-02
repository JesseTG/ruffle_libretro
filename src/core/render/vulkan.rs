use std::error::Error;
use std::ffi::{c_char, c_void, CStr};
use std::mem::transmute;
use std::ptr;
use std::sync::Arc;

use ash::vk::{
    ApplicationInfo, Format, Image, ImageLayout, ImageViewCreateInfo, ImageViewType, StaticFn, TaggedStructure,
};
use ash::{vk, Device, Entry, Instance};
use futures::executor::block_on;
use log::debug;
use ruffle_core::Player;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro::environment;
use rust_libretro_sys::retro_hw_render_context_negotiation_interface_type::*;
use rust_libretro_sys::retro_hw_render_interface_type::RETRO_HW_RENDER_INTERFACE_VULKAN;
use rust_libretro_sys::vulkan::VkApplicationInfo;
use rust_libretro_sys::{
    retro_environment_t, retro_hw_render_context_negotiation_interface_vulkan, retro_hw_render_interface_type,
    retro_hw_render_interface_vulkan, retro_vulkan_image, RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan as VulkanApi;

use crate::core::render::vulkan::VulkanRenderStateError::*;
use crate::core::render::RenderState;

pub const VULKAN_VERSION: u32 = vk::API_VERSION_1_3;

#[derive(ThisError, Debug)]
pub enum VulkanRenderStateError {
    #[error("Frontend does not recognize RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE environment callback")]
    UnknownEnvironmentCallback,

    #[error("Frontend provided a null pointer for the render interface")]
    RenderInterfaceWasNull,

    #[error("Failed to get retro_hw_render_interface_vulkan from libretro")]
    FailedToGetRenderInterface,

    #[error("Failed to get retro_hw_render_context_negotiation_interface_vulkan from libretro")]
    FailedToGetRenderContextNegotiationInterface,

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
        let entry = Entry::from_static_fn(static_fn);
        let instance = Instance::load(entry.static_fn(), interface.instance);
        let device = Device::load(instance.fp_v1_0(), interface.device);
        let extensions: Vec<&CStr> = match entry.enumerate_instance_extension_properties(None) {
            Ok(properties) => properties
                .iter()
                .map(|p| CStr::from_ptr(p.extension_name.as_ptr()))
                .collect(),
            Err(error) => Err(VulkanError("vkEnumerateDeviceExtensionProperties", error))?,
        };
        debug!("{extensions:#?}");

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

    pub unsafe extern "C" fn get_application_info() -> *const VkApplicationInfo {
        &Self::APPLICATION_INFO
    }

    // Not using ApplicationInfoBuilder because it complicates lifetimes;
    // thankfully, this struct doesn't need to exist after
    // retro_hw_render_context_negotiation_interface_vulkan.get_application_info is done with it.
    const APPLICATION_INFO: ApplicationInfo = ApplicationInfo {
        s_type: ApplicationInfo::STRUCTURE_TYPE,
        p_next: ptr::null(),
        p_application_name: b"ruffle_libretro\x00" as *const u8 as *const c_char, // TODO: Write a macro to extract this from built_info
        application_version: 0,
        p_engine_name: ptr::null(),
        engine_version: 0,
        api_version: VULKAN_VERSION,
    };
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
        let create_info = ImageViewCreateInfo::builder()
            .image(image)
            .view_type(ImageViewType::TYPE_2D)
            .format(Format::R8G8B8A8_UNORM)
            .build();
        let image_view = unsafe { self.device.create_image_view(&create_info, None)? };

        self.image = Some(retro_vulkan_image {
            image_view,
            image_layout: ImageLayout::GENERAL,
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
