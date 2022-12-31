use std::cell::Cell;
use std::sync::{Arc, Mutex};

use ruffle_core::{Player, PlayerBuilder};
use rust_libretro_sys::*;

pub enum PlayerState {
    Uninitialized,
    Pending(Cell<PlayerBuilder>),
    Active(Arc<Mutex<Player>>),
}

pub enum RenderInterface {
    Default(retro_hw_render_interface),
    Vulkan(retro_hw_render_interface_vulkan),
}

pub enum RenderContextNegotiationInterface {
    Vulkan(retro_hw_render_context_negotiation_interface_vulkan)
}