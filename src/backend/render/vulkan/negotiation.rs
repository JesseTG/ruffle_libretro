use std::error::Error;
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::fmt::{Debug, Formatter};
use std::mem::transmute;
use std::slice::from_raw_parts;
use std::sync::Once;

use ash::vk;
use ash::vk::{ApplicationInfo, PFN_vkGetInstanceProcAddr};
use log::{debug, error, info, log_enabled, warn};
use rust_libretro_sys::retro_hw_render_context_negotiation_interface_type::RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN;
use rust_libretro_sys::{
    retro_hw_render_context_negotiation_interface_type, retro_hw_render_context_negotiation_interface_vulkan,
    retro_vulkan_context, RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;
use wgpu_hal::{Api, ExposedAdapter, OpenDevice};

use crate::backend::render::vulkan::context::{RetroVulkanCreatedContext, RetroVulkanInitialContext};
use crate::backend::render::vulkan::negotiation::VulkanNegotiationError::*;
use crate::backend::render::HardwareRenderContextNegotiationInterface;
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

                INSTANCE = Some(VulkanContextNegotiationInterface {
                    interface,
                    application_info,
                    initial_context: None,
                    created_context: None,
                })
            });

            Ok(INSTANCE.as_ref().unwrap())
        }
    }

    unsafe extern "C" fn get_application_info() -> *const ApplicationInfo {
        &INSTANCE.as_ref().unwrap().application_info
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

        if physical_device_features_any(initial_context.required_features) {
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

pub struct Names {
    cstring: Vec<CString>,
    ptr: Vec<*const c_char>,
}

impl Names {
    pub unsafe fn from_raw_parts(data: *mut *const c_char, len: c_uint) -> Self {
        let ptr = from_raw_parts(data, len as usize);

        let cstring: Vec<CString> = ptr.iter().map(|c| CString::from(CStr::from_ptr(*c))).collect();
        let ptr: Vec<*const c_char> = cstring.iter().map(|c| c.as_ptr()).collect();

        Self { cstring, ptr }
    }

    pub fn is_empty(&self) -> bool {
        self.cstring.is_empty()
    }

    pub fn ptr_slice(&self) -> &[*const c_char] {
        &self.ptr
    }
}

impl From<Vec<CString>> for Names {
    fn from(value: Vec<CString>) -> Self {
        let ptr = value.iter().map(|c| c.as_ptr()).collect();
        Self { cstring: value, ptr }
    }
}

impl From<&[*const c_char]> for Names {
    fn from(value: &[*const c_char]) -> Self {
        let cstring: Vec<CString> = value
            .iter()
            .map(|c| unsafe { CString::from(CStr::from_ptr(*c)) })
            .collect();
        let ptr = cstring.iter().map(|c| c.as_ptr()).collect();

        Self { cstring, ptr }
    }
}

impl<'a> Debug for Names {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.cstring.iter()).finish()
    }
}

pub fn physical_device_features_any(features: vk::PhysicalDeviceFeatures) -> bool {
    let features: [vk::Bool32; 55] = unsafe { transmute(features) };

    features.iter().sum::<vk::Bool32>() > 0
}
