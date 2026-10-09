# Random Map

Adds a **Random map** button to the Create Multiplayer Game screen, with a slider to only pick
maps for a chosen number of players.

- **Slider:** *any number of players*, or 1 to 8. A map counts by its maximum players, the number
  shown in the map details ("Max players"). The choice is remembered between launches.
- **Button:** selects a random map from every map under your maps folder (all sub-folders, the
  same list the map browser's search uses) that fits the slider. It never picks the same map twice
  in a row while more than one fits. The button shows how many maps fit; with none it says so
  instead.
- The first time the screen opens, the map index is read from the cache or built a few folders per
  frame; until it is ready the button area says "Indexing maps...".

It changes only which map you are about to host, exactly as picking one in the map browser does,
so `affects_simulation` is false.

## Requirements

Needs the **enhanced Create screen** (on by default; `enhanced_create_screen` in `host.json`) and a
w2rpm host with the create-screen functions (`W2R_HOST_CREATE_UI_SIZE`, see `docs/abi.md`, "Create
screen"). On an older host the plugin refuses to load and says so in the log. With the stock create
screen nothing is shown.

## Capabilities

None. It uses only the implicit host functions: `create_ui_register`, `create_map_count` and
`create_map_select`, plus its own settings file (`config\w2r-random-map.json`) for the slider.
