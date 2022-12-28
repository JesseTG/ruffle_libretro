use std::rc::Rc;
use ruffle_core::{LoadBehavior, Player};
use rust_libretro::contexts::GenericContext;
use rust_libretro::sys::retro_system_av_info;
use rust_libretro::{contexts::*, proc::CoreOptions, sys::*};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ruffle_core::config::Letterbox;
use crate::core::config::Config;
use crate::options::{FileAccessPolicy, WebBrowserAccess};

#[derive(CoreOptions)]
#[categories(
{
    "video_settings",
    "Video",
    "Options related to video output."
},
{
    "audio_settings",
    "Audio",
    "Options related to audio output."
},
{
    "content_settings",
    "Content",
    "Options related to content."
}
)]
#[options(
{
    "ruffle_autoplay",
    "Video > Autoplay",
    "Autoplay",
    "Setting 'Video > Autoplay' will start playing the movie immediately upon load.",
    "",
    "video_settings",
    {
        { "true" },
        { "false" },
    }
},
{
    "ruffle_letterbox",
    "Video > Letterbox",
    "Letterbox",
    "Controls whether the content is letterboxed or pillarboxed when the player's aspect ratio does not match the movie's aspect ratio.
When letterboxed, black bars will be rendered around the exterior margins of the content.",
    "",
    "video_settings",
    {
        { "off", "Off" },
        { "fullscreen", "Fullscreen Only" },
        { "on", "Always" }
    },
    "fullscreen"
},
{
    "ruffle_max_execution_duration",
    "Content > Max Execution Duration",
    "Max Execution Duration",
    "Sets the maximum execution time of ActionScript code, in seconds.",
    "",
    "content_settings",
    {
        { "10" },
        { "15" },
        { "30" },
        { "45" },
        { "60" },
        { "120" },
        { "18446744073709551616", "No Limit" },
    },
    "15"
},
{
    "ruffle_msaa",
    "Video > MSAA",
    "MSAA",
    "TODO",
    "",
    "video_settings",
    {
        { "0", "Off" },
        { "2", "2x" },
        { "4", "4x" },
    },
},
{
    "ruffle_warn_on_unsupported_content",
    "Content > Warn on Unsupported Content",
    "Warn on Unsupported Content",
    "Configures the player to warn if unsupported content is detected (ActionScript 3.0).",
    "Configures the player to warn if unsupported content is detected (ActionScript 3.0).",
    "content_settings",
    {
        { "true" },
        { "false" },
    }
},
{
    "ruffle_file_access_policy",
    "Content > file:// Protocol Policy",
    "file:// Protocol Policy",
    "Decide what to do if the movie requests a file on the local file system with file:// URLs. Make sure you trust this movie!",
    "",
    "content_settings",
    {
        { "never", "Never" },
        { "notify", "Notify of Access" },
        { "always", "Always" },
    }
},
{
    "ruffle_web_browser_access",
    "Content > Web Browser Access",
    "Web Browser Access",
    "Decide what to do if the movie navigates the browser to a URL.",
    "",
    "content_settings",
    {
        { "off", "Off" },
        { "off-notify", "Off (but Notify)" },
        { "external", "External Window" },
    }
},
{
    "ruffle_spoofed_url",
    "Content > Spoofed URL",
    "Spoofed URL",
    "TODO",
    "",
    "content_settings",
    {
        { "none" },
        { "http_localhost", "http://localhost" },
        { "https_localhost", "https://localhost" },
        { "https_localhost", "https://localhost" },
        { "file_path", "Movie path (via file://)" },
    }
},
{
    "ruffle_load_behavior",
    "Content > Load Behavior",
    "Load Behavior",
    "Configures how the root movie should be loaded.",
    "Configures how the root movie should be loaded.",
    "content_settings",
    {
        { "streaming" },
        { "blocking" },
        { "delayed" },
    }
},
{
    "ruffle_audio_sample_rate",
    "Audio > Sample Rate",
    "Sample Rate",
    "Configures how the root movie should be loaded.",
    "Configures how the root movie should be loaded.",
    "audio_settings",
    {
        { "44100" },
        { "48000" },
    }
}
)]
pub struct Ruffle {
    player: Option<Arc<Mutex<Player>>>,
    vfs_interface_version: Option<u32>,
    av_info: Option<retro_system_av_info>,

    config: Config,
}

impl Ruffle {
    pub fn new() -> Ruffle {
        Ruffle {
            player: None,
            vfs_interface_version: None,
            av_info: None,
            config: Config {
                autoplay: true,
                letterbox: Letterbox::Fullscreen,
                max_execution_duration: Duration::from_secs(15),
                warn_on_unsupported_content: true,
                load_behavior: LoadBehavior::Streaming,
                file_access_policy: FileAccessPolicy::Never,
                web_browser_access: WebBrowserAccess::Notify,
                spoofed_url: None,
                sample_rate: 44100,
                msaa: 0,
            }
        }
    }
}

mod core;
pub mod config;
