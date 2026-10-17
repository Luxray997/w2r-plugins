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
type WidgetTextFn = unsafe extern "C" fn(
    *mut c_void,
    f32,
    f32,
    f32,
    f32,
    *const core::ffi::c_char,
    i32,
    *const c_void,
    u32,
    *const c_void,
);
type TextWidthFn = unsafe extern "C" fn(*mut c_void, f32, *const core::ffi::c_char, i32) -> f32;

const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
/// The default key that shows every player's name (the tilde key, left of 1); the setting `names_key`
/// holds another virtual-key code.
const DEFAULT_NAMES_KEY: i64 = 0xc0;
const DEPARTED_DIM: u32 = 0x5000_0000;
const TEXT_DEPARTED: u32 = 0xffa0_a0a0;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(vk: i32) -> i16;
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
}

/// UI context: mouse position (floats), font, text padding, text style colours.
const CTX_MOUSE_X: usize = 0x1bc;
const CTX_MOUSE_Y: usize = 0x1c0;
const CTX_FONT: usize = 0x1e0;
const CTX_TEXT_EXTRA0: usize = 0x210;
const CTX_TEXT_EXTRA1: usize = 0x21c;
const CTX_TEXT_EXTRA2: usize = 0x220;
const CTX_WINDOW_BACKGROUND: usize = 0x1f9c;
const USER_FONT_HEIGHT: usize = 0x04;
const USER_FONT_WIDTH: usize = 0x08;
/// Bytes per player in the game's name table.
const NAME_STRIDE: usize = 0x38;
const TIP_PAD: f32 = 4.0;
const TIP_BACK: u32 = 0xf010_1010;
const TIP_TEXT: u32 = 0xffff_ffff;

struct Game {
    hud_ctx: *const *mut c_void,
    fill_rect: FillRectFn,
    widget_text: WidgetTextFn,
    player_names: *const u8,
    colour_rgba: *const f32,
    colour_reorder: *const u8,
    local_player: *const u8,
    owner: *const u8,
    lobby_slots: *const u8,
    result: *const u8,
    peers: *const u8,
    slot_by_net: *const u32,
    chat_target_mode: *const u8,
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

fn setting(key: &str, default: i64) -> i64 {
    let mut v = 0i64;
    match host() {
        Some(h) if unsafe { (h.settings_get_i64)(h.token, s(key), &mut v) } == W2R_OK => v,
        _ => default,
    }
}

static NAMES_KEY: AtomicUsize = AtomicUsize::new(0);

/// Whether the names key is held: only while the game's window has the focus and the chat box is
/// closed, and not with Ctrl, Shift or Alt (those make other commands).
fn names_key_held(game: &Game) -> bool {
    unsafe {
        if *game.chat_target_mode != 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        if pid != GetCurrentProcessId() {
            return false;
        }
        let mut vk = NAMES_KEY.load(Ordering::Relaxed) as i32;
        if vk == 0 {
            vk = setting("names_key", DEFAULT_NAMES_KEY).clamp(1, 254) as i32;
            NAMES_KEY.store(vk as usize, Ordering::Relaxed);
        }
        let down = |vk: i32| GetAsyncKeyState(vk) as u16 & 0x8000 != 0;
        down(vk) && !down(VK_CONTROL) && !down(VK_SHIFT) && !down(VK_MENU)
    }
}

fn resolve() -> Result<Game, &'static str> {
    let at = |name: &'static str| lookup(name).ok_or(name);
    Ok(Game {
        hud_ctx: at("HudCtx")? as *const *mut c_void,
        fill_rect: unsafe { core::mem::transmute::<usize, FillRectFn>(at("NkFillRect")?) },
        widget_text: unsafe { core::mem::transmute::<usize, WidgetTextFn>(at("NkWidgetText")?) },
        player_names: at("PlayerNames")? as *const u8,
        colour_rgba: at("TeamColourRgba")? as *const f32,
        colour_reorder: at("TeamColourReorder")? as *const u8,
        local_player: at("LocalPlayer")? as *const u8,
        owner: at("PlayerOwner")? as *const u8,
        lobby_slots: at("MpLobbySlots")? as *const u8,
        result: at("PlayerMatchResult")? as *const u8,
        peers: at("PeerTable")? as *const u8,
        slot_by_net: at("LobbySlotByNetSlot")? as *const u32,
        chat_target_mode: at("ChatTargetMode")? as *const u8,
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
        let names = names_key_held(game);
        // With the names up everyone shows, those who are out dimmed.
        let items = model::layout(&roster, |p| names || still_in(game, p));
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
        if names {
            draw_named(game, ctx.cast::<u8>(), canvas, &items, (panel_x, panel_y, panel_w, panel_h), (strip_left, strip_right));
            return;
        }
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
        let mouse = (
            ctx.cast::<u8>().add(CTX_MOUSE_X).cast::<f32>().read_unaligned(),
            ctx.cast::<u8>().add(CTX_MOUSE_Y).cast::<f32>().read_unaligned(),
        );
        let mut hovered = None;
        for (item, at) in items.iter().zip(offsets) {
            let bx = left + at;
            match *item {
                Item::Box(player) => {
                    if mouse.0 >= bx
                        && mouse.0 < bx + model::BOX
                        && mouse.1 >= top
                        && mouse.1 < top + model::BOX
                    {
                        hovered = Some((player, bx));
                    }
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
        if let Some((player, _)) = hovered {
            // Past the bar's right end, inside the top bar's own strip (it clips anything below).
            let tip_x = if left + width + 120.0 <= panel_x + panel_w { left + width + 8.0 } else { left - 128.0 };
            draw_tip(game, ctx.cast::<u8>(), canvas, player, tip_x, panel_y, panel_h);
        }
    }
}

/// The bar with every player's name after their box, on a dark backing so the text reads over the
/// resource strip when the bar is too wide to sit beside it.
unsafe fn draw_named(
    game: &Game,
    ctx: *const u8,
    canvas: *mut c_void,
    items: &[Item],
    panel: (f32, f32, f32, f32),
    strip: (f32, f32),
) {
    unsafe {
        let font = ctx.add(CTX_FONT).cast::<*const u8>().read_unaligned();
        if font.is_null() {
            return;
        }
        let userdata = font.cast::<*mut c_void>().read_unaligned();
        let font_h = font.add(USER_FONT_HEIGHT).cast::<f32>().read_unaligned();
        let Some(width_fn) = font.add(USER_FONT_WIDTH).cast::<Option<TextWidthFn>>().read_unaligned()
        else {
            return;
        };
        let (panel_x, panel_y, panel_w, panel_h) = panel;
        let text_width = |p: u8| {
            let name = name_of(game, p);
            width_fn(userdata, font_h, name.as_ptr().cast(), name.len() as i32)
        };
        let (offsets, width) = model::named_offsets(items, &text_width);
        let left = model::named_start_x(strip.0, strip.1, panel_x, panel_x + panel_w, width);
        let top = panel_y + ((panel_h - model::BOX) / 2.0).floor();
        let me = *game.local_player;
        let fill = game.fill_rect;
        fill(canvas, left - 6.0, panel_y, width + 12.0, panel_h, 2.0, TIP_BACK);
        let mut style = [0u32; 7];
        style[2] = ctx.add(CTX_WINDOW_BACKGROUND).cast::<u32>().read_unaligned();
        style[4] = ctx.add(CTX_TEXT_EXTRA0).cast::<u32>().read_unaligned();
        style[5] = ctx.add(CTX_TEXT_EXTRA1).cast::<u32>().read_unaligned();
        style[6] = ctx.add(CTX_TEXT_EXTRA2).cast::<u32>().read_unaligned();
        for (item, at) in items.iter().zip(offsets) {
            let bx = left + at;
            match *item {
                Item::Box(player) => {
                    let alive = still_in(game, player);
                    let border = if player == me { BORDER_SELF } else { BORDER };
                    let outer = model::BOX + 2.0;
                    let colour = colour_of(game, player);
                    let (border, colour) = if alive {
                        (border, colour)
                    } else {
                        ((border & 0x00ff_ffff) | DEPARTED_DIM, (colour & 0x00ff_ffff) | DEPARTED_DIM)
                    };
                    fill(canvas, bx - 1.0, top - 1.0, outer, outer, 0.0, border);
                    fill(canvas, bx, top, model::BOX, model::BOX, 0.0, colour);
                    let name = name_of(game, player);
                    style[3] = if alive { TIP_TEXT } else { TEXT_DEPARTED };
                    (game.widget_text)(
                        canvas,
                        bx + model::BOX + model::NAME_GAP,
                        panel_y + ((panel_h - font_h) / 2.0).floor(),
                        text_width(player) + 1.0,
                        font_h,
                        name.as_ptr().cast(),
                        name.len() as i32,
                        style.as_ptr().cast(),
                        0x11,
                        font.cast(),
                    );
                }
                Item::Divider => {
                    let dx = bx + model::TEAM_GAP;
                    fill(canvas, dx, top, model::DIVIDER, model::BOX, 0.0, DIVIDER_COLOUR);
                }
            }
        }
    }
}

/// The name shown for a player: the game's name table entry, or a stand-in when it is empty.
fn name_of(game: &Game, player: u8) -> String {
    let slot = unsafe {
        core::slice::from_raw_parts(game.player_names.add(player as usize * NAME_STRIDE), NAME_STRIDE)
    };
    let len = slot.iter().position(|&b| b == 0).unwrap_or(0);
    if len == 0 {
        let computer = unsafe { *game.owner.add(player as usize) } == OWNER_COMPUTER;
        return if computer { format!("Computer {}", player + 1) } else { format!("Player {}", player + 1) };
    }
    String::from_utf8_lossy(&slot[..len]).into_owned()
}

/// A small label with the player's name, left edge at `x`, top at `y`, nudged left to stay on screen.
unsafe fn draw_tip(game: &Game, ctx: *const u8, canvas: *mut c_void, player: u8, x: f32, y: f32, bar_h: f32) {
    unsafe {
        let font = ctx.add(CTX_FONT).cast::<*const u8>().read_unaligned();
        if font.is_null() {
            return;
        }
        let userdata = font.cast::<*mut c_void>().read_unaligned();
        let font_h = font.add(USER_FONT_HEIGHT).cast::<f32>().read_unaligned();
        let Some(width) = font.add(USER_FONT_WIDTH).cast::<Option<TextWidthFn>>().read_unaligned()
        else {
            return;
        };
        let name = name_of(game, player);
        let text_w = width(userdata, font_h, name.as_ptr().cast(), name.len() as i32);
        let (w, h) = (text_w + TIP_PAD * 2.0, bar_h);
        let x = x.max(0.0);
        (game.fill_rect)(canvas, x, y, w, h, 2.0, TIP_BACK);
        let mut style = [0u32; 7];
        style[2] = ctx.add(CTX_WINDOW_BACKGROUND).cast::<u32>().read_unaligned();
        style[3] = TIP_TEXT;
        style[4] = ctx.add(CTX_TEXT_EXTRA0).cast::<u32>().read_unaligned();
        style[5] = ctx.add(CTX_TEXT_EXTRA1).cast::<u32>().read_unaligned();
        style[6] = ctx.add(CTX_TEXT_EXTRA2).cast::<u32>().read_unaligned();
        (game.widget_text)(
            canvas,
            x + TIP_PAD,
            y + ((bar_h - font_h) / 2.0).floor(),
            text_w + 1.0,
            font_h,
            name.as_ptr().cast(),
            name.len() as i32,
            style.as_ptr().cast(),
            0x11,
            font.cast(),
        );
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
