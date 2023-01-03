use std::ffi::c_void;
use std::mem::transmute;
use std::ptr;

use ash::{Entry, Instance, vk, vk::{Semaphore, StaticFn}};
use rust_libretro::environment;
use rust_libretro_sys::{
    RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE, retro_environment_t, retro_hw_render_interface_vulkan,
    retro_vulkan_image,
};
use rust_libretro_sys::retro_hw_render_interface_type::RETRO_HW_RENDER_INTERFACE_VULKAN;

use crate::backend::render::HardwareRenderError;
use crate::backend::render::HardwareRenderError::*;
use crate::backend::render::vulkan::negotiation::VulkanContextNegotiationInterface;

pub struct VulkanRenderInterface {
    // We don't own this
    interface: *const retro_hw_render_interface_vulkan,
    entry: Entry,
    instance: Instance,
    device: ash::Device,
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
                Some((ptr, true)) if (*ptr).interface_type != RETRO_HW_RENDER_INTERFACE_VULKAN => Err(
                    WrongRenderInterfaceType(RETRO_HW_RENDER_INTERFACE_VULKAN, (*ptr).interface_type),
                )?,
                Some((ptr, true)) => ptr,
                _ => Err(FailedToGetRenderInterface(RETRO_HW_RENDER_INTERFACE_VULKAN))?,
            };

            let get_instance_proc_addr = (*interface).get_instance_proc_addr;
            let instance = (*interface).instance;
            let static_fn = StaticFn::load(|sym| {
                let fun = get_instance_proc_addr(instance, sym.as_ptr());
                fun.unwrap_or(transmute::<*const c_void, unsafe extern "system" fn()>(ptr::null())) as *const c_void
            });

            let entry = Entry::from_static_fn(static_fn);
            let instance = Instance::load(entry.static_fn(), instance);
            let device = match negotiation_interface.device() {
                Some(device) => {
                    assert_eq!((*interface).device, device.handle());
                    device.clone()
                }
                None => {
                    // The context negotiation interface didn't create a device,
                    // so we'll create a wrapper around the device that the render interface gave us

                    ash::Device::load(instance.fp_v1_0(), (*interface).device)
                }
            };

            Ok(Self {
                interface,
                device,
                instance,
                entry,
            })
        }
    }

    pub fn gpu(&self) -> vk::PhysicalDevice {
        unsafe { (*self.interface).gpu }
    }

    pub fn device(&self) -> &ash::Device {
        &self.device
    }

    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn queue_index(&self) -> u32 {
        unsafe { (*self.interface).queue_index }
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
            let ptr = if semaphores.len() > 0 {
                semaphores.as_ptr()
            } else {
                ptr::null()
            };

            Ok(set_image(
                interface.handle,
                image,
                semaphores.len() as u32,
                ptr,
                src_queue_family,
            ))
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

            Ok(wait_sync_index(interface.handle))
        }
    }

    pub fn lock_queue(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let lock_queue = interface.lock_queue.ok_or(NullInterfaceFunction("lock_queue"))?;

            Ok(lock_queue(interface.handle))
        }
    }

    pub fn unlock_queue(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let unlock_queue = interface.unlock_queue.ok_or(NullInterfaceFunction("unlock_queue"))?;

            Ok(unlock_queue(interface.handle))
        }
    }

    pub fn set_command_buffers(&self, cmd: &[vk::CommandBuffer]) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let set_command_buffers = interface
                .set_command_buffers
                .ok_or(NullInterfaceFunction("set_command_buffers"))?;
            let ptr = if cmd.len() > 0 { cmd.as_ptr() } else { ptr::null() };

            Ok(set_command_buffers(interface.handle, cmd.len() as u32, ptr))
        }
    }

    pub fn set_signal_semaphore(&self, semaphore: vk::Semaphore) -> Result<(), HardwareRenderError> {
        unsafe {
            let interface = &*self.interface;
            let set_signal_semaphore = interface
                .set_signal_semaphore
                .ok_or(NullInterfaceFunction("set_signal_semaphore"))?;

            Ok(set_signal_semaphore(interface.handle, semaphore))
        }
    }

    pub fn reset_context(&mut self, environ_cb: retro_environment_t) {
        todo!()
    }
}
