use ash::vk;
use ash::vk::{ExtensionProperties, LayerProperties};
use ash::{
    extensions::ext::DebugUtils,
    vk::{DebugUtilsObjectNameInfoEXT, Handle},
};
use rust_libretro::anyhow;
use std::error::Error;
use std::ffi::{c_char, c_uint, CStr, CString};
use std::fmt::{Debug, Display, Formatter};
use std::intrinsics::transmute;
use std::slice::from_raw_parts;

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
