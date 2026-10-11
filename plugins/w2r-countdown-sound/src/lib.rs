//! Countdown sound: a beep per second of the lobby start countdown. `model` is pure and tested
//! everywhere; the game-facing layer is 32-bit Windows only, like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
