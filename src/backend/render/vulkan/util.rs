use ash::vk;
use ash::vk::{ExtensionProperties, LayerProperties};
use ash::{
    extensions::ext::DebugUtils,
    vk::{DebugUtilsObjectNameInfoEXT, Handle},
};
use log::debug;
use ruffle_render_wgpu::descriptors::Descriptors;
use rust_libretro::anyhow;
use std::error::Error;
use std::ffi::{c_char, c_uint, CStr, CString};
use std::fmt::{Debug, Display, Formatter};
use std::intrinsics::transmute;
use std::slice::from_raw_parts;
use wgpu_core::api::Vulkan;
use wgpu_hal::Api;
use wgpu_hal::InstanceFlags;

use crate::backend::render::wgpu::required_limits;

use super::render_interface::VulkanRenderInterface;

type VulkanInstance = <Vulkan as Api>::Instance;

#[derive(Clone, Copy, Debug)]
pub(crate) struct QueueFamily(pub vk::QueueFamilyProperties, pub u32);

#[derive(Clone, Copy, Debug)]
pub(crate) struct Queue(pub vk::Queue, pub u32);

#[derive(Clone, Copy, Debug)]
pub(crate) enum QueueFamilies {
    /// Represents a single VkQueue that supports graphics, compute, and present.
    Single(QueueFamily),
    Split {
        graphics_compute: QueueFamily,
        present: QueueFamily,
    },
    GraphicsComputeOnly(QueueFamily),
}

impl QueueFamilies {
    pub(crate) fn queue_family_index(&self) -> u32 {
        match self {
            Self::Single(q) => q.1,
            Self::Split { graphics_compute, .. } => graphics_compute.1,
            Self::GraphicsComputeOnly(q) => q.1,
        }
    }

    pub(crate) fn presentation_queue_family_index(&self) -> u32 {
        match self {
            Self::Single(q) => q.1,
            Self::Split { present, .. } => present.1,
            Self::GraphicsComputeOnly(_) => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Queues {
    /// Represents a single VkQueue that supports graphics, compute, and present.
    Single(Queue),
    Split {
        graphics_compute: Queue,
        present: Queue,
    },
    GraphicsComputeOnly(Queue),
}

impl Queues {
    pub(crate) unsafe fn new(device: &ash::Device, families: &QueueFamilies) -> Self {
        match families {
            QueueFamilies::Single(family) => {
                let queue = device.get_device_queue(family.1, 0);
                Self::Single(Queue(queue, 0))
            }
            QueueFamilies::Split {
                graphics_compute,
                present,
            } => {
                let graphics_compute = device.get_device_queue(graphics_compute.1, 0);
                let present = device.get_device_queue(present.1, 0);
                Self::Split {
                    graphics_compute: Queue(graphics_compute, 0),
                    present: Queue(present, 0),
                }
            }
            QueueFamilies::GraphicsComputeOnly(family) => {
                let queue = device.get_device_queue(family.1, 0);
                Self::GraphicsComputeOnly(Queue(queue, 0))
            }
        }
    }

    pub(crate) fn queue(&self) -> vk::Queue {
        match self {
            Self::Single(q) => q.0,
            Self::Split { graphics_compute, .. } => graphics_compute.0,
            Self::GraphicsComputeOnly(q) => q.0,
        }
    }

    pub(crate) fn presentation_queue(&self) -> vk::Queue {
        match self {
            Self::Single(q) => q.0,
            Self::Split { present, .. } => present.0,
            Self::GraphicsComputeOnly(_) => vk::Queue::null(),
        }
    }
}

pub(crate) struct PropertiesFormat<'a, T> {
    properties: &'a [T],
}

impl<'a, T> PropertiesFormat<'a, T> {
    pub fn new(properties: &'a [T]) -> Self {
        Self { properties }
    }
}

impl<'a> Debug for PropertiesFormat<'a, ExtensionProperties> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.properties.iter().map(|p| unsafe {
                let cstr = CStr::from_ptr(p.extension_name.as_ptr());
                format!("ExtensionProperties({cstr:?}, spec={})", p.spec_version)
            }))
            .finish()
    }
}

impl<'a> Debug for PropertiesFormat<'a, LayerProperties> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.properties.iter().map(|p| unsafe {
                let layer_name = CStr::from_ptr(p.layer_name.as_ptr());
                let description = CStr::from_ptr(p.description.as_ptr());
                format!(
                    "LayerProperties({layer_name:?}, description={description:?}, spec={}, implementation={})",
                    p.spec_version, p.implementation_version
                )
            }))
            .finish()
    }
}

pub fn physical_device_features_any(features: vk::PhysicalDeviceFeatures) -> bool {
    let features: [vk::Bool32; 55] = unsafe { transmute(features) };

    features.iter().sum::<vk::Bool32>() > 0
}

pub fn get_android_sdk_version() -> anyhow::Result<u32> {
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

#[cfg(debug_assertions)]
pub unsafe fn set_debug_name<T: Handle + Debug + Copy>(
    debug_utils: &DebugUtils,
    device: &ash::Device,
    handle: T,
    name: &[u8],
) {
    use log::warn;

    let device = device.handle();
    let object_name_info = DebugUtilsObjectNameInfoEXT::builder()
        .object_handle(handle.as_raw())
        .object_type(T::TYPE)
        .object_name(CStr::from_bytes_with_nul(name).unwrap());

    if let Err(e) = debug_utils.set_debug_utils_object_name(device, &object_name_info) {
        warn!("vkSetDebugUtilsObjectNameEXT failed on {handle:?}: {e}");
    }
}

pub fn create_descriptors(interface: &VulkanRenderInterface) -> anyhow::Result<Descriptors> {
    let entry = interface.entry();
    let instance = interface.instance();
    let physical_device = interface.physical_device();
    let device = interface.device();

    let driver_api_version = entry.try_enumerate_instance_version()?.unwrap_or(vk::API_VERSION_1_0);
    // vkEnumerateInstanceVersion isn't available in Vulkan 1.0

    let flags = if cfg!(debug_assertions) {
        InstanceFlags::VALIDATION | InstanceFlags::DEBUG
    } else {
        InstanceFlags::empty()
    }; // Logic taken from `VulkanHalInstance::init`

    let instance_extensions = VulkanInstance::required_extensions(entry, 0, flags)?;
    debug!("Instance extensions required by wgpu: {instance_extensions:#?}");

    let has_nv_optimus = unsafe {
        let instance_layers = entry.enumerate_instance_layer_properties()?;
        let nv_optimus_layer = CStr::from_bytes_with_nul(b"VK_LAYER_NV_optimus\0")?;
        instance_layers
            .iter()
            .any(|inst_layer| CStr::from_ptr(inst_layer.layer_name.as_ptr()) == nv_optimus_layer)
    };

    let instance = unsafe {
        VulkanInstance::from_raw(
            entry.clone(),
            instance.clone(),
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
        .expose_adapter(physical_device)
        .ok_or(anyhow::anyhow!("Failed to expose physical device {physical_device:?}"))?;

    let open_device = unsafe {
        let device_extensions = adapter.adapter.required_device_extensions(adapter.features);
        adapter.adapter.device_from_raw(
            device.clone(),
            false,
            &device_extensions,
            adapter.features,
            0,                       // TODO: Add interface.queue_family_index()
            interface.queue_index(), // wgpu assumes this to be 0
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
