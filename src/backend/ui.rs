use arboard::Clipboard;
use log::{warn, Level};
use ruffle_core::backend::ui::{
    DialogResultFuture, FileDialogResult, FileFilter, FontDefinition, FullscreenError, LanguageIdentifier,
    MouseCursor, MultiDialogResultFuture, MultiFileDialogResult, UiBackend, US_ENGLISH,
};
use ruffle_core::font::FontQuery;
use rust_libretro::sys::retro_environment_t;
use url::Url;

use crate::util::message;

pub struct RetroUiBackend {
    environ_cb: retro_environment_t,
    /// Created on first use, since not every host has a clipboard
    /// (e.g. a console, or a Linux session without X11 or Wayland).
    clipboard: Option<Clipboard>,
    clipboard_failed: bool,
    cursor_visible: bool,
}

impl RetroUiBackend {
    pub fn new(environ_cb: retro_environment_t) -> Self {
        Self {
            environ_cb,
            clipboard: None,
            clipboard_failed: false,
            cursor_visible: true,
        }
    }

    fn clipboard(&mut self) -> Option<&mut Clipboard> {
        if self.clipboard.is_none() && !self.clipboard_failed {
            match Clipboard::new() {
                Ok(clipboard) => self.clipboard = Some(clipboard),
                Err(e) => {
                    warn!("Clipboard is unavailable: {e}");
                    self.clipboard_failed = true;
                }
            }
        }

        self.clipboard.as_mut()
    }
}

impl UiBackend for RetroUiBackend {
    fn mouse_visible(&self) -> bool {
        self.cursor_visible
    }

    fn set_mouse_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    fn set_mouse_cursor(&mut self, _cursor: MouseCursor) {
        // libretro has no way to change the frontend's cursor.
    }

    fn clipboard_content(&mut self) -> String {
        self.clipboard()
            .and_then(|clipboard| clipboard.get_text().ok())
            .unwrap_or_default()
    }

    fn set_clipboard_content(&mut self, content: String) {
        if let Some(clipboard) = self.clipboard() {
            if let Err(e) = clipboard.set_text(content) {
                warn!("Failed to set clipboard content: {e}");
            }
        }
    }

    fn set_fullscreen(&mut self, _is_full: bool) -> Result<(), FullscreenError> {
        // The frontend owns the window, so there's nothing to change.
        Ok(())
    }

    fn display_root_movie_download_failed_message(&self, _invalid_swf: bool, fetch_error: String) {
        message::show(
            self.environ_cb,
            &format!("Ruffle failed to open this file: {fetch_error}"),
            5000,
            Level::Error,
        );
    }

    fn message(&self, message: &str) {
        message::show(self.environ_cb, message, 3000, Level::Info);
    }

    fn open_virtual_keyboard(&self) {}

    fn close_virtual_keyboard(&self) {}

    fn language(&self) -> LanguageIdentifier {
        US_ENGLISH.clone()
    }

    fn display_unsupported_video(&self, url: Url) {
        message::show(
            self.environ_cb,
            &format!("This movie contains video that Ruffle can't play: {url}"),
            5000,
            Level::Warn,
        );
    }

    fn load_device_font(&self, _query: &FontQuery, _register: &mut dyn FnMut(FontDefinition)) {
        // No system font lookup; Ruffle falls back to its bundled Noto Sans.
    }

    fn sort_device_fonts(&self, _query: &FontQuery, _register: &mut dyn FnMut(FontDefinition)) -> Vec<FontQuery> {
        Vec::new()
    }

    fn display_file_open_dialog(&mut self, _filters: Vec<FileFilter>) -> Option<DialogResultFuture> {
        Some(Box::pin(async move { Ok(FileDialogResult::Canceled) }))
    }

    fn display_file_open_dialog_multiple(&mut self, _filters: Vec<FileFilter>) -> Option<MultiDialogResultFuture> {
        Some(Box::pin(async move { Ok(MultiFileDialogResult::Canceled) }))
    }

    fn display_file_save_dialog(&mut self, _file_name: String, _title: String) -> Option<DialogResultFuture> {
        None
    }

    fn close_file_dialog(&mut self) {}
}
