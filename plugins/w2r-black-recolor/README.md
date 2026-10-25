# Black Recolor

Draws the black player in pink (default) or cyan on your screen. Purely cosmetic and local: nobody
else sees it and the simulation is untouched. Port of the "Black2Pink" option of
[wc2r-plus](https://github.com/Luxray997/wc2r-plus) by Luxray997 (MIT).

- **Setting:** **Black Team Color** in the game's options screen, under Accessibility / Team Colors:
  Default, Pink (`e87bdd`) or Cyan (`00c6e0`). Changes apply when the screen applies them. A W2R
  Mod Loader without the options screen shows it as a slider on the plugin's card instead.
- **What changes:** the game's HD color table entry for index 5 (units, color boxes, anything that
  reads the table) and the 4-entry black ramp in the render palette (classic-graphics sprites and the
  minimap), shaded by the stock ramp's brightness steps.
- **Restores:** choosing Default, or unloading the plugin, writes the stock values back.

## Capabilities

- `game.read`: symbol lookup for `TeamColourRgba`, `RenderPalette` and `HudMenuButtons`.
- `hooks.raw`: one detour on `HudMenuButtons` (drawn every frame in a match; original called first) as
  the per-frame tick, and direct writes to the two color tables. `affects_simulation` is false.

## Also follows the recolor

The player color bar and black names in `w2r-chat` read the game table, so they match.

## Not done yet


- Custom hex values (wc2r-plus allows them in its settings file only).
