//! Lobby Log: a research tool that records the game's traffic with Battle.net's game master
//! service. `model` holds the build 2818 addresses and the pure formatting (tested everywhere);
//! the game-facing layer is 32-bit Windows only, like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
#[cfg(target_arch = "x86")]
mod stubs;
