use std::ffi::c_void;

use ash::vk;
use rust_libretro_sys::{retro_hw_render_interface_vulkan, retro_vulkan_image};
use thiserror::Error as ThisError;

use self::VulkanRenderInterfaceError::*;

#[derive(ThisError, Copy, Clone, Debug)]
pub enum VulkanRenderInterfaceError {
    #[error("Render interface function {0} was null")]
    NullInterfaceFunction(&'static str),

    #[error("Interface handle was null")]
    NullHandle,

    #[error("VkInstance was null")]
    NullInstance,

    #[error("VkPhysicalDevice was null")]
    NullPhysicalDevice,

    #[error("VkDevice was null")]
    NullDevice,

    #[error("VkQueue was null")]
    NullQueue,
}

pub struct VulkanRenderInterface {
    handle: *mut c_void,
    instance: vk::Instance,
    gpu: vk::PhysicalDevice,
    device: vk::Device,
    queue: vk::Queue,
    queue_index: u32,
    set_image: unsafe extern "C" fn(*mut c_void, *const retro_vulkan_image, u32, *const vk::Semaphore, u32),
    get_sync_index: unsafe extern "C" fn(*mut c_void) -> u32,
    get_sync_index_mask: unsafe extern "C" fn(*mut c_void) -> u32,
    wait_sync_index: unsafe extern "C" fn(*mut c_void),
    lock_queue: unsafe extern "C" fn(*mut c_void),
    unlock_queue: unsafe extern "C" fn(*mut c_void),
    set_command_buffers: unsafe extern "C" fn(*mut c_void, u32, *const vk::CommandBuffer),
    set_signal_semaphore: unsafe extern "C" fn(*mut c_void, vk::Semaphore),
}

impl VulkanRenderInterface {
    pub fn new(interface: &retro_hw_render_interface_vulkan) -> Result<Self, VulkanRenderInterfaceError> {
        if interface.handle.is_null() {
            Err(NullHandle)?;
        }

        if interface.instance == vk::Instance::null() {
            Err(NullInstance)?;
        }

        if interface.gpu == vk::PhysicalDevice::null() {
            Err(NullPhysicalDevice)?;
        }

        if interface.device == vk::Device::null() {
            Err(NullDevice)?;
        }

        if interface.queue == vk::Queue::null() {
            Err(NullQueue)?;
        }

        let get_sync_index = interface
            .get_sync_index
            .ok_or(NullInterfaceFunction("get_sync_index"))?;

        let set_image = interface.set_image.ok_or(NullInterfaceFunction("set_image"))?;

        let get_sync_index_mask = interface
            .get_sync_index_mask
            .ok_or(NullInterfaceFunction("get_sync_index_mask"))?;

        let wait_sync_index = interface
            .wait_sync_index
            .ok_or(NullInterfaceFunction("wait_sync_index"))?;

        let lock_queue = interface.lock_queue.ok_or(NullInterfaceFunction("lock_queue"))?;

        let unlock_queue = interface.unlock_queue.ok_or(NullInterfaceFunction("unlock_queue"))?;

        let set_command_buffers = interface
            .set_command_buffers
            .ok_or(NullInterfaceFunction("set_command_buffers"))?;

        let set_signal_semaphore = interface
            .set_signal_semaphore
            .ok_or(NullInterfaceFunction("set_signal_semaphore"))?;

        Ok(Self {
            handle: interface.handle,
            instance: interface.instance,
            gpu: interface.gpu,
            device: interface.device,
            queue: interface.queue,
            queue_index: interface.queue_index,
            set_image,
            get_sync_index,
            get_sync_index_mask,
            wait_sync_index,
            lock_queue,
            unlock_queue,
            set_command_buffers,
            set_signal_semaphore,
        })
    }

    pub fn instance(&self) -> vk::Instance {
        self.instance
    }

    pub fn device(&self) -> vk::Device {
        self.device
    }

    pub fn gpu(&self) -> vk::PhysicalDevice {
        self.gpu
    }

    pub fn queue_index(&self) -> u32 {
        self.queue_index
    }

    pub fn queue(&self) -> vk::Queue {
        self.queue
    }

    pub fn set_image(&self, image: &retro_vulkan_image, semaphores: &[vk::Semaphore], src_queue_family: u32) {
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_hw_render_interface_vulkan::set_image");
        unsafe {
            (self.set_image)(self.handle, image, semaphores.len() as u32, semaphores.as_ptr(), src_queue_family);
        }
    }

    pub fn get_sync_index(&self) -> u32 {
        unsafe { (self.get_sync_index)(self.handle) }
    }

    pub fn get_sync_index_mask(&self) -> u32 {
        unsafe { (self.get_sync_index_mask)(self.handle) }
    }

    pub fn wait_sync_index(&self) {
        unsafe {
            (self.wait_sync_index)(self.handle);
        }
    }

    pub fn lock_queue(&self) {
        unsafe {
            (self.lock_queue)(self.handle);
        }
    }

    pub fn unlock_queue(&self) {
        unsafe {
            (self.unlock_queue)(self.handle);
        }
    }

    pub fn set_command_buffers(&self, cmd: &[vk::CommandBuffer]) {
        unsafe {
            (self.set_command_buffers)(self.handle, cmd.len() as u32, cmd.as_ptr());
        }
    }

    pub fn set_signal_semaphore(&self, semaphore: vk::Semaphore) {
        unsafe {
            (self.set_signal_semaphore)(self.handle, semaphore);
        }
    }
}
