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
    pub(crate) upgrade_to_https: bool,
}

impl Config {
    pub fn new() -> Self {
        Self {
            autoplay: defaults::AUTOPLAY,
            letterbox: defaults::LETTERBOX,
            max_execution_duration: defaults::MAX_EXECUTION_DURATION,
            warn_on_unsupported_content: defaults::WARN_ON_UNSUPPORTED_CONTENT,
            load_behavior: defaults::LOAD_BEHAVIOR,
            file_access_policy: defaults::FILE_ACCESS_POLICY,
            web_browser_access: defaults::WEB_BROWSER_ACCESS,
            spoofed_url: None,
            sample_rate: defaults::SAMPLE_RATE,
            msaa: defaults::MSAA,
            upgrade_to_https: defaults::UPGRADE_TO_HTTPS,
        }
    }
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
    pub const UPGRADE_TO_HTTPS: bool = true;
}
