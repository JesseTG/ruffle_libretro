use std::error::Error;
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::mem::transmute;
use std::ptr;
use std::slice::from_raw_parts;
use std::sync::Once;

use ash::extensions::khr::Surface;
use ash::prelude::VkResult;
use ash::vk::{
    ApplicationInfo, ExtensionProperties, LayerProperties, PFN_vkGetInstanceProcAddr, PhysicalDevice,
    PhysicalDeviceFeatures, PhysicalDeviceProperties, QueueFamilyProperties, QueueFlags, StaticFn, SurfaceKHR,
};
use ash::{vk, Entry, Instance};
use log::{debug, error, info, log, warn};
use rust_libretro_sys::retro_hw_render_context_negotiation_interface_type::RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN;
use rust_libretro_sys::{
    retro_hw_render_context_negotiation_interface_type, retro_hw_render_context_negotiation_interface_vulkan,
    retro_vulkan_context, RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
};
use tap::pipe::Pipe;
use thiserror::Error as ThisError;

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
    surface_fns: Option<ash::extensions::khr::Surface>,
}

/// This MUST be kept as a constant, and must *not* be given to a CString.
/// Otherwise you risk undefined behavior; this has already bitten me in the ass.
/// (See the git blame for this line for details.)
const APPLICATION_NAME: &[u8] = b"ruffle_libretro\0";

// TODO: Should I put this behind a mutex?
static mut INSTANCE: Option<VulkanContextNegotiationInterface> = None;
static ONCE: Once = Once::new();

impl VulkanContextNegotiationInterface {
    pub fn instance() -> Result<&'static VulkanContextNegotiationInterface, Box<dyn Error>> {
        unsafe {
            ONCE.call_once(|| {
                let interface = retro_hw_render_context_negotiation_interface_vulkan {
                    interface_type: RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN,
                    interface_version: RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
                    get_application_info: Some(Self::get_application_info),
                    create_device: Some(Self::create_device),
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
                    instance: None,
                    entry: None,
                    required_device_extensions: vec![],
                    device: None,
                    surface_fns: None,
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

        // The frontend will request certain extensions and layers for a device which is created.
        let required_device_extensions =
            from_raw_parts(required_device_extensions, num_required_device_extensions as usize);
        let required_device_extensions_cstr: Vec<&CStr> =
            required_device_extensions.iter().map(|c| CStr::from_ptr(*c)).collect();
        debug!("Required physical device extensions: {required_device_extensions_cstr:?}");

        let required_device_layers = from_raw_parts(required_device_layers, num_required_device_layers as usize);
        let required_device_layers_cstr: Vec<&CStr> =
            required_device_layers.iter().map(|c| CStr::from_ptr(*c)).collect();
        debug!("Required physical device layers: {required_device_layers_cstr:?}");

        let required_features = if required_features.is_null() {
            PhysicalDeviceFeatures::default()
        } else {
            *required_features
        };
        debug!("Required physical device features: {required_features:?}");

        /*
         * The frontend will request certain extensions and layers for a device which is created.
         * The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
         */
        let surface_fn = if surface == SurfaceKHR::null() {
            None
        } else {
            Some(Surface::new(&entry, &instance))
        };

        let surface = if surface_fn.is_none() { None } else { Some(surface) };

        let mut interface = INSTANCE.as_mut().unwrap();
        interface.entry = Some(entry);
        interface
            .required_device_extensions
            .reserve(num_required_device_extensions as usize);

        /*
         * If gpu is not VK_NULL_HANDLE, the physical device provided to the frontend must be this PhysicalDevice.
         * The core is still free to use other physical devices.
         */
        let physical_device = if gpu != vk::PhysicalDevice::null() {
            match PhysicalDeviceInfo::new(&instance, gpu) {
                Ok(device) => {
                    info!(
                        "Using VkPhysicalDevice {:?}, per the frontend's request",
                        CStr::from_ptr(device.properties.device_name.as_ptr())
                    );
                    device
                }
                Err(error) => {
                    let props = instance.get_physical_device_properties(gpu);
                    let name = CStr::from_ptr(props.device_name.as_ptr());
                    error!("Failed to completely query the frontend-requested VkPhysicalDevice {name:?}: {error}");
                    return false;
                }
            }
        } else {
            // If the frontend hasn't selected a GPU...
            info!("Frontend didn't pick a VkPhysicalDevice, core will do so instead");
            match Self::select_gpu(
                &instance,
                required_device_extensions_cstr.as_slice(),
                required_device_layers_cstr.as_slice(),
                &required_features,
            ) {
                Ok(device) => {
                    info!(
                        "Selected VkPhysicalDevice {:?} {:?}",
                        device.properties.device_type,
                        CStr::from_ptr(device.properties.device_name.as_ptr())
                    );
                    device
                }
                Err(error) => {
                    error!("Error selecting a VkPhysicalDevice: {error:?}");
                    return false;
                }
            }
        };

        let queue_families = if let (Some(surface), Some(surface_fn)) = (surface, surface_fn.as_ref()) {
            // If VK_KHR_surface is enabled and the surface is valid...
            match Self::select_queue_families(&physical_device, &surface_fn, surface) {
                Ok(families) => families,
                Err(error) => {
                    error!("Error when searching for usable compute/graphics/present families: {error:?}");
                    return false;
                }
            }
        } else {
            // Just get a queue with COMPUTE and GRAPHICS, then
            match Self::select_queue_family(&physical_device) {
                Ok(family) => QueueFamilies::new(family, family),
                Err(error) => {
                    error!("Error when searching for usable compute/graphics family: {error:?}");
                    return false;
                }
            }
        };

        (*context).gpu = physical_device.device;
        (*context).queue_family_index = queue_families.queue_family_index;
        (*context).presentation_queue_family_index = queue_families.presentation_queue_family_index;

        interface.instance = Some(instance);
        interface.surface_fns = surface_fn;

        todo!()
    }

    /*
     * The frontend will request certain extensions and layers for a device which is created.
     * The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
     */
    fn select_gpu(
        instance: &Instance,
        required_extensions: &[&CStr],
        required_layers: &[&CStr],
        required_features: &PhysicalDeviceFeatures,
    ) -> Result<PhysicalDeviceInfo, VulkanNegotiationError> {
        let physical_devices = unsafe {
            instance
                .enumerate_physical_devices()
                .map_err(|e| VulkanError("vkEnumeratePhysicalDevices", e))?
        };

        if physical_devices.is_empty() {
            return Err(NoPhysicalDevicesFound);
        }

        let physical_devices: Vec<PhysicalDeviceInfo> = physical_devices
            .iter()
            .filter_map(|device| unsafe {
                Self::filter_physical_device(
                    instance,
                    *device,
                    required_extensions,
                    required_layers,
                    required_features,
                )
            })
            .collect();

        match physical_devices.len() {
            0 => Err(NoAcceptablePhysicalDevice),
            1 => Ok(physical_devices[0].clone()),
            _ => Ok(Self::select_best_physical_device(physical_devices.as_slice()).clone()),
        }
    }

    unsafe fn filter_physical_device(
        instance: &Instance,
        device: PhysicalDevice,
        required_extensions: &[&CStr],
        required_layers: &[&CStr],
        required_features: &PhysicalDeviceFeatures,
    ) -> Option<PhysicalDeviceInfo> {
        // See if this VkPhysicalDevice meets the following conditions...
        let properties = instance.get_physical_device_properties(device);
        let device_name = CStr::from_ptr(properties.device_name.as_ptr());
        info!("Evaluating VkPhysicalDevice {device_name:?}");

        // A device that supports the required extensions, if we need any in particular...
        let extensions = match instance.enumerate_device_extension_properties(device) {
            Ok(extensions) => {
                let names: Vec<&CStr> = extensions
                    .iter()
                    .map(|e| CStr::from_ptr(e.extension_name.as_ptr()))
                    .collect();

                info!("\tSupported device extensions: {names:?}");
                if !required_extensions.iter().all(|e| names.contains(e)) {
                    // An empty iter().all() will return true
                    warn!("\t{device_name:?} does not support all required extensions; can't use it");
                    return None;
                }

                extensions
            }
            Err(error) => {
                warn!("\tFailed to query {device_name:?} for supported extensions: {error}");
                return None;
            }
        };

        // A device that supports the required layers, if we need any in particular...
        let layers = match instance.enumerate_device_layer_properties(device) {
            Ok(layers) => {
                let names: Vec<&CStr> = layers.iter().map(|e| CStr::from_ptr(e.layer_name.as_ptr())).collect();
                info!("\tSupported device layers: {names:?}");
                if !required_layers.iter().all(|e| names.contains(e)) {
                    warn!("\t{device_name:?} does not support all required layers; can't use it");
                    return None;
                }

                layers
            }
            Err(error) => {
                warn!("\tFailed to query {device_name:?} for supported layers: {error}");
                return None;
            }
        };

        let features = instance.get_physical_device_features(device);
        if Self::physical_device_features_any(*required_features) {
            // If the frontend requires any specific VkPhysicalDeviceFeatures...
            warn!("Frontend requires VkPhysicalDeviceFeatures, but this core doesn't check for them yet.");
            warn!("Please file a bug here, and be sure to say which frontend you're using https://github.com/JesseTG/ruffle_libretro");
            warn!("Required features: {features:#?}");
            // TODO: Check that the supported features are provided
        }

        // A device with a queue that supports GRAPHICS and COMPUTE...
        let queue_families = instance.get_physical_device_queue_family_properties(device);
        if !queue_families
            .iter()
            .any(|q| q.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE))
        {
            // If none of this physical device's queue families support the required features...
            warn!(
                "\t{device_name:?} does not support these queue families: {:?}; cannot use it",
                QueueFlags::GRAPHICS | QueueFlags::COMPUTE
            );
            return None;
        }

        Some(PhysicalDeviceInfo {
            device,
            properties,
            features,
            extensions,
            layers,
            queue_families,
        })
    }

    fn select_best_physical_device(devices: &[PhysicalDeviceInfo]) -> &PhysicalDeviceInfo {
        &devices[0] // TODO: Implement for real
    }

    fn physical_device_features_any(features: PhysicalDeviceFeatures) -> bool {
        let features: [vk::Bool32; 55] = unsafe { transmute(features) };

        features.iter().sum::<vk::Bool32>() > 0
    }

    // Only used if the surface extension isn't available
    // (Probably means we're not rendering to the screen)
    fn select_queue_family(physical_device: &PhysicalDeviceInfo) -> Result<u32, VulkanNegotiationError> {
        // The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
        physical_device
            .queue_families
            .iter()
            .enumerate()
            .find_map(|(i, family)| {
                // Get the first queue family that supports the features we need.
                if family.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE) {
                    Some(i as u32)
                } else {
                    None
                }
            })
            .ok_or(NoAcceptableQueueFamily)
    }

    // If presentation to "surface" is supported on the queue, presentation_queue must be equal to queue.
    // If not, a second queue must be provided in presentation_queue and presentation_queue_index.
    // If surface is not VK_NULL_HANDLE, the instance from frontend will have been created with supported for
    // VK_KHR_surface extension.
    fn select_queue_families(
        physical_device: &PhysicalDeviceInfo,
        surface_fn: &Surface,
        surface: SurfaceKHR,
    ) -> Result<QueueFamilies, VulkanNegotiationError> {
        let single_queue_family = physical_device
            .queue_families
            .iter()
            .enumerate()
            .find_map(|(i, family)| unsafe {
                let i = i as u32;
                // The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
                if !family.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE) {
                    return None;
                }

                match surface_fn.get_physical_device_surface_support(physical_device.device, i, surface) {
                    Ok(true) => Some(Ok(QueueFamilies::new(i, i))),
                    // This queue also supports presentation, so let's use it!
                    Ok(false) => None,
                    // This queue doesn't support presentation, let's keep searching.
                    Err(error) => Some(Err(VulkanError("vkGetPhysicalDeviceSurfaceSupportKHR", error))),
                    // We have a problem, gotta report it.
                }
            })
            .transpose()?;

        if let Some(single_queue_family) = single_queue_family {
            return Ok(single_queue_family);
        }

        // We couldn't find a single queue that supported graphics, compute, *and* present.
        // So we'll have to split them up.

        let queue_family = Self::select_queue_family(physical_device)?;
        // Here's our graphics/compute queue, now for a present queue

        let presentation_queue_family = physical_device
            .queue_families
            .iter()
            .enumerate()
            .find_map(|(i, properties)| unsafe {
                match surface_fn.get_physical_device_surface_support(physical_device.device, i as u32, surface) {
                    Ok(true) => Some(Ok(i as u32)), // This queue family supports presentation, let's use it
                    Ok(false) => None, // This queue family doesn't support presentation, let's not use it
                    Err(error) => Some(Err(VulkanError("vkGetPhysicalDeviceSurfaceSupportKHR", error))), // There was an error, let's report it
                }
            })
            .transpose()?;

        if let Some(presentation_queue_family) = presentation_queue_family {
            // If we found a queue family that supports presentation...
            Ok(QueueFamilies::new(queue_family, presentation_queue_family))
        }
        else {
            Err(NoAcceptableQueueFamily)
        }
    }
}

#[derive(Clone, Debug)]
struct PhysicalDeviceInfo {
    device: PhysicalDevice,
    properties: PhysicalDeviceProperties,
    features: PhysicalDeviceFeatures,
    extensions: Vec<ExtensionProperties>,
    layers: Vec<LayerProperties>,
    queue_families: Vec<QueueFamilyProperties>,
}
impl PhysicalDeviceInfo {
    pub unsafe fn new(instance: &Instance, device: PhysicalDevice) -> VkResult<PhysicalDeviceInfo> {
        assert_ne!(device, PhysicalDevice::null());

        Ok(PhysicalDeviceInfo {
            device,
            properties: instance.get_physical_device_properties(device),
            extensions: instance.enumerate_device_extension_properties(device)?,
            layers: instance.enumerate_device_layer_properties(device)?,
            features: instance.get_physical_device_features(device),
            queue_families: instance.get_physical_device_queue_family_properties(device),
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct QueueFamilies {
    queue_family_index: u32,
    presentation_queue_family_index: u32,
}

impl QueueFamilies {
    pub fn new(queue_family_index: u32, presentation_queue_family_index: u32) -> Self {
        Self {
            queue_family_index,
            presentation_queue_family_index,
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
