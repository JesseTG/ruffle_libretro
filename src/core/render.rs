use crate::core::render::RenderInterfaceError::*;
use crate::core::state::RenderInterface;
use crate::core::state::RenderInterface::Default;
use crate::core::Ruffle;
use ash::vk::InstanceFnV1_0;
use futures::executor::block_on;
use log::trace;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro::environment;
use rust_libretro::environment::get_hw_render_interface;
use rust_libretro_sys::retro_hw_render_interface_type::RETRO_HW_RENDER_INTERFACE_VULKAN;
use rust_libretro_sys::{retro_hw_context_type, retro_hw_context_type::*, retro_hw_get_proc_address_t, retro_hw_render_callback, retro_hw_render_interface_vulkan, retro_system_av_info, retro_vulkan_image, RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE, retro_hw_render_context_negotiation_interface_vulkan};
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum RenderInterfaceError {
    #[error("retro_system_av_info isn't ready")]
    AvInfoNotReady,

    #[error("retro_hw_render_callback isn't ready")]
    HwRenderCallbackNotReady,

    #[error("Frontend did not provide retro_hw_render_callback.get_proc_address")]
    GetProcAddressNotInitialized,

    #[error("Unknown environment callback (does this frontend support it?)")]
    UnknownEnvironmentCallback,

    #[error("Interface not found")]
    InterfaceNotFound,

    #[error("Unsupported hardware context {0:?}")]
    UnsupportedHardwareContext(retro_hw_context_type),

    #[error("Incorrect render interface")]
    WrongRenderInterface,
}

#[derive(Debug)]
pub enum RenderInterface {
    Default,
    Vulkan(retro_hw_render_interface_vulkan),
}

pub enum RenderContextNegotiationInterface {
    Vulkan(retro_hw_render_context_negotiation_interface_vulkan)
}

impl Ruffle {
    pub(crate) fn get_render_backend(
        &self,
        hw_render_callback: &retro_hw_render_callback,
        av_info: &retro_system_av_info,
    ) -> Result<(WgpuRenderBackend<TextureTarget>, RenderInterface), Box<dyn Error>> {
        let interface = self.get_hw_render_interface(hw_render_callback.context_type)?;
        let descriptors = Arc::new(self.get_descriptors(hw_render_callback, &interface)?);
        let (width, height) = (av_info.geometry.base_width, av_info.geometry.base_height);
        let target = TextureTarget::new(&descriptors.device, (width, height))?;
        let backend = WgpuRenderBackend::new(descriptors, target)?;

        Ok((backend, interface))
    }

    fn get_descriptors(
        &self,
        hw_render: &retro_hw_render_callback,
        interface: &RenderInterface,
    ) -> Result<Descriptors, Box<dyn Error>> {
        let descriptors = match hw_render.context_type {
            RETRO_HW_CONTEXT_OPENGL
            | RETRO_HW_CONTEXT_OPENGLES2
            | RETRO_HW_CONTEXT_OPENGLES3
            | RETRO_HW_CONTEXT_OPENGL_CORE
            | RETRO_HW_CONTEXT_OPENGLES_VERSION => unsafe { self.get_gl_descriptors(hw_render.get_proc_address) },
            RETRO_HW_CONTEXT_VULKAN => unsafe { self.get_vulkan_descriptors(interface) },
            context => Err(UnsupportedHardwareContext(context))?,
        };

        descriptors
    }

    unsafe fn get_gl_descriptors(
        &self,
        get_proc_address: retro_hw_get_proc_address_t,
    ) -> Result<Descriptors, Box<dyn Error>> {
        let get_proc_address = get_proc_address.ok_or(GetProcAddressNotInitialized)?;
        let descriptors = unsafe {
            WgpuRenderBackend::<TextureTarget>::build_descriptors_for_gl(
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
            )
        };

        block_on(descriptors)
    }

    unsafe fn get_vulkan_descriptors(&self, interface: &RenderInterface) -> Result<Descriptors, Box<dyn Error>> {
        let interface = match interface {
            Default => Err(WrongRenderInterface)?,
            RenderInterface::Vulkan(vulkan) => vulkan,
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
    }

    fn get_hw_render_interface(
        &self,
        context_type: retro_hw_context_type,
    ) -> Result<RenderInterface, RenderInterfaceError> {
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

                Ok(RenderInterface::Vulkan(*interface))
            },
            _ => Ok(Default),
        }
    }
}
