use std::error::Error;
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::mem::transmute;
use std::ptr;

use ash::extensions::khr::Surface;
use ash::prelude::VkResult;
use ash::vk;
use ash::vk::{
    DeviceCreateInfo, DeviceQueueCreateInfo, PFN_vkGetInstanceProcAddr, PhysicalDeviceFeatures, QueueFamilyProperties,
    QueueFlags, StaticFn,
};
use log::{debug, info, log_enabled, warn};
use ruffle_render_wgpu::descriptors::Descriptors;
use rust_libretro_sys::retro_vulkan_create_device_wrapper_t;
use rust_libretro_sys::vulkan::VkPhysicalDevice;
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;
use wgpu_hal::{Api, ExposedAdapter, InstanceFlags, OpenDevice};

use crate::backend::render::required_limits;
use crate::backend::render::vulkan::context::ContextConversionError::FailedToExposePhysicalDevice;
use crate::backend::render::vulkan::negotiation::VulkanNegotiationError;
use crate::backend::render::vulkan::negotiation::VulkanNegotiationError::{
    NoAcceptablePhysicalDevice, NoAcceptableQueueFamily, NoPhysicalDevicesFound,
};
use crate::backend::render::vulkan::util;
use crate::backend::render::vulkan::util::PropertiesFormat;
use crate::backend::render::vulkan::util::{physical_device_features_any, Names, QueueFamilies, Queues};
use crate::backend::render::vulkan::VulkanRenderBackendError::VulkanError;

pub type VulkanHalInstance = <Vulkan as Api>::Instance;
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
    pub create_device_wrapper: retro_vulkan_create_device_wrapper_t,
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
        get_instance_proc_addr: Option<PFN_vkGetInstanceProcAddr>,
        create_device_wrapper: retro_vulkan_create_device_wrapper_t,
    ) -> Result<Self, Box<dyn Error>> {
        if instance == vk::Instance::null() {
            Err("Frontend called create_device without a valid VkInstance")?
        }

        if get_instance_proc_addr.is_none() {
            Err("Frontend called create_device with a null PFN_vkGetInstanceProcAddr")?
        }
        let load_symbols = |sym: &CStr| {
            let fun = (get_instance_proc_addr.unwrap())(instance, sym.as_ptr());
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
            create_device_wrapper,
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
            .filter_map(|device| self.filter_physical_device(device))
            .collect::<Result<Vec<_>, _>>()?;

        match available_physical_devices.len() {
            0 => Err(NoAcceptablePhysicalDevice)?,
            _ => Ok(available_physical_devices[0]),
            // TODO: Implement real logic to pick a device, instead of just getting the first
        }
    }

    fn filter_physical_device(&self, physical_device: vk::PhysicalDevice) -> Option<VkResult<VkPhysicalDevice>> {
        // See if this VkPhysicalDevice meets the following conditions...
        info!("Evaluating VkPhysicalDevice {physical_device:?}");

        // A device with a queue that supports GRAPHICS and COMPUTE...
        let queue_families = unsafe {
            self.instance
                .get_physical_device_queue_family_properties(physical_device)
        };

        let required_families_supported = queue_families
            .iter()
            .any(|q| q.queue_flags.contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE));

        if required_families_supported {
            Some(Ok(physical_device))
        } else {
            None
        }
    }
}

impl RetroVulkanCreatedContext {
    pub unsafe fn new(
        initial_context: &RetroVulkanInitialContext,
        opaque: *mut c_void,
    ) -> Result<Self, Box<dyn Error>> {
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

        if log_enabled!(log::Level::Info) {
            let available_device_extensions = initial_context
                .instance
                .enumerate_device_extension_properties(physical_device)?;
            info!("Available extensions for this device: {:#?}", PropertiesFormat::new(&available_device_extensions));
        }

        let device = Self::create_logical_device(
            &initial_context.instance,
            physical_device,
            &queue_families,
            |info| {
                initial_context.create_device_wrapper.unwrap()(physical_device, opaque, info)
            }
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

        let instance_extensions = VulkanHalInstance::required_extensions(&self.entry, vk::API_VERSION_1_2, flags)?;
        debug!("Instance extensions required by wgpu: {instance_extensions:#?}");

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
                util::get_android_sdk_version()?,
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

        let open_device = unsafe {
            let device_extensions = adapter.adapter.required_device_extensions(adapter.features);
            adapter.adapter.device_from_raw(
                self.device.clone(),
                false,
                &device_extensions,
                adapter.features,
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

    fn create_logical_device<F>(
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        queue_families: &QueueFamilies,
        create_device_wrapper: F,
    ) -> Result<ash::Device, Box<dyn Error>>
    where
        F: FnOnce(&DeviceCreateInfo) -> vk::Device,
    {
        let queue_create_info = DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_families.queue_family_index)
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
            .push_next(&mut physical_device_vulkan_12_features)
            // .flags(DeviceCreateFlags) VkDeviceCreateFlags is empty, currently reserved
            .build();

        let device = create_device_wrapper(&device_create_info);
        unsafe {
            Ok(ash::Device::load(instance.fp_v1_0(), device))
        }
    }
}

impl TryFrom<&RetroVulkanCreatedContext> for RetroVulkanCreatedContextWgpuHal {
    type Error = Box<dyn Error>;

    fn try_from(value: &RetroVulkanCreatedContext) -> Result<Self, Self::Error> {
        todo!()
    }
}
