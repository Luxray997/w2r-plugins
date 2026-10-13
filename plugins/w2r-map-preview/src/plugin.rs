//! The game-facing half (32-bit only). One detour on `HudBeginFrame` (the top bar's gold, lumber
//! and oil cells; the player color bar uses `HudFoodCell`, and two plugins cannot hook the same
//! function). The original always runs first. On the first cell of the bar we read the hotkey and,
//! when the overlay is open, draw the map picture under the bar with the game's own `NkDrawImage`
//! onto the same canvas. Nothing in the game's state changes, so `affects_simulation` is false.
//!
//! The map is the one the game selected in the lobby (`SelectedMapPath`), read when the match
//! starts and rendered with the same code as the lobby's preview (`w2r-preview`). A map that
//! cannot be read (a campaign map inside an archive, say) simply shows nothing.

use core::ffi::{CStr, c_void};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use w2r_abi::*;
use w2r_preview::mappreview::{self, Preview, TILESET_FILES, Tileset};
use w2r_preview::pud;

use crate::model::{self, Rect, Toggle};

/// Wc2r-plus `struct_offsets.h`: the HUD context's current window, its draw buffer and panel.
const CTX_CURRENT_WINDOW: usize = 0x3d28;
const WINDOW_BUFFER: usize = 0x64;
const WINDOW_NO_SCROLLBAR: u32 = 0x20;
const WINDOW_NAME: &CStr = c"w2r_map_preview";

/// The game keeps a texture this big on both sides; the picture uses a corner of it.
const TEXTURE_DIM: i32 = 512;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const FORMAT_RGBA8: i32 = 0;
const FILTER_LINEAR: i32 = 1;
const BORDER: f32 = 3.0;
const BORDER_COLOUR: u32 = mappreview::rgba(0x18, 0x14, 0x0c);
const WHITE: u32 = 0xffff_ffff;
const MAX_MAP_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ART_BYTES: u64 = 16 * 1024 * 1024;
const PATH_CAP: usize = 260;

const VK_BACKSLASH: i32 = 0xdc;
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;

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

#[repr(C)]
struct GameTexture {
    w: i32,
    h: i32,
    gl_name: u32,
    format: i32,
    filter: i32,
}

#[repr(C)]
struct NkImage {
    handle: *mut c_void,
    w: u16,
    h: u16,
    region: [u16; 4],
}

type BeginWindowFn =
    unsafe extern "C" fn(*mut c_void, *const core::ffi::c_char, f32, f32, f32, f32, u32) -> i32;
type EndWindowFn = unsafe extern "C" fn(*mut c_void);
type CreateTextureFn =
    unsafe extern "C" fn(*mut *mut GameTexture, i32, i32, *const c_void, i32, i32) -> *mut c_void;
type DrawImageFn = unsafe extern "C" fn(*mut c_void, f32, f32, f32, f32, *const NkImage, u32);
type FillRectFn = unsafe extern "C" fn(*mut c_void, f32, f32, f32, f32, f32, u32);
type SubImageFn = unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);
type UiHeightFn = unsafe extern "C" fn() -> f32;

struct Game {
    hud_ctx: *const *mut c_void,
    fill_rect: FillRectFn,
    draw_image: DrawImageFn,
    create_texture: CreateTextureFn,
    ui_height: UiHeightFn,
    begin_window: BeginWindowFn,
    end_window: EndWindowFn,
    sub_image_slot: *const usize,
    map_path: *const u8,
    chat_target_mode: *const u8,
}

// SAFETY: addresses of the game's globals and functions, fixed for the session; used from the
// game's main thread.
unsafe impl Send for Game {}
unsafe impl Sync for Game {}

#[derive(Default)]
struct State {
    toggle: Toggle,
    size: i32,
    /// The map chosen at match start, and its picture once built.
    path: Option<String>,
    preview: Option<Preview>,
    built: bool,
    uploaded: bool,
    texture: Option<usize>,
    texture_failed: bool,
}

static GAME: OnceLock<Game> = OnceLock::new();
static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<W2rHandle>> = Mutex::new(None);
static LOGGED_PLACEMENT: AtomicBool = AtomicBool::new(false);
static LOGGED_INSIDE: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(|| State {
        size: model::SIZE_DEFAULT,
        ..State::default()
    }))
}

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

fn lookup(name: &str) -> Option<usize> {
    let h = host()?;
    let mut handle = 0;
    if unsafe { (h.symbol_lookup)(h.token, s(name), &mut handle) } != W2R_OK {
        return None;
    }
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

fn save_setting(key: &str, value: i64) {
    if let Some(h) = host() {
        unsafe { (h.settings_set_i64)(h.token, s(key), value) };
    }
}

fn resolve() -> Result<Game, &'static str> {
    let at = |name: &'static str| lookup(name).ok_or(name);
    Ok(Game {
        hud_ctx: at("HudCtx")? as *const *mut c_void,
        fill_rect: unsafe { core::mem::transmute::<usize, FillRectFn>(at("NkFillRect")?) },
        draw_image: unsafe { core::mem::transmute::<usize, DrawImageFn>(at("NkDrawImage")?) },
        create_texture: unsafe {
            core::mem::transmute::<usize, CreateTextureFn>(at("CreateGameTexture")?)
        },
        ui_height: unsafe { core::mem::transmute::<usize, UiHeightFn>(at("HudUiHeight")?) },
        begin_window: unsafe {
            core::mem::transmute::<usize, BeginWindowFn>(at("BeginNamedWindow")?)
        },
        end_window: unsafe { core::mem::transmute::<usize, EndWindowFn>(at("EndCurrentWindow")?) },
        sub_image_slot: at("GlTextureSubImage2DSlot")? as *const usize,
        map_path: at("SelectedMapPath")? as *const u8,
        chat_target_mode: at("ChatTargetMode")? as *const u8,
    })
}

fn game_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

fn read_limited(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut out)
        .ok()?;
    (!out.is_empty() && out.len() as u64 <= limit).then_some(out)
}

/// The era's art, read from the game's own files.
fn load_tileset(era: u16) -> Option<Tileset> {
    let (folder, stem) = TILESET_FILES[(era & 3) as usize];
    let base = game_dir()
        .join("Data")
        .join("Art")
        .join("bgs")
        .join(folder)
        .join(stem);
    let read = |ext: &str| read_limited(&base.with_extension(ext), MAX_ART_BYTES);
    let (cv4, vx4, vr4, ppl) = (read("cv4")?, read("vx4")?, read("vr4")?, read("ppl")?);
    let ts = Tileset::build(&cv4, &vx4, &vr4, &ppl);
    (!ts.is_empty()).then_some(ts)
}

fn build(path: &str) -> Option<Preview> {
    if !mappreview::safe_map_path(path) {
        log(W2R_LOG_INFO, &format!("no preview for '{path}': the path is not allowed"));
        return None;
    }
    let Some(bytes) = read_limited(Path::new(path), MAX_MAP_BYTES) else {
        log(W2R_LOG_INFO, &format!("no preview for '{path}': cannot read it"));
        return None;
    };
    let map = match pud::parse(&bytes) {
        Ok(p) => p,
        Err(fault) => {
            log(W2R_LOG_INFO, &format!("no preview for '{path}': not a usable PUD ({fault:?})"));
            return None;
        }
    };
    let Some(ts) = load_tileset(map.era) else {
        log(W2R_LOG_ERROR, &format!("tileset art for era {} is missing", map.era));
        return None;
    };
    Some(mappreview::render(&map, &ts, map.era, mappreview::TEX_SIZE))
}

/// The selected map's path as a string, if the game holds one.
fn read_map_path(game: &Game) -> Option<String> {
    let bytes = unsafe { core::slice::from_raw_parts(game.map_path, PATH_CAP) };
    let text = CStr::from_bytes_until_nul(bytes).ok()?.to_str().ok()?;
    (!text.is_empty()).then(|| text.to_owned())
}

fn ensure_texture(game: &Game, st: &mut State) -> Option<*mut GameTexture> {
    if let Some(t) = st.texture {
        return Some(t as *mut GameTexture);
    }
    if st.texture_failed {
        return None;
    }
    st.texture_failed = true;
    if unsafe { *game.sub_image_slot } == 0 {
        log(W2R_LOG_ERROR, "GL entry points are not ready; no map preview");
        return None;
    }
    let blank = vec![0u8; (TEXTURE_DIM * TEXTURE_DIM * 4) as usize];
    let mut tex: *mut GameTexture = core::ptr::null_mut();
    unsafe {
        (game.create_texture)(
            &mut tex,
            TEXTURE_DIM,
            TEXTURE_DIM,
            blank.as_ptr().cast(),
            FORMAT_RGBA8,
            FILTER_LINEAR,
        )
    };
    let ok = !tex.is_null()
        && unsafe { (*tex).gl_name != 0 && (*tex).w == TEXTURE_DIM && (*tex).h == TEXTURE_DIM };
    if !ok {
        log(W2R_LOG_ERROR, "the game would not make a preview texture");
        return None;
    }
    st.texture_failed = false;
    st.texture = Some(tex as usize);
    Some(tex)
}

/// The hotkey (the backslash key on its own), read only while the game's window has the focus and
/// the chat box is closed, so typing a backslash into chat does not toggle it. Tab was tried first
/// but the game uses it to toggle the minimap terrain.
fn hotkey_down(game: &Game) -> bool {
    unsafe {
        if *game.chat_target_mode != 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        if pid != GetCurrentProcessId() {
            return false;
        }
        let down = |vk: i32| GetAsyncKeyState(vk) as u16 & 0x8000 != 0;
        down(VK_BACKSLASH) && !down(VK_CONTROL) && !down(VK_SHIFT) && !down(VK_MENU)
    }
}

unsafe fn draw(game: &Game) {
    unsafe {
        let ctx = *game.hud_ctx;
        if ctx.is_null() {
            return;
        }
        let down = hotkey_down(game);
        let open = with_state(|st| st.toggle.update(down));
        if !open {
            return;
        }
        // A window of our own can only be opened between windows; inside one it would corrupt
        // the game's UI state, so say so once and draw nothing.
        let inside = !(*ctx.cast::<u8>().add(CTX_CURRENT_WINDOW).cast::<*mut u8>()).is_null();
        if inside {
            if !LOGGED_INSIDE.swap(true, Ordering::Relaxed) {
                log(W2R_LOG_ERROR, "HudBeginFrame runs inside a window; nothing drawn");
            }
            return;
        }
        let prepared = with_state(|st| {
            if !st.built {
                st.built = true;
                st.preview = st.path.as_deref().and_then(build);
            }
            let p = st.preview.as_ref()?;
            let (pw, ph) = (p.width, p.height);
            let tex = ensure_texture(game, st)?;
            let p = st.preview.as_ref()?;
            if !st.uploaded {
                let upload = *game.sub_image_slot;
                if upload == 0 {
                    return None;
                }
                let upload: SubImageFn = core::mem::transmute(upload);
                upload(
                    (*tex).gl_name,
                    0,
                    0,
                    0,
                    pw as i32,
                    ph as i32,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    p.texels.as_ptr().cast(),
                );
                st.uploaded = true;
            }
            Some((tex as usize, pw, ph, st.size as f32))
        });
        let Some((tex, pw, ph, size)) = prepared else {
            return;
        };
        let screen_h = (game.ui_height)();
        let Some(area) = model::place(
            Rect { x: 0.0, y: 0.0, w: 0.0, h: model::TOP_BAR_H },
            size,
            screen_h,
        ) else {
            return;
        };
        let pic = model::fit(area, pw, ph);
        let pad = BORDER;
        let opened = (game.begin_window)(
            ctx,
            WINDOW_NAME.as_ptr(),
            area.x - pad,
            area.y - pad,
            area.w + 2.0 * pad,
            area.h + 2.0 * pad,
            WINDOW_NO_SCROLLBAR,
        ) != 0;
        if opened {
            let window = *ctx.cast::<u8>().add(CTX_CURRENT_WINDOW).cast::<*mut u8>();
            if !window.is_null() {
                let canvas = window.add(WINDOW_BUFFER).cast::<c_void>();
                if !LOGGED_PLACEMENT.swap(true, Ordering::Relaxed) {
                    log(
                        W2R_LOG_INFO,
                        &format!(
                            "picture ({}, {}, {} x {}), screen height {screen_h}",
                            pic.x, pic.y, pic.w, pic.h
                        ),
                    );
                }
                let fill = game.fill_rect;
                let (x, y, w, h) = (pic.x, pic.y, pic.w, pic.h);
                fill(canvas, x - pad, y - pad, w + 2.0 * pad, pad, 0.0, BORDER_COLOUR);
                fill(canvas, x - pad, y + h, w + 2.0 * pad, pad, 0.0, BORDER_COLOUR);
                fill(canvas, x - pad, y, pad, h, 0.0, BORDER_COLOUR);
                fill(canvas, x + w, y, pad, h, 0.0, BORDER_COLOUR);
                let image = NkImage {
                    handle: tex as *mut c_void,
                    w: TEXTURE_DIM as u16,
                    h: TEXTURE_DIM as u16,
                    region: [0, 0, pw as u16, ph as u16],
                };
                (game.draw_image)(canvas, x, y, w, h, &image, WHITE);
            }
        }
        (game.end_window)(ctx);
    }
}

unsafe extern "C" fn frame_detour() {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original != 0 {
        let original: unsafe extern "C" fn() = unsafe { core::mem::transmute(original) };
        unsafe { original() };
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            unsafe { draw(game) };
        }
    }));
}

fn load_settings() {
    with_state(|st| {
        st.size = model::clamp_size(setting("size", model::SIZE_DEFAULT as i64));
    });
}

unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    match event.id {
        W2R_EVT_MATCH_START => {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let path = GAME.get().and_then(read_map_path);
                with_state(|st| {
                    if path.is_none() {
                        log(W2R_LOG_INFO, "the game holds no map path; no map preview this match");
                    }
                    st.path = path;
                    st.preview = None;
                    st.built = false;
                    st.uploaded = false;
                    st.toggle = Toggle::new(false);
                });
            }));
        }
        W2R_EVT_SETTINGS_CHANGED => load_settings(),
        _ => {}
    }
}

unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let Some(ui) = (unsafe { ui.as_ref() }) else {
        return;
    };
    let mut size = with_state(|st| st.size);
    unsafe {
        (ui.label)(ui.ctx, s("\\ opens and closes the map preview during a match."));
        if (ui.slider_int)(ui.ctx, s("Picture size"), &mut size, model::SIZE_MIN, model::SIZE_MAX)
            != 0
        {
            save_setting("size", size as i64);
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
    let game = match resolve() {
        Ok(g) => g,
        Err(name) => {
            log(W2R_LOG_ERROR, &format!("symbol {name} not available; no map preview"));
            return W2R_E_STATE;
        }
    };
    let _ = GAME.set(game);
    load_settings();
    let Some(h) = self::host() else {
        return W2R_E_STATE;
    };
    for event in [W2R_EVT_MATCH_START, W2R_EVT_SETTINGS_CHANGED] {
        if unsafe { (h.subscribe)(h.token, event) } != W2R_OK {
            log(W2R_LOG_ERROR, &format!("cannot subscribe to event {event}; no map preview"));
            return W2R_E_STATE;
        }
    }
    let mut handle = 0;
    if unsafe { (h.symbol_lookup)(h.token, s("HudBeginFrame"), &mut handle) } != W2R_OK {
        log(W2R_LOG_ERROR, "symbol HudBeginFrame not available; no map preview");
        return W2R_E_STATE;
    }
    let mut trampoline: *mut c_void = core::ptr::null_mut();
    let mut hook = 0;
    let status = unsafe {
        (h.hook_install)(h.token, handle, frame_detour as *mut c_void, &mut trampoline, &mut hook)
    };
    if status != W2R_OK {
        log(
            W2R_LOG_ERROR,
            &format!("HudBeginFrame: hook_install returned {status}; no map preview"),
        );
        return W2R_E_STATE;
    }
    ORIGINAL.store(trampoline as usize, Ordering::Release);
    *HOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    log(W2R_LOG_INFO, "map preview ready (\\)");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    remove_hook();
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
