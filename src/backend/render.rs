use std::any::Any;
use std::sync::{Arc, Mutex};

use log::{info, warn};
use ruffle_core::Player;
use ruffle_render_wgpu::backend::{create_wgpu_instance, request_adapter_and_device, WgpuRenderBackend};
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::{RenderTarget, RenderTargetFrame};
use ruffle_render_wgpu::utils::{capture_image, BufferDimensions};
use ruffle_render_wgpu::wgpu;
use rust_libretro::anyhow::{self, anyhow};

/// Creates a renderer that draws into an offscreen texture on its own device.
///
/// Rendering doesn't go through the frontend's hardware context: frames are copied
/// back to system memory and handed over as a normal framebuffer. That works with
/// every libretro video driver, at the cost of one small GPU-to-CPU copy per frame.
pub fn create_renderer(width: u32, height: u32) -> anyhow::Result<WgpuRenderBackend<ReadbackTarget>> {
    let size = (width, height);
    let power = wgpu::PowerPreference::HighPerformance;

    let backend = match create_backend(size, wgpu::Backends::PRIMARY, power) {
        Ok(backend) => backend,
        Err(primary_error) => {
            warn!("No primary graphics backend available ({primary_error}), falling back to OpenGL");
            create_backend(size, wgpu::Backends::GL, power)
                .map_err(|gl_error| anyhow!("No usable graphics device was found: {primary_error}; {gl_error}"))?
        }
    };

    let info = backend.descriptors().adapter.get_info();
    info!("Rendering with {} on {:?} ({})", info.name, info.backend, info.driver);

    Ok(backend)
}

/// Like `WgpuRenderBackend::for_offscreen`, but rendering into a [`ReadbackTarget`].
fn create_backend(
    size: (u32, u32),
    backends: wgpu::Backends,
    power: wgpu::PowerPreference,
) -> Result<WgpuRenderBackend<ReadbackTarget>, Box<dyn std::error::Error>> {
    let instance = create_wgpu_instance(backends, wgpu::BackendOptions::default(), None);
    let (adapter, device, queue) =
        futures::executor::block_on(request_adapter_and_device(backends, &instance, None, power))?;
    let target = ReadbackTarget::new(&device, size);
    WgpuRenderBackend::new(Arc::new(Descriptors::new(instance, adapter, device, queue)), target)
}

/// An offscreen render target that copies each frame into one of two buffers,
/// so the previous frame can be read while the GPU is still drawing the next one.
#[derive(Debug)]
pub struct ReadbackTarget {
    size: wgpu::Extent3d,
    texture: wgpu::Texture,
    dimensions: BufferDimensions,
    buffers: [wgpu::Buffer; 2],
    state: Mutex<ReadbackState>,
}

#[derive(Debug, Default)]
struct ReadbackState {
    /// The buffer the next frame is copied into.
    next: usize,
    /// The submission that copied a frame into each buffer, until that frame is read.
    pending: [Option<wgpu::SubmissionIndex>; 2],
}

#[derive(Debug)]
pub struct ReadbackFrame(wgpu::TextureView);

impl RenderTargetFrame for ReadbackFrame {
    fn into_view(self) -> wgpu::TextureView {
        self.0
    }

    fn view(&self) -> &wgpu::TextureView {
        &self.0
    }
}

impl ReadbackTarget {
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    fn new(device: &wgpu::Device, (width, height): (u32, u32)) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Readback target texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            view_formats: &[Self::FORMAT],
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        let dimensions = BufferDimensions::new(width as usize, height as usize, Self::FORMAT);
        let buffers = [(); 2].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Readback target buffer"),
                size: dimensions.size(),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });

        Self {
            size,
            texture,
            dimensions,
            buffers,
            state: Mutex::default(),
        }
    }

    /// Passes the oldest unread frame to `with_rgba`, skipping one submitted since the
    /// last call if `rendered` is set, since the GPU is likely still drawing it.
    /// Returns `None` if there's no such frame.
    fn read<R>(&self, device: &wgpu::Device, rendered: bool, with_rgba: impl FnOnce(&[u8], u32) -> R) -> Option<R> {
        let mut state = self.state.lock().expect("readback lock poisoned");
        let index = if rendered { state.next } else { 1 - state.next };
        let submission = state.pending[index].take()?;
        Some(capture_image(device, &self.buffers[index], &self.dimensions, Some(submission), with_rgba))
    }
}

impl RenderTarget for ReadbackTarget {
    type Frame = ReadbackFrame;

    fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        *self = Self::new(device, (width, height));
    }

    fn format(&self) -> wgpu::TextureFormat {
        Self::FORMAT
    }

    fn width(&self) -> u32 {
        self.size.width
    }

    fn height(&self) -> u32 {
        self.size.height
    }

    fn get_next_texture(&mut self) -> Option<Self::Frame> {
        Some(ReadbackFrame(self.texture.create_view(&Default::default())))
    }

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        command_buffers: I,
        _frame: Self::Frame,
    ) -> wgpu::SubmissionIndex {
        let mut state = self.state.lock().expect("readback lock poisoned");
        let index = state.next;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Readback transfer encoder"),
        });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffers[index],
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.dimensions.padded_bytes_per_row),
                    rows_per_image: None,
                },
            },
            self.size,
        );

        let submission = queue.submit(command_buffers.into_iter().chain([encoder.finish()]));
        state.pending[index] = Some(submission.clone());
        state.next = 1 - index;
        submission
    }
}

/// Copies the oldest unread frame into `frame` as XRGB8888 pixels, resizing it to
/// `width * height` if needed. Set `rendered` if the player rendered since the last call.
/// Returns `false` if there was nothing to copy.
pub fn read_frame(player: &mut Player, frame: &mut Vec<u8>, rendered: bool) -> bool {
    profiling::scope!("read_frame");
    let Some(renderer) = <dyn Any>::downcast_ref::<WgpuRenderBackend<ReadbackTarget>>(player.renderer_mut()) else {
        return false;
    };

    let target = renderer.target();
    let (width, height) = (target.size.width as usize, target.size.height as usize);
    frame.resize(width * height * 4, 0);

    target
        .read(renderer.device(), rendered, |rgba, padded_bytes_per_row| {
            for (src_row, dst_row) in rgba
                .chunks_exact(padded_bytes_per_row as usize)
                .zip(frame.chunks_exact_mut(width * 4))
                .take(height)
            {
                for (src, dst) in src_row.as_chunks::<4>().0.iter().zip(dst_row.as_chunks_mut::<4>().0) {
                    // The texture holds premultiplied RGBA, which is exactly the
                    // colour composited over black; XRGB8888 ignores the X byte.
                    let pixel = 0xFF00_0000 | u32::from(src[0]) << 16 | u32::from(src[1]) << 8 | u32::from(src[2]);
                    dst.copy_from_slice(&pixel.to_ne_bytes());
                }
            }
        })
        .is_some()
}
