use std::error::Error;
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::fmt::Debug;
use std::sync::Once;

use ash::extensions::{ext, khr};
use ash::vk;
use ash::vk::{ApplicationInfo, PFN_vkGetInstanceProcAddr};
use log::{debug, error, info, log_enabled, warn};
use rust_libretro_sys::{
    retro_hw_render_context_negotiation_interface_type, retro_hw_render_context_negotiation_interface_vulkan,
    RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION, retro_vulkan_context,
};
use rust_libretro_sys::retro_hw_render_context_negotiation_interface_type::RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN;
use thiserror::Error as ThisError;
use wgpu_hal::{Api, ExposedAdapter, InstanceFlags, OpenDevice};
use wgpu_hal::api::Vulkan;

use crate::backend::render::HardwareRenderContextNegotiationInterface;
use crate::backend::render::vulkan::context::{
    RetroVulkanCreatedContext, RetroVulkanInitialContext,
};
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
    interface: retro_hw_render_context_negotiation_interface_vulkan,
    application_info: ApplicationInfo,
    initial_context: Option<RetroVulkanInitialContext>,
    pub created_context: Option<RetroVulkanCreatedContext>,
    required_instance_extensions: Vec<&'static CStr>,
}

/// This MUST be kept as a constant, and must *not* be given to a CString.
/// Otherwise you risk undefined behavior; this has already bitten me in the ass.
/// (See the git blame for this line for details.)
const APPLICATION_NAME: &[u8] = b"ruffle_libretro\0";

// TODO: Should I put this behind a mutex?
static mut INSTANCE: Option<VulkanContextNegotiationInterface> = None;
static ONCE: Once = Once::new();

impl VulkanContextNegotiationInterface {
    pub fn get_instance() -> Result<&'static VulkanContextNegotiationInterface, Box<dyn Error>> {
        unsafe {
            ONCE.call_once(|| {
                let interface = retro_hw_render_context_negotiation_interface_vulkan {
                    interface_type: RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN,
                    interface_version: RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
                    get_application_info: Some(Self::get_application_info),
                    create_device: Some(Self::create_device),
                    //get_instance_extensions: Some(Self::get_instance_extensions),
                    //get_instance_layers: None,

                    // No need for a destroy_device because create_device won't allocate
                    // any resources that the frontend won't deallocate itself.
                    // That includes VkQueues and VkPhysicalDevices;
                    // those are deallocated by Vulkan automatically.
                    destroy_device: None,
                };

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

                // I'd like to be able to use VulkanHalInstance here to get the extensions,
                // but that requires an ash::Entry, and Vulkan hasn't been loaded yet.
                let flags = if cfg!(debug_assertions) {
                    InstanceFlags::VALIDATION | InstanceFlags::DEBUG
                } else {
                    InstanceFlags::empty()
                }; // Logic taken from `VulkanHalInstance::init`

                let mut required_instance_extensions: Vec<&'static CStr> = vec![
                    khr::Surface::name(),
                    vk::KhrGetPhysicalDeviceProperties2Fn::name(),
                    vk::ExtSwapchainColorspaceFn::name(),
                ];

                if flags.contains(InstanceFlags::DEBUG) {
                    required_instance_extensions.push(ext::DebugUtils::name());
                } // Logic taken from `VulkanHalInstance::required_extensions`

                INSTANCE = Some(VulkanContextNegotiationInterface {
                    interface,
                    application_info,
                    initial_context: None,
                    created_context: None,
                    required_instance_extensions,
                })
            });

            Ok(INSTANCE.as_ref().unwrap())
        }
    }

    unsafe extern "C" fn get_application_info() -> *const ApplicationInfo {
        &INSTANCE.as_ref().unwrap().application_info
    }

    unsafe extern "C" fn get_instance_extensions(num_instance_extensions: *mut c_uint) -> *const *const c_char {
        let interface = INSTANCE.as_mut().unwrap();
        let required_instance_extensions = &interface.required_instance_extensions;

        debug!("Instance extensions required by wgpu: {required_instance_extensions:#?}");
        let required_instance_extensions: Vec<*const c_char> = required_instance_extensions
            .iter()
            .map(|e| e.as_ptr())
            .collect();

        required_instance_extensions.as_ptr()
    }

    /// Creates a [`vk::Device`] and assigns ownership of it to the libretro frontend,
    /// per the description of [`retro_hw_render_context_negotiation_interface_vulkan`]`::create_device`.
    ///
    unsafe extern "C" fn create_device(
        context: *mut retro_vulkan_context,
        instance: vk::Instance,
        gpu: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        get_instance_proc_addr: Option<PFN_vkGetInstanceProcAddr>,
        required_device_extensions: *mut *const c_char,
        num_required_device_extensions: c_uint,
        required_device_layers: *mut *const c_char,
        num_required_device_layers: c_uint,
        required_features: *const vk::PhysicalDeviceFeatures,
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
            required_device_extensions,
            num_required_device_extensions,
            required_device_layers,
            num_required_device_layers,
            required_features,
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
        }

        if log_enabled!(log::Level::Debug) {
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

        if !initial_context.required_device_layers.is_empty() {
            warn!("Frontend requested specific device layers, but this core doesn't check for them yet");
            warn!("Please file a bug");
        }

        if util::physical_device_features_any(initial_context.required_features) {
            warn!("Frontend requested some VkPhysicalDeviceFeatures, but this core doesn't check for them yet");
            warn!("Please file a bug");
            warn!("The features in question: {required_features:?}");
        }

        interface.initial_context = Some(initial_context);

        match RetroVulkanCreatedContext::new(interface.initial_context.as_ref().unwrap()) {
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
}

impl HardwareRenderContextNegotiationInterface for VulkanContextNegotiationInterface {
    unsafe fn get_ptr(&self) -> *const c_void {
        (&self.interface as *const _) as *const c_void
    }

    fn r#type(&self) -> retro_hw_render_context_negotiation_interface_type {
        RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN
    }
}
