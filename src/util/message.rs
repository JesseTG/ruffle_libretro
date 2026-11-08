use log::{log, Level};
use rust_libretro::environment;
use rust_libretro::sys::{retro_environment_t, retro_log_level, retro_message_target, retro_message_type};
use rust_libretro::types::MessageProgress;

/// Shows `message` on the frontend's on-screen display for `duration_ms`,
/// and also writes it to the log at `level`.
pub fn show(environ_cb: retro_environment_t, message: &str, duration_ms: u32, level: Level) {
    log!(level, "{message}");

    let retro_level = match level {
        Level::Error => retro_log_level::RETRO_LOG_ERROR,
        Level::Warn => retro_log_level::RETRO_LOG_WARN,
        Level::Info => retro_log_level::RETRO_LOG_INFO,
        Level::Debug | Level::Trace => retro_log_level::RETRO_LOG_DEBUG,
    };

    let result = unsafe {
        environment::set_message_ext(
            environ_cb,
            message,
            duration_ms,
            0,
            retro_level,
            retro_message_target::RETRO_MESSAGE_TARGET_OSD,
            retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
            MessageProgress::Indeterminate,
        )
    };

    if let Err(e) = result {
        log::debug!("RETRO_ENVIRONMENT_SET_MESSAGE_EXT failed: {e}");
    }
}
