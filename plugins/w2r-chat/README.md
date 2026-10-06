# Chat (`w2r-chat`)

In-match chat history with channels and player colours: a scrollable history that also works while
paused, fade and backlog settings. A port of the chat feature of wc2r-plus by Luxray997 (MIT).
Ported to Rust by Beau Hastings.

Capabilities: `game.read`, `hooks.raw` (it hooks the game's in-match message functions). It does not
change anything that must match between players (`affects_simulation: false`).
