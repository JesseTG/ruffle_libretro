use std::mem;

use ruffle_render_wgpu::{ColorAdjustments, Transforms};

// We try to request the highest limits we can get away with
pub fn required_limits(adapter: &wgpu::Adapter) -> (wgpu::Limits, wgpu::Features) {
    // We start off with the lowest limits we actually need - basically GL-ES 3.0
    let adapter_limits = adapter.limits();
    let adapter_features = adapter.features();
    let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
    // Then we increase parts of it to the maximum supported by the adapter, to take advantage of
    // more powerful hardware or capabilities
    limits = limits.using_resolution(adapter_limits.clone());
    limits = limits.using_alignment(adapter_limits.clone());

    let mut features = wgpu::Features::DEPTH32FLOAT_STENCIL8;

    let needed_size = (mem::size_of::<Transforms>() + mem::size_of::<ColorAdjustments>()) as u32;
    if adapter_features.contains(wgpu::Features::PUSH_CONSTANTS)
        && adapter_limits.max_push_constant_size >= needed_size
    {
        limits.max_push_constant_size = needed_size;
        features |= wgpu::Features::PUSH_CONSTANTS;
    }

    if adapter_features.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    }

    limits.max_storage_buffers_per_shader_stage = adapter_limits.max_storage_buffers_per_shader_stage;
    limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
    limits.max_bind_groups = adapter_limits.max_bind_groups;

    (limits, features)
}
