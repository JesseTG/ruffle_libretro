use std::cell::Cell;
use std::sync::Arc;
use arboard::Clipboard;
use log::error;
use ruffle_core::backend::ui::{FullscreenError, MouseCursor, UiBackend};
use rust_libretro::environment;
use rust_libretro::sys::{retro_log_level, retro_message_target, retro_message_type};
use rust_libretro::types::MessageProgress;
use rust_libretro_sys::retro_environment_t;

const UNSUPPORTED_CONTENT_MESSAGE: &str = "\
Ruffle doesn't yet support ActionScript 3, which this content requires.
Interactivity will be missing or limited.";

const DOWNLOAD_FAILED_MESSAGE: &str = "Ruffle failed to open or download this file.";

pub struct RetroUiBackend {
    clipboard: Clipboard,
    cursor_visible: bool,
    cursor: MouseCursor,
    cursor_position: (i32, i32),
    environment: Arc<Cell<retro_environment_t>>,
}

impl RetroUiBackend {
    pub fn new(environment: Arc<Cell<retro_environment_t>>) -> Self {
        Self {
            clipboard: Clipboard::new().unwrap(),
            cursor_visible: true,
            cursor_position: (0, 0),
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

    fn set_fullscreen(&mut self, is_full: bool) -> Result<(), FullscreenError> {
        todo!()
    }

    fn display_unsupported_message(&self) {
        unsafe {
            environment::set_message_ext(
                self.environment.get(),
                DOWNLOAD_FAILED_MESSAGE,
                3000,
                0,
                retro_log_level::RETRO_LOG_WARN,
                retro_message_target::RETRO_MESSAGE_TARGET_ALL,
                retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
                MessageProgress::Indeterminate,
            );
        }
    }

    fn display_root_movie_download_failed_message(&self) {
        unsafe {
            environment::set_message_ext(
                self.environment.get(),
                "Ruffle failed to open or download this file.",
                3000,
                0,
                retro_log_level::RETRO_LOG_WARN,
                retro_message_target::RETRO_MESSAGE_TARGET_ALL,
                retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
                MessageProgress::Indeterminate,
            );
        }
    }

    fn message(&self, message: &str) {
        unsafe {
            environment::set_message_ext(
                self.environment.get(),
                message,
                1000,
                0,
                retro_log_level::RETRO_LOG_INFO,
                retro_message_target::RETRO_MESSAGE_TARGET_ALL,
                retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
                MessageProgress::Indeterminate,
            );
        }
    }
}
