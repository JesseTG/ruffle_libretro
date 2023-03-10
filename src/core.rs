use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::Arc;

#[cfg(feature = "profiler")]
use profiling::tracy_client::Client;
use ruffle_core::PlayerEvent;
use rust_libretro::contexts::GenericContext;
use rust_libretro::sys::retro_system_av_info;
use rust_libretro::{contexts::*, proc::CoreOptions, sys::*};

use crate::core::config::Config;
use crate::core::state::PlayerState;
use crate::util::mouse::MouseState;

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
    player: PlayerState,
    av_info: Option<retro_system_av_info>,
    vfs: Arc<Cell<Option<retro_vfs_interface>>>,
    environ_cb: Arc<Cell<retro_environment_t>>,
    config: Config,
    frontend_preferred_hw_render: retro_hw_context_type,
    queued_events: VecDeque<PlayerEvent>,
    mouse_state: MouseState,
    #[cfg(feature = "profiler")]
    tracy_client: Option<Client>,
}

impl Ruffle {
    pub fn new() -> Ruffle {
        Ruffle {
            player: PlayerState::Uninitialized,
            av_info: None,
            vfs: Arc::new(Cell::new(None)),
            environ_cb: Arc::new(Cell::new(None)),
            config: Config::new(),
            frontend_preferred_hw_render: retro_hw_context_type::RETRO_HW_CONTEXT_NONE,
            queued_events: VecDeque::with_capacity(16),
            mouse_state: MouseState::default(),
            #[cfg(feature = "profiler")]
            tracy_client: None,
        }
    }
}

pub mod config;
mod core;
mod input;
mod state;
