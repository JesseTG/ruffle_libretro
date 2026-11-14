use ruffle_core::events::{MouseButton, MouseWheelDelta};
use ruffle_core::PlayerEvent;
use rust_libretro::contexts::RunContext;
use rust_libretro::sys::*;

const BUTTONS: [MouseButton; 3] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

/// Tracks the cursor across frames and turns changes into Ruffle mouse events.
///
/// The position comes from the absolute pointer device, so the cursor follows the
/// host's mouse (or touch) without needing it grabbed. When the pointer doesn't move,
/// relative mouse motion is used instead, which covers a grabbed mouse.
pub struct MouseState {
    x: f64,
    y: f64,
    last_pointer: Option<(i16, i16)>,
    buttons: [bool; 3],
}

impl Default for MouseState {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            last_pointer: None,
            buttons: [false; 3],
        }
    }
}

impl MouseState {
    pub fn update(&mut self, ctx: &RunContext, width: u32, height: u32, events: &mut impl Extend<PlayerEvent>) {
        let (width, height) = (f64::from(width), f64::from(height));
        let mouse = |id| ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, id);
        let pointer = |id| ctx.get_input_state(0, RETRO_DEVICE_POINTER, 0, id);

        let pointer_pos = (pointer(RETRO_DEVICE_ID_POINTER_X), pointer(RETRO_DEVICE_ID_POINTER_Y));
        let (old_x, old_y) = (self.x, self.y);

        if self.last_pointer != Some(pointer_pos) {
            // Pointer coordinates span [-0x7fff, 0x7fff] across the content's viewport.
            // -0x8000 means the pointer is outside of it.
            if pointer_pos.0 != i16::MIN && pointer_pos.1 != i16::MIN {
                self.x = (f64::from(pointer_pos.0) + 32767.0) / 65534.0 * width;
                self.y = (f64::from(pointer_pos.1) + 32767.0) / 65534.0 * height;
            }
            self.last_pointer = Some(pointer_pos);
        } else {
            self.x += f64::from(mouse(RETRO_DEVICE_ID_MOUSE_X));
            self.y += f64::from(mouse(RETRO_DEVICE_ID_MOUSE_Y));
        }

        self.x = self.x.clamp(0.0, width);
        self.y = self.y.clamp(0.0, height);
        let (x, y) = (self.x, self.y);

        if x != old_x || y != old_y {
            events.extend([PlayerEvent::MouseMove { x, y }]);
        }

        let buttons = [
            mouse(RETRO_DEVICE_ID_MOUSE_LEFT) != 0 || pointer(RETRO_DEVICE_ID_POINTER_PRESSED) != 0,
            mouse(RETRO_DEVICE_ID_MOUSE_RIGHT) != 0,
            mouse(RETRO_DEVICE_ID_MOUSE_MIDDLE) != 0,
        ];

        for ((was_down, is_down), button) in self.buttons.iter().zip(buttons).zip(BUTTONS) {
            match (*was_down, is_down) {
                (false, true) => events.extend([PlayerEvent::MouseDown {
                    x,
                    y,
                    button,
                    index: None,
                }]),
                (true, false) => events.extend([PlayerEvent::MouseUp { x, y, button }]),
                _ => {}
            }
        }
        self.buttons = buttons;

        let wheel = match (
            mouse(RETRO_DEVICE_ID_MOUSE_WHEELUP) != 0,
            mouse(RETRO_DEVICE_ID_MOUSE_WHEELDOWN) != 0,
        ) {
            (true, false) => 1.0,
            (false, true) => -1.0,
            _ => 0.0,
        };
        if wheel != 0.0 {
            events.extend([PlayerEvent::MouseWheel {
                delta: MouseWheelDelta::Lines(wheel),
            }]);
        }
    }
}
