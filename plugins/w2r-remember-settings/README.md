# Remember Game Settings

Saves the options of the last game you created and puts them back the next time you open the
Create Multiplayer Game screen.

**Remembered:** the map (and the folder it is in), game speed, resources, units, tileset, starting location.

**Not remembered:** lobby name, password and the Teams/Slots locks. The locks are
already persisted by w2rpm itself. A map that was moved, renamed or deleted is skipped and the game keeps its own selection.

## How it works

- When you press **Create**, the plugin reads the current values and stores them in its own
  settings file (`config\w2r-remember-settings.json`).
- When the screen opens, after the game has filled in its defaults, the plugin writes the saved
  values over them. A saved value that is missing, out of range or not a plain short key is
  skipped and the game's default stays.

## Capabilities

- `game.read`: look up the symbols of the create screen's fields.
- `hooks.raw`: two detours, each of which always calls the original function:
  - `LobbyCreationInit` (the screen opens): after the original runs, write the saved values into
    the option globals (`LobbyGameSpeed`, `LobbyResourcesCurrent`, `LobbyUnitsCurrent`,
    `LobbyTilesetCurrent`, `LobbyStartLocCurrent`), using the game's own `StringAssign` for the
    strings. For the map it points the map list at the saved folder (`LobbyFolderRelPath`,
    `RefreshMapListFromFolder`, as the map browser does), finds the entry whose name and path
    match, and selects it with the game's `CopyMapListEntry` into `LobbyCreationSelected`, then
    records its name in `PendingGameSetup` if `ValidateMapFile` accepts the file. If the entry is
    gone, the folder the game had is put back.
  - `LobbyCreateButton` (Create is pressed): before the original runs, read the same globals plus
    the selected map's name and path.

It changes only the host's local options before the game exists, so `affects_simulation` is false.
