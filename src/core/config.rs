use std::sync::{Arc, Mutex};
use std::time::Duration;

use ruffle_render::quality::StageQuality;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebBrowserAccess {
    Ignore,
    Notify,
    OpenInBrowser,
}

/// Shared with the navigator, so a changed option applies to a movie that's already running.
#[derive(Debug, Clone)]
pub struct SharedWebBrowserAccess(Arc<Mutex<WebBrowserAccess>>);

impl SharedWebBrowserAccess {
    pub fn get(&self) -> WebBrowserAccess {
        *self.0.lock().expect("lock poisoned")
    }

    pub fn set(&self, access: WebBrowserAccess) {
        *self.0.lock().expect("lock poisoned") = access;
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub quality: StageQuality,
    pub max_execution_duration: Duration,
    pub web_browser_access: SharedWebBrowserAccess,
    pub spoofed_url: Option<String>,
    pub sample_rate: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            quality: StageQuality::High,
            max_execution_duration: Duration::from_secs(15),
            web_browser_access: SharedWebBrowserAccess(Arc::new(Mutex::new(WebBrowserAccess::Notify))),
            spoofed_url: None,
            sample_rate: 44100,
        }
    }
}
