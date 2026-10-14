# Plugin ideas

A scratchpad for plugins worth building. Nothing here is committed to; capabilities are a first
guess at the least the plugin would need (see [CONTRIBUTING.md](CONTRIBUTING.md)).

Capabilities: `game.read`, `chat.read`, `chat.write`, `net.send`, `hooks.raw`.

---

## Ladder matchmaking

A ranked ladder: players queue, get paired with similarly rated opponents, and ratings update when
the game ends.

- **Queue:** join/leave via chat commands (`!queue 1v1`, `!queue 2v2`, `!leave`), with per-mode
  queues.
- **Rating:** Elo or Glicko-2 per mode, with a provisional period for new players so they converge
  quickly. Rating deviation widens the acceptable match window the longer someone waits.
- **Pairing:** nearest-rating match within a window that grows over wait time; avoid immediate
  rematches.
- **Result reporting:** read the end-of-game state from the game itself instead of trusting players
  to self-report. Both clients must agree, otherwise the game is flagged and not rated.
- **Leaderboard and history:** `!rank`, `!top 10`, recent games.
- **Open questions:**
  - Where does the rating state live? Needs a central service, or a signed result that a
    server-side component accepts. A purely peer-to-peer ladder is easy to cheat.
  - Handling disconnects and drops (count as a loss? only if before a time threshold?).
  - Season resets and decay for inactive players.
- **Capabilities:** `game.read`, `chat.read`, `chat.write`, `net.send` (to reach the ladder
  service).

## Observer system (WIP)

An observer mode in the style of war2combat's, for spectating and casting games.

- **Free camera and player switching:** jump between players' viewpoints or follow a unit or
  group.
- **Full-information view:** see all players' resources, supply, upgrades, and production
  without fog of war restrictions.
- **Stats overlay:** army size, income and spend rate, APM, unit counts, and tech progress for each
  player.
- **Observer chat:** a separate channel so spectators can't leak information to players.
- **Casting aids:** production and upgrade timelines, a game clock, and replay-style highlights of
  big fights or expansions.
- **Open questions:**
  - How war2combat implements observer slots and whether the same approach carries over.
  - Making sure observers can never influence the simulation or be used to ghost.
- **Capabilities:** `game.read`, `hooks.raw` (most likely). Needs the two-approval review, with
  every hook documented.

## Resource sharing

Lets teammates in a team game send resources to each other.

- **Commands:** `!send <player> <amount> <resource>` (for example `!send Bob 500 gold`), plus
  optional hotkeys or a small panel for quick fixed amounts.
- **Team only:** allow transfers only between allied players, never to opponents or observers.
- **Limits:** validate that the sender has enough, and optionally cap per-transfer amounts or add
  a cooldown to prevent spam.
- **Feedback:** confirm to both players in chat (`Alice sent 500 gold to Bob`) so it is never
  silent.
- **Host options:** let the lobby enable or disable the feature, or charge a small transfer fee,
  so it can be turned off for competitive or ladder games.
- **Open questions:**
  - Whether resources can be modified at runtime, and how to keep the change identical on every
    client.
  - Preventing abuse such as feeding a teammate in a 2v2 ladder game, and whether it should be
    disabled for rated games.
- **Capabilities:** `game.read`, `chat.read`, `chat.write`, `net.send` to sync transfers, and
  `hooks.raw` to change resource values. Set `affects_simulation: true`, since all players must
  agree on the result.

---

## More ideas

Add new ideas below as they come up. A line or two is enough; flesh out when one is picked up.

- ability for the host to ban a player
- in-game timer
- Daily or weekly challenge. One seeded random map with a fixed modifier, such as “no towers” or “double gold”. The stats service ranks completion time, and everyone plays the same thing.
- A system to report players for hacking. It should be part of the replay system as the replay itself is the proof. It should upload the proof somewhere for the community to validate. Once confirmed, the system records their battle.net ID into the database. We could use this data to ban, restrict, or show a notice in the game lobby when that player is in the lobby
