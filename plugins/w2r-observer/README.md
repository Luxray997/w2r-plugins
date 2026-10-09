# Observer

Watch a game instead of playing it. In the lobby, set your team to **OBS** (the ninth entry of the
team dropdown). In the match you see the whole map, and the game does not count you as defeated.

**Every player in the game needs this plugin** (it changes the simulation, so the lobby checks that
you all have it). Only humans can be observers.

## Status

Prototype, not yet tried in a real game. Known gaps: no free camera or player switching, no stats
overlay, no observer-only chat, and the match-end screen shows a defeat for the observer.

## How it works

- The host adds "OBS" (team 9) to the lobby team list when this plugin is loaded.
- Each frame the plugin gives every observer an ally row and every player's vision, in the game's
  own alliance and vision tables.
- `CheckMatchEnd`, `PlayerHasAssets` and `EliminatePlayer` are hooked so an observer with no units
  is not thrown out as defeated; see `src/plugin.rs`.

## Capabilities

- `game.read`: the symbols above.
- `hooks.raw`: four detours (`HudFoodCell`, `CheckMatchEnd`, `PlayerHasAssets`, `EliminatePlayer`).
  Each calls the original except `EliminatePlayer`, which is skipped for an observer only.
- `affects_simulation: true`.
