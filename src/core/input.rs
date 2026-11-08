use std::collections::HashMap;
use std::ptr;

use ruffle_core::events::{GamepadButton, KeyCode};
use ruffle_core::PlayerEvent;
use rust_libretro::contexts::RunContext;
use rust_libretro::{c_char_ptr, input_descriptors};
use rust_libretro_sys::*;

/// RetroPad buttons, the Flash key each one presses, and its description.
///
/// The keys match RetroArch's default keyboard binds for the same buttons,
/// so a keyboard press that RetroArch also reports as a RetroPad press
/// sends the same key twice instead of two different keys.
const RETROPAD_BINDINGS: [(u32, GamepadButton, KeyCode); 14] = [
    (RETRO_DEVICE_ID_JOYPAD_UP, GamepadButton::DPadUp, KeyCode::UP),
    (RETRO_DEVICE_ID_JOYPAD_DOWN, GamepadButton::DPadDown, KeyCode::DOWN),
    (RETRO_DEVICE_ID_JOYPAD_LEFT, GamepadButton::DPadLeft, KeyCode::LEFT),
    (RETRO_DEVICE_ID_JOYPAD_RIGHT, GamepadButton::DPadRight, KeyCode::RIGHT),
    (RETRO_DEVICE_ID_JOYPAD_B, GamepadButton::South, KeyCode::Z),
    (RETRO_DEVICE_ID_JOYPAD_A, GamepadButton::East, KeyCode::X),
    (RETRO_DEVICE_ID_JOYPAD_Y, GamepadButton::West, KeyCode::A),
    (RETRO_DEVICE_ID_JOYPAD_X, GamepadButton::North, KeyCode::S),
    (RETRO_DEVICE_ID_JOYPAD_L, GamepadButton::LeftTrigger, KeyCode::Q),
    (RETRO_DEVICE_ID_JOYPAD_R, GamepadButton::RightTrigger, KeyCode::W),
    (RETRO_DEVICE_ID_JOYPAD_L2, GamepadButton::LeftTrigger2, KeyCode::E),
    (RETRO_DEVICE_ID_JOYPAD_R2, GamepadButton::RightTrigger2, KeyCode::R),
    (RETRO_DEVICE_ID_JOYPAD_SELECT, GamepadButton::Select, KeyCode::SHIFT),
    (RETRO_DEVICE_ID_JOYPAD_START, GamepadButton::Start, KeyCode::ENTER),
];

pub const INPUT_DESCRIPTORS: &[retro_input_descriptor] = &input_descriptors!(
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP, "Up Arrow" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, "Down Arrow" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, "Left Arrow" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, "Right Arrow" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B, "Z" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_A, "X" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_Y, "A" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_X, "S" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_L, "Q" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_R, "W" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_L2, "E" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_R2, "R" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_SELECT, "Shift" },
    { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_START, "Enter" },
);

const CONTROLLER_DESCRIPTIONS: &[retro_controller_description] = &[
    retro_controller_description {
        desc: c_char_ptr!("RetroPad"),
        id: RETRO_DEVICE_JOYPAD,
    },
    retro_controller_description {
        desc: c_char_ptr!("Keyboard + Mouse"),
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

pub fn gamepad_button_mapping() -> HashMap<GamepadButton, KeyCode> {
    RETROPAD_BINDINGS
        .iter()
        .map(|&(_, button, key)| (button, key))
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
        for (bit, &(id, button, _)) in RETROPAD_BINDINGS.iter().enumerate() {
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
        for (bit, &(_, button, _)) in RETROPAD_BINDINGS.iter().enumerate() {
            if self.pressed & (1 << bit) != 0 {
                events.extend([PlayerEvent::GamepadButtonUp { button }]);
            }
        }
        self.pressed = 0;
    }
}
