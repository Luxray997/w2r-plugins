# Player Color Bar

Shows one small box per player, filled with their color, in the top bar of a match to the right
of your food count.

- **Leavers:** a player's box disappears when they leave, drop or are defeated, so the bar shows
  who is still in the game.
- **Teams:** in a team game the boxes are grouped by team with a divider between groups
  (`■ ■ | ■ ■` in a 2v2). A free-for-all is a plain row.
- **You:** your own box has a white outline.
- **Colors:** read from the game's own color table, so a recolor from another mod shows here too.

- **Names (also noted on the plugin's settings screen):** hold the **tilde key** (the one left of 1) to show every player's name next to their color, with
  players who have left or been defeated shown dimmed. It works only while the game window has the
  focus and the chat box is closed, and not together with Ctrl, Shift or Alt. Another key can be
  set with the plugin setting `names_key` (a Windows virtual-key code, 192 by default). If the
  names do not fit beside the resource strip they are drawn over it, on a dark backing.

If the resource bar has been moved flush right and there is no room beyond the food count, the
boxes go to the left of the resource strip instead.

## How it works

The roster (who is playing, and each player's lobby team) is read once when the match starts. Each
frame, who is still in is read the way the game's alliances screen decides it: the match result
for the player, or, for a human, a peer that has departed.

## Capabilities

- `game.read`: look up the symbols for the player, team, color and result tables.
- `hooks.raw`: one detour, on `HudFoodCell` (the top bar's food count). It always calls the
  original first, then draws the boxes with the game's own `NkFillRect` onto the same canvas. It
  changes no game state, so `affects_simulation` is false.

## Not done yet

- A key setting screen (today only the `names_key` setting value).
- Dim a defeated player's box on the normal bar, for observers.
