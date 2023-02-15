use ash::vk;
use ash::vk::{ExtensionProperties, LayerProperties};
use rust_libretro::anyhow;
use std::error::Error;
use std::ffi::{c_char, c_uint, CStr, CString};
use std::fmt::{Debug, Display, Formatter};
use std::intrinsics::transmute;
use std::slice::from_raw_parts;

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
    pub(crate) queue: vk::Queue,
    pub(crate) presentation_queue: vk::Queue,
}

impl Queues {
    pub fn new(queue: vk::Queue, presentation_queue: vk::Queue) -> Self {
        Self {
            queue,
            presentation_queue,
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
