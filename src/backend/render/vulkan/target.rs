use ash::vk;

use ruffle_render_wgpu::target::RenderTarget;
use ruffle_render_wgpu::target::RenderTargetFrame;
use wgpu_core::api::Vulkan;

type Error = Box<dyn std::error::Error>;

use std::fmt::Debug;

#[derive(Debug)]
pub struct RetroTextureTarget {
    pub size: wgpu::Extent3d,
    pub texture: wgpu::Texture,
    pub format: wgpu::TextureFormat,
}

#[derive(Debug)]
pub struct RetroTextureTargetFrame(wgpu::TextureView);

impl RenderTargetFrame for RetroTextureTargetFrame {
    fn view(&self) -> &wgpu::TextureView {
        &self.0
    }

    fn into_view(self) -> wgpu::TextureView {
        self.0
    }
}

const TARGET_DEBUG_LABEL: &str = "Ruffle Intermediate Texture";

impl RetroTextureTarget {
    pub fn new(device: &wgpu::Device, size: (u32, u32), format: wgpu::TextureFormat) -> Result<Self, Error> {
        let device_limits = device.limits();
        if size.0 > device_limits.max_texture_dimension_2d
            || size.1 > device_limits.max_texture_dimension_2d
            || size.0 < 1
            || size.1 < 1
        {
            return Err(format!(
                "Texture target cannot be smaller than 1 or larger than {}px on either dimension (requested {} x {})",
                device_limits.max_texture_dimension_2d, size.0, size.1
            )
            .into());
        }
        let size = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(TARGET_DEBUG_LABEL),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            view_formats: &[format],
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        Ok(Self { size, texture, format })
    }

    pub fn get_texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub unsafe fn get_vk_image(&self) -> Option<vk::Image> {
        let mut image: Option<vk::Image> = None;
        self.texture.as_hal::<Vulkan, _>(|t| {
            image = t.map(|t| t.raw_handle());
        });
        image
    }
}

impl RenderTarget for RetroTextureTarget {
    type Frame = RetroTextureTargetFrame;

    fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: self.texture.depth_or_array_layers(),
        };
        self.texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(TARGET_DEBUG_LABEL),
            size: self.size,
            mip_level_count: self.texture.mip_level_count(),
            sample_count: self.texture.sample_count(),
            dimension: self.texture.dimension(),
            format: self.format,
            view_formats: &[self.format],
            usage: self.texture.usage(),
        });
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    fn width(&self) -> u32 {
        self.size.width
    }

    fn height(&self) -> u32 {
        self.size.height
    }

    fn get_next_texture(&mut self) -> Result<Self::Frame, wgpu::SurfaceError> {
        Ok(RetroTextureTargetFrame(self.texture.create_view(&Default::default())))
    }

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        command_buffers: I,
        _frame: Self::Frame,
    ) -> wgpu::SubmissionIndex {
        queue.submit(command_buffers)
    }
}
