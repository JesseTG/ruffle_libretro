use std::ffi::{c_char, c_uint, c_void, CString};
use std::mem::transmute;
use std::ptr;
use std::slice::from_raw_parts;
use std::sync::Once;

use ash::{Entry, Instance, vk};
use ash::vk::{
    ApplicationInfo, DeviceCreateInfo, DeviceQueueCreateInfo, PFN_vkGetInstanceProcAddr, PhysicalDeviceFeatures,
    StaticFn,
};
use log::error;
use rust_libretro_sys::{
    retro_hw_render_context_negotiation_interface_type,
    retro_hw_render_context_negotiation_interface_vulkan, RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
    retro_vulkan_context,
};
use rust_libretro_sys::retro_hw_render_context_negotiation_interface_type::RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN;
use thiserror::Error as ThisError;

use crate::backend::render::HardwareRenderContextNegotiationInterface;
use crate::built_info;

#[derive(ThisError, Debug)]
pub enum NegotiationInterfaceError {
    #[error("Couldn't use the provided environment callback")]
    InvalidEnvironmentCallback,
}

pub struct VulkanContextNegotiationInterface {
    interface: retro_hw_render_context_negotiation_interface_vulkan,
    application_info: ApplicationInfo,
    entry: Option<ash::Entry>,
    instance: Option<ash::Instance>,
    required_device_extensions: Vec<CString>,

    // We *could* just recreate the Device object instead of passing it around,
    // but that would entail looking up all of Vulkan's function pointers again.
    device: Option<ash::Device>,
}

// TODO: Should I put this behind a mutex?
static mut INSTANCE: Option<VulkanContextNegotiationInterface> = None;
static ONCE: Once = Once::new();

impl VulkanContextNegotiationInterface {
    pub fn instance() -> Result<&'static VulkanContextNegotiationInterface, NegotiationInterfaceError> {
        unsafe {
            ONCE.call_once(|| {
                let interface = retro_hw_render_context_negotiation_interface_vulkan {
                    interface_type: RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN,
                    interface_version: RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
                    get_application_info: None,//Some(Self::get_application_info),
                    create_device: None, //Some(Self::create_device),
                    destroy_device: None,
                };

                let application_info = ApplicationInfo::builder()
                    .api_version(vk::API_VERSION_1_3)
                    .application_name(CString::new(built_info::PKG_NAME).unwrap().as_c_str())
                    .application_version(vk::make_api_version(
                        0,
                        built_info::PKG_VERSION_MAJOR.parse().unwrap(),
                        built_info::PKG_VERSION_MINOR.parse().unwrap(),
                        built_info::PKG_VERSION_PATCH.parse().unwrap(),
                    ))
                    .build();

                INSTANCE = Some(VulkanContextNegotiationInterface {
                    interface,
                    application_info,
                    instance: None,
                    entry: None,
                    required_device_extensions: vec![],
                    device: None,
                })
            });

            Ok(INSTANCE.as_ref().unwrap())
        }
    }

    pub fn device(&self) -> Option<&ash::Device> {
        self.device.as_ref()
    }

    pub fn required_device_extensions(&self) -> &[CString] {
        self.required_device_extensions.as_slice()
    }

    pub unsafe extern "C" fn get_application_info() -> *const ApplicationInfo {
        &INSTANCE.as_ref().unwrap().application_info
    }

    /* If non-NULL, the libretro core will choose one or more physical devices,
     * create one or more logical devices and create one or more queues.
     * The core must prepare a designated PhysicalDevice, Device, Queue and queue family index
     * which the frontend will use for its internal operation.
     *
     * If gpu is not VK_NULL_HANDLE, the physical device provided to the frontend must be this PhysicalDevice.
     * The core is still free to use other physical devices.
     *
     * The frontend will request certain extensions and layers for a device which is created.
     * The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
     *
     * If surface is not VK_NULL_HANDLE, the core must consider presentation when creating the queues.
     * If presentation to "surface" is supported on the queue, presentation_queue must be equal to queue.
     * If not, a second queue must be provided in presentation_queue and presentation_queue_index.
     * If surface is not VK_NULL_HANDLE, the instance from frontend will have been created with supported for
     * VK_KHR_surface extension.
     *
     * The core is free to set its own queue priorities.
     * Device provided to frontend is owned by the frontend, but any additional device resources must be freed by core
     * in destroy_device callback.
     *
     * If this function returns true, a PhysicalDevice, Device and Queues are initialized.
     * If false, none of the above have been initialized and the frontend will attempt
     * to fallback to "default" device creation, as if this function was never called.
     */
    pub unsafe extern "C" fn create_device(
        context: *mut retro_vulkan_context,
        instance: vk::Instance,
        gpu: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
        required_device_extensions: *mut *const c_char,
        num_required_device_extensions: c_uint,
        required_device_layers: *mut *const c_char,
        num_required_device_layers: c_uint,
        required_features: *const vk::PhysicalDeviceFeatures,
    ) -> bool {
        todo!();

        if context.is_null() {
            error!("Frontend provided create_device with a null retro_vulkan_context");
            return false;
        }

        if instance == vk::Instance::null() {
            error!("Frontend called create_device without a valid VkInstance");
            return false;
        }

        if get_instance_proc_addr as usize == 0 {
            error!("Frontend called create_device with a null PFN_vkGetInstanceProcAddr");
            return false;
        }

        let static_fn = StaticFn::load(|sym| {
            let fun = get_instance_proc_addr(instance, sym.as_ptr());
            fun.unwrap_or(transmute::<*const c_void, unsafe extern "system" fn()>(ptr::null())) as *const c_void
        });

        let entry = Entry::from_static_fn(static_fn);
        let instance = Instance::load(entry.static_fn(), instance);
        let mut interface = INSTANCE.as_mut().unwrap();
        interface.entry = Some(entry);
        interface.instance = Some(instance);
        interface
            .required_device_extensions
            .reserve(num_required_device_extensions as usize);

        let required_device_extensions =
            from_raw_parts(required_device_extensions, num_required_device_extensions as usize);
        let required_device_layers = from_raw_parts(required_device_layers, num_required_device_layers as usize);
        let required_features = if required_features.is_null() {
            PhysicalDeviceFeatures::default()
        } else {
            *required_features
        };

        let mut context = &mut *context;
        context.gpu = if gpu == vk::PhysicalDevice::null() {
            // If the frontend hasn't selected a GPU...

            return false; // TODO: Select a GPU ourselves based on the criteria in the docs
        } else {
            gpu
        };

        let device = {
            let queue_create_info = DeviceQueueCreateInfo::builder().build();

            let presentation_queue_create_info = DeviceQueueCreateInfo::builder().build();

            let queues = [queue_create_info, presentation_queue_create_info];

            let device_create_info = DeviceCreateInfo::builder()
                .enabled_extension_names(required_device_extensions)
                .enabled_layer_names(required_device_layers)
                .enabled_features(&required_features)
                .queue_create_infos(queues.as_slice())
                .build();
            // TODO: Only get the first element if we don't need presentation_queue_create_info

            match instance.create_device(context.gpu, &device_create_info, None) {
                Ok(device) => device,
                Err(error) => {
                    error!("Failed to create a VkDevice: {error}");
                    return false;
                }
            }
        };

        context.device = device.handle();
        interface.device = Some(device);

        todo!()
    }
}

impl HardwareRenderContextNegotiationInterface for VulkanContextNegotiationInterface {
    unsafe fn get_ptr(&self) -> *const c_void {
        (&self.interface as *const _) as *const c_void
    }

    fn r#type(&self) -> retro_hw_render_context_negotiation_interface_type {
        RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN
    }
}
