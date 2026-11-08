use std::any::Any;

use log::{info, warn};
use ruffle_core::Player;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::target::TextureTarget;
use ruffle_render_wgpu::utils::capture_image;
use ruffle_render_wgpu::wgpu;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum RenderError {
    #[error("No usable graphics device was found: {0}")]
    NoDevice(String),
}

/// Creates a renderer that draws into an offscreen texture on its own device.
///
/// Rendering doesn't go through the frontend's hardware context: frames are copied
/// back to system memory and handed over as a normal framebuffer. That works with
/// every libretro video driver, at the cost of one small GPU-to-CPU copy per frame.
pub fn create_renderer(width: u32, height: u32) -> Result<WgpuRenderBackend<TextureTarget>, RenderError> {
    let size = (width, height);
    let power = wgpu::PowerPreference::HighPerformance;

    let backend = match WgpuRenderBackend::for_offscreen(size, wgpu::Backends::PRIMARY, power) {
        Ok(backend) => backend,
        Err(primary_error) => {
            warn!("No primary graphics backend available ({primary_error}), falling back to OpenGL");
            WgpuRenderBackend::for_offscreen(size, wgpu::Backends::GL, power)
                .map_err(|gl_error| RenderError::NoDevice(format!("{primary_error}; {gl_error}")))?
        }
    };

    let info = backend.descriptors().adapter.get_info();
    info!("Rendering with {} on {:?} ({})", info.name, info.backend, info.driver);

    Ok(backend)
}

/// Copies the most recently rendered frame into `frame` as XRGB8888 pixels,
/// resizing it to `width * height` if needed. Returns `false` if there was nothing to copy.
pub fn read_frame(player: &mut Player, frame: &mut Vec<u8>) -> bool {
    profiling::scope!("read_frame");
    let Some(renderer) = <dyn Any>::downcast_ref::<WgpuRenderBackend<TextureTarget>>(player.renderer_mut()) else {
        return false;
    };

    let target = renderer.target();
    let Some(buffer_info) = &target.buffer else {
        return false;
    };

    let (buffer, dimensions) = buffer_info.buffer.inner();
    let (width, height) = (target.size.width as usize, target.size.height as usize);
    frame.resize(width * height * 4, 0);

    capture_image(&renderer.descriptors().device, buffer, dimensions, None, |rgba, padded_bytes_per_row| {
        for (src_row, dst_row) in rgba
            .chunks_exact(padded_bytes_per_row as usize)
            .zip(frame.chunks_exact_mut(width * 4))
            .take(height)
        {
            for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                // The texture holds premultiplied RGBA, which is exactly the
                // colour composited over black; XRGB8888 ignores the X byte.
                let pixel = 0xFF00_0000 | u32::from(src[0]) << 16 | u32::from(src[1]) << 8 | u32::from(src[2]);
                dst.copy_from_slice(&pixel.to_ne_bytes());
            }
        }
    });

    true
}
