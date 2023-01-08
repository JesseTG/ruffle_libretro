use crate::backend::render::vulkan::context::ContextConversionError::FailedToExposePhysicalDevice;
use ash::vk;
use rust_libretro_sys::retro_vulkan_context;
use std::error::Error;
use std::ffi::CStr;
use thiserror::Error as ThisError;
use wgpu_hal::api::Vulkan;
use wgpu_hal::{Adapter, Api, ExposedAdapter, InstanceFlags, OpenDevice, UpdateAfterBindTypes};

pub type VulkanHalInstance = <Vulkan as Api>::Instance;
pub type VulkanHalDevice = <Vulkan as Api>::Device;
pub type VulkanHalAdapter = <Vulkan as Api>::Adapter;
pub type VulkanHalQueue = <Vulkan as Api>::Queue;
pub type VulkanHalExposedAdapter = ExposedAdapter<Vulkan>;
pub type VulkanHalOpenDevice = OpenDevice<Vulkan>;

#[derive(ThisError, Debug)]
pub enum ContextConversionError {
    #[error("Failed to expose VkPhysicalDevice {0:?}")]
    FailedToExposePhysicalDevice(vk::PhysicalDevice),
}
/// The Vulkan resources that are exposed to cores,
/// but owned by libretro.
/// These must not be destroyed by the core.
struct RetroVulkanContextRaw {
    static_fn: vk::StaticFn,
    physical_device: vk::PhysicalDevice,
    instance: vk::Instance,
    surface: vk::SurfaceKHR,
}

/// Ash wrappers around the Vulkan resources that are exposed to cores,
/// but owned by libretro.
#[derive(Copy, Clone)]
struct RetroVulkanContextAsh {
    entry: ash::Entry,
    instance: ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    surface_fn: ash::extensions::khr::Surface,
}

struct RetroVulkanContextWgpuHal {
    instance: VulkanHalInstance,
    adapter: VulkanHalExposedAdapter,
    open_device: VulkanHalOpenDevice,
}

impl From<&RetroVulkanContextWgpuHal> for RetroVulkanContextAsh {
    fn from(value: &RetroVulkanContextWgpuHal) -> Self {
        let device = &value.open_device.device;
        let instance = value.instance.shared_instance();
        let entry = instance.entry();
        let surface_fn = ash::extensions::khr::Surface::new(entry, instance.raw_instance());
        Self {
            device: device.raw_device().clone(),
            instance: instance.raw_instance().clone(),
            entry: entry().clone(),
            surface_fn,
            physical_device: device.raw_physical_device(),
        }
    }
}

impl From<&RetroVulkanContextRaw> for RetroVulkanContextAsh {
    fn from(value: &RetroVulkanContextRaw) -> Self {
        Self {
            entry: (),
            instance: (),
            physical_device: Default::default(),
            device: (),
            surface_fn: (),
        }
    }
}

impl TryFrom<&RetroVulkanContextAsh> for RetroVulkanContextWgpuHal {
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
            let nv_optimus_layer = CStr::from_bytes_with_nul(b"VK_LAYER_NV_optimus\0").unwrap();
            instance_layers
                .iter()
                .any(|inst_layer| CStr::from_ptr(inst_layer.layer_name.as_ptr()) == nv_optimus_layer)
        };

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

        let uab_types = UpdateAfterBindTypes::from_limits(limits, phd_limits);

        let open_device = unsafe {
            adapter.adapter.device_from_raw(
                value.device.clone(),
                false,
                &extensions,
                wgpu_types::Features::all(), // TODO: Populate properly
                uab_types,
                0, // TODO: Populate properly
                0, // wgpu assumes this to be 0
            )?
        };

        Ok(Self {
            instance,
            adapter,
            open_device,
        })
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
