use ash::vk;
use ash::extensions::ext;

// TODO: Should I put these behind locks?

pub(super) static mut APPLICATION_INFO: Option<vk::ApplicationInfo> = None;
pub(super) static mut ENTRY: Option<ash::Entry> = None;
pub(super) static mut INSTANCE: Option<wgpu::Instance> = None;

// We can't make DEVICE a wgpu::Device because Ruffle's `Descriptors`
// will want to take ownership of it.
pub(super) static mut DEVICE: Option<ash::Device> = None;

#[cfg(debug_assertions)]
pub(super) static mut DEBUG_UTILS: Option<ext::DebugUtils> = None;