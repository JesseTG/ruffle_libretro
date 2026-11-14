use std::collections::HashMap;
use std::ffi::CStr;
use std::ptr;

use ruffle_core::events::{GamepadButton, KeyCode};
use ruffle_core::PlayerEvent;
use rust_libretro::contexts::RunContext;
use rust_libretro_sys::*;

/// RetroPad buttons, the Flash key each one presses, and the label shown for it.
///
/// The keys match RetroArch's default keyboard binds for the same buttons,
/// so a keyboard press that RetroArch also reports as a RetroPad press
/// means the same key either way; see [`is_retropad_key`].
const RETROPAD_BINDINGS: [(u32, GamepadButton, KeyCode, &CStr); 14] = [
    (RETRO_DEVICE_ID_JOYPAD_UP, GamepadButton::DPadUp, KeyCode::UP, c"Up Arrow"),
    (RETRO_DEVICE_ID_JOYPAD_DOWN, GamepadButton::DPadDown, KeyCode::DOWN, c"Down Arrow"),
    (RETRO_DEVICE_ID_JOYPAD_LEFT, GamepadButton::DPadLeft, KeyCode::LEFT, c"Left Arrow"),
    (RETRO_DEVICE_ID_JOYPAD_RIGHT, GamepadButton::DPadRight, KeyCode::RIGHT, c"Right Arrow"),
    (RETRO_DEVICE_ID_JOYPAD_B, GamepadButton::South, KeyCode::Z, c"Z"),
    (RETRO_DEVICE_ID_JOYPAD_A, GamepadButton::East, KeyCode::X, c"X"),
    (RETRO_DEVICE_ID_JOYPAD_Y, GamepadButton::West, KeyCode::A, c"A"),
    (RETRO_DEVICE_ID_JOYPAD_X, GamepadButton::North, KeyCode::S, c"S"),
    (RETRO_DEVICE_ID_JOYPAD_L, GamepadButton::LeftTrigger, KeyCode::Q, c"Q"),
    (RETRO_DEVICE_ID_JOYPAD_R, GamepadButton::RightTrigger, KeyCode::W, c"W"),
    (RETRO_DEVICE_ID_JOYPAD_L2, GamepadButton::LeftTrigger2, KeyCode::E, c"E"),
    (RETRO_DEVICE_ID_JOYPAD_R2, GamepadButton::RightTrigger2, KeyCode::R, c"R"),
    (RETRO_DEVICE_ID_JOYPAD_SELECT, GamepadButton::Select, KeyCode::SHIFT, c"Shift"),
    (RETRO_DEVICE_ID_JOYPAD_START, GamepadButton::Start, KeyCode::ENTER, c"Enter"),
];

/// Describes [`RETROPAD_BINDINGS`] to the frontend. The list ends with a null description,
/// as libretro requires; the frontend keeps the label pointers, so they must be `'static`.
pub fn input_descriptors() -> Vec<retro_input_descriptor> {
    RETROPAD_BINDINGS
        .iter()
        .map(|&(id, _, _, label)| retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id,
            description: label.as_ptr(),
        })
        .chain([retro_input_descriptor {
            port: 0,
            device: 0,
            index: 0,
            id: 0,
            description: ptr::null(),
        }])
        .collect()
}

const CONTROLLER_DESCRIPTIONS: &[retro_controller_description] = &[
    retro_controller_description {
        desc: c"RetroPad".as_ptr(),
        id: RETRO_DEVICE_JOYPAD,
    },
    retro_controller_description {
        desc: c"Keyboard + Mouse".as_ptr(),
        id: RETRO_DEVICE_KEYBOARD,
    },
];

pub const CONTROLLER_INFO: &[retro_controller_info] = &[
    retro_controller_info {
        types: CONTROLLER_DESCRIPTIONS.as_ptr(),
        num_types: CONTROLLER_DESCRIPTIONS.len() as u32,
    },
    retro_controller_info {
        types: ptr::null(),
        num_types: 0,
    },
];

/// Whether RetroArch binds this key to a [`RETROPAD_BINDINGS`] button by default.
/// Such a press also arrives as that button, which already sends its Flash key.
pub fn is_retropad_key(keycode: retro_key) -> bool {
    use retro_key as k;
    matches!(
        keycode,
        k::RETROK_UP
            | k::RETROK_DOWN
            | k::RETROK_LEFT
            | k::RETROK_RIGHT
            | k::RETROK_z
            | k::RETROK_x
            | k::RETROK_a
            | k::RETROK_s
            | k::RETROK_q
            | k::RETROK_w
            | k::RETROK_e
            | k::RETROK_r
            | k::RETROK_RSHIFT
            | k::RETROK_RETURN
    )
}

pub fn gamepad_button_mapping() -> HashMap<GamepadButton, KeyCode> {
    RETROPAD_BINDINGS
        .iter()
        .map(|&(_, button, key, _)| (button, key))
        .collect()
}

/// Tracks RetroPad buttons on port 0 and reports presses and releases.
#[derive(Default)]
pub struct GamepadState {
    pressed: u16,
}

impl GamepadState {
    pub fn update(&mut self, ctx: &RunContext, events: &mut impl Extend<PlayerEvent>) {
        let mut pressed = 0u16;
        for (bit, &(id, button, _, _)) in RETROPAD_BINDINGS.iter().enumerate() {
            let mask = 1 << bit;
            let is_down = ctx.get_input_state(0, RETRO_DEVICE_JOYPAD, 0, id) != 0;
            if is_down {
                pressed |= mask;
            }

            match (self.pressed & mask != 0, is_down) {
                (false, true) => events.extend([PlayerEvent::GamepadButtonDown { button }]),
                (true, false) => events.extend([PlayerEvent::GamepadButtonUp { button }]),
                _ => {}
            }
        }
        self.pressed = pressed;
    }

    /// Releases every held button, e.g. when the RetroPad is unplugged.
    pub fn release_all(&mut self, events: &mut impl Extend<PlayerEvent>) {
        for (bit, &(_, button, _, _)) in RETROPAD_BINDINGS.iter().enumerate() {
            if self.pressed & (1 << bit) != 0 {
                events.extend([PlayerEvent::GamepadButtonUp { button }]);
            }
        }
        self.pressed = 0;
    }
}
