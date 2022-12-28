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
    pub(crate) msaa: u8,
}

pub mod defaults {
    use ruffle_core::config::Letterbox;
    use ruffle_core::LoadBehavior;
    use std::time::Duration;
    use crate::options::{FileAccessPolicy, WebBrowserAccess};

    pub const AUTOPLAY: bool = true;
    pub const LETTERBOX: Letterbox = Letterbox::Fullscreen;
    pub const MAX_EXECUTION_DURATION: Duration = Duration::from_secs(15);
    pub const MSAA: u8 = 0;
    pub const WARN_ON_UNSUPPORTED_CONTENT: bool = true;
    pub const LOAD_BEHAVIOR: LoadBehavior = LoadBehavior::Streaming;
    pub const FILE_ACCESS_POLICY: FileAccessPolicy = FileAccessPolicy::Never;
    pub const WEB_BROWSER_ACCESS: WebBrowserAccess = WebBrowserAccess::Ignore;
    pub const SAMPLE_RATE: u32 = 44100;
}
