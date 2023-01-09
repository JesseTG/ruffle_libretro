use ash::vk;
use ash::vk::{ExtensionProperties, LayerProperties};
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

    pub fn add_str(&mut self, name: &'static str) -> &mut Self {
        let cstring = CString::new(name).unwrap();
        self.cstring.push(cstring);
        self.ptr.push(unsafe { self.cstring[self.cstring.len() - 1].as_ptr() });
        self
    }

    pub fn add_cstr(&mut self, name: &'static CStr) -> &mut Self {
        let cstring = CString::from(name);
        self.cstring.push(cstring);
        self.ptr.push(unsafe { self.cstring[self.cstring.len() - 1].as_ptr() });
        self
    }

    pub fn is_empty(&self) -> bool {
        self.cstring.is_empty()
    }

    pub fn ptr_slice(&self) -> &[*const c_char] {
        &self.ptr
    }

    /// Returns true if the provided extension properties include all of this object's names
    pub fn supported_by(&self, available_extensions: &[ExtensionProperties]) -> bool {
        if available_extensions.is_empty() {
            // If no extensions are available, then any requirements listed by this Names
            // won't be met (unless it's empty).
            return self.cstring.is_empty();
        }

        if self.cstring.is_empty() {
            // But if there *are* available extensions
            // and this Names doesn't ask for any,
            // then we're good.
            return true;
        }

        let available_extensions: Vec<&CStr> = available_extensions
            .iter()
            .map(|e| unsafe { CStr::from_ptr(e.extension_name.as_ptr()) })
            .collect();

        self.cstring
            .iter()
            .all(|n| available_extensions.contains(&n.as_c_str()))
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

pub fn get_android_sdk_version() -> Result<u32, Box<dyn Error>> {
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
