//! Player color bar: one box per player in the top bar, right of the food count. `model` is pure
//! and tested everywhere; the game-facing layer is 32-bit Windows only, like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
