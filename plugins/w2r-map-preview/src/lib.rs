//! In-game map preview: the lobby's picture of the map, drawn over a running match and toggled by
//! a hotkey. `model` is pure and tested everywhere; the game-facing layer is 32-bit Windows only,
//! like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
