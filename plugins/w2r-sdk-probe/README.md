# SDK Probe (research)

A research tool, not a feature. It looks at the `LocalWebsocketServerV1` object inside the game's
`ClientSdk.dll` and can call its start method (vtable slot 17, no arguments) once, to find out
whether Warcraft II's SDK can serve the same local `/web-api/` routes that StarCraft: Remastered
uses for its stats screens. See `docs/battlenet-stats-research.md` in the w2rpm repository.

- **Check (read only):** reads the game's service wrapper, the SDK object, its vtable and slot 17,
  compares them with the addresses found in Ghidra for build 1.0.2.2818, and lists this process's
  listening TCP ports. The result is in the host log and a status line.
- **Call buttons (experimental):** slots 4, 5 (port 0), 6 and 17 of the SDK server object, each once per session after the same check. Slot 17 registers the `web-api` routes; the others are candidates for what opens the listening socket. A mismatch refuses to call. If a call itself faults, the host disables the plugin; the game could still crash.
- Nothing is written to the game, hooked, or sent anywhere. Build 2818 only.

## Capabilities

- `game.read`, `hooks.raw`: the plugin reads game memory and calls a function in the SDK. It uses
  no host symbol; the addresses are in `src/model.rs`.
