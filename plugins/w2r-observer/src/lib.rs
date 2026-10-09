//! Observer: a player on lobby team 9 ("OBS") watches instead of playing. `model` is pure and
//! tested everywhere; the game-facing layer is 32-bit Windows only, like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
