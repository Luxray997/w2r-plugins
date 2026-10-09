//! The game-facing half (32-bit only). Detours are called by the game outside the host's crash
//! guard (docs/abi.md), so each one catches panics and always reaches the original unless it is
//! deliberately skipping it.
//!
//! - `HudFoodCell`: after the original, writes the observers' alliance row and vision bits (the
//!   game resets them at match start and when it eliminates a player, so it is done every frame).
//! - `CheckMatchEnd` and `PlayerHasAssets`: an observer owns nothing, so the game would end the
//!   match for them at once. The first assets check inside a match-end check, for the local
//!   observer, is answered "yes". Later checks in the same pass are not, so the observer is never
//!   counted as a survivor and the match still ends for the players.
//! - `EliminatePlayer`: skipped for an observer, which would otherwise lose its vision again.

use core::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use w2r_abi::*;

use crate::model::{self, MAX_PLAYERS};

/// Lobby slot (wc2r-plus `struct_offsets.h`): stride, and where the team byte sits.
const LOBBY_SLOT_STRIDE: usize = 0x26;
const LOBBY_SLOT_TEAM: usize = 0x0a;
/// `PlayerOwner` value of a human in the match.
const OWNER_HUMAN: u8 = 0;

type FoodCellFn = unsafe extern "C" fn(f32, f32, f32, f32, i32, i32, i32, i32, i32);
type CheckEndFn = unsafe extern "C" fn();
type AssetsFn = unsafe extern "C" fn(i32) -> u8;
type EliminateFn = unsafe extern "C" fn(i32);

struct Game {
    local_player: *const u8,
    owner: *const u8,
    lobby_slots: *const u8,
    stance: *mut u8,
    masks: *mut u8,
}

// SAFETY: addresses of the game's globals, fixed for the session; used from the main thread.
unsafe impl Send for Game {}
unsafe impl Sync for Game {}

static GAME: OnceLock<Game> = OnceLock::new();
static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static HOOKS: Mutex<Vec<W2rHandle>> = Mutex::new(Vec::new());
static ORIG_FOOD: AtomicUsize = AtomicUsize::new(0);
static ORIG_CHECK: AtomicUsize = AtomicUsize::new(0);
static ORIG_ASSETS: AtomicUsize = AtomicUsize::new(0);
static ORIG_ELIMINATE: AtomicUsize = AtomicUsize::new(0);
/// Bit mask of the players who observe this match, read when it starts.
static OBSERVERS: AtomicU8 = AtomicU8::new(0);
/// Set around the original match-end check while the local player observes.
static TOP_CHECK: AtomicBool = AtomicBool::new(false);

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
        local_player: at("LocalPlayer")? as *const u8,
        owner: at("PlayerOwner")? as *const u8,
        lobby_slots: at("MpLobbySlots")? as *const u8,
        stance: at("DiplomacyStance")? as *mut u8,
        masks: at("VisionMasks")? as *mut u8,
    })
}

fn is_observer(player: usize) -> bool {
    player < MAX_PLAYERS && OBSERVERS.load(Ordering::Relaxed) >> player & 1 == 1
}

fn local_observes(game: &Game) -> bool {
    is_observer(unsafe { *game.local_player } as usize)
}

/// Reads who observes while the lobby still holds the teams (the event fires before the game's
/// own match-start reset).
fn read_observers(game: &Game) -> u8 {
    let mut teams = [0u8; MAX_PLAYERS];
    for (i, team) in teams.iter_mut().enumerate() {
        *team = unsafe { *game.lobby_slots.add(i * LOBBY_SLOT_STRIDE + LOBBY_SLOT_TEAM) };
    }
    model::observers(&teams, |i| unsafe { *game.owner.add(i) } == OWNER_HUMAN)
}

fn give_vision(game: &Game) {
    let observers = OBSERVERS.load(Ordering::Relaxed);
    if observers == 0 {
        return;
    }
    unsafe {
        let stance = &mut *game.stance.cast::<[u8; MAX_PLAYERS * model::STANCE_ROW]>();
        let masks = &mut *game.masks.cast::<[u8; MAX_PLAYERS]>();
        model::apply(stance, masks, observers);
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
    let original = ORIG_FOOD.load(Ordering::Acquire);
    if original != 0 {
        let original: FoodCellFn = unsafe { core::mem::transmute(original) };
        unsafe { original(x, y, w, h, slot, slots, used, cap, view) };
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            give_vision(game);
        }
    }));
}

unsafe extern "C" fn check_end_detour() {
    let local = catch_unwind(AssertUnwindSafe(|| GAME.get().is_some_and(local_observes)))
        .unwrap_or(false);
    TOP_CHECK.store(local, Ordering::Relaxed);
    let original = ORIG_CHECK.load(Ordering::Acquire);
    if original != 0 {
        let original: CheckEndFn = unsafe { core::mem::transmute(original) };
        unsafe { original() };
    }
    TOP_CHECK.store(false, Ordering::Relaxed);
}

unsafe extern "C" fn assets_detour(player: i32) -> u8 {
    // The first question of a check pass is about the local player; only that one is answered.
    if TOP_CHECK.swap(false, Ordering::Relaxed) {
        let local = catch_unwind(AssertUnwindSafe(|| {
            GAME.get().map(|g| unsafe { *g.local_player } as i32)
        }))
        .ok()
        .flatten();
        if local == Some(player) {
            return 1;
        }
    }
    let original: AssetsFn = unsafe { core::mem::transmute(ORIG_ASSETS.load(Ordering::Acquire)) };
    unsafe { original(player) }
}

unsafe extern "C" fn eliminate_detour(player: i32) {
    if player >= 0 && is_observer(player as usize) {
        return;
    }
    let original: EliminateFn =
        unsafe { core::mem::transmute(ORIG_ELIMINATE.load(Ordering::Acquire)) };
    unsafe { original(player) };
}

unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    if event.id == W2R_EVT_MATCH_START {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if let Some(game) = GAME.get() {
                let mask = read_observers(game);
                OBSERVERS.store(mask, Ordering::Relaxed);
                log(W2R_LOG_INFO, &format!("match start, observers {mask:#010b}"));
            }
        }));
    } else if event.id == W2R_EVT_LOBBY_ENTER {
        OBSERVERS.store(0, Ordering::Relaxed);
    }
}

fn install(h: &W2rHost, symbol: &str, detour: *mut c_void, original: &AtomicUsize) -> bool {
    let Some(target) = handle_of(symbol) else {
        log(W2R_LOG_ERROR, &format!("symbol {symbol} not available"));
        return false;
    };
    let mut trampoline: *mut c_void = core::ptr::null_mut();
    let mut hook = 0;
    let status = unsafe { (h.hook_install)(h.token, target, detour, &mut trampoline, &mut hook) };
    if status != W2R_OK {
        log(W2R_LOG_ERROR, &format!("{symbol}: hook_install returned {status}"));
        return false;
    }
    original.store(trampoline as usize, Ordering::Release);
    HOOKS.lock().unwrap_or_else(|e| e.into_inner()).push(hook);
    true
}

fn remove_hooks() {
    let hooks = std::mem::take(&mut *HOOKS.lock().unwrap_or_else(|e| e.into_inner()));
    if let Some(h) = host() {
        for hook in hooks {
            unsafe { (h.hook_remove)(h.token, hook) };
        }
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    let game = match resolve() {
        Ok(g) => g,
        Err(name) => {
            log(W2R_LOG_ERROR, &format!("symbol {name} not available; no observer"));
            return W2R_E_STATE;
        }
    };
    let _ = GAME.set(game);
    let Some(h) = self::host() else {
        return W2R_E_STATE;
    };
    for event in [W2R_EVT_MATCH_START, W2R_EVT_LOBBY_ENTER] {
        if unsafe { (h.subscribe)(h.token, event) } != W2R_OK {
            log(W2R_LOG_ERROR, "cannot subscribe to match events; no observer");
            return W2R_E_STATE;
        }
    }
    let ok = install(h, "HudFoodCell", food_cell_detour as *mut c_void, &ORIG_FOOD)
        && install(h, "CheckMatchEnd", check_end_detour as *mut c_void, &ORIG_CHECK)
        && install(h, "PlayerHasAssets", assets_detour as *mut c_void, &ORIG_ASSETS)
        && install(h, "EliminatePlayer", eliminate_detour as *mut c_void, &ORIG_ELIMINATE);
    if !ok {
        remove_hooks();
        return W2R_E_STATE;
    }
    log(W2R_LOG_INFO, "observer ready");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    remove_hooks();
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
