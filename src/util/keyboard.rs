use std::collections::HashMap;

use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, NamedKey, PhysicalKey, TextControlCode};
use ruffle_core::PlayerEvent;
use rust_libretro::sys::{retro_key, retro_mod};

/// Turns libretro keyboard callbacks into Ruffle events.
#[derive(Default)]
pub struct KeyboardState {
    /// The logical key reported when each physical key went down. Frontends rarely
    /// send a character on release, so this keeps non-US layouts consistent.
    pressed: HashMap<PhysicalKey, LogicalKey>,
}

impl KeyboardState {
    /// `from_retropad` marks a key the frontend also reports as a RetroPad button,
    /// which presses its Flash key already; only its text input is sent here.
    pub fn handle(
        &mut self,
        down: bool,
        from_retropad: bool,
        keycode: retro_key,
        character: u32,
        modifiers: retro_mod,
        events: &mut impl Extend<PlayerEvent>,
    ) {
        let character = char::from_u32(character).filter(|c| *c != '\0');
        let shift = has(modifiers, retro_mod::RETROKMOD_SHIFT);
        let caps = has(modifiers, retro_mod::RETROKMOD_CAPSLOCK);
        let ctrl = has(modifiers, retro_mod::RETROKMOD_CTRL) || has(modifiers, retro_mod::RETROKMOD_META);

        if keycode != retro_key::RETROK_UNKNOWN {
            let (physical_key, key_location) = physical_key(keycode);
            let logical_key = if down {
                let logical = match character {
                    Some(c) if !c.is_control() => LogicalKey::Character(c),
                    _ => logical_key(keycode, shift, caps),
                };
                if physical_key != PhysicalKey::Unknown && !from_retropad {
                    self.pressed.insert(physical_key, logical);
                }
                logical
            } else {
                let released = self.pressed.remove(&physical_key);
                // Still release keys pressed before the RetroPad was plugged in.
                if from_retropad && released.is_none() {
                    return;
                }
                match released {
                    // Like Flash Player, a released letter takes its case from the modifiers held now.
                    // Only ASCII: both cases map to the same key code, unlike other scripts.
                    Some(LogicalKey::Character(c)) if c.is_ascii_alphabetic() => {
                        LogicalKey::Character(if shift != caps {
                            c.to_ascii_uppercase()
                        } else {
                            c.to_ascii_lowercase()
                        })
                    }
                    Some(logical) => logical,
                    None => logical_key(keycode, shift, caps),
                }
            };

            let key = KeyDescriptor {
                physical_key,
                logical_key,
                key_location,
            };
            if !down {
                events.extend([PlayerEvent::KeyUp { key }]);
            } else if !from_retropad {
                events.extend([PlayerEvent::KeyDown { key }]);
            }

            if down {
                if let Some(code) = text_control(keycode, shift, ctrl) {
                    events.extend([PlayerEvent::TextControl { code }]);
                    return;
                }
            }
        }

        // Characters can arrive with their keypress or in a separate event with no keycode.
        if down && !ctrl {
            if let Some(codepoint) = character.filter(|c| !c.is_control()) {
                events.extend([PlayerEvent::TextInput { codepoint }]);
            }
        }
    }
}

fn has(modifiers: retro_mod, flag: retro_mod) -> bool {
    (modifiers & flag) != retro_mod::RETROKMOD_NONE
}

fn text_control(keycode: retro_key, shift: bool, ctrl: bool) -> Option<TextControlCode> {
    use retro_key as k;
    use TextControlCode as T;

    Some(match keycode {
        k::RETROK_RETURN | k::RETROK_KP_ENTER => T::Enter,
        k::RETROK_a if ctrl => T::SelectAll,
        k::RETROK_c if ctrl => T::Copy,
        k::RETROK_v if ctrl => T::Paste,
        k::RETROK_x if ctrl => T::Cut,
        k::RETROK_BACKSPACE if ctrl => T::BackspaceWord,
        k::RETROK_BACKSPACE => T::Backspace,
        k::RETROK_DELETE if ctrl => T::DeleteWord,
        k::RETROK_DELETE => T::Delete,
        k::RETROK_LEFT => match (ctrl, shift) {
            (true, true) => T::SelectLeftWord,
            (true, false) => T::MoveLeftWord,
            (false, true) => T::SelectLeft,
            (false, false) => T::MoveLeft,
        },
        k::RETROK_RIGHT => match (ctrl, shift) {
            (true, true) => T::SelectRightWord,
            (true, false) => T::MoveRightWord,
            (false, true) => T::SelectRight,
            (false, false) => T::MoveRight,
        },
        k::RETROK_HOME => match (ctrl, shift) {
            (true, true) => T::SelectLeftDocument,
            (true, false) => T::MoveLeftDocument,
            (false, true) => T::SelectLeftLine,
            (false, false) => T::MoveLeftLine,
        },
        k::RETROK_END => match (ctrl, shift) {
            (true, true) => T::SelectRightDocument,
            (true, false) => T::MoveRightDocument,
            (false, true) => T::SelectRightLine,
            (false, false) => T::MoveRightLine,
        },
        _ => return None,
    })
}

/// The character a key produces on a US layout, used when the frontend doesn't supply one.
fn logical_key(keycode: retro_key, shift: bool, caps: bool) -> LogicalKey {
    use retro_key as k;
    use LogicalKey::{Character as C, Named as N};

    let code = keycode.0;
    if let Some(c) = char::from_u32(code).filter(char::is_ascii_lowercase) {
        return C(if shift != caps { c.to_ascii_uppercase() } else { c });
    }
    if let Some(c) = char::from_u32(code).filter(|c| c.is_ascii_graphic() || *c == ' ') {
        return C(if shift { us_shifted(c) } else { c });
    }
    if (k::RETROK_KP0.0..=k::RETROK_KP9.0).contains(&code) {
        return C(char::from(b'0' + (code - k::RETROK_KP0.0) as u8));
    }
    if (k::RETROK_F1.0..=k::RETROK_F15.0).contains(&code) {
        return N(function_key(code - k::RETROK_F1.0));
    }

    match keycode {
        k::RETROK_KP_PERIOD => C('.'),
        k::RETROK_KP_DIVIDE => C('/'),
        k::RETROK_KP_MULTIPLY => C('*'),
        k::RETROK_KP_MINUS => C('-'),
        k::RETROK_KP_PLUS => C('+'),
        k::RETROK_KP_EQUALS => C('='),
        k::RETROK_BACKSPACE => N(NamedKey::Backspace),
        k::RETROK_TAB => N(NamedKey::Tab),
        k::RETROK_CLEAR => N(NamedKey::Clear),
        k::RETROK_RETURN | k::RETROK_KP_ENTER => N(NamedKey::Enter),
        k::RETROK_PAUSE | k::RETROK_BREAK => N(NamedKey::Pause),
        k::RETROK_ESCAPE => N(NamedKey::Escape),
        k::RETROK_DELETE => N(NamedKey::Delete),
        k::RETROK_UP => N(NamedKey::ArrowUp),
        k::RETROK_DOWN => N(NamedKey::ArrowDown),
        k::RETROK_RIGHT => N(NamedKey::ArrowRight),
        k::RETROK_LEFT => N(NamedKey::ArrowLeft),
        k::RETROK_INSERT => N(NamedKey::Insert),
        k::RETROK_HOME => N(NamedKey::Home),
        k::RETROK_END => N(NamedKey::End),
        k::RETROK_PAGEUP => N(NamedKey::PageUp),
        k::RETROK_PAGEDOWN => N(NamedKey::PageDown),
        k::RETROK_NUMLOCK => N(NamedKey::NumLock),
        k::RETROK_CAPSLOCK => N(NamedKey::CapsLock),
        k::RETROK_SCROLLOCK => N(NamedKey::ScrollLock),
        k::RETROK_LSHIFT | k::RETROK_RSHIFT => N(NamedKey::Shift),
        k::RETROK_LCTRL | k::RETROK_RCTRL => N(NamedKey::Control),
        k::RETROK_LALT | k::RETROK_RALT => N(NamedKey::Alt),
        k::RETROK_LMETA | k::RETROK_RMETA | k::RETROK_LSUPER | k::RETROK_RSUPER => N(NamedKey::Super),
        k::RETROK_MODE => N(NamedKey::AltGraph),
        k::RETROK_PRINT | k::RETROK_SYSREQ => N(NamedKey::PrintScreen),
        k::RETROK_MENU => N(NamedKey::ContextMenu),
        k::RETROK_UNDO => N(NamedKey::Undo),
        _ => LogicalKey::Unknown,
    }
}

fn us_shifted(c: char) -> char {
    match c {
        '`' => '~',
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        other => other,
    }
}

fn function_key(index: u32) -> NamedKey {
    const KEYS: [NamedKey; 15] = [
        NamedKey::F1,
        NamedKey::F2,
        NamedKey::F3,
        NamedKey::F4,
        NamedKey::F5,
        NamedKey::F6,
        NamedKey::F7,
        NamedKey::F8,
        NamedKey::F9,
        NamedKey::F10,
        NamedKey::F11,
        NamedKey::F12,
        NamedKey::F13,
        NamedKey::F14,
        NamedKey::F15,
    ];
    KEYS[index as usize]
}

fn physical_key(keycode: retro_key) -> (PhysicalKey, KeyLocation) {
    use retro_key as k;
    use KeyLocation::{Left, Numpad, Right, Standard};
    use PhysicalKey as P;

    let key = match keycode {
        k::RETROK_BACKSPACE => P::Backspace,
        k::RETROK_TAB => P::Tab,
        k::RETROK_RETURN => P::Enter,
        k::RETROK_PAUSE | k::RETROK_BREAK => P::Pause,
        k::RETROK_ESCAPE => P::Escape,
        k::RETROK_SPACE => P::Space,
        k::RETROK_QUOTE | k::RETROK_QUOTEDBL => P::Quote,
        k::RETROK_COMMA | k::RETROK_LESS => P::Comma,
        k::RETROK_MINUS | k::RETROK_UNDERSCORE => P::Minus,
        k::RETROK_PERIOD | k::RETROK_GREATER => P::Period,
        k::RETROK_SLASH | k::RETROK_QUESTION => P::Slash,
        k::RETROK_0 | k::RETROK_RIGHTPAREN => P::Digit0,
        k::RETROK_1 | k::RETROK_EXCLAIM => P::Digit1,
        k::RETROK_2 | k::RETROK_AT => P::Digit2,
        k::RETROK_3 | k::RETROK_HASH => P::Digit3,
        k::RETROK_4 | k::RETROK_DOLLAR => P::Digit4,
        k::RETROK_5 => P::Digit5,
        k::RETROK_6 | k::RETROK_CARET => P::Digit6,
        k::RETROK_7 | k::RETROK_AMPERSAND => P::Digit7,
        k::RETROK_8 | k::RETROK_ASTERISK => P::Digit8,
        k::RETROK_9 | k::RETROK_LEFTPAREN => P::Digit9,
        k::RETROK_SEMICOLON | k::RETROK_COLON => P::Semicolon,
        k::RETROK_EQUALS | k::RETROK_PLUS => P::Equal,
        k::RETROK_LEFTBRACKET | k::RETROK_LEFTBRACE => P::BracketLeft,
        k::RETROK_BACKSLASH | k::RETROK_BAR => P::Backslash,
        k::RETROK_RIGHTBRACKET | k::RETROK_RIGHTBRACE => P::BracketRight,
        k::RETROK_BACKQUOTE | k::RETROK_TILDE => P::Backquote,
        k::RETROK_OEM_102 => P::IntlBackslash,
        k::RETROK_a => P::KeyA,
        k::RETROK_b => P::KeyB,
        k::RETROK_c => P::KeyC,
        k::RETROK_d => P::KeyD,
        k::RETROK_e => P::KeyE,
        k::RETROK_f => P::KeyF,
        k::RETROK_g => P::KeyG,
        k::RETROK_h => P::KeyH,
        k::RETROK_i => P::KeyI,
        k::RETROK_j => P::KeyJ,
        k::RETROK_k => P::KeyK,
        k::RETROK_l => P::KeyL,
        k::RETROK_m => P::KeyM,
        k::RETROK_n => P::KeyN,
        k::RETROK_o => P::KeyO,
        k::RETROK_p => P::KeyP,
        k::RETROK_q => P::KeyQ,
        k::RETROK_r => P::KeyR,
        k::RETROK_s => P::KeyS,
        k::RETROK_t => P::KeyT,
        k::RETROK_u => P::KeyU,
        k::RETROK_v => P::KeyV,
        k::RETROK_w => P::KeyW,
        k::RETROK_x => P::KeyX,
        k::RETROK_y => P::KeyY,
        k::RETROK_z => P::KeyZ,
        k::RETROK_DELETE => P::Delete,
        k::RETROK_KP0 => return (P::Numpad0, Numpad),
        k::RETROK_KP1 => return (P::Numpad1, Numpad),
        k::RETROK_KP2 => return (P::Numpad2, Numpad),
        k::RETROK_KP3 => return (P::Numpad3, Numpad),
        k::RETROK_KP4 => return (P::Numpad4, Numpad),
        k::RETROK_KP5 => return (P::Numpad5, Numpad),
        k::RETROK_KP6 => return (P::Numpad6, Numpad),
        k::RETROK_KP7 => return (P::Numpad7, Numpad),
        k::RETROK_KP8 => return (P::Numpad8, Numpad),
        k::RETROK_KP9 => return (P::Numpad9, Numpad),
        k::RETROK_KP_PERIOD => return (P::NumpadDecimal, Numpad),
        k::RETROK_KP_DIVIDE => return (P::NumpadDivide, Numpad),
        k::RETROK_KP_MULTIPLY => return (P::NumpadMultiply, Numpad),
        k::RETROK_KP_MINUS => return (P::NumpadSubtract, Numpad),
        k::RETROK_KP_PLUS => return (P::NumpadAdd, Numpad),
        k::RETROK_KP_ENTER => return (P::NumpadEnter, Numpad),
        k::RETROK_UP => P::ArrowUp,
        k::RETROK_DOWN => P::ArrowDown,
        k::RETROK_RIGHT => P::ArrowRight,
        k::RETROK_LEFT => P::ArrowLeft,
        k::RETROK_INSERT => P::Insert,
        k::RETROK_HOME => P::Home,
        k::RETROK_END => P::End,
        k::RETROK_PAGEUP => P::PageUp,
        k::RETROK_PAGEDOWN => P::PageDown,
        k::RETROK_F1 => P::F1,
        k::RETROK_F2 => P::F2,
        k::RETROK_F3 => P::F3,
        k::RETROK_F4 => P::F4,
        k::RETROK_F5 => P::F5,
        k::RETROK_F6 => P::F6,
        k::RETROK_F7 => P::F7,
        k::RETROK_F8 => P::F8,
        k::RETROK_F9 => P::F9,
        k::RETROK_F10 => P::F10,
        k::RETROK_F11 => P::F11,
        k::RETROK_F12 => P::F12,
        k::RETROK_F13 => P::F13,
        k::RETROK_F14 => P::F14,
        k::RETROK_F15 => P::F15,
        k::RETROK_NUMLOCK => P::NumLock,
        k::RETROK_CAPSLOCK => P::CapsLock,
        k::RETROK_SCROLLOCK => P::ScrollLock,
        k::RETROK_LSHIFT => return (P::ShiftLeft, Left),
        k::RETROK_RSHIFT => return (P::ShiftRight, Right),
        k::RETROK_LCTRL => return (P::ControlLeft, Left),
        k::RETROK_RCTRL => return (P::ControlRight, Right),
        k::RETROK_LALT => return (P::AltLeft, Left),
        k::RETROK_RALT => return (P::AltRight, Right),
        k::RETROK_LMETA | k::RETROK_LSUPER => return (P::SuperLeft, Left),
        k::RETROK_RMETA | k::RETROK_RSUPER => return (P::SuperRight, Right),
        k::RETROK_PRINT | k::RETROK_SYSREQ => P::PrintScreen,
        k::RETROK_MENU => P::ContextMenu,
        _ => P::Unknown,
    };

    (key, Standard)
}
