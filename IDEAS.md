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

## Auto-balance teams

Rebalances teams automatically based on the lobby format (2v2, 3v3, 4v4, and so on).

- **Format detection:** infer the format from the player count and slots, and re-evaluate when
  players join, leave, or switch slots.
- **Balancing:** split players into teams that minimize the rating difference (ratings from the
  ladder plugin, or a simple skill estimate if none exists). With small lobbies, brute-force all
  splits; it is cheap.
- **Constraints:** keep pre-made parties together, respect slot or color preferences where
  possible, and handle odd counts or uneven formats gracefully.
- **Host control:** announce the proposed teams in chat and let the host accept or veto, with a
  `!balance` command to re-run on demand and `!lock` to stop further adjustment.
- **Open questions:**
  - Whether the plugin can move players between slots itself, or only suggest. This decides
    whether it needs `hooks.raw`.
  - A rating source when the ladder plugin isn't installed.
- **Capabilities:** `game.read`, `chat.write`; moving slots may need `hooks.raw`. If it changes
  anything that must match between players, set `affects_simulation: true`.

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
- in-game apm display
- Check which battle.net features Stacraft Remastered has that we could potentially port into Warcraft 2 Remastered
- A system to report players for hacking. It should be part of the replay system as the replay itself is the proof. It should upload the proof somewhere for the community to validate. Once confirmed, the system records their battle.net ID into the database. We could use this data to ban, restrict, or show a notice in the game lobby when that player is in the lobby
