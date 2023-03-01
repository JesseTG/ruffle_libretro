use std::ffi::CStr;

use rust_libretro::contexts::LoadGameContext;
use rust_libretro::{anyhow, environment, retro_hw_context_destroyed_callback, retro_hw_context_reset_callback};
use rust_libretro_sys::retro_hw_context_type::*;
use rust_libretro_sys::{
    retro_environment_t, retro_hw_context_type, retro_hw_render_callback,
    retro_hw_render_context_negotiation_interface_type, retro_hw_render_interface_type, retro_proc_address_t,
};
use thiserror::Error as ThisError;

pub mod opengl;
pub mod vulkan;
mod wgpu;

#[derive(ThisError, Debug)]
pub enum HardwareRenderError {
    #[error("Couldn't use the provided environment callback")]
    InvalidEnvironmentCallback,

    #[error("Frontend prefers unrecognized hardware context type {0}")]
    UnknownContextType(u32),

    #[error("Couldn't set the renderer to type {0:?} (does the frontend support it?)")]
    FailedToSetRenderer(retro_hw_context_type),

    #[error("Couldn't set the context negotiation to type {0:?} (does the frontend support it?)")]
    FailedToSetNegotiationInterface(retro_hw_render_context_negotiation_interface_type),

    #[error("Video driver switching is disabled or unavailable, you get no choice")]
    DriverSwitchingNotAvailable,

    #[error("Rendering with {0:?} is not currently supported")]
    UnsupportedHardwareContext(retro_hw_context_type),

    #[error("Expected a render interface of type {0:?}, got one of type {1:?}")]
    WrongRenderInterfaceType(retro_hw_render_interface_type, retro_hw_render_interface_type),

    #[error("Failed to get a render interface of type {0:?}")]
    FailedToGetRenderInterface(retro_hw_render_interface_type),

    #[error("Expected a render interface of type {0:?}, got a null pointer")]
    NullRenderInterface(retro_hw_render_interface_type),

    #[error("Render interface function {0} was null")]
    NullInterfaceFunction(&'static str),
}

#[derive(Debug, Copy, Clone)]
pub struct HardwareRenderCallback {
    // RetroArch maintains its own instance, so we don't need to keep this one around
    // unless we want to use the functions it provides.
    callback: retro_hw_render_callback,
}

impl HardwareRenderCallback {
    pub fn set(preferred_renderer: retro_hw_context_type, environ_cb: retro_environment_t) -> anyhow::Result<Self> {
        let callback = retro_hw_render_callback {
            context_type: match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGL => RETRO_HW_CONTEXT_OPENGLES2,
                RETRO_HW_CONTEXT_OPENGLES_VERSION | RETRO_HW_CONTEXT_OPENGL_CORE => RETRO_HW_CONTEXT_OPENGLES3,
                // wgpu supports OpenGL ES, but *not* plain OpenGL.
                _ => preferred_renderer,
            },
            bottom_left_origin: true,
            version_major: match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGLES3 => 3,
                RETRO_HW_CONTEXT_OPENGLES2 | RETRO_HW_CONTEXT_OPENGL => 2,
                RETRO_HW_CONTEXT_DIRECT3D => 11, // Direct3D 12 is buggy in RetroArch
                RETRO_HW_CONTEXT_VULKAN => ash::vk::API_VERSION_1_3,
                _ => 0, // Other video contexts don't need a major version number
            },
            version_minor: match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGLES3 => 1,
                _ => 0, // Other video contexts don't need a minor version number
            },
            cache_context: true,
            debug_context: true,

            depth: false,   // obsolete
            stencil: false, // obsolete

            context_reset: Some(retro_hw_context_reset_callback),
            context_destroy: Some(retro_hw_context_destroyed_callback),

            // Set by the frontend
            get_current_framebuffer: None,
            get_proc_address: None,
        };

        Ok(Self {
            callback: unsafe { environment::set_hw_render(environ_cb, callback)? },
        })
    }

    pub fn get_proc_address(&self, sym: &CStr) -> retro_proc_address_t {
        if let Some(get_proc_address) = self.callback.get_proc_address {
            unsafe { get_proc_address(sym.as_ptr()) }
        } else {
            None
        }
    }

    pub fn context_type(&self) -> retro_hw_context_type {
        self.callback.context_type
    }
}

pub fn enable_hw_render(ctx: &mut LoadGameContext, preferred_renderer: retro_hw_context_type) -> anyhow::Result<()> {
    unsafe {
        ctx.enable_hw_render(
            match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGL => RETRO_HW_CONTEXT_OPENGLES2,
                RETRO_HW_CONTEXT_OPENGLES_VERSION | RETRO_HW_CONTEXT_OPENGL_CORE => RETRO_HW_CONTEXT_OPENGLES3,
                // wgpu supports OpenGL ES, but *not* plain OpenGL.
                _ => preferred_renderer,
            },
            true,
            match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGLES3 => 3,
                RETRO_HW_CONTEXT_OPENGLES2 | RETRO_HW_CONTEXT_OPENGL => 2,
                RETRO_HW_CONTEXT_DIRECT3D => 11, // Direct3D 12 is buggy in RetroArch
                RETRO_HW_CONTEXT_VULKAN => ash::vk::API_VERSION_1_3,
                _ => 0, // Other video contexts don't need a major version number
            },
            match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGLES3 => 1,
                _ => 0, // Other video contexts don't need a minor version number
            },
            true,
        )?;
    };

    Ok(())
}

pub fn enable_hw_render_negotiation_interface(
    ctx: &mut LoadGameContext,
    preferred_renderer: retro_hw_context_type,
) -> anyhow::Result<()> {
    if preferred_renderer == RETRO_HW_CONTEXT_VULKAN {
        vulkan::negotiation::enable(ctx)?;
    }

    // Enable the Vulkan context negotiation interface if using Vulkan,
    // otherwise do nothing.

    Ok(())
}
