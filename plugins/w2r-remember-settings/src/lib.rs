//! Remember game settings: saves the create-game options when a game is created and puts them
//! back the next time the Create Multiplayer Game screen opens. `model` is pure and tested
//! everywhere; the game-facing layer is 32-bit Windows only, like the game.

pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;
