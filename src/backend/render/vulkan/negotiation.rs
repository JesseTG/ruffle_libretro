use std::error::Error;
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::fmt::Debug;
use std::mem::transmute;
use std::ptr;
use std::sync::Once;

use ash::extensions::{ext, khr};
use ash::vk;
use ash::vk::{ApplicationInfo, PFN_vkGetInstanceProcAddr};
use log::{debug, error, info, log_enabled, warn};
use rust_libretro::anyhow;
use rust_libretro::contexts::LoadGameContext;
use rust_libretro_sys::retro_hw_render_context_negotiation_interface_type::RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN;
use rust_libretro_sys::{
    retro_hw_render_context_negotiation_interface_type, retro_hw_render_context_negotiation_interface_vulkan,
    retro_vulkan_context, retro_vulkan_create_device_wrapper_t, retro_vulkan_create_instance_wrapper_t,
    RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;
use wgpu_hal::{Api, ExposedAdapter, InstanceFlags, OpenDevice};

use crate::backend::render::vulkan::context::{RetroVulkanCreatedContext, RetroVulkanInitialContext};
use crate::backend::render::vulkan::util;
use crate::backend::render::vulkan::util::PropertiesFormat;
use crate::built_info;

#[derive(ThisError, Debug)]
pub enum VulkanNegotiationError {
    #[error("Vulkan error in {0}: {1}")]
    VulkanError(&'static str, ash::vk::Result),

    #[error("No VkPhysicalDevices were found")]
    NoPhysicalDevicesFound,

    #[error("No available VkPhysicalDevices were acceptable")]
    NoAcceptablePhysicalDevice,

    #[error("No acceptable Vulkan queue family could be found")]
    NoAcceptableQueueFamily,

    #[error("Cannot expose VkPhysicalDevice from VkInstance")]
    CannotExposePhysicalDevice,
}

type VulkanInstance = <Vulkan as Api>::Instance;
type VulkanDevice = <Vulkan as Api>::Device;
type VulkanPhysicalDevice = <Vulkan as Api>::Adapter;
type VulkanQueue = <Vulkan as Api>::Queue;
type VulkanPhysicalDeviceInfo = ExposedAdapter<Vulkan>;
type VulkanOpenDevice = OpenDevice<Vulkan>;

pub struct VulkanContextNegotiationInterface {
    application_info: ApplicationInfo,
    initial_context: Option<RetroVulkanInitialContext>,
    pub created_context: Option<RetroVulkanCreatedContext>,
}

/// This MUST be kept as a constant, and must *not* be given to a CString.
/// Otherwise you risk undefined behavior; this has already bitten me in the ass.
/// (See the git blame for this line for details.)
const APPLICATION_NAME: &[u8] = b"ruffle_libretro\0";

// TODO: Should I put this behind a mutex?
static mut INSTANCE: Option<VulkanContextNegotiationInterface> = None;
static ONCE: Once = Once::new();

unsafe extern "C" fn get_application_info() -> *const ApplicationInfo {
    &INSTANCE.as_ref().unwrap().application_info
}

unsafe extern "C" fn create_instance(
    get_instance_proc_addr: Option<ash::vk::PFN_vkGetInstanceProcAddr>,
    app: *const ApplicationInfo,
    create_instance_wrapper: retro_vulkan_create_instance_wrapper_t,
    opaque: *mut c_void,
) -> vk::Instance {
    let mut required_instance_extensions: Vec<&'static CStr> = vec![
        khr::Surface::name(),
        vk::KhrGetPhysicalDeviceProperties2Fn::name(),
        vk::ExtSwapchainColorspaceFn::name(),
    ];

    if cfg!(debug_assertions) {
        required_instance_extensions.push(ext::DebugUtils::name());
    } // Logic taken from `VulkanHalInstance::required_extensions`

    let required_instance_extensions : Vec<*const c_char> = required_instance_extensions
        .iter()
        .map(|c| c.as_ptr())
        .collect();

    let instance_create_info = vk::InstanceCreateInfo::builder()
        .application_info(&*app)
        .enabled_extension_names(&required_instance_extensions)
        .build();

    let instance = create_instance_wrapper.unwrap()(opaque, &instance_create_info);

    instance
}

unsafe extern "C" fn create_device2(
    context: *mut retro_vulkan_context,
    instance: vk::Instance,
    gpu: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    get_instance_proc_addr: Option<PFN_vkGetInstanceProcAddr>,
    create_device_wrapper: retro_vulkan_create_device_wrapper_t,
    opaque: *mut c_void,
) -> bool {
    if context.is_null() {
        error!("Frontend provided create_device with a null retro_vulkan_context");
        return false;
    }

    let interface = INSTANCE.as_mut().unwrap();
    let initial_context = match RetroVulkanInitialContext::new(
        instance,
        gpu,
        surface,
        get_instance_proc_addr,
        create_device_wrapper,
    ) {
        Ok(initial_context) => initial_context,
        Err(error) => {
            error!("Error creating initialization context: {error}");
            return false;
        }
    };

    match initial_context.entry.try_enumerate_instance_version() {
        Ok(Some(version)) => {
            let major = vk::api_version_major(version);
            let minor = vk::api_version_minor(version);
            let patch = vk::api_version_patch(version);
            let variant = vk::api_version_variant(version);

            info!("Using Vulkan {major}.{minor}.{patch} (variant {variant})");
        }
        Ok(None) => {
            info!("Using unknown Vulkan version");
        }
        Err(error) => {
            error!("Error querying active Vulkan version: {error}");
            return false;
        }
    };

    if log_enabled!(log::Level::Debug) {
        match initial_context.entry.enumerate_instance_extension_properties(None) {
            Ok(extensions) => {
                let extensions = PropertiesFormat::new(&extensions);
                debug!("Available instance extensions: {extensions:#?}");
            }
            Err(error) => {
                warn!("Failed to query available instance extensions: {error}");
            }
        };

        match initial_context.entry.enumerate_instance_layer_properties() {
            Ok(layers) => {
                let layers = PropertiesFormat::new(&layers);
                debug!("Available instance layers: {layers:#?}");
            }
            Err(error) => {
                warn!("Failed to query available instance layers: {error}");
            }
        };
    }

    interface.initial_context = Some(initial_context);

    match RetroVulkanCreatedContext::new(interface.initial_context.as_ref().unwrap(), opaque) {
        Ok(created_context) => {
            info!("Created VkDevice {:?}", created_context.device.handle());

            let mut context = &mut (*context);
            context.gpu = created_context.physical_device;
            context.device = created_context.device.handle();
            context.queue = created_context.queue;
            context.queue_family_index = created_context.queue_family_index;
            context.presentation_queue = created_context.presentation_queue;
            context.presentation_queue_family_index = created_context.presentation_queue_family_index;
            interface.created_context = Some(created_context);

            true
        }
        Err(error) => {
            error!("Failed to create VkDevice: {error}");
            false
        }
    }
}

pub fn enable(ctx: &mut LoadGameContext) -> anyhow::Result<()> {
    ONCE.call_once(|| unsafe {
        let application_info = ApplicationInfo::builder()
            .api_version(vk::API_VERSION_1_3)
            .application_name(CStr::from_ptr(APPLICATION_NAME.as_ptr() as *const c_char))
            .application_version(vk::make_api_version(
                0,
                built_info::PKG_VERSION_MAJOR.parse().unwrap(),
                built_info::PKG_VERSION_MINOR.parse().unwrap(),
                built_info::PKG_VERSION_PATCH.parse().unwrap(),
            ))
            .build();


        let result = ctx.enable_hw_render_negotiation_interface_vulkan(
            Some(get_application_info),
            None,
            // No need for a destroy_device because create_device won't allocate
            // any resources that the frontend won't deallocate itself.
            // That includes VkQueues and VkPhysicalDevices;
            // those are deallocated by Vulkan automatically.
            None,
            Some(create_instance),
            Some(create_device2),
        );


        INSTANCE = Some(VulkanContextNegotiationInterface {
            application_info,
            initial_context: None,
            created_context: None,
        });
    });

    Ok(())
}
