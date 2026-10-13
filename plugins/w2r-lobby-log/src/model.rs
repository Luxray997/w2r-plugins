//! Build 1.0.2.2818 addresses and the pure parts: which exe functions are logged, how a call is
//! written down. The numbers come from Ghidra (docs/battlenet-stats-research.md in the w2rpm
//! repository, section "How the Multiplayer lobby list is fed").
//!
//! Three exe vtables are patched. Their slots are exe functions, so the plugin only swaps a
//! pointer for a stub that logs and jumps to the original:
//! - `InternetGameMasterService` (the wrapper the exe calls to talk to the SDK): what the game
//!   *sends* (advertise a game, stop, find games, start, end, invites...);
//! - `ClientGameMaster` Internet events (what the service *pushes*: games found and removed,
//!   advertise results, invites);
//! - its LAN events.

pub const EXE_IMAGE_BASE: u32 = 0x0040_0000;
pub const SDK_IMAGE_BASE: u32 = 0x1000_0000;

/// The singleton of service wrappers; the Internet game master wrapper is `{vftable, sdk*}` inline at +0xc0.
pub const CONTAINER_VA: u32 = 0x0096_4d78;
pub const INTERNET_WRAPPER_OFFSET: u32 = 0xc0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// The game calls the service.
    Out,
    /// The service calls the game (Internet game master events).
    In,
    /// LAN game master events.
    Lan,
}

impl Dir {
    pub fn tag(self) -> &'static str {
        match self {
            Dir::Out => "OUT",
            Dir::In => "IN ",
            Dir::Lan => "LAN",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub dir: Dir,
    /// Virtual address of the vtable this slot lives in (exe).
    pub vtable_va: u32,
    pub slot: u32,
    /// The function the slot must hold before it is patched.
    pub fn_va: u32,
    pub name: &'static str,
    /// Stack arguments to record.
    pub nargs: u32,
}

pub const WRAPPER_VTABLE_VA: u32 = 0x0084_43e8;
pub const EVENTS_VTABLE_VA: u32 = 0x0086_60b4;
pub const LAN_EVENTS_VTABLE_VA: u32 = 0x0086_6098;

const fn e(dir: Dir, vt: u32, slot: u32, fn_va: u32, name: &'static str, nargs: u32) -> Entry {
    Entry {
        dir,
        vtable_va: vt,
        slot,
        fn_va,
        name,
        nargs,
    }
}

use Dir::{In, Lan, Out};

/// Every patched slot. The index is the stub id. Names ending in `?` are guesses from the argument
/// count and the protocol (`classic.protocol.v1.game_master`), to be confirmed from the log.
pub static ENTRIES: [Entry; 45] = [
    e(Out, WRAPPER_VTABLE_VA, 1, 0x0062_25d0, "out1(1)", 1),
    e(Out, WRAPPER_VTABLE_VA, 2, 0x0062_2600, "out2(1)", 1),
    e(Out, WRAPPER_VTABLE_VA, 3, 0x0062_2630, "out3(1)", 1),
    e(Out, WRAPPER_VTABLE_VA, 4, 0x0062_2660, "out4()->bool", 0),
    e(Out, WRAPPER_VTABLE_VA, 5, 0x0062_2b00, "out5()", 0),
    e(Out, WRAPPER_VTABLE_VA, 6, 0x0062_2680, "AdvertiseGame?(id64,info)", 3),
    e(Out, WRAPPER_VTABLE_VA, 7, 0x0062_26c0, "StopAdvertising?(id64)", 2),
    e(Out, WRAPPER_VTABLE_VA, 8, 0x0062_2700, "out8(3)", 3),
    e(Out, WRAPPER_VTABLE_VA, 9, 0x0062_2740, "out9(2)", 2),
    e(Out, WRAPPER_VTABLE_VA, 10, 0x0062_2780, "out10(3)", 3),
    e(Out, WRAPPER_VTABLE_VA, 11, 0x0062_2a70, "out11(5)", 5),
    e(Out, WRAPPER_VTABLE_VA, 12, 0x0062_2ab0, "out12(6)", 6),
    e(Out, WRAPPER_VTABLE_VA, 13, 0x0062_27c0, "out13(2)", 2),
    e(Out, WRAPPER_VTABLE_VA, 14, 0x0062_2800, "out14(2)", 2),
    e(Out, WRAPPER_VTABLE_VA, 15, 0x0062_2840, "StartGame?(id64,info)", 3),
    e(Out, WRAPPER_VTABLE_VA, 16, 0x0062_2880, "EndGame?(id64,info)", 3),
    e(Out, WRAPPER_VTABLE_VA, 17, 0x0062_28c0, "out17()", 0),
    e(Out, WRAPPER_VTABLE_VA, 18, 0x0062_28e0, "out18()", 0),
    e(Out, WRAPPER_VTABLE_VA, 19, 0x0062_2900, "out19(1)", 1),
    e(In, EVENTS_VTABLE_VA, 1, 0x0062_f650, "AdvertiseSuccess", 4),
    e(In, EVENTS_VTABLE_VA, 2, 0x0062_f6c0, "AdvertiseFailure?", 5),
    e(In, EVENTS_VTABLE_VA, 3, 0x0062_f730, "in3", 4),
    e(In, EVENTS_VTABLE_VA, 4, 0x0062_f760, "GameFound", 3),
    e(In, EVENTS_VTABLE_VA, 5, 0x0062_fa30, "GameRemoved?", 4),
    e(In, EVENTS_VTABLE_VA, 6, 0x0062_fab0, "in6", 4),
    e(In, EVENTS_VTABLE_VA, 7, 0x0062_fac0, "in7", 4),
    e(In, EVENTS_VTABLE_VA, 8, 0x0062_faf0, "in8", 4),
    e(In, EVENTS_VTABLE_VA, 9, 0x0062_fb00, "in9", 4),
    e(In, EVENTS_VTABLE_VA, 10, 0x0062_fb20, "in10(after start)", 4),
    e(In, EVENTS_VTABLE_VA, 11, 0x0062_fb30, "in11", 4),
    e(In, EVENTS_VTABLE_VA, 12, 0x0062_fb50, "in12(after end)", 4),
    e(In, EVENTS_VTABLE_VA, 13, 0x0062_fb60, "in13", 4),
    e(In, EVENTS_VTABLE_VA, 14, 0x0062_fb80, "InviteSent?", 4),
    e(In, EVENTS_VTABLE_VA, 15, 0x0062_fbc0, "InviteFailed?", 4),
    e(In, EVENTS_VTABLE_VA, 16, 0x0062_fd20, "InviteStatusSent?", 4),
    e(In, EVENTS_VTABLE_VA, 17, 0x0062_fd60, "InviteStatusFailed?", 4),
    e(In, EVENTS_VTABLE_VA, 18, 0x0062_fec0, "InviteReceived?", 4),
    e(In, EVENTS_VTABLE_VA, 19, 0x0062_ffe0, "InviteStatusReceived?", 4),
    e(In, EVENTS_VTABLE_VA, 20, 0x0063_00e0, "in20", 4),
    e(In, EVENTS_VTABLE_VA, 21, 0x0063_0100, "in21", 4),
    e(Lan, LAN_EVENTS_VTABLE_VA, 1, 0x0062_f5b0, "lan1", 2),
    e(Lan, LAN_EVENTS_VTABLE_VA, 2, 0x0062_f5d0, "lan2", 2),
    e(Lan, LAN_EVENTS_VTABLE_VA, 3, 0x0062_f5f0, "lan3", 2),
    e(Lan, LAN_EVENTS_VTABLE_VA, 4, 0x0062_f610, "lan4", 3),
    e(Lan, LAN_EVENTS_VTABLE_VA, 5, 0x0062_f630, "lan5", 2),
];

/// Number of slots patched from the static table above; ids from here on belong to slots found at run time.
pub const STATIC_COUNT: usize = 45;
/// How many run-time slots can be patched (the stub pool is `STATIC_COUNT + DYN_MAX`).
pub const DYN_MAX: usize = 205;
/// Slots per vtable that are looked at.
pub const MAX_SLOTS_PER_VTABLE: u32 = 40;
/// The exe's code range (virtual addresses): a vtable slot pointing outside it is the end of the vtable.
pub const TEXT_START_VA: u32 = 0x0040_1000;
pub const TEXT_END_VA: u32 = 0x0083_4000;
/// Read-only data (where vtables live): `.rdata` follows the code.
pub const RDATA_START_VA: u32 = 0x0083_4000;
pub const RDATA_END_VA: u32 = 0x008c_0000;
/// Log at most this many lines per patched slot; a counter line says how many were left out.
pub const LINES_PER_SLOT: u32 = 40;

/// The other service wrappers in the container (`{vftable, sdk*}` inline at these offsets) and what
/// the analysis calls them. The Internet game master (+0xc0) is in the static table.
pub const SERVICES: [(u32, &str); 11] = [
    (0x50, "svc50"),
    (0x70, "svc70"),
    (0x94, "svc94"),
    (0xa0, "svca0"),
    (0xb8, "Gateway"),
    (0xe0, "Lobby"),
    (0xf0, "LocalGameMaster"),
    (0x100, "Network"),
    (0x118, "svc118"),
    (0x11c, "svc11c"),
    (0x124, "svc124"),
];

/// True when `va` (a link-time virtual address) lies in the exe's code.
pub fn is_code(va: u32) -> bool {
    (TEXT_START_VA..TEXT_END_VA).contains(&va)
}

/// Walks a vtable from slot 1 while the slots point into code (`read` returns the pointer stored at
/// an address). Slot 0 (the destructor) is left alone. `to_va` converts a runtime pointer to a link-time one.
pub fn walk_vtable(
    vtable: u32,
    read: impl Fn(u32) -> Option<u32>,
    to_va: impl Fn(u32) -> u32,
) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for slot in 1..=MAX_SLOTS_PER_VTABLE {
        let Some(f) = read(vtable.wrapping_add(slot * 4)) else { break };
        if !is_code(to_va(f)) {
            break;
        }
        out.push((slot, f));
    }
    out
}

/// `InternetGameMasterV1Impl` inside ClientSdk.dll (link-time VA): the SDK object every exe-side call ends in.
/// Slots 1..=19 are listed (slot 0 is the destructor); the functions are control-flow flattened, so they are
/// only logged and forwarded, never read.
pub const SDK_INET_VTABLE_VA: u32 = 0x10ab_00fc;
pub const SDK_INET_SLOTS: [u32; 19] = [
    0x1023_f410, 0x1022_48e0, 0x1022_8570, 0x1022_fe80, 0x1022_6960, 0x1024_3260, 0x1024_7b90, 0x1024_0b30,
    0x1024_5dc0, 0x1022_9cb0, 0x1023_be40, 0x1023_d9c0, 0x1023_54e0, 0x1023_7450, 0x1023_90e0, 0x1023_3030,
    0x1023_1660, 0x1022_c1a0, 0x1022_dd00,
];

/// Index of `GameFound` in [`ENTRIES`]: its arguments carry an `IGameInfoV1*`.
pub const GAME_FOUND: usize = 22;

/// Index of `GameRemoved` in [`ENTRIES`]: arguments are the request token (u64) then the game id (u64).
pub const GAME_REMOVED: usize = 23;
/// `War2Multiplayer::GameSearchInfo::vftable` (exe): the object passed to the SDK's find-games call.
pub const SEARCH_INFO_VTABLE_VA: u32 = 0x0084_7268;
/// Bytes of that object that matter (vtable pointer, two empty `std::string`s and counters).
pub const SEARCH_INFO_SIZE: usize = 0x40;
/// Slot of the SDK game master object that starts a find-games request `(token u64, GameSearchInfo*)`,
/// and the one the game calls when it leaves the list.
pub const SDK_FIND_SLOT: usize = 8;
pub const SDK_STOP_FIND_SLOT: usize = 9;

/// A lobby as the service describes it. The password itself is never kept, only whether there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenGame {
    pub id: u64,
    pub name: String,
    pub creator: String,
    pub map: String,
    pub players: u32,
    pub max_players: u32,
    pub speed: u32,
    pub flags: u32,
    pub locked: bool,
    pub seen_ms: u64,
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> &'a str {
    attrs.iter().find(|(k, _)| k == key).map_or("", |(_, v)| v.as_str())
}

fn num(attrs: &[(String, String)], key: &str) -> u32 {
    attr(attrs, key).trim().parse().unwrap_or(0)
}

/// Builds the entry from a `GameFound`: the id, the name and the attribute list.
pub fn open_game_from(id: u64, name: &str, attrs: &[(String, String)], now_ms: u64) -> OpenGame {
    OpenGame {
        id,
        name: name.to_string(),
        creator: attr(attrs, "szCreatorName").to_string(),
        map: attr(attrs, "szMapTitle").trim_end_matches(".pud").to_string(),
        players: num(attrs, "nCurPlayers"),
        max_players: num(attrs, "nMaxPlayers"),
        speed: num(attrs, "bInitialGameSpeed"),
        flags: num(attrs, "lFlags"),
        locked: !attr(attrs, "password").is_empty(),
        seen_ms: now_ms,
    }
}

/// The lobbies the service has told us about, newest first.
#[derive(Default)]
pub struct GameTable {
    games: Vec<OpenGame>,
}

impl GameTable {
    pub const fn new() -> Self {
        GameTable { games: Vec::new() }
    }
    /// Adds or replaces the lobby with this id.
    pub fn upsert(&mut self, g: OpenGame) {
        match self.games.iter_mut().find(|x| x.id == g.id) {
            Some(slot) => *slot = g,
            None => self.games.push(g),
        }
        self.games.sort_by_key(|x| core::cmp::Reverse(x.id));
        self.games.truncate(200);
    }
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.games.len();
        self.games.retain(|x| x.id != id);
        self.games.len() != before
    }
    pub fn clear(&mut self) {
        self.games.clear();
    }
    pub fn games(&self) -> &[OpenGame] {
        &self.games
    }
    pub fn total_players(&self) -> u32 {
        self.games.iter().map(|g| g.players).sum()
    }
}

/// One line of the plugin list: "name by creator - map - 2/8 players, speed 7 (password)".
pub fn row_text(g: &OpenGame) -> String {
    let mut s = format!("{} by {} - {} - {}/{}", g.name, g.creator, g.map, g.players, g.max_players);
    if g.locked {
        s.push_str(" (password)");
    }
    clean(s.as_bytes()).chars().take(120).collect()
}

/// Runtime address of a static in a module that was loaded at `base`.
pub fn runtime(va: u32, image_base: u32, base: u32) -> u32 {
    base.wrapping_add(va.wrapping_sub(image_base))
}

/// Address of the slot itself (where the pointer is stored).
pub fn slot_address(e: &Entry, exe_base: u32) -> u32 {
    runtime(e.vtable_va, EXE_IMAGE_BASE, exe_base) + e.slot * 4
}

/// Printable ASCII only; anything else becomes `?`. Lobby names come from other players.
pub fn clean(text: &[u8]) -> String {
    text.iter()
        .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '?' })
        .collect()
}

/// One log line for a call: time in ms since logging started, direction, name, `this`, the
/// recorded stack arguments in hex, then any decoded notes.
pub fn format_call(ms: u64, dir: Dir, name: &str, this: u32, args: &[u32], notes: &[String]) -> String {
    let mut s = format!("{ms:>8} {} {name} this={this:#x}", dir.tag());
    if !args.is_empty() {
        s.push_str(" args=");
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{a:#x}"));
        }
    }
    for n in notes {
        s.push_str(" | ");
        s.push_str(n);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_in_order_and_inside_the_exe() {
        for e in &ENTRIES {
            assert!((0x0040_1000..0x0090_0000).contains(&e.fn_va), "{}", e.name);
            assert!(e.slot >= 1);
        }
        for pair in ENTRIES.windows(2) {
            if pair[0].vtable_va == pair[1].vtable_va {
                assert_eq!(pair[0].slot + 1, pair[1].slot, "{}", pair[1].name);
            }
        }
    }

    #[test]
    fn the_static_table_matches_its_count() {
        assert_eq!(ENTRIES.len(), STATIC_COUNT);
    }

    #[test]
    fn a_vtable_walk_stops_at_the_first_non_code_slot() {
        let mem = [0x0062_25d0u32, 0x0062_2600, 0x0086_60b4, 0x0062_2630];
        let read = |a: u32| mem.get(((a - 0x1000) / 4) as usize).copied();
        // vtable at 0x1000 - 4, so slot 1 is mem[0]
        let got = walk_vtable(0x1000 - 4, read, |x| x);
        assert_eq!(got, vec![(1, 0x0062_25d0), (2, 0x0062_2600)]);
    }

    fn attrs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(k, x)| (k.to_string(), x.to_string())).collect()
    }

    #[test]
    fn a_lobby_is_read_from_its_attributes() {
        let a = attrs(&[
            ("szCreatorName", "Beau"),
            ("szMapTitle", "Mud in Your Eye BNE.pud"),
            ("nCurPlayers", "2"),
            ("nMaxPlayers", "8"),
            ("bInitialGameSpeed", "7"),
            ("lFlags", "12800"),
            ("password", "testx"),
        ]);
        let g = open_game_from(0xa7bc4af, "TESTING", &a, 5);
        assert_eq!((g.players, g.max_players, g.speed, g.flags), (2, 8, 7, 12800));
        assert!(g.locked);
        assert_eq!(g.map, "Mud in Your Eye BNE");
        assert_eq!(row_text(&g), "TESTING by Beau - Mud in Your Eye BNE - 2/8 (password)");
    }

    #[test]
    fn the_table_updates_removes_and_orders() {
        let mut t = GameTable::new();
        let mk = |id, p| open_game_from(id, "g", &attrs(&[("nCurPlayers", p)]), 0);
        t.upsert(mk(5, "1"));
        t.upsert(mk(9, "2"));
        t.upsert(mk(5, "3"));
        assert_eq!(t.games().iter().map(|g| g.id).collect::<Vec<_>>(), vec![9, 5]);
        assert_eq!(t.total_players(), 5);
        assert!(t.remove(9));
        assert!(!t.remove(9));
        t.clear();
        assert!(t.games().is_empty());
    }

    #[test]
    fn game_removed_index_is_right() {
        assert_eq!(ENTRIES[GAME_REMOVED].fn_va, 0x0062_fa30);
        assert_eq!(ENTRIES[GAME_REMOVED].nargs, 4);
    }

    #[test]
    fn game_found_index_is_right() {
        assert_eq!(ENTRIES[GAME_FOUND].name, "GameFound");
        assert_eq!(ENTRIES[GAME_FOUND].dir, Dir::In);
        assert_eq!(ENTRIES[GAME_FOUND].fn_va, 0x0062_f760);
    }

    #[test]
    fn slot_addresses_follow_the_load_base() {
        let e = &ENTRIES[0];
        assert_eq!(slot_address(e, 0x0040_0000), WRAPPER_VTABLE_VA + 4);
        assert_eq!(
            slot_address(e, 0x006d_0000),
            0x006d_0000 + (WRAPPER_VTABLE_VA - EXE_IMAGE_BASE) + 4
        );
    }

    #[test]
    fn text_from_the_network_is_cleaned() {
        assert_eq!(clean(b"Hi\x00\xffthere\n"), "Hi??there?");
    }

    #[test]
    fn a_call_line_has_everything() {
        let e = &ENTRIES[5];
        let line = format_call(1234, e.dir, e.name, 0x1000, &[1, 0x20, 0], &["info vt=0x86".into()]);
        assert_eq!(
            line,
            "    1234 OUT AdvertiseGame?(id64,info) this=0x1000 args=0x1,0x20,0x0 | info vt=0x86"
        );
        let bare = format_call(0, Dir::In, "x", 4, &[], &[]);
        assert!(bare.ends_with("this=0x4"));
    }
}
