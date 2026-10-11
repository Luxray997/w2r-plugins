//! The game-facing half (32-bit only). One detour on `HudMenuButtons` (the HUD menu buttons, drawn every frame in a match; HudFoodCell, HudResourceCell and HudBeginFrame belong to other plugins and a function takes one hook) is the per-frame tick: it calls
//! the original, then makes the black entries of the HD color table and of the render palette match
//! the setting. Panics are caught before they unwind into the game (detours are outside the host's
//! crash guard, docs/abi.md).

use core::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI64, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use w2r_abi::*;

use crate::model::{self, BLACK, RAMP};

/// Render palette: the black ramp starts at this entry; four bytes (RGBA) per entry.
const RAMP_BASE: usize = 0xe4;
const PALETTE_STRIDE: usize = 4;

type MenuButtonsFn = unsafe extern "C" fn(i32);

struct Game {
    table: *mut f32,
    palette: *const *mut u8,
}

// SAFETY: addresses of the game's globals, fixed for the session; used from the main thread.
unsafe impl Send for Game {}
unsafe impl Sync for Game {}

/// What we last wrote, so a foreign change is noticed and the stock values are never lost.
struct Applied {
    stock_table: [f32; 4],
    stock_ramp: [[u8; 3]; RAMP],
    written_ramp: [[u8; 3]; RAMP],
    owns_ramp: bool,
}

static GAME: OnceLock<Game> = OnceLock::new();
static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<W2rHandle>> = Mutex::new(None);
static CHOICE: AtomicI64 = AtomicI64::new(0);
static APPLIED: Mutex<Option<Applied>> = Mutex::new(None);

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

fn load_choice() {
    let Some(h) = host() else { return };
    let mut v = 0i64;
    let ok = unsafe { (h.settings_get_i64)(h.token, s(model::KEY), &mut v) } == W2R_OK;
    let v = if ok { v.clamp(0, model::MAX_CHOICE as i64) } else { 0 };
    CHOICE.store(v, Ordering::Relaxed);
}

unsafe fn read_ramp(live: *const u8) -> [[u8; 3]; RAMP] {
    let mut out = [[0u8; 3]; RAMP];
    for (k, entry) in out.iter_mut().enumerate() {
        for (c, v) in entry.iter_mut().enumerate() {
            *v = unsafe { *live.add(k * PALETTE_STRIDE + c) };
        }
    }
    out
}

unsafe fn write_ramp(live: *mut u8, ramp: &[[u8; 3]; RAMP]) {
    for (k, entry) in ramp.iter().enumerate() {
        for (c, v) in entry.iter().enumerate() {
            unsafe { *live.add(k * PALETTE_STRIDE + c) = *v };
        }
    }
}

/// Brings both tables in line with the setting. Cheap when nothing differs.
unsafe fn apply(game: &Game) {
    unsafe {
        let mut guard = APPLIED.lock().unwrap_or_else(|e| e.into_inner());
        let t = game.table.add(BLACK * 4);
        let state = guard.get_or_insert_with(|| Applied {
            stock_table: [*t, *t.add(1), *t.add(2), *t.add(3)],
            stock_ramp: [[0; 3]; RAMP],
            written_ramp: [[0; 3]; RAMP],
            owns_ramp: false,
        });
        let target = model::target(CHOICE.load(Ordering::Relaxed));

        // HD table entry.
        let mut want = state.stock_table;
        if let Some(rgb) = target {
            want[..3].copy_from_slice(&model::float_rgb(rgb));
        }
        for (c, w) in want.iter().enumerate() {
            if *t.add(c) != *w {
                *t.add(c) = *w;
            }
        }

        // Render palette ramp (not allocated early on).
        let palette = *game.palette;
        if palette.is_null() {
            return;
        }
        let live = palette.add(RAMP_BASE * PALETTE_STRIDE);
        if state.owns_ramp && read_ramp(live) != state.written_ramp {
            // The palette was rebuilt: what is there now is the new stock.
            state.owns_ramp = false;
        }
        match target {
            Some(rgb) => {
                if !state.owns_ramp {
                    state.stock_ramp = read_ramp(live);
                }
                let ramp = model::ramp(&state.stock_ramp, rgb);
                if !state.owns_ramp || read_ramp(live) != ramp {
                    write_ramp(live, &ramp);
                    state.written_ramp = ramp;
                    state.owns_ramp = true;
                }
            }
            None => {
                if state.owns_ramp {
                    write_ramp(live, &state.stock_ramp);
                    state.owns_ramp = false;
                }
            }
        }
    }
}

unsafe extern "C" fn menu_buttons_detour(view: i32) {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original != 0 {
        let original: MenuButtonsFn = unsafe { core::mem::transmute(original) };
        unsafe { original(view) };
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            unsafe { apply(game) };
        }
    }));
}

unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    if event.id == W2R_EVT_SETTINGS_CHANGED {
        load_choice();
    }
}

unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let Some(ui) = (unsafe { ui.as_ref() }) else {
        return;
    };
    let mut value = CHOICE.load(Ordering::Relaxed) as i32;
    unsafe {
        (ui.label)(ui.ctx, s("Black team color on your screen: 0 default, 1 pink, 2 cyan"));
        if (ui.slider_int)(ui.ctx, s("Black team color"), &mut value, 0, model::MAX_CHOICE) != 0 {
            let value = value.clamp(0, model::MAX_CHOICE);
            CHOICE.store(value as i64, Ordering::Relaxed);
            if let Some(h) = host() {
                (h.settings_set_i64)(h.token, s(model::KEY), value as i64);
            }
        }
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
    let (Some(table), Some(palette)) = (lookup("TeamColourRgba"), lookup("RenderPalette")) else {
        log(W2R_LOG_ERROR, "TeamColourRgba or RenderPalette not available; no recolor");
        return W2R_E_STATE;
    };
    let _ = GAME.set(Game {
        table: table as *mut f32,
        palette: palette as *const *mut u8,
    });
    load_choice();
    let Some(h) = self::host() else {
        return W2R_E_STATE;
    };
    unsafe { (h.subscribe)(h.token, W2R_EVT_SETTINGS_CHANGED) };
    let Some(target) = handle_of("HudMenuButtons") else {
        log(W2R_LOG_ERROR, "symbol HudMenuButtons not available; no recolor");
        return W2R_E_STATE;
    };
    let mut trampoline: *mut c_void = core::ptr::null_mut();
    let mut hook = 0;
    let status = unsafe {
        (h.hook_install)(h.token, target, menu_buttons_detour as *mut c_void, &mut trampoline, &mut hook)
    };
    if status != W2R_OK {
        log(W2R_LOG_ERROR, &format!("HudMenuButtons: hook_install returned {status}; no recolor"));
        return W2R_E_STATE;
    }
    ORIGINAL.store(trampoline as usize, Ordering::Release);
    *HOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    log(W2R_LOG_INFO, "black recolor ready");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    remove_hook();
    // Put the stock colours back.
    CHOICE.store(0, Ordering::Relaxed);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            unsafe { apply(game) };
        }
    }));
}

static TABLE: W2rPlugin = W2rPlugin {
    struct_size: W2R_PLUGIN_V1_SIZE,
    on_load: Some(on_load),
    on_event: Some(on_event),
    on_ui: Some(on_ui),
    on_unload: Some(on_unload),
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &TABLE
}
