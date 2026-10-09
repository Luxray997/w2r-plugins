//! The game-facing half (32-bit only). One detour on `HudFoodCell`, the top bar's food count:
//! after the original has drawn it, we draw the player boxes with the game's own `NkFillRect`
//! on the same canvas, just past the cell. The detour always calls the original first, and a panic
//! is caught before it can unwind into the game (detours are outside the host's crash guard,
//! docs/abi.md).
//!
//! The roster (who is playing, on which team) is read once when the match starts; who is still in
//! is read every frame, the way the game's own alliances screen decides it.

use core::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use w2r_abi::*;

use crate::model::{self, Item, MAX_PLAYERS, Player};

/// The food count is the fourth of the four resource cells, each a quarter of the 300-unit strip.
const FOOD_SLOT: i32 = 3;
const STRIP_CELLS: i32 = 4;
const STRIP_WIDTH: f32 = 300.0;

/// Lobby slot (wc2r-plus `struct_offsets.h`): stride, and where the team byte sits.
const LOBBY_SLOT_STRIDE: usize = 0x26;
const LOBBY_SLOT_TEAM: usize = 0x0a;
/// Peer table entry stride; state byte 2 means the peer has departed.
const PEER_STRIDE: usize = 0x22;
const PEER_DEPARTED: u8 = 2;
/// `PlayerOwner` values of a player in the match: a human or a computer.
const OWNER_HUMAN: u8 = 0;
const OWNER_COMPUTER: u8 = 1;
/// `PlayerMatchResult` values meaning the player left or was defeated.
const RESULT_LEFT: u8 = 1;
const RESULT_DEFEATED: u8 = 2;

/// The UI context, window and panel layouts (wc2r-plus `struct_offsets.h`).
const CTX_CURRENT_WINDOW: usize = 0x3d28;
const WINDOW_BUFFER: usize = 0x64;
const WINDOW_PANEL: usize = 0x8c;
const PANEL_BOUNDS: usize = 0x08;

const BORDER: u32 = 0xc800_0000;
const BORDER_SELF: u32 = 0xffff_ffff;
const DIVIDER_COLOUR: u32 = 0xc8b4_b4b4;

type FoodCellFn = unsafe extern "C" fn(f32, f32, f32, f32, i32, i32, i32, i32, i32);
type FillRectFn = unsafe extern "C" fn(*mut c_void, f32, f32, f32, f32, f32, u32);

struct Game {
    hud_ctx: *const *mut c_void,
    fill_rect: FillRectFn,
    colour_rgba: *const f32,
    colour_reorder: *const u8,
    local_player: *const u8,
    owner: *const u8,
    lobby_slots: *const u8,
    result: *const u8,
    peers: *const u8,
    slot_by_net: *const u32,
}

// SAFETY: addresses of the game's globals and a function, fixed for the session; used from the
// game's main thread.
unsafe impl Send for Game {}
unsafe impl Sync for Game {}

static GAME: OnceLock<Game> = OnceLock::new();
static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<W2rHandle>> = Mutex::new(None);
static ROSTER: Mutex<Vec<Player>> = Mutex::new(Vec::new());
static LOGGED_PLACEMENT: AtomicBool = AtomicBool::new(false);

fn host() -> Option<&'static W2rHost> {
    unsafe { HOST.load(Ordering::Acquire).as_ref() }
}

fn s(text: &str) -> W2rStr {
    W2rStr {
        ptr: text.as_ptr().cast(),
        len: text.len() as u32,
    }
}

fn log(level: u32, msg: &str) {
    if let Some(h) = host() {
        unsafe { (h.log)(h.token, level, s(msg)) };
    }
}

fn handle_of(name: &str) -> Option<W2rHandle> {
    let h = host()?;
    let mut handle = 0;
    (unsafe { (h.symbol_lookup)(h.token, s(name), &mut handle) } == W2R_OK).then_some(handle)
}

fn lookup(name: &str) -> Option<usize> {
    let h = host()?;
    let handle = handle_of(name)?;
    let mut addr = 0usize;
    (unsafe { (h.symbol_address)(h.token, handle, &mut addr) } == W2R_OK).then_some(addr)
}

fn resolve() -> Result<Game, &'static str> {
    let at = |name: &'static str| lookup(name).ok_or(name);
    Ok(Game {
        hud_ctx: at("HudCtx")? as *const *mut c_void,
        fill_rect: unsafe { core::mem::transmute::<usize, FillRectFn>(at("NkFillRect")?) },
        colour_rgba: at("TeamColourRgba")? as *const f32,
        colour_reorder: at("TeamColourReorder")? as *const u8,
        local_player: at("LocalPlayer")? as *const u8,
        owner: at("PlayerOwner")? as *const u8,
        lobby_slots: at("MpLobbySlots")? as *const u8,
        result: at("PlayerMatchResult")? as *const u8,
        peers: at("PeerTable")? as *const u8,
        slot_by_net: at("LobbySlotByNetSlot")? as *const u32,
    })
}

/// Who is playing and on which team, read as the match starts (the lobby still holds it then).
fn read_roster(game: &Game) -> Vec<Player> {
    (0..MAX_PLAYERS as u8)
        .filter(|&i| {
            let owner = unsafe { *game.owner.add(i as usize) };
            owner == OWNER_HUMAN || owner == OWNER_COMPUTER
        })
        .map(|i| Player {
            index: i,
            team: unsafe {
                *game
                    .lobby_slots
                    .add(i as usize * LOBBY_SLOT_STRIDE + LOBBY_SLOT_TEAM)
            },
        })
        .collect()
}

/// A human whose connection is gone (the game's alliances screen counts these as out too).
fn has_departed(game: &Game, player: u8) -> bool {
    (0..MAX_PLAYERS).any(|net| unsafe {
        let lobby = *game.slot_by_net.add(net) & 0xff;
        lobby == player as u32 && *game.peers.add(net * PEER_STRIDE) == PEER_DEPARTED
    })
}

fn still_in(game: &Game, player: u8) -> bool {
    unsafe {
        let result = *game.result.add(player as usize);
        if result == RESULT_LEFT || result == RESULT_DEFEATED {
            return false;
        }
        !(*game.owner.add(player as usize) == OWNER_HUMAN && has_departed(game, player))
    }
}

/// The player's color as the game would draw it, including any recoloring another mod applied.
fn colour_of(game: &Game, player: u8) -> u32 {
    unsafe {
        let index = (*game.colour_reorder.add(player as usize) as usize).min(MAX_PLAYERS - 1);
        let c = game.colour_rgba.add(index * 4);
        model::pack([*c, *c.add(1), *c.add(2)])
    }
}

/// Draws the bar for a food cell drawn at group-relative `x`, in the resource strip's group,
/// which is the current window of the HUD context.
unsafe fn draw(game: &Game, x: f32) {
    unsafe {
        let roster = ROSTER.lock().unwrap_or_else(|e| e.into_inner());
        if roster.len() < 2 {
            return;
        }
        let items = model::layout(&roster, |p| still_in(game, p));
        if items.is_empty() {
            return;
        }
        let ctx = *game.hud_ctx;
        if ctx.is_null() {
            return;
        }
        let window = *ctx.cast::<u8>().add(CTX_CURRENT_WINDOW).cast::<*mut u8>();
        if window.is_null() {
            return;
        }
        let panel = *window.add(WINDOW_PANEL).cast::<*const u8>();
        if panel.is_null() {
            return;
        }
        let canvas = window.add(WINDOW_BUFFER).cast::<c_void>();
        let bounds = panel.add(PANEL_BOUNDS).cast::<f32>();
        let (panel_x, panel_y, panel_w, panel_h) =
            (*bounds, *bounds.add(1), *bounds.add(2), *bounds.add(3));
        if !(panel_w > 0.0 && panel_h > 0.0) {
            return;
        }

        let strip_left = panel_x + x;
        let strip_right = strip_left + STRIP_WIDTH;
        let (offsets, width) = model::offsets(&items);
        if !LOGGED_PLACEMENT.swap(true, Ordering::Relaxed) {
            log(
                W2R_LOG_INFO,
                &format!(
                    "top bar panel ({panel_x}, {panel_y}, {panel_w} x {panel_h}), food cell x {x}, bar {width} wide"
                ),
            );
        }
        let Some(left) = model::start_x(strip_left, strip_right, panel_x, panel_x + panel_w, width)
        else {
            return;
        };
        let top = panel_y + ((panel_h - model::BOX) / 2.0).floor();
        let me = *game.local_player;
        let fill = game.fill_rect;
        for (item, at) in items.iter().zip(offsets) {
            let bx = left + at;
            match *item {
                Item::Box(player) => {
                    let border = if player == me { BORDER_SELF } else { BORDER };
                    let outer = model::BOX + 2.0;
                    fill(canvas, bx - 1.0, top - 1.0, outer, outer, 0.0, border);
                    fill(canvas, bx, top, model::BOX, model::BOX, 0.0, colour_of(game, player));
                }
                Item::Divider => {
                    let dx = bx + model::TEAM_GAP;
                    fill(canvas, dx, top, model::DIVIDER, model::BOX, 0.0, DIVIDER_COLOUR);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn food_cell_detour(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    slot: i32,
    slots: i32,
    used: i32,
    cap: i32,
    view: i32,
) {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original != 0 {
        let original: FoodCellFn = unsafe { core::mem::transmute(original) };
        unsafe { original(x, y, w, h, slot, slots, used, cap, view) };
    }
    if slot == FOOD_SLOT && slots == STRIP_CELLS {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if let Some(game) = GAME.get() {
                unsafe { draw(game, x) };
            }
        }));
    }
}

unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    if event.id == W2R_EVT_MATCH_START {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if let Some(game) = GAME.get() {
                *ROSTER.lock().unwrap_or_else(|e| e.into_inner()) = read_roster(game);
            }
        }));
    }
}

fn remove_hook() {
    let hook = HOOK.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let (Some(h), Some(hook)) = (host(), hook) {
        unsafe { (h.hook_remove)(h.token, hook) };
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    let game = match resolve() {
        Ok(g) => g,
        Err(name) => {
            log(
                W2R_LOG_ERROR,
                &format!("symbol {name} not available; no player color bar"),
            );
            return W2R_E_STATE;
        }
    };
    let _ = GAME.set(game);
    let Some(h) = self::host() else {
        return W2R_E_STATE;
    };
    if unsafe { (h.subscribe)(h.token, W2R_EVT_MATCH_START) } != W2R_OK {
        log(W2R_LOG_ERROR, "cannot subscribe to match start; no player color bar");
        return W2R_E_STATE;
    }
    let Some(target) = handle_of("HudFoodCell") else {
        log(W2R_LOG_ERROR, "symbol HudFoodCell not available; no player color bar");
        return W2R_E_STATE;
    };
    let mut trampoline: *mut c_void = core::ptr::null_mut();
    let mut hook = 0;
    let status = unsafe {
        (h.hook_install)(
            h.token,
            target,
            food_cell_detour as *mut c_void,
            &mut trampoline,
            &mut hook,
        )
    };
    if status != W2R_OK {
        log(
            W2R_LOG_ERROR,
            &format!("HudFoodCell: hook_install returned {status}; no player color bar"),
        );
        return W2R_E_STATE;
    }
    ORIGINAL.store(trampoline as usize, Ordering::Release);
    *HOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    log(W2R_LOG_INFO, "drawing the player color bar");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    remove_hook();
}

static TABLE: W2rPlugin = W2rPlugin {
    struct_size: W2R_PLUGIN_V1_SIZE,
    on_load: Some(on_load),
    on_event: Some(on_event),
    on_ui: None,
    on_unload: Some(on_unload),
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &TABLE
}
