use std::error::Error;
use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use futures::executor::block_on;
use log::trace;
use ruffle_core::Player;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro_sys::retro_hw_get_proc_address_t;
use thiserror::Error as ThisError;

use crate::core::render::opengl::OpenGlRenderStateError::{FailedToCreateDescriptors, GetProcAddressNotInitialized};
use crate::core::render::RenderState;

#[derive(ThisError, Debug)]
pub enum OpenGlRenderStateError {
    #[error("Frontend did not provide retro_hw_render_callback.get_proc_address")]
    GetProcAddressNotInitialized,

    #[error("Failed to create OpenGL descriptors: {0}")]
    FailedToCreateDescriptors(Box<dyn Error>),
}


pub(crate) struct OpenGlRenderState {
    descriptors: Arc<Descriptors>,
}

impl OpenGlRenderState {
    pub fn new(get_proc_address: retro_hw_get_proc_address_t) -> Result<Self, OpenGlRenderStateError> {
        let get_proc_address = get_proc_address.ok_or(GetProcAddressNotInitialized)?;
        let descriptors = block_on(unsafe {
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
        }).or_else(|error| Err(FailedToCreateDescriptors(error)))?;

        Ok(Self {
            descriptors: Arc::new(descriptors),
        })
    }
}

impl RenderState for OpenGlRenderState {
    fn descriptors(&self) -> Arc<Descriptors> {
        self.descriptors.clone()
    }

    fn render(&mut self, player: &mut Player) -> Result<(), Box<dyn Error>> {
        player.render();
        Ok(())
    }

    fn reset(&mut self) -> Result<(), Box<dyn Error>> {
        todo!()
    }

    fn set_target(&mut self, target: &TextureTarget) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}