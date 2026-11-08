use std::collections::VecDeque;
use std::path::PathBuf;

use ruffle_core::PlayerEvent;
use rust_libretro::proc::CoreOptions;
#[allow(unused_imports)] // The CoreOptions derive needs these in scope.
use rust_libretro::{contexts::*, sys::*};

use crate::core::config::Config;
use crate::core::session::Session;
use crate::util::keyboard::KeyboardState;

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
    "Options related to how content behaves."
}
)]
#[options(
{
    "ruffle_quality",
    "Video > Quality",
    "Quality",
    "Default rendering quality. Higher settings use more anti-aliasing. Content can change this itself.",
    "Default rendering quality. Higher settings use more anti-aliasing. Content can change this itself.",
    "video_settings",
    {
        { "low", "Low" },
        { "medium", "Medium" },
        { "high", "High" },
        { "best", "Best" },
        { "high8x8", "High (8x8)" },
        { "high16x16", "High (16x16)" },
    },
    "high"
},
{
    "ruffle_max_execution_duration",
    "Content > Max Execution Duration",
    "Max Execution Duration",
    "Maximum time, in seconds, that a single ActionScript call may run before it is stopped.",
    "Maximum time, in seconds, that a single ActionScript call may run before it is stopped.",
    "content_settings",
    {
        { "10", "10 seconds" },
        { "15", "15 seconds" },
        { "30", "30 seconds" },
        { "60", "60 seconds" },
        { "120", "120 seconds" },
        { "unlimited", "No Limit" },
    },
    "15"
},
{
    "ruffle_web_browser_access",
    "Content > Web Browser Access",
    "Web Browser Access",
    "What to do when the movie tries to open a web page.",
    "What to do when the movie tries to open a web page.",
    "content_settings",
    {
        { "off", "Ignore" },
        { "notify", "Show the URL" },
        { "external", "Open in Web Browser" },
    },
    "notify"
},
{
    "ruffle_spoofed_url",
    "Content > Spoofed URL",
    "Spoofed URL",
    "Pretend the movie was loaded from this URL instead of from disk. Helps with some site-locked content. Restart required.",
    "Pretend the movie was loaded from this URL instead of from disk. Helps with some site-locked content. Restart required.",
    "content_settings",
    {
        { "none", "Off" },
        { "http://localhost/", "http://localhost/" },
        { "https://localhost/", "https://localhost/" },
    },
    "none"
},
{
    "ruffle_audio_sample_rate",
    "Audio > Sample Rate",
    "Sample Rate",
    "Output sample rate in Hz. Restart required.",
    "Output sample rate in Hz. Restart required.",
    "audio_settings",
    {
        { "44100", "44100 Hz" },
        { "48000", "48000 Hz" },
    },
    "44100"
}
)]
pub struct Ruffle {
    config: Config,
    environ_cb: retro_environment_t,
    session: Option<Session>,
    /// Kept so `retro_reset` can load the movie again.
    content_path: Option<PathBuf>,
    save_directory: Option<PathBuf>,
    keyboard: KeyboardState,
    /// Keyboard events arrive through a callback outside of `retro_run`,
    /// so they're queued here until the next frame.
    queued_events: VecDeque<PlayerEvent>,
    port_device: u32,
}

impl Ruffle {
    pub fn new() -> Ruffle {
        Ruffle {
            config: Config::default(),
            environ_cb: None,
            session: None,
            content_path: None,
            save_directory: None,
            keyboard: KeyboardState::default(),
            queued_events: VecDeque::with_capacity(16),
            port_device: RETRO_DEVICE_JOYPAD,
        }
    }
}

pub mod config;
mod core;
mod input;
mod session;
