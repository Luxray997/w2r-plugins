//! Random map: a button on the Create Multiplayer Game screen that selects a random map,
//! optionally only among maps for a chosen number of players. `model` is pure and tested
//! everywhere; `plugin` is the thin layer over the host's create-screen functions.

pub mod model;
mod plugin;
