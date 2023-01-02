use std::error::Error;
use std::sync::Arc;

use ruffle_core::Player;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro_sys::{retro_hw_context_type, retro_hw_context_type::*};
use thiserror::Error as ThisError;

use crate::core::render::opengl::OpenGlRenderState;
use crate::core::render::vulkan::VulkanRenderState;
use crate::core::render::RenderInterfaceError::*;
use crate::core::Ruffle;

mod opengl;
mod vulkan;

pub trait RenderState {
    fn descriptors(&self) -> Arc<Descriptors>;

    fn render(&mut self, player: &mut Player) -> Result<(), Box<dyn Error>>;

    fn reset(&mut self) -> Result<(), Box<dyn Error>>;

    fn set_target(&mut self, target: &TextureTarget) -> Result<(), Box<dyn Error>>;
}

#[derive(ThisError, Debug)]
pub enum RenderInterfaceError {
    #[error("retro_system_av_info isn't ready")]
    AvInfoNotReady,

    #[error("retro_hw_render_callback isn't ready")]
    HwRenderCallbackNotReady,

    #[error("Unsupported hardware context {0:?}")]
    UnsupportedHardwareContext(retro_hw_context_type),

    #[error("No render state available")]
    NoRenderState,
}

impl Ruffle {
    pub(crate) fn get_render_backend(
        &self,
    ) -> Result<(WgpuRenderBackend<TextureTarget>, Box<dyn RenderState>), Box<dyn Error>> {
        let hw_render_callback = self.hw_render_callback.as_ref().ok_or(HwRenderCallbackNotReady)?;
        let environ_cb = self.environ_cb.get();
        let av_info = self.av_info.ok_or(AvInfoNotReady)?;
        let (width, height) = (av_info.geometry.base_width, av_info.geometry.base_height);

        let mut render_state: Box<dyn RenderState> = match hw_render_callback.context_type {
            RETRO_HW_CONTEXT_OPENGL
            | RETRO_HW_CONTEXT_OPENGLES2
            | RETRO_HW_CONTEXT_OPENGLES3
            | RETRO_HW_CONTEXT_OPENGL_CORE
            | RETRO_HW_CONTEXT_OPENGLES_VERSION => {
                Box::new(OpenGlRenderState::new(hw_render_callback.get_proc_address)?)
            }
            RETRO_HW_CONTEXT_VULKAN => unsafe { Box::new(VulkanRenderState::new(environ_cb)?) },
            other => Err(UnsupportedHardwareContext(other))?,
        };

        let descriptors = render_state.descriptors();
        let target = TextureTarget::new(&descriptors.device, (width, height))?;
        render_state.set_target(&target)?;

        Ok((WgpuRenderBackend::new(descriptors, target)?, render_state))
    }
}
