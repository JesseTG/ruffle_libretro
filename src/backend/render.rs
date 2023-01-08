use std::error::Error;
use std::ffi::{c_void, CStr};

use rust_libretro::{environment, retro_hw_context_destroyed_callback, retro_hw_context_reset_callback};
use rust_libretro_sys::retro_hw_context_type::*;
use rust_libretro_sys::{
    retro_environment_t, retro_hw_context_type, retro_hw_render_callback,
    retro_hw_render_context_negotiation_interface_type, retro_hw_render_interface_type, retro_proc_address_t,
    RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER, RETRO_ENVIRONMENT_SET_HW_RENDER,
    RETRO_ENVIRONMENT_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE,
};
use thiserror::Error as ThisError;

use crate::backend::render::vulkan::negotiation::VulkanContextNegotiationInterface;
use crate::backend::render::HardwareRenderError::*;

pub mod opengl;
pub mod vulkan;

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

pub fn get_preferred_hw_render(environ_cb: retro_environment_t) -> Result<retro_hw_context_type, HardwareRenderError> {
    let preferred_renderer =
        match { unsafe { environment::get::<u32>(environ_cb, RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER) } } {
            Some((0, true)) => RETRO_HW_CONTEXT_NONE,
            Some((1, true)) => RETRO_HW_CONTEXT_OPENGL,
            Some((2, true)) => RETRO_HW_CONTEXT_OPENGLES2,
            Some((3, true)) => RETRO_HW_CONTEXT_OPENGL_CORE,
            Some((4, true)) => RETRO_HW_CONTEXT_OPENGLES3,
            Some((5, true)) => RETRO_HW_CONTEXT_OPENGLES_VERSION,
            Some((6, true)) => RETRO_HW_CONTEXT_VULKAN,
            Some((7, true)) => RETRO_HW_CONTEXT_DIRECT3D,
            Some((_, false)) => Err(DriverSwitchingNotAvailable)?,
            Some((unknown, true)) => Err(UnknownContextType(unknown))?,
            None => Err(InvalidEnvironmentCallback)?,
        };

    Ok(preferred_renderer)
}

#[derive(Debug, Copy, Clone)]
pub struct HardwareRenderCallback {
    // RetroArch maintains its own instance, so we don't need to keep this one around
    // unless we want to use the functions it provides.
    callback: retro_hw_render_callback,
}

impl HardwareRenderCallback {
    pub fn set(
        preferred_renderer: retro_hw_context_type,
        environ_cb: retro_environment_t,
    ) -> Result<Self, HardwareRenderError> {
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

        // Using ctx.set_hw_render doesn't set the proc address
        match unsafe { environment::set_ptr(environ_cb, RETRO_ENVIRONMENT_SET_HW_RENDER, &callback) } {
            Some(true) => Ok(Self { callback }),
            Some(false) => Err(FailedToSetRenderer(preferred_renderer))?,
            None => Err(InvalidEnvironmentCallback)?,
        }
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

pub trait HardwareRenderContextNegotiationInterface {
    unsafe fn get_ptr(&self) -> *const c_void;

    fn r#type(&self) -> retro_hw_render_context_negotiation_interface_type;
}

impl dyn HardwareRenderContextNegotiationInterface {
    pub fn instance(
        hw_render: &HardwareRenderCallback,
    ) -> Result<Option<&'static impl HardwareRenderContextNegotiationInterface>, Box<dyn Error>> {
        match hw_render.callback.context_type {
            RETRO_HW_CONTEXT_VULKAN => Ok(Some(VulkanContextNegotiationInterface::get_instance()?)),
            _ => Ok(None), // Not an error;
        }
    }

    pub fn set(
        interface: &dyn HardwareRenderContextNegotiationInterface,
        environ_cb: retro_environment_t,
    ) -> Result<(), Box<dyn Error>> {
        match unsafe {
            environment::set_ptr(
                environ_cb,
                RETRO_ENVIRONMENT_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE,
                interface.get_ptr(),
            )
        } {
            Some(true) => Ok(()),
            Some(false) => Err(FailedToSetNegotiationInterface(interface.r#type()))?,
            _ => Err(InvalidEnvironmentCallback)?,
        }
    }
}

// We try to request the highest limits we can get away with
fn required_limits(adapter: &wgpu::Adapter) -> (wgpu::Limits, wgpu::Features) {
    // We start off with the lowest limits we actually need - basically GL-ES 3.0
    let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
    // Then we increase parts of it to the maximum supported by the adapter, to take advantage of
    // more powerful hardware or capabilities
    limits = limits.using_resolution(adapter.limits());
    limits = limits.using_alignment(adapter.limits());

    limits.max_storage_buffers_per_shader_stage =
        adapter.limits().max_storage_buffers_per_shader_stage;
    limits.max_storage_buffer_binding_size = adapter.limits().max_storage_buffer_binding_size;

    let features = wgpu::Features::DEPTH24PLUS_STENCIL8;

    (limits, features)
}