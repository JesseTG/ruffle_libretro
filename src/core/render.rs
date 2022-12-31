use rust_libretro::environment;
use crate::core::state::RenderInterface;
use crate::core::Ruffle;
use rust_libretro_sys::{RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE, retro_hw_context_type, retro_hw_render_interface_vulkan};
use rust_libretro_sys::retro_hw_render_interface_type::RETRO_HW_RENDER_INTERFACE_VULKAN;
use crate::core::render::RenderInterfaceError::InterfaceNotFound;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum RenderInterfaceError {
    #[error("Unknown environment callback (does this frontend support it?)")]
    UnknownEnvironmentCallback,

    #[error("Interface not found")]
    InterfaceNotFound,
}

impl Ruffle {
    pub fn get_hw_render_interface(
        &self,
        context_type: retro_hw_context_type,
    ) -> Result<Option<RenderInterface>, RenderInterfaceError> {
        match context_type {
            retro_hw_context_type::RETRO_HW_CONTEXT_VULKAN => unsafe {
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
