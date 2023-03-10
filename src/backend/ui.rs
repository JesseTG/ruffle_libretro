use arboard::Clipboard;
use log::{error, warn, info};
use ruffle_core::backend::ui::{FullscreenError, MouseCursor, UiBackend};
use rust_libretro::environment;
use rust_libretro::sys::{retro_environment_t, retro_log_level, retro_message_target, retro_message_type};
use rust_libretro::types::MessageProgress;
use std::cell::Cell;
use std::sync::Arc;

const UNSUPPORTED_CONTENT_MESSAGE: &str = "\
This content requires ActionScript 3, which Ruffle doesn't support yet.
Interactivity will be missing or limited.";

const DOWNLOAD_FAILED_MESSAGE: &str = "Ruffle failed to open or download this file.";

pub struct RetroUiBackend {
    clipboard: Clipboard,
    cursor_visible: bool,
    cursor: MouseCursor,
    environment: Arc<Cell<retro_environment_t>>,
}

impl RetroUiBackend {
    pub fn new(environment: Arc<Cell<retro_environment_t>>) -> Self {
        Self {
            clipboard: Clipboard::new().unwrap(),
            cursor_visible: true,
            cursor: MouseCursor::Arrow,
            environment,
        }
    }
}

impl UiBackend for RetroUiBackend {
    fn mouse_visible(&self) -> bool {
        self.cursor_visible
    }

    fn set_mouse_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    fn set_mouse_cursor(&mut self, cursor: MouseCursor) {
        self.cursor = cursor;
    }

    fn set_clipboard_content(&mut self, content: String) {
        if let Err(error) = self.clipboard.set_text(content) {
            error!("[ruffle] Failed to set clipboard content: {error}");
        }
    }

    fn set_fullscreen(&mut self, _is_full: bool) -> Result<(), FullscreenError> {
        todo!()
    }

    fn display_unsupported_message(&self) {
        let result = unsafe {
            environment::set_message_ext(
                self.environment.get(),
                UNSUPPORTED_CONTENT_MESSAGE,
                3000,
                0,
                retro_log_level::RETRO_LOG_WARN,
                retro_message_target::RETRO_MESSAGE_TARGET_ALL,
                retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
                MessageProgress::Indeterminate,
            )
        };

        if let Err(e) = result {
            warn!("{}", UNSUPPORTED_CONTENT_MESSAGE);
            warn!("RETRO_ENVIRONMENT_SET_MESSAGE_EXT failed: {e}");
        }
    }

    fn display_root_movie_download_failed_message(&self) {
        let result = unsafe {
            environment::set_message_ext(
                self.environment.get(),
                DOWNLOAD_FAILED_MESSAGE,
                3000,
                0,
                retro_log_level::RETRO_LOG_WARN,
                retro_message_target::RETRO_MESSAGE_TARGET_ALL,
                retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
                MessageProgress::Indeterminate,
            )
        };

        if let Err(e) = result {
            warn!("{}", DOWNLOAD_FAILED_MESSAGE);
            warn!("RETRO_ENVIRONMENT_SET_MESSAGE_EXT failed: {e}");
        }
    }

    fn message(&self, message: &str) {
        let result = unsafe {
            environment::set_message_ext(
                self.environment.get(),
                message,
                1000,
                0,
                retro_log_level::RETRO_LOG_INFO,
                retro_message_target::RETRO_MESSAGE_TARGET_ALL,
                retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
                MessageProgress::Indeterminate,
            )
        };

        if let Err(e) = result {
            info!("{}", message);
            warn!("RETRO_ENVIRONMENT_SET_MESSAGE_EXT failed: {e}");
        }
    }

    fn open_virtual_keyboard(&self) {
        todo!("Open RetroArch's virtual keyboard");
    }
}
