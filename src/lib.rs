use rust_libretro::retro_core;

use crate::core::Ruffle;

mod backend;
mod util;
mod core;
mod options;

pub mod built_info {
    // The file has been placed there by the build script.
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

retro_core!(Ruffle::new());

