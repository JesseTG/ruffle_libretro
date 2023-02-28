// We try to request the highest limits we can get away with
pub fn required_limits(adapter: &wgpu::Adapter) -> (wgpu::Limits, wgpu::Features) {
    // We start off with the lowest limits we actually need - basically GL-ES 3.0
    let adapter_limits = adapter.limits();
    let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
    // Then we increase parts of it to the maximum supported by the adapter, to take advantage of
    // more powerful hardware or capabilities
    limits = limits.using_resolution(adapter_limits.clone());
    limits = limits.using_alignment(adapter_limits.clone());

    limits.max_storage_buffers_per_shader_stage = adapter_limits.max_storage_buffers_per_shader_stage;
    limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
    limits.max_bind_groups = adapter_limits.max_bind_groups;

    let features = wgpu::Features::DEPTH32FLOAT_STENCIL8;

    (limits, features)
}
