use std::sync::{Arc, Mutex};
use ruffle_core::Player;
use rust_libretro::contexts::GenericContext;
use rust_libretro::sys::retro_system_av_info;

pub struct Ruffle<'a> {
    player: Option<Arc<Mutex<Player>>>,
    vfs_interface_version: Option<u32>,
    av_info: Option<retro_system_av_info>,
    context: Option<GenericContext<'a>>,
}

impl<'a> Ruffle<'a> {
    pub fn new() -> Ruffle<'a> {
        Ruffle {
            player: None,
            vfs_interface_version: None,
            av_info: None,
            context: None,
        }
    }
}

mod core;
mod options;