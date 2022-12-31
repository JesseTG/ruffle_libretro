use crate::core::render::RenderInterfaceError::InterfaceNotFound;
use crate::core::state::RenderInterface;
use crate::core::Ruffle;
use ash::vk::InstanceFnV1_0;
use futures::executor::block_on;
use log::trace;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro::environment;
use rust_libretro_sys::retro_hw_render_interface_type::RETRO_HW_RENDER_INTERFACE_VULKAN;
use rust_libretro_sys::{
    retro_hw_context_type, retro_hw_context_type::*, retro_hw_render_interface_vulkan,
    RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE,
};
use std::error::Error;
use std::ffi::CString;
use std::{mem, ptr};
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum RenderInterfaceError {
    #[error("Unknown environment callback (does this frontend support it?)")]
    UnknownEnvironmentCallback,

    #[error("Interface not found")]
    InterfaceNotFound,
}

impl Ruffle {
    pub(crate) fn get_descriptors(&self) -> Result<Descriptors, Box<dyn Error>> {
        let hw_render = self
            .hw_render_callback
            .as_ref()
            .ok_or("Hardware render callback not initialized")?;
        let get_proc_address = hw_render.get_proc_address.ok_or("get_proc_address not initialized")?;
        let descriptors = match hw_render.context_type {
            RETRO_HW_CONTEXT_OPENGL
            | RETRO_HW_CONTEXT_OPENGLES2
            | RETRO_HW_CONTEXT_OPENGLES3
            | RETRO_HW_CONTEXT_OPENGL_CORE
            | RETRO_HW_CONTEXT_OPENGLES_VERSION => unsafe {
                let descriptors = WgpuRenderBackend::<TextureTarget>::build_descriptors_for_gl(
                    |sym| {
                        CString::new(sym)
                            .ok() // Get the symbol name ready for C...
                            .and_then(|sym| {
                                let address = get_proc_address(sym.as_ptr());
                                trace!("get_proc_address({sym:?}) = {address:?}");
                                address
                            }) // Then get the function address from libretro...
                            .map(|f| f as *const libc::c_void) // Then cast it to the right pointer type...
                            .unwrap_or(ptr::null()) // ...or if all else fails, return a null pointer (gl will handle it)
                    },
                    None,
                );

                block_on(descriptors)
            },
            RETRO_HW_CONTEXT_VULKAN => unsafe {
                let interface = match self
                    .get_hw_render_interface(RETRO_HW_CONTEXT_VULKAN)?
                    .ok_or("Not found")?
                {
                    RenderInterface::Vulkan(interface) => interface,
                    _ => Err("Not found")?,
                };

                let descriptors = WgpuRenderBackend::<TextureTarget>::build_descriptors_for_vulkan(
                    interface.gpu,
                    ash::Device::load(
                        &InstanceFnV1_0::load(|sym| {
                            match (interface.get_instance_proc_addr)(interface.instance, sym.as_ptr()) {
                                Some(ptr) => mem::transmute(ptr),
                                None => ptr::null(),
                            }
                        }),
                        interface.device,
                    ),
                    false,
                    &[],
                    wgpu::Features::all(),
                    wgpu_hal::UpdateAfterBindTypes::all(),
                    0,
                    interface.queue_index,
                    None,
                );

                block_on(descriptors)
            },
            _ => Err("Context not available")?,
        };

        descriptors
    }

    pub(crate) fn get_hw_render_interface(
        &self,
        context_type: retro_hw_context_type,
    ) -> Result<Option<RenderInterface>, RenderInterfaceError> {
        match context_type {
            RETRO_HW_CONTEXT_VULKAN => unsafe {
                let interface = environment::get_unchecked::<*mut retro_hw_render_interface_vulkan>(
                    self.environ_cb.get(),
                    RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE,
                );

                let interface = match interface {
                    Some((ptr, true))
                        if !ptr.is_null() && (&*ptr).interface_type == RETRO_HW_RENDER_INTERFACE_VULKAN =>
                    {
                        &*ptr
                    }
                    _ => Err(InterfaceNotFound)?,
                };

                Ok(Some(RenderInterface::Vulkan(*interface)))
            },
            _ => Ok(None),
        }
    }
}
