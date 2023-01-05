use std::borrow::Cow;
use std::error::Error;
use std::ffi::{c_void, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use gc_arena::MutationContext;
use log::trace;
use ruffle_core::Color;
use ruffle_core::swf::Glyph;
use ruffle_render::backend::{Context3D, Context3DCommand, RenderBackend, ShapeHandle, ViewportDimensions};
use ruffle_render::bitmap::{Bitmap, BitmapHandle, BitmapSource};
use ruffle_render::commands::CommandList;
use ruffle_render::error::Error as RuffleError;
use ruffle_render::shape_utils::DistilledShape;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use rust_libretro_sys::retro_game_geometry;

use crate::backend::render::{HardwareRenderCallback, required_limits};

pub struct OpenGlWgpuRenderBackend {
    backend: WgpuRenderBackend<TextureTarget>,
}

impl OpenGlWgpuRenderBackend {
    pub async fn new(
        hw_render: &HardwareRenderCallback,
        geometry: &retro_game_geometry,
    ) -> Result<OpenGlWgpuRenderBackend, Box<dyn Error>> {
        let descriptors = unsafe {
            Self::build_descriptors_for_gl(
                |sym| {
                    CString::new(sym)
                        .ok() // Get the symbol name ready for C...
                        .and_then(|sym| {
                            let address = hw_render.get_proc_address(sym.as_c_str());
                            trace!("get_proc_address({sym:?}) = {address:?}");
                            address
                        }) // Then get the function address from libretro...
                        .map(|f| f as *const c_void) // Then cast it to the right pointer type...
                        .unwrap_or(ptr::null()) // ...or if all else fails, return a null pointer (gl will handle it)
                },
                None,
            )
        }.await?;
        let target = TextureTarget::new(&descriptors.device, (geometry.base_width, geometry.max_height))?;

        Ok(Self {
            backend: WgpuRenderBackend::new(Arc::new(descriptors), target, 4)?,
            // TODO: Get the sample count from the core config
        })
    }

      async unsafe fn build_descriptors_for_gl(
        fun: impl FnMut(&str) -> *const core::ffi::c_void,
        trace_path: Option<&Path>,
    ) -> Result<Descriptors, Box<dyn Error>> {
        use wgpu_hal::api::Gles;
        use wgpu_hal::Api;
        let instance = wgpu::Instance::new(wgpu::Backends::GL);
        let adapter_hal =
            <Gles as Api>::Adapter::new_external(fun).expect("expose_adapter should be infallible");
        let adapter = instance.create_adapter_from_hal(adapter_hal);
        let (limits, features) = required_limits(&adapter);
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features,
                    limits,
                },
                trace_path,
            )
            .await?;

        Ok(Descriptors::new(adapter, device, queue))
    }
}

impl RenderBackend for OpenGlWgpuRenderBackend {
    fn viewport_dimensions(&self) -> ViewportDimensions {
        self.backend.viewport_dimensions()
    }

    fn set_viewport_dimensions(&mut self, dimensions: ViewportDimensions) {
        self.backend.set_viewport_dimensions(dimensions)
    }

    fn register_shape(&mut self, shape: DistilledShape, bitmap_source: &dyn BitmapSource) -> ShapeHandle {
        self.backend.register_shape(shape, bitmap_source)
    }

    fn replace_shape(&mut self, shape: DistilledShape, bitmap_source: &dyn BitmapSource, handle: ShapeHandle) {
        self.backend.replace_shape(shape, bitmap_source, handle)
    }

    fn register_glyph_shape(&mut self, shape: &Glyph) -> ShapeHandle {
        self.backend.register_glyph_shape(shape)
    }

    fn render_offscreen(
        &mut self,
        handle: BitmapHandle,
        width: u32,
        height: u32,
        commands: CommandList,
    ) -> Result<Bitmap, RuffleError> {
        self.backend.render_offscreen(handle, width, height, commands)
    }

    fn submit_frame(&mut self, clear: Color, commands: CommandList) {
        self.backend.submit_frame(clear, commands)
    }

    fn register_bitmap(&mut self, bitmap: Bitmap) -> Result<BitmapHandle, RuffleError> {
        self.backend.register_bitmap(bitmap)
    }

    fn update_texture(
        &mut self,
        bitmap: &BitmapHandle,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), RuffleError> {
        self.backend.update_texture(bitmap, width, height, rgba)
    }

    fn create_context3d(&mut self) -> Result<Box<dyn Context3D>, RuffleError> {
        self.backend.create_context3d()
    }

    fn context3d_present<'gc>(
        &mut self,
        context: &mut dyn Context3D,
        commands: Vec<Context3DCommand<'gc>>,
        mc: MutationContext<'gc, '_>,
    ) -> Result<(), RuffleError> {
        self.backend.context3d_present(context, commands, mc)
    }

    fn debug_info(&self) -> Cow<'static, str> {
        self.backend.debug_info()
    }
}
