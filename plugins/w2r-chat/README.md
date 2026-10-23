# Chat (`w2r-chat`)

In-match chat history with channels and player colours: a scrollable history that also works while
paused, fade and backlog settings. A port of the chat feature of wc2r-plus by Luxray997 (MIT).
Ported to Rust by Beau Hastings.

Capabilities: `game.read`, `hooks.raw` (it hooks the game's in-match message functions). It does not
change anything that must match between players (`affects_simulation: false`).

## Settings

In the game's options screen, under **Chat** (W2R Mod Loader with the unified options screen; an
older one shows them on the plugin's card instead):

| Subcategory | Setting | Default |
|---|---|---|
| Behavior | History Height (Lines), 2 to 20 | 8 |
| Behavior | Fade Time, seconds, 0 = never | 7 |
| Behavior | Click Closes Chat | off |
| Behavior | Saved Message Limit, 25 to 500 | 100 |
| Behavior | Show Timestamps | off |
| Behavior | Log Chat to Disk | off |
| Style | Background Opacity, 0 to 255 | 100 |
| Style | Color Player Names | on |
| Style | Enhanced Chat Channels | on |
| Style | [Team] / [All] / [Allies] Color, a color picker (greyed while channels are off) | `25e65f` / `929292` / `69a7ff` |
