//! Black recolor: pink or cyan for the black player, local only. `model` is pure and tested
//! everywhere; the game-facing layer is 32-bit Windows only, like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
