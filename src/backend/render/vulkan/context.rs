use std::error::Error;
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::fmt::Debug;
use std::mem::transmute;
use std::ptr;
use std::sync::Once;

use ash::extensions::ext::DebugUtils;
use ash::extensions::khr::Surface;
use ash::extensions::{ext, khr};
use ash::prelude::VkResult;
use ash::vk::{
    self, DebugUtilsObjectNameInfoEXT, DeviceCreateInfo, DeviceQueueCreateInfo, Handle, QueueFamilyProperties,
    QueueFlags, StaticFn, SurfaceKHR,
};
use ash::vk::{ApplicationInfo, PFN_vkGetInstanceProcAddr};
use log::{debug, error, info, log_enabled, warn};
use rust_libretro::anyhow::{self, anyhow, bail};
use rust_libretro::contexts::LoadGameContext;

use rust_libretro_sys::{
    retro_vulkan_context, retro_vulkan_create_device_wrapper_t, retro_vulkan_create_instance_wrapper_t,
};
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;
use wgpu_hal::{Api, ExposedAdapter, OpenDevice};

use crate::backend::render::vulkan::util::{PropertiesFormat, QueueFamilies, Queues, set_debug_name};
use crate::built_info;

use super::util::QueueFamily;

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

/// This MUST be kept as a constant, and must *not* be given to a CString.
/// Otherwise you risk undefined behavior; this has already bitten me in the ass.
/// (See the git blame for this line for details.)
const APPLICATION_NAME: &[u8] = b"ruffle_libretro\0";

static mut APPLICATION_INFO: Option<ApplicationInfo> = None;
pub(super) static mut ENTRY: Option<ash::Entry> = None;
pub(super) static mut INSTANCE: Option<ash::Instance> = None;
pub(super) static mut DEVICE: Option<ash::Device> = None;

#[cfg(debug_assertions)]
pub(super) static mut DEBUG_UTILS: Option<DebugUtils> = None;

unsafe extern "C" fn get_application_info() -> *const ApplicationInfo {
    debug!("get_application_info()");
    if APPLICATION_INFO.is_none() {
        APPLICATION_INFO = Some(
            ApplicationInfo::builder()
                .api_version(vk::API_VERSION_1_3)
                .application_name(CStr::from_ptr(APPLICATION_NAME.as_ptr() as *const c_char))
                .application_version(vk::make_api_version(
                    0,
                    built_info::PKG_VERSION_MAJOR.parse().unwrap(),
                    built_info::PKG_VERSION_MINOR.parse().unwrap(),
                    built_info::PKG_VERSION_PATCH.parse().unwrap(),
                ))
                .build(),
        );
    }

    APPLICATION_INFO.as_ref().unwrap()
}

unsafe extern "C" fn create_instance(
    get_instance_proc_addr: Option<ash::vk::PFN_vkGetInstanceProcAddr>,
    app: *const ApplicationInfo,
    create_instance_wrapper: retro_vulkan_create_instance_wrapper_t,
    opaque: *mut c_void,
) -> vk::Instance {
    debug!("create_instance(..., {app:?}, {create_instance_wrapper:?}, {opaque:?})");
    let create_instance_wrapper = match create_instance_wrapper {
        Some(w) => w,
        None => {
            error!("Frontend provided a null create_instance_wrapper, cannot create vkInstance");
            return vk::Instance::null();
        }
    };

    let get_instance_proc_addr = match get_instance_proc_addr {
        Some(p) => p,
        None => {
            error!("Frontend provided a null get_instance_proc_addr, cannot create vkInstance");
            return vk::Instance::null();
        }
    };

    let required_instance_extensions: Vec<&'static CStr> = vec![
        khr::Surface::name(),
        vk::KhrGetPhysicalDeviceProperties2Fn::name(),
        #[cfg(debug_assertions)]
        ext::DebugUtils::name(),
    ];

    // TODO: vk::ExtSwapchainColorspaceFn is optional, only ask for it if it's available

    let required_instance_extensions: Vec<*const c_char> =
        required_instance_extensions.iter().map(|c| c.as_ptr()).collect();

    let instance_create_info = vk::InstanceCreateInfo::builder()
        .application_info(&*app)
        .enabled_extension_names(&required_instance_extensions)
        .build();

    let instance = create_instance_wrapper(opaque, &instance_create_info);

    if instance == vk::Instance::null() {
        error!("Failed to create VkInstance");
        return vk::Instance::null();
    }

    let static_fn = StaticFn::load(|sym: &CStr| {
        get_instance_proc_addr(instance, sym.as_ptr())
            .unwrap_or(transmute::<*const c_void, unsafe extern "system" fn()>(ptr::null())) as *const c_void
    });
    let entry = ash::Entry::from_static_fn(static_fn.clone());
    let ash_instance = ash::Instance::load(&static_fn.clone(), instance);
    ENTRY = Some(entry.clone());
    INSTANCE = Some(ash_instance.clone());

    #[cfg(debug_assertions)]
    {
        DEBUG_UTILS = Some(ash::extensions::ext::DebugUtils::new(&entry, &ash_instance));
    }
    instance
}

/// Provided to pacify RetroArch, as it still wants create_device defined
/// even if it uses create_device2 instead
unsafe extern "C" fn create_device(
    _context: *mut retro_vulkan_context,
    _instance: vk::Instance,
    _gpu: vk::PhysicalDevice,
    _surface: vk::SurfaceKHR,
    _get_instance_proc_addr: Option<vk::PFN_vkGetInstanceProcAddr>,
    _required_device_extensions: *mut *const c_char,
    _num_required_device_extensions: c_uint,
    _required_device_layers: *mut *const c_char,
    _num_required_device_layers: c_uint,
    _required_features: *const vk::PhysicalDeviceFeatures,
) -> bool {
    warn!("create_device is not supported due to its inability to specify instance extensions. If you see this, the core will likely fail.");
    return false;
}

/// Exists to simplify error reporting
unsafe fn create_device2_impl(
    instance: vk::Instance,
    gpu: vk::PhysicalDevice,
    surface: Option<vk::SurfaceKHR>,
    get_instance_proc_addr: Option<PFN_vkGetInstanceProcAddr>,
    create_device_wrapper: retro_vulkan_create_device_wrapper_t,
    opaque: *mut c_void,
) -> anyhow::Result<retro_vulkan_context> {
    let get_instance_proc_addr = match get_instance_proc_addr {
        Some(g) => g,
        None => {
            bail!("Frontend provided create_device2 with a null get_instance_proc_addr, cannot create VkDevice");
        }
    };

    let create_device_wrapper = match create_device_wrapper {
        Some(d) => d,
        None => {
            bail!("Frontend provided create_device2 with a null create_device_wrapper, cannot create VkDevice");
        }
    };

    let entry = ENTRY
        .as_ref()
        .expect("ENTRY should've been initialized in create_instance");
    let instance = INSTANCE
        .as_ref()
        .expect("INSTANCE should've been initialized in create_instance");

    #[cfg(debug_assertions)]
    let debug_utils = DEBUG_UTILS
        .as_ref()
        .expect("DEBUG_UTILS should've been initialized in create_instance");

    match entry.try_enumerate_instance_version() {
        Ok(Some(version)) => {
            let major = vk::api_version_major(version);
            let minor = vk::api_version_minor(version);
            let patch = vk::api_version_patch(version);
            let variant = vk::api_version_variant(version);

            info!("Using Vulkan {major}.{minor}.{patch} (variant {variant})");
        }
        Ok(None) => {
            warn!("Using unknown Vulkan version");
        }
        Err(error) => {
            bail!("Error querying active Vulkan version: {error}");
        }
    };

    if log_enabled!(log::Level::Debug) {
        match entry.enumerate_instance_extension_properties(None) {
            Ok(extensions) => {
                let extensions = PropertiesFormat::new(&extensions);
                debug!("Available instance extensions: {extensions:#?}");
            }
            Err(error) => {
                warn!("Failed to query available instance extensions: {error}");
            }
        };

        match entry.enumerate_instance_layer_properties() {
            Ok(layers) => {
                let layers = PropertiesFormat::new(&layers);
                debug!("Available instance layers: {layers:#?}");
            }
            Err(error) => {
                warn!("Failed to query available instance layers: {error}");
            }
        };
    }

    //If gpu is not VK_NULL_HANDLE, the physical device provided to the frontend must be this PhysicalDevice.
    //The core is still free to use other physical devices.
    let gpu = if gpu != vk::PhysicalDevice::null() {
        // If the frontend has already selected a gpu...
        gpu
    } else {
        // If the frontend hasn't selected a GPU...
        info!("Frontend didn't pick a VkPhysicalDevice, core will do so instead");
        select_physical_device(&instance)?
    };

    let gpu_properties = instance.get_physical_device_properties(gpu);
    let gpu_name = CStr::from_ptr(gpu_properties.device_name.as_ptr());
    info!("Using VkPhysicalDevice {gpu_name:?} ({gpu:?})");

    let surface_fn = if surface.is_none() {
        None
    } else {
        Some(Surface::new(&entry, &instance))
    };

    // We need to select queues manually because
    // wgpu assumes that there will be a single device queue family
    // that supports graphics, compute, and present.
    // We don't make that assumption.
    let queue_families = if let (Some(surface), Some(surface_fn)) = (surface, surface_fn.as_ref()) {
        // If VK_KHR_surface is enabled and the surface is valid...
        select_queue_families(gpu, &instance, surface, surface_fn)?
    } else {
        // Just get a queue with COMPUTE and GRAPHICS, then
        let queue_families = instance.get_physical_device_queue_family_properties(gpu);
        QueueFamilies::GraphicsComputeOnly(select_queue_family(&queue_families)?)
    };

    match queue_families {
        QueueFamilies::Single(family) => {
            info!("Using queue family no. {0} for graphics, compute, and present", family.1);
            debug!("Details: {0:?}", family.0);
        }
        QueueFamilies::Split {
            graphics_compute,
            present,
        } => {
            info!(
                "Using queue family no. {0} for graphics and compute, family no. {1} for present",
                graphics_compute.1, present.1
            );
            debug!("Graphics/compute details: {0:?}", graphics_compute.0);
            debug!("Present details: {0:?}", present.0);
        }
        QueueFamilies::GraphicsComputeOnly(family) => {
            warn!("Using queue family no. {0} for graphics and compute, but none was found for present.", family.1);
            debug!("Details: {0:?}", family.0);
        }
    };

    if log_enabled!(log::Level::Info) {
        let available_device_extensions = instance.enumerate_device_extension_properties(gpu)?;
        info!("Available extensions for this device: {:#?}", PropertiesFormat::new(&available_device_extensions));
    }

    let device = create_logical_device(&instance, gpu, &queue_families, |info| {
        let device = create_device_wrapper(gpu, opaque, info);
        let instance_fn = instance.fp_v1_0();
        ash::Device::load(instance_fn, device)
    })?;

    debug!("Created VkDevice {:?}", device.handle());

    set_debug_name(debug_utils, &device, instance.handle(), b"Ruffle Instance\0");
    set_debug_name(debug_utils, &device, device.handle(), b"Ruffle-Created Device\0");
    set_debug_name(debug_utils, &device, gpu, b"Ruffle GPU\0");

    let queues = Queues::new(&device, &queue_families);

    #[cfg(debug_assertions)]
    match queues {
        Queues::Single(q) => {
            set_debug_name(debug_utils, &device, q.0, b"Ruffle-Selected Gfx/Compute/Present Queue\0");
        }
        Queues::Split {
            graphics_compute,
            present,
        } => {
            set_debug_name(debug_utils, &device, graphics_compute.0, b"Ruffle-Selected Gfx/Compute Queue\0");
            set_debug_name(debug_utils, &device, present.0, b"Ruffle-Selected Present Queue\0");
        }
        Queues::GraphicsComputeOnly(q) => {
            set_debug_name(debug_utils, &device, q.0, b"Ruffle-Selected Gfx/Compute Queue\0");
        }
    };

    if let Some(surface) = surface {
        set_debug_name(debug_utils, &device, surface, b"RetroArch Surface\0");
    }

    DEVICE = Some(device.clone());

    Ok(retro_vulkan_context {
        device: device.handle(),
        gpu,
        queue: queues.queue(),
        queue_family_index: queue_families.queue_family_index(),
        presentation_queue: queues.presentation_queue(),
        presentation_queue_family_index: queue_families.presentation_queue_family_index(),
    })
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
    debug!("create_device2({context:?}, {instance:?}, {gpu:?}, {surface:?}, ..., ..., {opaque:?})");
    let context = if !context.is_null() {
        &mut (*context)
    } else {
        error!("Frontend provided create_device2 with a null retro_vulkan_context, cannot create VkDevice");
        return false;
    };

    let surface = if surface == SurfaceKHR::null() {
        None
    } else {
        Some(surface)
    };

    match create_device2_impl(instance, gpu, surface, get_instance_proc_addr, create_device_wrapper, opaque) {
        Ok(ctx) => {
            context.device = ctx.device;
            context.gpu = ctx.gpu;
            context.presentation_queue = ctx.presentation_queue;
            context.presentation_queue_family_index = ctx.presentation_queue_family_index;
            context.queue = ctx.queue;
            context.queue_family_index = ctx.queue_family_index;

            return true;
        }
        Err(error) => {
            error!("Failed to create VkDevice: {error}");
            // TODO: Clean up logical device if the below functions fail
            return false;
        }
    };
}

unsafe extern "C" fn destroy_device() {
    APPLICATION_INFO = None;
    ENTRY = None;
    INSTANCE = None;
    DEVICE = None;

    #[cfg(debug_assertions)]
    {
        DEBUG_UTILS = None;
    }
}

// The frontend will request certain extensions and layers for a device which is created.
// The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
fn select_physical_device(instance: &ash::Instance) -> anyhow::Result<vk::PhysicalDevice> {
    let available_physical_devices = unsafe { instance.enumerate_physical_devices()? };
    if available_physical_devices.is_empty() {
        bail!("No VkPhysicalDevices found");
    }

    let available_physical_devices: Vec<vk::PhysicalDevice> = available_physical_devices
        .into_iter()
        .filter_map(|device| filter_physical_device(instance, device).ok())
        .collect();

    match available_physical_devices.len() {
        0 => bail!("No VkPhysicalDevice that supports the required features is available"),
        _ => Ok(available_physical_devices[0]),
        // TODO: Implement real logic to pick a device, instead of just getting the first
    }
}

fn filter_physical_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
) -> anyhow::Result<vk::PhysicalDevice> {
    // See if this VkPhysicalDevice meets the following conditions...
    info!("Evaluating VkPhysicalDevice {physical_device:?}");

    // A device with a queue that supports GRAPHICS and COMPUTE...
    let queue_families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

    let required_families_supported = queue_families
        .iter()
        .any(|q| q.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE));

    if required_families_supported {
        Ok(physical_device)
    } else {
        bail!("VkPhysicalDevice {physical_device:?} isn't suitable for ruffle.")
    }
}

// If presentation to "surface" is supported on the queue, presentation_queue must be equal to queue.
// If not, a second queue must be provided in presentation_queue and presentation_queue_index.
// If surface is not VK_NULL_HANDLE, the instance from frontend will have been created with supported for
// VK_KHR_surface extension.
fn select_queue_families(
    physical_device: vk::PhysicalDevice,
    instance: &ash::Instance,
    surface: vk::SurfaceKHR,
    surface_fn: &ash::extensions::khr::Surface,
) -> anyhow::Result<QueueFamilies> {
    let queue_families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

    let single_queue_family = queue_families
        .iter()
        .enumerate()
        .find_map(|(i, family)| unsafe {
            let i = i as u32;
            // The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
            if !family.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE) {
                return None;
            }

            match surface_fn.get_physical_device_surface_support(physical_device, i, surface) {
                Ok(true) => Some(Ok(QueueFamilies::Single(QueueFamily(*family, i)))),
                // This queue also supports presentation, so let's use it!
                Ok(false) => None,
                // This queue doesn't support presentation, let's keep searching.
                Err(error) => Some(Err(anyhow!("Error in vkGetPhysicalDeviceSurfaceSupportKHR: {error}"))),
                // We have a problem, gotta report it.
            }
        })
        .transpose()?;

    if let Some(single_queue_family) = single_queue_family {
        return Ok(single_queue_family);
    }

    // We couldn't find a single queue that supported graphics, compute, *and* present.
    // So we'll have to split them up.

    let queue_family = unsafe { select_queue_family(&queue_families)? };
    // Here's our graphics/compute queue, now for a present queue

    let presentation_queue_family = queue_families
        .iter()
        .enumerate()
        .find_map(|(i, family)| unsafe {
            match surface_fn.get_physical_device_surface_support(physical_device, i as u32, surface) {
                Ok(true) => Some(Ok(QueueFamily(*family, i as u32))), // This queue family supports presentation, let's use it
                Ok(false) => None, // This queue family doesn't support presentation, let's not use it
                Err(error) => Some(Err(anyhow!("Error in vkGetPhysicalDeviceSurfaceSupportKHR: {error}"))),
                // There was an error, let's report it
            }
        })
        .transpose()?;

    if let Some(presentation_queue_family) = presentation_queue_family {
        // If we found a queue family that supports presentation...
        Ok(QueueFamilies::Split {
            graphics_compute: queue_family,
            present: presentation_queue_family,
        })
    } else {
        bail!("No acceptable queue family was found")
    }
}

// Only used if the surface extension isn't available
// (Probably means we're not rendering to the screen)
unsafe fn select_queue_family(queue_families: &[QueueFamilyProperties]) -> anyhow::Result<QueueFamily> {
    // The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
    queue_families
        .iter()
        .enumerate()
        .find_map(|(i, family)| {
            // Get the first queue family that supports the features we need.
            if family.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE) {
                Some(QueueFamily(*family, i as u32))
            } else {
                None
            }
        })
        .ok_or(anyhow!("No acceptable queue family was found"))
}

fn create_logical_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    queue_families: &QueueFamilies,
    create_device_wrapper: impl FnOnce(&DeviceCreateInfo) -> ash::Device,
) -> anyhow::Result<ash::Device> {
    let queue_create_info = DeviceQueueCreateInfo::builder()
        .queue_family_index(queue_families.queue_family_index())
        .queue_priorities(&[1.0f32]) //  The core is free to set its own queue priorities.
        .build();

    let mut physical_device_vulkan_12_features = vk::PhysicalDeviceVulkan12Features::default();
    let mut physical_device_features2 = vk::PhysicalDeviceFeatures2::builder()
        .push_next(&mut physical_device_vulkan_12_features)
        .build();

    unsafe {
        instance.get_physical_device_features2(physical_device, &mut physical_device_features2);
    } // TODO: Move these extension objects to the constructor so the various initialization phases can query for them

    debug!("VkPhysicalDeviceFeatures2: {physical_device_features2:#?}");
    debug!("VkPhysicalDeviceVulkan12Features: {physical_device_vulkan_12_features:#?}");

    let presentation_queue_create_info = match queue_families {
        QueueFamilies::Split { present, .. } => {
            DeviceQueueCreateInfo::builder()
                .queue_family_index(present.1)
                .queue_priorities(&[1.0f32]) //  The core is free to set its own queue priorities.
                .build()
        }
        _ => queue_create_info,
    };

    let queue_create_infos = [queue_create_info, presentation_queue_create_info];
    let queue_create_infos = match queue_families {
        QueueFamilies::Split { .. } => &queue_create_infos,
        _ => &queue_create_infos[0..1],
    };

    let device_create_info = DeviceCreateInfo::builder()
        .queue_create_infos(queue_create_infos)
        .push_next(&mut physical_device_vulkan_12_features)
        // .flags(DeviceCreateFlags) VkDeviceCreateFlags is empty, currently reserved
        .build();

    Ok(create_device_wrapper(&device_create_info))
}

pub fn enable(ctx: &mut LoadGameContext) -> anyhow::Result<()> {
    unsafe {
        ctx.enable_hw_render_negotiation_interface_vulkan(
            Some(get_application_info),
            Some(create_device),
            Some(destroy_device),
            Some(create_instance),
            Some(create_device2),
        )?;

        debug!("Enabled retro_hw_render_context_negotiation_interface_vulkan");
        Ok(())
    }
}
