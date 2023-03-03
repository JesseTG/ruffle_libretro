use std::fmt::Debug;

use ash::vk;
#[cfg(feature = "profiler")]
use profiling;
use ruffle_render_wgpu::target::RenderTarget;
use ruffle_render_wgpu::target::RenderTargetFrame;
use rust_libretro_sys::retro_vulkan_image;
use wgpu_core::api::Vulkan;

type Error = Box<dyn std::error::Error>;

#[derive(Debug)]
pub struct RetroTextureTarget {
    size: wgpu::Extent3d,
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    create_info: vk::ImageViewCreateInfo,
    image_view: vk::ImageView,
    retro_image: retro_vulkan_image,
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
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroTextureTarget::new");

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

        unsafe {
            let device = get_vk_device(device);
            let (create_info, image_view) = Self::create_image_view(&device, &texture);
            let retro_image = retro_vulkan_image {
                image_view,
                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                create_info,
            };

            Ok(Self {
                size,
                texture,
                format,
                create_info,
                image_view,
                retro_image,
            })
        }
    }

    pub fn get_texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn get_retro_image(&self) -> &retro_vulkan_image {
        &self.retro_image
    }

    pub fn get_image_view(&self) -> vk::ImageView {
        self.image_view
    }

    unsafe fn get_vk_image(texture: &wgpu::Texture) -> Option<vk::Image> {
        let mut image = None;

        texture.as_hal::<Vulkan, _>(|t| {
            image = t.map(|t| t.raw_handle());
        });

        image
    }

    unsafe fn create_image_view(
        device: &ash::Device,
        texture: &wgpu::Texture,
    ) -> (vk::ImageViewCreateInfo, vk::ImageView) {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroTextureTarget::create_image_view");
        let image = Self::get_vk_image(texture).unwrap();

        let create_info = vk::ImageViewCreateInfo::builder()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .subresource_range(vk::ImageSubresourceRange {
                base_mip_level: 0,
                base_array_layer: 0,
                level_count: 1,
                layer_count: 1,
                aspect_mask: vk::ImageAspectFlags::COLOR,
            })
            .components(vk::ComponentMapping {
                r: vk::ComponentSwizzle::R,
                g: vk::ComponentSwizzle::G,
                b: vk::ComponentSwizzle::B,
                a: vk::ComponentSwizzle::A,
            })
            .build();

        let image_view = device.create_image_view(&create_info, None).unwrap();

        (create_info, image_view)
    }
}

impl RenderTarget for RetroTextureTarget {
    type Frame = RetroTextureTargetFrame;

    fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroTextureTarget::resize");
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

        unsafe {
            let device = get_vk_device(device);
            device.destroy_image_view(self.image_view, None);

            let (create_info, image_view) = Self::create_image_view(&device, &self.texture);
            self.create_info = create_info;
            self.image_view = image_view;
            self.retro_image.image_view = image_view;
            self.retro_image.create_info = create_info;
        }
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
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroTextureTarget::get_next_texture");
        Ok(RetroTextureTargetFrame(self.texture.create_view(&Default::default())))
    }

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        command_buffers: I,
        _frame: Self::Frame,
    ) -> wgpu::SubmissionIndex {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroTextureTarget::submit");
        queue.submit(command_buffers)
    }
}

unsafe fn get_vk_device(device: &wgpu::Device) -> ash::Device {
    let mut vk_device = None;
    device.as_hal::<Vulkan, _, _>(|t| vk_device = t.map(|t| t.raw_device().clone()));
    vk_device.unwrap()
}
