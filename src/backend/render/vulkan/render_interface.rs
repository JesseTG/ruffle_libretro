use ash::{
    vk,
    vk::{Semaphore, StaticFn},
};
use libc::c_void;
use log::warn;
use std::ptr;
use std::{error::Error, ffi::CStr, mem::transmute};

use rust_libretro_sys::{
    retro_hw_render_interface_vulkan, retro_vulkan_image,
};

use crate::backend::render::HardwareRenderError;
use crate::backend::render::HardwareRenderError::*;

use super::context::{DEVICE, ENTRY, INSTANCE};

pub struct VulkanRenderInterface {
    // We don't own this
    interface: retro_hw_render_interface_vulkan,
    entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
}

impl VulkanRenderInterface {
    pub unsafe fn new(interface: &retro_hw_render_interface_vulkan) -> Result<Self, Box<dyn Error>> {
        let entry =
            ENTRY.clone().unwrap_or_else(|| {
                let static_fn =
                    StaticFn::load(|sym: &CStr| {
                        interface.get_instance_proc_addr.unwrap()(interface.instance, sym.as_ptr())
                            .unwrap_or(transmute::<*const c_void, unsafe extern "system" fn()>(ptr::null()))
                            as *const c_void
                    });

                warn!("ENTRY not available, creating a new one from the render interface");
                ash::Entry::from_static_fn(static_fn)
            });

        let instance = INSTANCE.clone().unwrap_or_else(|| ash::Instance::load(entry.static_fn(), interface.instance));
        let device = DEVICE.clone().unwrap_or_else(|| ash::Device::load(instance.fp_v1_0(), interface.device));
        // TODO: Ensure that the handles provided by interface are the same as in the statics

        Ok(Self {
            interface: interface.clone(),
            entry,
            instance,
            device,
        })
    }

    pub fn instance(&self) -> &ash::Instance {
        &self.instance
    }

    pub fn device(&self) -> &ash::Device {
        &self.device
    }

    pub fn entry(&self) -> &ash::Entry {
        &self.entry
    }

    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.interface.gpu
    }

    pub fn queue_family_index(&self) -> u32 {
        self.interface.queue_index
    }

    pub fn set_image(
        &self,
        image: &retro_vulkan_image,
        semaphores: &[Semaphore],
        src_queue_family: u32,
    ) -> Result<(), HardwareRenderError> {
        unsafe {
            let set_image = self.interface.set_image.ok_or(NullInterfaceFunction("set_image"))?;
            let ptr = if !semaphores.is_empty() {
                semaphores.as_ptr()
            } else {
                ptr::null()
            };

            set_image(self.interface.handle, image, semaphores.len() as u32, ptr, src_queue_family);
            Ok(())
        }
    }

    pub fn get_sync_index(&self) -> Result<u32, HardwareRenderError> {
        unsafe {
            let get_sync_index = self
                .interface
                .get_sync_index
                .ok_or(NullInterfaceFunction("get_sync_index"))?;

            Ok(get_sync_index(self.interface.handle))
        }
    }

    pub fn get_sync_index_mask(&self) -> Result<u32, HardwareRenderError> {
        unsafe {
            let get_sync_index_mask = self
                .interface
                .get_sync_index_mask
                .ok_or(NullInterfaceFunction("get_sync_index_mask"))?;

            Ok(get_sync_index_mask(self.interface.handle))
        }
    }

    pub fn wait_sync_index(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let wait_sync_index = self
                .interface
                .wait_sync_index
                .ok_or(NullInterfaceFunction("wait_sync_index"))?;

            wait_sync_index(self.interface.handle);
            Ok(())
        }
    }

    pub fn lock_queue(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let lock_queue = self.interface.lock_queue.ok_or(NullInterfaceFunction("lock_queue"))?;

            lock_queue(self.interface.handle);
            Ok(())
        }
    }

    pub fn unlock_queue(&self) -> Result<(), HardwareRenderError> {
        unsafe {
            let unlock_queue = self
                .interface
                .unlock_queue
                .ok_or(NullInterfaceFunction("unlock_queue"))?;

            unlock_queue(self.interface.handle);
            Ok(())
        }
    }

    pub fn set_command_buffers(&self, cmd: &[vk::CommandBuffer]) -> Result<(), HardwareRenderError> {
        unsafe {
            let set_command_buffers = self
                .interface
                .set_command_buffers
                .ok_or(NullInterfaceFunction("set_command_buffers"))?;
            let ptr = if !cmd.is_empty() { cmd.as_ptr() } else { ptr::null() };

            set_command_buffers(self.interface.handle, cmd.len() as u32, ptr);
            Ok(())
        }
    }

    pub fn set_signal_semaphore(&self, semaphore: vk::Semaphore) -> Result<(), HardwareRenderError> {
        unsafe {
            let set_signal_semaphore = self
                .interface
                .set_signal_semaphore
                .ok_or(NullInterfaceFunction("set_signal_semaphore"))?;

            set_signal_semaphore(self.interface.handle, semaphore);
            Ok(())
        }
    }
}
