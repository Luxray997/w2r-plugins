# Countdown Sound

Plays a short beep for each second of the multiplayer lobby's start countdown (5, 4, 3, 2, 1) and a
longer, higher one when the countdown ends and the game starts. Local only; the simulation is
untouched.

- **Only in the background:** the game plays its own countdown sound while it has focus, so the
  beeps are skipped then and played only when another window is active, the game is minimised, or
  you are alt-tabbed. (The foreground window is compared with this process.)
- **The sound:** the first time a countdown runs, the plugin copies the game's own tick sound (sound 0) out of its DirectSound buffer and plays that copy with `PlaySoundW`, so it is heard in the background. If the copy fails it uses a generated beep (the plugin screen says which). The final "go" sound is always the generated higher beep.
- **Setting:** `Volume` (0 to 100, default 60). 0 is silent. Changes apply at once.
- **How it works:** the game keeps a "countdown running" flag and the seconds left in two globals
  (`MpLobbyCountdownActive` and the `u32` ten bytes after it). A helper thread reads them every
  20 ms and plays a tone through `PlaySoundW` when the seconds change. The beeps are generated in
  memory; no sound files are used.

## Capabilities

- `game.read`: symbol lookup for `MpLobbyCountdownActive` and `SoundTable`.
- `hooks.raw`: the runtime address of that symbol, read from the helper thread. Nothing is hooked
  and nothing is written to the game.
