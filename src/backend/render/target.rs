use ruffle_render_wgpu::target::{RenderTarget, RenderTargetFrame};
use std::fmt::{Debug, Formatter};
use wgpu::{CommandBuffer, Device, Queue, SurfaceError, TextureFormat, TextureView};

#[derive(Debug)]
pub struct RetroRenderTarget {}

#[derive(Debug)]
pub struct RetroRenderTargetFrame {}

impl RenderTarget for RetroRenderTarget {
    type Frame = RetroRenderTargetFrame;

    fn resize(&mut self, device: &Device, width: u32, height: u32) {
        todo!()
    }

    fn format(&self) -> TextureFormat {
        todo!()
    }

    fn width(&self) -> u32 {
        todo!()
    }

    fn height(&self) -> u32 {
        todo!()
    }

    fn get_next_texture(&mut self) -> Result<Self::Frame, SurfaceError> {
        todo!()
    }

    fn submit<I: IntoIterator<Item = CommandBuffer>>(
        &self,
        device: &Device,
        queue: &Queue,
        command_buffers: I,
        frame: Self::Frame,
    ) {
        todo!()
    }
}

impl RetroRenderTarget {
    pub fn new() -> Self {
        Self {

        }
    }
}

impl RenderTargetFrame for RetroRenderTargetFrame
{
    fn into_view(self) -> TextureView {
        todo!()
    }

    fn view(&self) -> &TextureView {
        todo!()
    }
}