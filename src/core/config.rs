use crate::options::{FileAccessPolicy, WebBrowserAccess};
use ruffle_core::config::Letterbox;
use ruffle_core::LoadBehavior;
use std::time::Duration;

pub struct Config {
    pub(crate) autoplay: bool,
    pub(crate) letterbox: Letterbox,
    pub(crate) max_execution_duration: Duration,
    pub(crate) warn_on_unsupported_content: bool,
    pub(crate) load_behavior: LoadBehavior,
    pub(crate) file_access_policy: FileAccessPolicy,
    pub(crate) web_browser_access: WebBrowserAccess,
    pub(crate) spoofed_url: Option<String>,
    pub(crate) sample_rate: u32,
}
