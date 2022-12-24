use ruffle_core::backend::navigator;
use ruffle_core::backend::navigator::{NavigationMethod, NavigatorBackend, OwnedFuture, Request, Response};
use ruffle_core::indexmap::IndexMap;
use ruffle_core::loader::Error;


pub struct RetroNavigatorBackend {}

impl RetroNavigatorBackend {
    pub fn new() -> Self {
        Self {}
    }
}

impl NavigatorBackend for RetroNavigatorBackend {
    fn navigate_to_url(&self, url: String, target: String, vars_method: Option<(NavigationMethod, IndexMap<String, String>)>) {
        todo!()
    }

    fn fetch(&self, request: Request) -> OwnedFuture<Response, Error> {
        todo!()
    }

    fn spawn_future(&mut self, future: OwnedFuture<(), Error>) {
        todo!()
    }

    fn pre_process_url(&self, url: url::Url) -> url::Url {
        todo!()
    }
}