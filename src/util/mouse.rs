use euclid::{vec2, Point2D, Vector2D};
use ruffle_core::events::{MouseButton, MouseWheelDelta};
use rust_libretro::contexts::RunContext;
use rust_libretro_sys::*;

use super::math::Pixels;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct MouseState {
    pub position: Point2D<i16, Pixels>,
    pub delta: Vector2D<i16, Pixels>,
    pub button: Option<MouseButton>,
    pub wheel: Option<MouseWheelDelta>,
}

impl MouseState {
    pub fn from_context(&self, geometry: &retro_game_geometry, ctx: &RunContext) -> Self {
        let mouse_dx = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_X);
        let mouse_dy = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_Y);
        let mouse_left_button = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_LEFT) != 0;
        let mouse_right_button = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_RIGHT) != 0;
        let mouse_middle_button = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_MIDDLE) != 0;
        let mouse_wheel_down = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_WHEELDOWN) != 0;
        let mouse_wheel_up = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_WHEELUP) != 0;

        let screen_size = Point2D::<i16, Pixels>::new(geometry.base_width as i16, geometry.base_height as i16);
        let delta = vec2(mouse_dx, mouse_dy);
        let new_position = (self.position + delta).clamp(Point2D::zero(), screen_size);

        Self {
            delta,
            position: new_position,
            button: if mouse_left_button {
                Some(MouseButton::Left)
            } else if mouse_right_button {
                Some(MouseButton::Right)
            } else if mouse_middle_button {
                Some(MouseButton::Middle)
            } else {
                None
            },
            wheel: if mouse_wheel_up {
                Some(MouseWheelDelta::Lines(1.0))
            } else if mouse_wheel_down {
                Some(MouseWheelDelta::Lines(-1.0))
            } else {
                None
            },
        }
    }
}
