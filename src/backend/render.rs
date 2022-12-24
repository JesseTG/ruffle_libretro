use ruffle_core::Color;
use ruffle_core::swf::Glyph;
use ruffle_render::backend;
use ruffle_render::backend::{Context3D, Context3DCommand, RenderBackend, ShapeHandle, ViewportDimensions};
use ruffle_render::bitmap::{Bitmap, BitmapHandle, BitmapSource};
use ruffle_render::commands::CommandList;
use ruffle_render::error::Error;
use ruffle_render::shape_utils::DistilledShape;


pub struct RetroRenderBackend {
}

impl RetroRenderBackend {
    pub fn new() -> Self {
        Self {}
    }
}

impl RenderBackend for RetroRenderBackend {
    fn viewport_dimensions(&self) -> ViewportDimensions {
        todo!()
    }

    fn set_viewport_dimensions(&mut self, dimensions: ViewportDimensions) {
        todo!()
    }

    fn register_shape(&mut self, shape: DistilledShape, bitmap_source: &dyn BitmapSource) -> ShapeHandle {
        todo!()
    }

    fn replace_shape(&mut self, shape: DistilledShape, bitmap_source: &dyn BitmapSource, handle: ShapeHandle) {
        todo!()
    }

    fn register_glyph_shape(&mut self, shape: &Glyph) -> ShapeHandle {
        todo!()
    }

    fn render_offscreen(&mut self, handle: BitmapHandle, width: u32, height: u32, commands: CommandList) -> Result<Bitmap, Error> {
        todo!()
    }

    fn submit_frame(&mut self, clear: Color, commands: CommandList) {
        todo!()
    }

    fn register_bitmap(&mut self, bitmap: Bitmap) -> Result<BitmapHandle, Error> {
        todo!()
    }

    fn update_texture(&mut self, bitmap: &BitmapHandle, width: u32, height: u32, rgba: Vec<u8>) -> Result<(), Error> {
        todo!()
    }

    fn create_context3d(&mut self) -> Result<Box<dyn Context3D>, Error> {
        todo!()
    }

    fn context3d_present<'gc>(&mut self, context: &mut dyn Context3D, commands: Vec<Context3DCommand<'gc>>, mc: gc_arena::context::MutationContext<'gc, '_>) -> Result<(), Error> {
        todo!()
    }
}