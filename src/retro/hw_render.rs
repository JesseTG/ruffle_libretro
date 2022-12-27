use std::ffi::CString;
use std::mem::transmute;
use libc::c_char;
use rust_libretro::sys::{retro_hw_context_reset_t, retro_hw_context_type, retro_hw_get_current_framebuffer_t, retro_hw_get_proc_address_t, retro_hw_render_callback, retro_proc_address_t};

struct Framebuffer(usize);

impl From<usize> for Framebuffer {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<Framebuffer> for usize {
    fn from(value: Framebuffer) -> Self {
        value.0
    }
}

pub struct HardwareRenderCallback {
    context_type: retro_hw_context_type,
    bottom_left_origin: bool,
    version_major: u32,
    version_minor: u32,
    debug_context: bool,
    context_reset: retro_hw_context_reset_t,
    context_destroy: retro_hw_context_reset_t,

    // Set by the frontend
    get_current_framebuffer: retro_hw_get_current_framebuffer_t,
    get_proc_address: retro_hw_get_proc_address_t,
}

impl HardwareRenderCallback {
    fn new(
        context_type: retro_hw_context_type,
        bottom_left_origin: bool,
        version_major: u32,
        version_minor: u32,
        debug_context: bool,
        context_reset: retro_hw_context_reset_t,
        context_destroy: retro_hw_context_reset_t,
    ) -> Self {
        Self {
            context_type,
            bottom_left_origin,
            version_major,
            version_minor,
            debug_context,
            context_reset,
            context_destroy,
            get_current_framebuffer: None,
            get_proc_address: None
        }
    }

    pub fn get_current_framebuffer(&self) -> Option<Framebuffer>
    {
        let a =        self.get_proc_address::<i32, i32>("france");
        Some(self.get_current_framebuffer?().into())
    }

    pub fn get_proc_address<Args, Ret>(&self, sym: &str) -> Option<unsafe extern "C" fn(Args) -> Ret>
    {
        let sym = CString::new(sym).ok()?;
        unsafe {
            transmute(self.get_proc_address?(sym.as_ptr())?)
        }
    }
}

impl From<retro_hw_render_callback> for HardwareRenderCallback {
    fn from(value: retro_hw_render_callback) -> Self {
        Self {
            context_type: value.context_type,
            bottom_left_origin: value.bottom_left_origin,
            version_major: value.version_major,
            version_minor: value.version_minor,
            debug_context: value.debug_context,
            context_reset: value.context_reset,
            context_destroy: value.context_destroy,
            get_current_framebuffer: value.get_current_framebuffer,
            get_proc_address: value.get_proc_address,
        }
    }
}