use std::cell::Cell;
use std::sync::{Arc, Mutex};

use ruffle_core::{Player, PlayerBuilder};

pub enum PlayerState {
    Uninitialized,
    Pending(Cell<PlayerBuilder>),
    Active(Arc<Mutex<Player>>),
    Exiting,
}