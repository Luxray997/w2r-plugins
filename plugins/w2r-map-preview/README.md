# In-game Map Preview

Shows the lobby's map preview (terrain, gold mines and start locations) over a running match.
Press the **backslash key (\)** to open and close it; it does not pause the game and the hotkey only works while
the game window has the focus and the chat box is closed (while you type, Tab is the chat
plugin's channel switch). The game's own Tab action (minimap terrain) still happens too.

- **Placement:** just under the top bar, at the left edge.
- **Settings** (in the in-game plugin list): the picture size.
- **What it shows:** exactly what the lobby preview shows, drawn by the same code
  (`w2r-preview` in the w2rpm repository). It is the whole map as a file, not what you have
  explored, so it reveals nothing the lobby did not.

## How it works

When a match starts the plugin reads the game's selected map path, and the first time you open the
overlay it reads the map and the era's tileset art, renders the picture and uploads it into one
game texture. A map that cannot be read as a file (a campaign map inside an archive, for example)
shows nothing, and the reason is in the log.

## Capabilities

- `game.read`: look up the symbols for the selected map, the texture and drawing functions.
- `hooks.raw`: one detour, on `HudResourceCell` (the top bar's gold, lumber and oil cells; the
  player color bar hooks `HudFoodCell`, and two plugins cannot share a function). It always calls
  the original first, reads the hotkey once per frame, and when open draws the picture with the
  game's own `NkDrawImage` onto the same canvas. It changes no game state, so
  `affects_simulation` is false.

## Not done yet

- A live view: your units and structures on the picture, and explored versus unexplored.
- A rebindable hotkey.
