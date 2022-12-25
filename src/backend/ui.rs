use ruffle_core::backend::ui::{FullscreenError, MouseCursor, UiBackend};
use arboard::Clipboard;
use log::error;
use rust_libretro::contexts::GenericContext;
use rust_libretro::sys::{retro_log_level, retro_message_target, retro_message_type};
use rust_libretro::types::MessageProgress;

const UNSUPPORTED_CONTENT_MESSAGE: &str = "\
Ruffle does not yet support ActionScript 3, required by this content.
Interactivity will be missing or limited.";

const DOWNLOAD_FAILED_MESSAGE: &str = "Ruffle failed to open or download this file.";

pub struct RetroUiBackend<'a> {
    clipboard: Clipboard,
    cursor_visible: bool,
    cursor: MouseCursor,
    cursor_position: (i32, i32),
    context: GenericContext<'a>,
}

impl<'a> RetroUiBackend<'a> {
    pub fn new(context: GenericContext) -> Self {
        Self {
            clipboard: Clipboard::new().unwrap(),
            cursor_visible: true,
            cursor_position: (0, 0),
            cursor: MouseCursor::Arrow,
            context,
        }
    }
}

impl<'a> UiBackend for RetroUiBackend<'a> {
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
        if let Err(error) = self.clipboard.set_text(content)
        {
            error!("[ruffle] Failed to set clipboard content: {error}");
        }
    }

    fn set_fullscreen(&mut self, is_full: bool) -> Result<(), FullscreenError> {
        todo!()
    }

    fn display_unsupported_message(&self) {
        self.context.set_message_ext(
            DOWNLOAD_FAILED_MESSAGE,
            3000,
            0,
            retro_log_level::RETRO_LOG_WARN,
            retro_message_target::RETRO_MESSAGE_TARGET_ALL,
            retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
            MessageProgress::Indeterminate
        );
    }

    fn display_root_movie_download_failed_message(&self) {
        self.context.set_message_ext(
            "Ruffle failed to open or download this file.",
            3000,
            0,
            retro_log_level::RETRO_LOG_WARN,
            retro_message_target::RETRO_MESSAGE_TARGET_ALL,
            retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
            MessageProgress::Indeterminate
        );
    }

    fn message(&self, message: &str) {
        self.context.set_message_ext(
            message,
            1000,
            0,
            retro_log_level::RETRO_LOG_INFO,
            retro_message_target::RETRO_MESSAGE_TARGET_ALL,
            retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
            MessageProgress::Indeterminate
        );
    }
}