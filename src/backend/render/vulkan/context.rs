use std::error::Error;
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::mem::transmute;
use std::ptr;

use crate::backend::render::required_limits;
use ash::extensions::khr::Surface;
use ash::prelude::VkResult;
use ash::vk;
use ash::vk::{
    DeviceCreateInfo, DeviceQueueCreateInfo, KhrSurfaceFn, PFN_vkGetInstanceProcAddr, PhysicalDeviceFeatures,
    QueueFamilyProperties, QueueFlags, StaticFn,
};
use libc::open;
use log::{info, warn};
use ruffle_render_wgpu::descriptors::Descriptors;
use rust_libretro_sys::retro_hw_render_interface_vulkan;
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;
use wgpu_hal::{Api, ExposedAdapter, InstanceFlags, OpenDevice, UpdateAfterBindTypes};

use crate::backend::render::vulkan::context::ContextConversionError::FailedToExposePhysicalDevice;
use crate::backend::render::vulkan::negotiation::VulkanNegotiationError::{
    NoAcceptablePhysicalDevice, NoAcceptableQueueFamily, NoPhysicalDevicesFound,
};
use crate::backend::render::vulkan::negotiation::{physical_device_features_any, Names, VulkanNegotiationError};
use crate::backend::render::vulkan::VulkanRenderBackendError::VulkanError;

pub type VulkanHalInstance = <Vulkan as Api>::Instance;
pub type VulkanHalDevice = <Vulkan as Api>::Device;
pub type VulkanHalAdapter = <Vulkan as Api>::Adapter;
pub type VulkanHalQueue = <Vulkan as Api>::Queue;
pub type VulkanHalSurface = <Vulkan as Api>::Surface;
pub type VulkanHalExposedAdapter = ExposedAdapter<Vulkan>;
pub type VulkanHalOpenDevice = OpenDevice<Vulkan>;

#[derive(ThisError, Debug)]
pub enum ContextConversionError {
    #[error("Failed to expose VkPhysicalDevice {0:?}")]
    FailedToExposePhysicalDevice(vk::PhysicalDevice),
}

/// The Vulkan resources that were provided to [`retro_hw_render_context_negotiation_interface_vulkan`].
pub struct RetroVulkanInitialContext {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical_device: Option<vk::PhysicalDevice>,
    pub surface_fn: Option<ash::extensions::khr::Surface>,
    pub surface: Option<vk::SurfaceKHR>,
    pub required_device_extensions: Names,
    pub required_device_layers: Names,
    pub required_features: vk::PhysicalDeviceFeatures,
}

/// The Vulkan resources that are exposed to cores,
/// but owned by libretro.
/// These must not be destroyed by the core.
#[derive(Clone)]
pub struct RetroVulkanCreatedContext {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub queue_family_index: u32,
    pub presentation_queue: vk::Queue,
    pub presentation_queue_family_index: u32,
}

pub struct RetroVulkanInitialContextWgpuHal {
    instance: VulkanHalInstance,
    adapter: VulkanHalExposedAdapter,
}

pub struct RetroVulkanCreatedContextWgpuHal {
    open_device: VulkanHalOpenDevice,
    presentation_queue: vk::Queue,
    presentation_queue_family_index: u32,
}

pub struct RetroVulkanCreatedContextWgpu {
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    presentation_queue: vk::Queue,
    presentation_queue_family_index: u32,
}

impl RetroVulkanInitialContext {
    pub unsafe fn new(
        instance: vk::Instance,
        gpu: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
        required_device_extensions: *mut *const c_char,
        num_required_device_extensions: c_uint,
        required_device_layers: *mut *const c_char,
        num_required_device_layers: c_uint,
        required_features: *const vk::PhysicalDeviceFeatures,
    ) -> Result<Self, Box<dyn Error>> {
        if instance == vk::Instance::null() {
            Err("Frontend called create_device without a valid VkInstance")?
        }

        if get_instance_proc_addr as usize == 0 {
            Err("Frontend called create_device with a null PFN_vkGetInstanceProcAddr")?
        }
        let load_symbols = |sym: &CStr| {
            let fun = get_instance_proc_addr(instance, sym.as_ptr());
            fun.unwrap_or(transmute::<*const c_void, unsafe extern "system" fn()>(ptr::null())) as *const c_void
        };

        let static_fn = StaticFn::load(load_symbols);
        let entry = ash::Entry::from_static_fn(static_fn);
        let instance = ash::Instance::load(entry.static_fn(), instance);

        let surface = if surface == vk::SurfaceKHR::null() {
            None
        } else {
            Some(surface)
        };

        let surface_fn = if surface.is_none() {
            None
        } else {
            Some(Surface::new(&entry, &instance))
        };

        Ok(Self {
            entry,
            instance,
            physical_device: if gpu == vk::PhysicalDevice::null() {
                None
            } else {
                Some(gpu)
            },
            surface_fn,
            surface,
            required_device_extensions: Names::from_raw_parts(
                required_device_extensions,
                num_required_device_extensions,
            ),
            required_device_layers: Names::from_raw_parts(required_device_layers, num_required_device_layers),
            required_features: if required_features.is_null() {
                PhysicalDeviceFeatures::default()
            } else {
                *required_features
            },
        })
    }

    // The frontend will request certain extensions and layers for a device which is created.
    // The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
    pub fn select_physical_device(&self) -> Result<vk::PhysicalDevice, Box<dyn Error>> {
        if let Some(physical_device) = self.physical_device {
            return Ok(physical_device);
        }

        let available_physical_devices = unsafe { self.instance.enumerate_physical_devices()? };
        if available_physical_devices.is_empty() {
            Err(NoPhysicalDevicesFound)?
        }

        let available_physical_devices: Vec<vk::PhysicalDevice> = available_physical_devices
            .into_iter()
            .filter(|device| unsafe { self.filter_physical_device(*device) })
            .collect();

        match available_physical_devices.len() {
            0 => Err(NoAcceptablePhysicalDevice)?,
            _ => Ok(available_physical_devices[0]),
            // TODO: Implement real logic to pick a device, instead of just getting the first
        }
    }

    unsafe fn filter_physical_device(&self, physical_device: vk::PhysicalDevice) -> bool {
        // See if this VkPhysicalDevice meets the following conditions...
        info!("Evaluating VkPhysicalDevice {physical_device:?}");

        // A device that supports the required extensions, if we need any in particular...
        // let extensions = self.instance.enumerate_device_extension_properties(physical_device)?;
        // if !self.required_device_extensions.iter().all(|e| extensions.contains(e)) {
        //     return Ok(false);
        // }

        if physical_device_features_any(self.required_features) {
            // If the frontend requires any specific VkPhysicalDeviceFeatures...
            warn!("Frontend requires VkPhysicalDeviceFeatures, but this core doesn't check for them yet.");
            warn!("Please file a bug here, and be sure to say which frontend you're using https://github.com/JesseTG/ruffle_libretro");
            warn!("Required features: {:#?}", self.required_features);
            // TODO: Check that the supported features are provided
        }

        // A device with a queue that supports GRAPHICS and COMPUTE...
        let queue_families = self
            .instance
            .get_physical_device_queue_family_properties(physical_device);

        queue_families
            .iter()
            .any(|q| q.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE))
    }
}

impl RetroVulkanCreatedContext {
    pub unsafe fn new(initial_context: &RetroVulkanInitialContext) -> Result<Self, Box<dyn Error>> {
        // The frontend will request certain extensions and layers for a device which is created.
        // The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.

        let available_physical_devices = initial_context.instance.enumerate_physical_devices()?;

        //If gpu is not VK_NULL_HANDLE, the physical device provided to the frontend must be this PhysicalDevice.
        //The core is still free to use other physical devices.
        let physical_device = if let Some(gpu) = initial_context.physical_device {
            // If the frontend has already selected a gpu...
            info!("Frontend has already selected a VkPhysicalDevice ({gpu:?})");
            gpu
        } else {
            // If the frontend hasn't selected a GPU...
            info!("Frontend didn't pick a VkPhysicalDevice, core will do so instead");
            initial_context.select_physical_device()?
        };

        // We need to select queues manually because
        // wgpu assumes that there will be a single device queue family
        // that supports graphics, compute, and present.
        // We don't make that assumption.
        let queue_families =
            if let (Some(surface), Some(surface_fn)) = (initial_context.surface, initial_context.surface_fn.as_ref()) {
                // If VK_KHR_surface is enabled and the surface is valid...
                Self::select_queue_families(physical_device, initial_context, surface, surface_fn)?
            } else {
                // Just get a queue with COMPUTE and GRAPHICS, then
                let instance = &initial_context.instance;
                let queue_families = instance.get_physical_device_queue_family_properties(physical_device);
                let index = Self::select_queue_family(&queue_families)?;
                QueueFamilies::new(index, index)
            };

        info!(
            "Selected queue families {0} (gfx/compute) and {1} (presentation)",
            queue_families.queue_family_index, queue_families.presentation_queue_family_index
        );

        let device = Self::create_logical_device(
            &initial_context.instance,
            physical_device,
            &queue_families,
            &initial_context.required_device_extensions,
            &initial_context.required_device_layers,
            &initial_context.required_features,
        )?;

        let queues = Queues::new(
            device.get_device_queue(queue_families.queue_family_index, 0),
            device.get_device_queue(queue_families.presentation_queue_family_index, 0),
        );

        Ok(Self {
            entry: initial_context.entry.clone(),
            instance: initial_context.instance.clone(),
            physical_device,
            device,
            queue: queues.queue,
            queue_family_index: queue_families.queue_family_index,
            presentation_queue: queues.presentation_queue,
            presentation_queue_family_index: queue_families.presentation_queue_family_index,
        })
    }

    pub fn create_descriptors(&self) -> Result<Descriptors, Box<dyn Error>> {
        let driver_api_version = self
            .entry
            .try_enumerate_instance_version()?
            .unwrap_or(vk::API_VERSION_1_0);
        // vkEnumerateInstanceVersion isn't available in Vulkan 1.0

        let flags = if cfg!(debug_assertions) {
            InstanceFlags::VALIDATION | InstanceFlags::DEBUG
        } else {
            InstanceFlags::empty()
        }; // Logic taken from `VulkanHalInstance::init`

        let instance_extensions = VulkanHalInstance::required_extensions(&self.entry, flags)?;

        let has_nv_optimus = unsafe {
            let instance_layers = self.entry.enumerate_instance_layer_properties()?;
            let nv_optimus_layer = CStr::from_bytes_with_nul(b"VK_LAYER_NV_optimus\0")?;
            instance_layers
                .iter()
                .any(|inst_layer| CStr::from_ptr(inst_layer.layer_name.as_ptr()) == nv_optimus_layer)
        };

        let physical_device_properties = unsafe { self.instance.get_physical_device_properties(self.physical_device) };
        let instance = unsafe {
            VulkanHalInstance::from_raw(
                self.entry.clone(),
                self.instance.clone(),
                driver_api_version,
                get_android_sdk_version()?,
                instance_extensions,
                flags,
                has_nv_optimus,
                None,
                // None indicates that wgpu is *not* responsible for destroying the VkInstance
                // (in this case, that falls on the libretro frontend)
            )?
        };

        let adapter = instance
            .expose_adapter(self.physical_device)
            .ok_or(FailedToExposePhysicalDevice(self.physical_device))?;

        let uab_types =
            UpdateAfterBindTypes::from_limits(&adapter.capabilities.limits, &physical_device_properties.limits);

        let open_device = unsafe {
            let device_extensions = adapter.adapter.required_device_extensions(adapter.features);
            adapter.adapter.device_from_raw(
                self.device.clone(),
                false,
                &device_extensions,
                adapter.features,
                uab_types,
                self.queue_family_index,
                0, // wgpu assumes this to be 0
            )?
        };

        let instance = unsafe { wgpu::Instance::from_hal::<Vulkan>(instance) };
        let adapter = unsafe { instance.create_adapter_from_hal(adapter) };
        let (limits, features) = required_limits(&adapter);
        let (device, queue) = unsafe {
            adapter.create_device_from_hal(
                open_device,
                &wgpu::DeviceDescriptor {
                    label: None,
                    features,
                    limits,
                },
                None,
            )?
        };

        Ok(Descriptors::new(adapter, device, queue))
    }

    fn physical_device_features_any(features: vk::PhysicalDeviceFeatures) -> bool {
        let features: [vk::Bool32; 55] = unsafe { transmute(features) };

        features.iter().sum::<vk::Bool32>() > 0
    }

    // Only used if the surface extension isn't available
    // (Probably means we're not rendering to the screen)
    unsafe fn select_queue_family(queue_families: &[QueueFamilyProperties]) -> Result<u32, VulkanNegotiationError> {
        // The core must ensure that the queue and queue_family_index support GRAPHICS and COMPUTE.
        queue_families
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
        physical_device: vk::PhysicalDevice,
        initial_context: &RetroVulkanInitialContext,
        surface: vk::SurfaceKHR,
        surface_fn: &ash::extensions::khr::Surface,
    ) -> Result<QueueFamilies, Box<dyn Error>> {
        let queue_families = unsafe {
            initial_context
                .instance
                .get_physical_device_queue_family_properties(physical_device)
        };

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

        let queue_family = unsafe { Self::select_queue_family(&queue_families)? };
        // Here's our graphics/compute queue, now for a present queue

        let presentation_queue_family = queue_families
            .iter()
            .enumerate()
            .find_map(|(i, _)| unsafe {
                match surface_fn.get_physical_device_surface_support(physical_device, i as u32, surface) {
                    Ok(true) => Some(Ok(i as u32)), // This queue family supports presentation, let's use it
                    Ok(false) => None,              // This queue family doesn't support presentation, let's not use it
                    Err(error) => Some(Err(VulkanError("vkGetPhysicalDeviceSurfaceSupportKHR", error))), // There was an error, let's report it
                }
            })
            .transpose()?;

        if let Some(presentation_queue_family) = presentation_queue_family {
            // If we found a queue family that supports presentation...
            Ok(QueueFamilies::new(queue_family, presentation_queue_family))
        } else {
            Err(NoAcceptableQueueFamily)?
        }
    }

    fn create_logical_device(
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        queue_families: &QueueFamilies,
        enabled_extensions: &Names,
        enabled_layers: &Names,
        enabled_features: &vk::PhysicalDeviceFeatures,
    ) -> Result<ash::Device, Box<dyn Error>> {
        let queue_create_info = DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_families.queue_family_index)
            .queue_priorities(&[1.0f32]) //  The core is free to set its own queue priorities.
            .build();

        let presentation_queue_create_info = if queue_families.are_same() {
            queue_create_info
        } else {
            DeviceQueueCreateInfo::builder()
                .queue_family_index(queue_families.presentation_queue_family_index)
                .queue_priorities(&[1.0f32]) //  The core is free to set its own queue priorities.
                .build()
        };

        let queue_create_infos = [queue_create_info, presentation_queue_create_info];
        let queue_create_infos = if queue_families.are_same() {
            &queue_create_infos[0..1]
        } else {
            &queue_create_infos
        };

        let device_create_info = DeviceCreateInfo::builder()
            .queue_create_infos(queue_create_infos)
            .enabled_features(enabled_features)
            .enabled_extension_names(enabled_extensions.ptr_slice())
            .enabled_layer_names(enabled_layers.ptr_slice())
            // .flags(DeviceCreateFlags) VkDeviceCreateFlags is empty, currently reserved
            .build();

        unsafe { Ok(instance.create_device(physical_device, &device_create_info, None)?) }
    }
}

impl TryFrom<&RetroVulkanCreatedContext> for RetroVulkanCreatedContextWgpuHal {
    type Error = Box<dyn Error>;

    fn try_from(value: &RetroVulkanCreatedContext) -> Result<Self, Self::Error> {
        todo!()
    }
}

/*
impl From<&RetroVulkanInitialContextWgpuHal> for RetroVulkanContextAsh {
    fn from(value: &RetroVulkanInitialContextWgpuHal) -> Self {
        let device = &value.open_device.device;
        let instance = value.instance.shared_instance();
        let entry = instance.entry();
        let surface_fn = Surface::new(entry, instance.raw_instance());
        Self {
            entry: entry.clone(),
            instance: instance.raw_instance().clone(),
            physical_device: device.raw_physical_device(),
            device: device.raw_device().clone(),
            surface_fn,
            surface: Default::default(),
            queue: device.raw_queue(),
            queue_family_index: device.queue_family_index(),
            presentation_queue: value.presentation_queue,
            presentation_queue_family_index: value.presentation_queue_family_index,
        }
    }
}

impl TryFrom<&RetroVulkanContextAsh> for RetroVulkanInitialContextWgpuHal {
    type Error = Box<dyn Error>;

    fn try_from(value: &RetroVulkanContextAsh) -> Result<Self, Self::Error> {
        let entry = &value.entry;
        let instance = &value.instance;
        let driver_api_version = entry.try_enumerate_instance_version()?.unwrap_or(vk::API_VERSION_1_0);
        // vkEnumerateInstanceVersion isn't available in Vulkan 1.0

        let flags = if cfg!(debug_assertions) {
            InstanceFlags::VALIDATION | InstanceFlags::DEBUG
        } else {
            InstanceFlags::empty()
        }; // Logic taken from `VulkanHalInstance::init`

        // TODO: Get extensions that value already had enabled
        let extensions = VulkanHalInstance::required_extensions(&entry, flags)?;

        let has_nv_optimus = unsafe {
            let instance_layers = entry.enumerate_instance_layer_properties()?;
            let nv_optimus_layer = CStr::from_bytes_with_nul(b"VK_LAYER_NV_optimus\0")?;
            instance_layers
                .iter()
                .any(|inst_layer| CStr::from_ptr(inst_layer.layer_name.as_ptr()) == nv_optimus_layer)
        };

        let physical_device_properties = unsafe { instance.get_physical_device_properties(value.physical_device) };

        let instance = unsafe {
            VulkanHalInstance::from_raw(
                entry.clone(),
                instance.clone(),
                driver_api_version,
                get_android_sdk_version()?,
                extensions,
                flags,
                has_nv_optimus,
                None,
                // None indicates that wgpu is *not* responsible for destroying the VkInstance
                // (in this case, that falls on the libretro frontend)
            )?
        };

        let adapter = instance
            .expose_adapter(value.physical_device)
            .ok_or(FailedToExposePhysicalDevice(value.physical_device))?;

        let uab_types =
            UpdateAfterBindTypes::from_limits(&adapter.capabilities.limits, &physical_device_properties.limits);

        let open_device = unsafe {
            adapter.adapter.device_from_raw(
                value.device.clone(),
                false,
                &extensions,
                adapter.features,
                uab_types,
                value.queue_family_index,
                0, // wgpu assumes this to be 0
            )?
        };

        Ok(Self {
            instance,
            adapter,
            open_device,
            presentation_queue: value.presentation_queue,
            presentation_queue_family_index: value.presentation_queue_family_index,
        })
    }
}
*/

#[derive(Clone, Copy, Debug)]
pub struct QueueFamilies {
    pub queue_family_index: u32,
    pub presentation_queue_family_index: u32,
}

impl QueueFamilies {
    pub fn new(queue_family_index: u32, presentation_queue_family_index: u32) -> Self {
        Self {
            queue_family_index,
            presentation_queue_family_index,
        }
    }

    pub fn are_same(&self) -> bool {
        self.queue_family_index == self.presentation_queue_family_index
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Queues {
    queue: vk::Queue,
    presentation_queue: vk::Queue,
}

impl Queues {
    pub fn new(queue: vk::Queue, presentation_queue: vk::Queue) -> Self {
        Self {
            queue,
            presentation_queue,
        }
    }
}

fn get_android_sdk_version() -> Result<u32, Box<dyn Error>> {
    #[cfg(not(target_os = "android"))]
    return Ok(0);

    #[cfg(target_os = "android")]
    return {
        let properties = android_system_properties::AndroidSystemProperties::new();
        // See: https://developer.android.com/reference/android/os/Build.VERSION_CODES
        if let Some(val) = properties.get("ro.build.version.sdk") {
            match val.parse::<u32>() {
                Ok(sdk_ver) => sdk_ver,
                Err(err) => {
                    log::error!("Couldn't parse Android's ro.build.version.sdk system property ({val}): {err}");
                    0
                }
            }
        } else {
            log::error!("Couldn't read Android's ro.build.version.sdk system property");
            0
        }
    };
}
