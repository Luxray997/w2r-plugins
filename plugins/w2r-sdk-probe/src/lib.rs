//! SDK probe: a research tool for the game's ClientSdk local web server. `model` holds the build
//! 2818 addresses and the pure checks (tested everywhere); the game-facing layer is 32-bit Windows
//! only, like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
