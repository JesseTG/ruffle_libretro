use std::fs::File;
use std::io;
use std::path::Path;

use futures::executor::LocalSpawner;
use futures::task::LocalSpawnExt;
use log::{error, info, Level};
use ruffle_core::backend::navigator::OwnedFuture;
use ruffle_core::loader::Error;
use ruffle_frontend_utils::backends::navigator::{FutureSpawner, NavigatorInterface};
use rust_libretro::sys::retro_environment_t;
use url::Url;

use crate::core::config::{SharedWebBrowserAccess, WebBrowserAccess};
use crate::util::message;

#[derive(Clone)]
pub struct RetroNavigatorInterface {
    pub environ_cb: retro_environment_t,
    pub web_browser_access: SharedWebBrowserAccess,
}

impl NavigatorInterface for RetroNavigatorInterface {
    fn navigate_to_website(&self, url: Url) {
        match self.web_browser_access.get() {
            WebBrowserAccess::Ignore => info!("Ignoring request to open {url}"),
            WebBrowserAccess::Notify => {
                message::show(self.environ_cb, &format!("The movie wants to open {url}"), 5000, Level::Info)
            }
            WebBrowserAccess::OpenInBrowser => {
                if let Err(e) = webbrowser::open(url.as_str()) {
                    error!("Could not open {url}: {e}");
                }
            }
        }
    }

    async fn open_file(&self, path: &Path) -> io::Result<File> {
        File::open(path)
    }

    async fn confirm_socket(&self, _host: &str, _port: u16) -> bool {
        false
    }
}

/// Runs the player's `!Send` futures on the main thread;
/// the core polls them from `retro_run`.
pub struct RetroFutureSpawner(pub LocalSpawner);

impl FutureSpawner<Error> for RetroFutureSpawner {
    fn spawn(&self, future: OwnedFuture<(), Error>) {
        let result = self.0.spawn_local(async move {
            if let Err(e) = future.await {
                error!("Asynchronous error occurred: {e}");
            }
        });

        if let Err(e) = result {
            error!("Failed to spawn future: {e}");
        }
    }
}
