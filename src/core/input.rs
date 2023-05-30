use std::ptr;

use rust_libretro::{input_descriptors, c_char_ptr};
use rust_libretro_sys::retro_input_descriptor;
use rust_libretro_sys::*;

pub const INPUT_DESCRIPTORS: &[retro_input_descriptor] = &input_descriptors!(
    { 0, RETRO_DEVICE_KEYBOARD, 0, RETRO_DEVICE_ID_JOYPAD_UP, "Up Key" },
    { 0, RETRO_DEVICE_KEYBOARD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, "Down Key" },
    { 0, RETRO_DEVICE_KEYBOARD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, "Left Key" },
    { 0, RETRO_DEVICE_KEYBOARD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, "Right Key" },
    { 0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_LEFT, "Left Mouse Button" },
    { 0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_RIGHT, "Right Mouse Button" },
);

// TODO: Add a Keyboard subclass with just the supported keys
pub const CONTROLLER_DESCRIPTIONS: &[retro_controller_description] = &[
    retro_controller_description {
        desc: c_char_ptr!("Keyboard"),
        id: RETRO_DEVICE_KEYBOARD,
    },
    retro_controller_description {
        desc: c_char_ptr!("Mouse"),
        id: RETRO_DEVICE_MOUSE,
    },
    retro_controller_description {
        desc: c_char_ptr!("Pointer"),
        id: RETRO_DEVICE_POINTER,
    },
];
pub const CONTROLLER_INFO: &[retro_controller_info] = &[
    retro_controller_info {
        types: CONTROLLER_DESCRIPTIONS.as_ptr(),
        num_types: CONTROLLER_DESCRIPTIONS.len() as u32,
    },
    retro_controller_info { types: ptr::null(), num_types: 0 },
];