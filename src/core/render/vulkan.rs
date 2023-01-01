use std::error::Error;
use std::ffi::CStr;

use ash::vk::StaticFn;
use futures::executor::block_on;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;

use crate::core::render::RenderInterface;
use crate::core::render::RenderInterfaceError::WrongRenderInterface;
use crate::core::Ruffle;

impl Ruffle {
    pub(crate) unsafe fn get_vulkan_descriptors(
        &self,
        interface: &RenderInterface,
    ) -> Result<Descriptors, Box<dyn Error>> {
        let interface = match interface {
            RenderInterface::Vulkan(vulkan) => vulkan,
            _ => Err(WrongRenderInterface)?,
        };

        let static_fn = StaticFn {
            get_instance_proc_addr: interface.get_instance_proc_addr,
        };

        let instance = ash::Instance::load(&static_fn, interface.instance);
        let device = ash::Device::load(instance.fp_v1_0(), interface.device);
        let extensions: Vec<&CStr> = match instance.enumerate_device_extension_properties(interface.gpu) {
            Ok(properties) => properties
                .iter()
                .map(|p| CStr::from_ptr(p.extension_name.as_ptr()))
                .collect(),
            Err(error) => Err(error)?,
        };

        let descriptors = WgpuRenderBackend::<TextureTarget>::build_descriptors_for_vulkan(
            interface.gpu,
            device,
            false,
            extensions.as_slice(),
            wgpu::Features::all_native_mask(), // TODO: Populate this properly
            wgpu_hal::UpdateAfterBindTypes::all(),
            interface.queue_index, // I think this field is misnamed
            0,
            None,
        );

        block_on(descriptors)
    }
}
