use std::ffi::c_void;
use std::mem::transmute;
use std::ptr;

use ash::{
    vk,
    vk::{Semaphore, StaticFn},
    Entry, Instance,
};
use rust_libretro::environment;
use rust_libretro_sys::retro_hw_render_interface_type::RETRO_HW_RENDER_INTERFACE_VULKAN;
use rust_libretro_sys::{
    retro_environment_t, retro_hw_render_interface_vulkan, retro_vulkan_image,
    RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE,
};

use crate::backend::render::vulkan::context::RetroVulkanCreatedContext;
use crate::backend::render::vulkan::negotiation::VulkanContextNegotiationInterface;
use crate::backend::render::HardwareRenderError;
use crate::backend::render::HardwareRenderError::*;

pub struct VulkanRenderInterface {
    // We don't own this
    interface: *const retro_hw_render_interface_vulkan,
    created_context: RetroVulkanCreatedContext,
}

impl VulkanRenderInterface {
    pub fn new(
        environ_cb: retro_environment_t,
        negotiation_interface: &VulkanContextNegotiationInterface,
    ) -> Result<Self, HardwareRenderError> {
        unsafe {
            let interface = environment::get_unchecked::<*const retro_hw_render_interface_vulkan>(
                environ_cb,
                RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE,
            );

            let interface = match interface {
                Some((_, false)) => Err(InvalidEnvironmentCallback)?,
                Some((ptr, true)) if ptr.is_null() => Err(NullRenderInterface(RETRO_HW_RENDER_INTERFACE_VULKAN))?,
                Some((ptr, true)) if (*ptr).interface_type != RETRO_HW_RENDER_INTERFACE_VULKAN => {
                    Err(WrongRenderInterfaceType(RETRO_HW_RENDER_INTERFACE_VULKAN, (*ptr).interface_type))?
                }
                Some((ptr, true)) => ptr,
                _ => Err(FailedToGetRenderInterface(RETRO_HW_RENDER_INTERFACE_VULKAN))?,
            };

            let get_instance_proc_addr = (*interface).get_instance_proc_addr.unwrap();
            let instance = (*interface).instance;
            let static_fn = StaticFn::load(|sym| {
                let fun = get_instance_proc_addr(instance, sym.as_ptr());
                fun.unwrap_or(transmute::<*const c_void, unsafe extern "system" fn()>(ptr::null())) as *const c_void
            });

            let entry = Entry::from_static_fn(static_fn);
            let instance = Instance::load(entry.static_fn(), instance);
            let created_context = match negotiation_interface.created_context.as_ref() {
                Some(created_context) => created_context.clone(),
                None => {
                    // The context negotiation interface didn't create a device,
                    // so we'll create a wrapper around the device that the render interface gave us
                    let device = ash::Device::load(instance.fp_v1_0(), (*interface).device);
                    RetroVulkanCreatedContext {
                        entry,
                        instance,
                        physical_device: (*interface).gpu,
                        device,
                        queue: (*interface).queue,
                        queue_family_index: (*interface).queue_index,
                        presentation_queue: (*interface).queue,
                        presentation_queue_family_index: (*interface).queue_index,
                    }
                }
            };

            Ok(Self {
                interface,
                created_context,
            })
        }
    }

    pub fn created_context(&self) -> &RetroVulkanCreatedContext {
        &self.created_context
    }

    pub fn set_image(
        &self,
        image: &retro_vulkan_image,
        semaphores: &[Semaphore],
        src_queue_family: u32,
    ) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let set_image = interface.set_image.ok_or(NullInterfaceFunction("set_image"))?;
            let ptr = if !semaphores.is_empty() {
                semaphores.as_ptr()
            } else {
                ptr::null()
            };

            set_image(interface.handle, image, semaphores.len() as u32, ptr, src_queue_family);
            Ok(())
        }
    }

    pub fn get_sync_index(&self) -> Result<u32, HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let get_sync_index = interface
                .get_sync_index
                .ok_or(NullInterfaceFunction("get_sync_index"))?;

            Ok(get_sync_index(interface.handle))
        }
    }

    pub fn get_sync_index_mask(&self) -> Result<u32, HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let get_sync_index_mask = interface
                .get_sync_index_mask
                .ok_or(NullInterfaceFunction("get_sync_index_mask"))?;

            Ok(get_sync_index_mask(interface.handle))
        }
    }

    pub fn wait_sync_index(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let wait_sync_index = interface
                .wait_sync_index
                .ok_or(NullInterfaceFunction("wait_sync_index"))?;

            wait_sync_index(interface.handle);
            Ok(())
        }
    }

    pub fn lock_queue(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let lock_queue = interface.lock_queue.ok_or(NullInterfaceFunction("lock_queue"))?;

            lock_queue(interface.handle);
            Ok(())
        }
    }

    pub fn unlock_queue(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let unlock_queue = interface.unlock_queue.ok_or(NullInterfaceFunction("unlock_queue"))?;

            unlock_queue(interface.handle);
            Ok(())
        }
    }

    pub fn set_command_buffers(&self, cmd: &[vk::CommandBuffer]) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let set_command_buffers = interface
                .set_command_buffers
                .ok_or(NullInterfaceFunction("set_command_buffers"))?;
            let ptr = if !cmd.is_empty() { cmd.as_ptr() } else { ptr::null() };

            set_command_buffers(interface.handle, cmd.len() as u32, ptr);
            Ok(())
        }
    }

    pub fn set_signal_semaphore(&self, semaphore: vk::Semaphore) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let set_signal_semaphore = interface
                .set_signal_semaphore
                .ok_or(NullInterfaceFunction("set_signal_semaphore"))?;

            set_signal_semaphore(interface.handle, semaphore);
            Ok(())
        }
    }
}
