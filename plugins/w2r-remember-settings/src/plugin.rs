//! The game-facing half (32-bit only). Two hooks on the Create Multiplayer Game screen:
//!
//! - `LobbyCreationInit` runs when the screen opens; after the original has set the defaults we
//!   write the saved values over them.
//! - `LobbyCreateButton` runs when Create is pressed; before the original (which may leave the
//!   screen and reset state) we read the current values and save them.
//!
//! Detours are outside the host's crash guard (docs/abi.md): each one calls the original whatever
//! happens in our code, and a panic is caught before it can unwind into the game.

use core::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use w2r_abi::*;

use crate::model::{self, MAX_SPEED, MapRef, Saved};

/// MSVC `std::string` layout (game build 1.0.2.2818): inline buffer or pointer at 0, length at
/// 0x10, capacity at 0x14, inline when the capacity is at most 15.
const STR_SIZE: usize = 0x10;
const STR_CAPACITY: usize = 0x14;
const STR_SSO_MAX: u32 = 15;

/// A map-list entry (wc2r-plus `struct_offsets.h`): stride and where its strings sit.
const ENTRY_STRIDE: usize = 0x68;
const ENTRY_NAME: usize = 0x08;
const ENTRY_PATH: usize = 0x20;
/// The pending game setup's name field, which the game reads when the lobby is created.
const PENDING_SETUP_NAME: usize = 0x10;
const PENDING_SETUP_NAME_CAPACITY: usize = 0xdc;
/// Sanity limit on the live list we search.
const MAX_ENTRIES: usize = 20_000;

type StringAssignFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, usize) -> *mut c_void;
type CopyEntryFn = unsafe extern "thiscall" fn(*mut c_void, *const c_void) -> *mut c_void;
type RefreshFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);
type ValidateFn = unsafe extern "C" fn(*const c_char) -> i8;
type InitFn = unsafe extern "C" fn();
type CreateFn = unsafe extern "thiscall" fn(*mut c_void);

struct Game {
    assign: StringAssignFn,
    speed: *mut u32,
    resources: *mut u8,
    units: *mut u8,
    tileset: *mut u8,
    start_location: *mut u8,
    /// `LobbyCreationSelected`: the chosen map's list entry.
    selected: *mut u8,
    pending_setup: *mut u8,
    /// `LobbyMapListBegin`: the address of the list vector's `begin` pointer, `end` follows it.
    list_begin: *mut *mut u8,
    list_end: *const *mut u8,
    folder_list: *mut c_void,
    rel_path: *mut u8,
    folder_label: *mut u8,
    copy_entry: CopyEntryFn,
    refresh: RefreshFn,
    validate: ValidateFn,
}

// SAFETY: addresses of the game's globals and a function, fixed for the session; used from the
// game's main thread.
unsafe impl Send for Game {}
unsafe impl Sync for Game {}

static GAME: OnceLock<Game> = OnceLock::new();
static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static ORIGINAL_INIT: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_CREATE: AtomicUsize = AtomicUsize::new(0);
static HOOKS: Mutex<Vec<W2rHandle>> = Mutex::new(Vec::new());

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
        assign: unsafe { core::mem::transmute::<usize, StringAssignFn>(at("StringAssign")?) },
        speed: at("LobbyGameSpeed")? as *mut u32,
        resources: at("LobbyResourcesCurrent")? as *mut u8,
        units: at("LobbyUnitsCurrent")? as *mut u8,
        tileset: at("LobbyTilesetCurrent")? as *mut u8,
        start_location: at("LobbyStartLocCurrent")? as *mut u8,
        selected: at("LobbyCreationSelected")? as *mut u8,
        pending_setup: at("PendingGameSetup")? as *mut u8,
        list_begin: at("LobbyMapListBegin")? as *mut *mut u8,
        list_end: at("LobbyMapListEnd")? as *const *mut u8,
        folder_list: at("LobbyFolderList")? as *mut c_void,
        rel_path: at("LobbyFolderRelPath")? as *mut u8,
        folder_label: at("LobbyFolderCurrent")? as *mut u8,
        copy_entry: unsafe {
            core::mem::transmute::<usize, CopyEntryFn>(at("CopyMapListEntry")?)
        },
        refresh: unsafe {
            core::mem::transmute::<usize, RefreshFn>(at("RefreshMapListFromFolder")?)
        },
        validate: unsafe { core::mem::transmute::<usize, ValidateFn>(at("ValidateMapFile")?) },
    })
}

/// The bytes of a game `std::string` of at most `max` bytes; `None` if empty or too long.
unsafe fn string_bytes(string: *const u8, max: usize) -> Option<Vec<u8>> {
    unsafe {
        let len = string.add(STR_SIZE).cast::<u32>().read_unaligned() as usize;
        let cap = string.add(STR_CAPACITY).cast::<u32>().read_unaligned();
        if len == 0 || len > max {
            return None;
        }
        let data = if cap > STR_SSO_MAX {
            string.cast::<*const u8>().read_unaligned()
        } else {
            string
        };
        if data.is_null() {
            return None;
        }
        Some(core::slice::from_raw_parts(data, len).to_vec())
    }
}

/// The text of a dropdown value, or `None` if it is not a plain short key.
unsafe fn read_string(string: *const u8) -> Option<String> {
    let text = String::from_utf8(unsafe { string_bytes(string, model::MAX_VALUE_LEN) }?).ok()?;
    model::valid_value(&text).then_some(text)
}

/// The text of a map name, path or folder, or `None` if it is not printable ASCII.
unsafe fn read_map_text(string: *const u8) -> Option<String> {
    let text = String::from_utf8(unsafe { string_bytes(string, model::MAX_MAP_TEXT) }?).ok()?;
    model::valid_map_text(&text).then_some(text)
}

unsafe fn capture_map(game: &Game) -> Option<MapRef> {
    let map = unsafe {
        MapRef {
            folder: read_map_text(game.rel_path)?,
            name: read_map_text(game.selected.add(ENTRY_NAME))?,
            path: read_map_text(game.selected.add(ENTRY_PATH))?,
        }
    };
    map.valid().then_some(map)
}

/// Points the game's map list at `folder` (what the map browser does).
unsafe fn set_folder(game: &Game, folder: &str) {
    unsafe {
        write_string(game, game.rel_path, folder);
        let label = folder.rsplit('\\').next().unwrap_or(folder);
        write_string(game, game.folder_label, label);
        (game.refresh)(
            game.list_begin.cast(),
            game.folder_list,
            game.rel_path.cast(),
        );
    }
}

/// Selects the saved map if it is still in its folder. When it is not, the folder the game had
/// is put back and the game's own selection stays.
unsafe fn restore_map(game: &Game, map: &MapRef) {
    unsafe {
        let previous = read_map_text(game.rel_path);
        set_folder(game, &map.folder);
        let (begin, end) = (*game.list_begin, *game.list_end);
        let mut found = None;
        if !begin.is_null() && end > begin {
            let n = ((end as usize - begin as usize) / ENTRY_STRIDE).min(MAX_ENTRIES);
            found = (0..n).map(|i| begin.add(i * ENTRY_STRIDE)).find(|&e| {
                read_map_text(e.add(ENTRY_NAME)).as_deref() == Some(&map.name)
                    && read_map_text(e.add(ENTRY_PATH)).as_deref() == Some(&map.path)
            });
        }
        let Some(entry) = found else {
            if let Some(previous) = previous {
                set_folder(game, &previous);
            }
            return;
        };
        (game.copy_entry)(game.selected.cast(), entry.cast());
        // The game reads the map's name from the pending setup when it creates the lobby.
        let Ok(path) = std::ffi::CString::new(map.path.as_str()) else {
            return;
        };
        if (game.validate)(path.as_ptr()) != 0 {
            let bytes = map.name.as_bytes();
            let n = bytes.len().min(PENDING_SETUP_NAME_CAPACITY - 1);
            let dst = game.pending_setup.add(PENDING_SETUP_NAME);
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, n);
            *dst.add(n) = 0;
        }
    }
}

unsafe fn write_string(game: &Game, string: *mut u8, text: &str) {
    unsafe { (game.assign)(string.cast(), text.as_ptr().cast(), text.len()) };
}

fn capture(game: &Game) -> Saved {
    unsafe {
        let speed = *game.speed;
        Saved {
            speed: (speed <= MAX_SPEED).then_some(speed),
            resources: read_string(game.resources),
            units: read_string(game.units),
            tileset: read_string(game.tileset),
            start_location: read_string(game.start_location),
            map: capture_map(game),
        }
    }
}

fn apply(game: &Game, saved: &Saved) {
    unsafe {
        if let Some(v) = saved.speed {
            *game.speed = v;
        }
        for (target, value) in [
            (game.resources, &saved.resources),
            (game.units, &saved.units),
            (game.tileset, &saved.tileset),
            (game.start_location, &saved.start_location),
        ] {
            if let Some(v) = value {
                write_string(game, target, v);
            }
        }
        if let Some(map) = &saved.map {
            restore_map(game, map);
        }
    }
}

fn load_saved() -> Saved {
    let Some(h) = host() else {
        return Saved::default();
    };
    let mut buf = [0u8; 1024];
    let mut len = 0u32;
    let status = unsafe {
        (h.settings_get_str)(
            h.token,
            s(model::KEY),
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut len,
        )
    };
    if status != W2R_OK || len as usize > buf.len() {
        return Saved::default();
    }
    match core::str::from_utf8(&buf[..len as usize]) {
        Ok(text) => Saved::decode(text),
        Err(_) => Saved::default(),
    }
}

fn store_saved(saved: &Saved) {
    if let Some(h) = host() {
        let text = saved.encode();
        unsafe { (h.settings_set_str)(h.token, s(model::KEY), s(&text)) };
    }
}

unsafe extern "C" fn init_detour() {
    let original = ORIGINAL_INIT.load(Ordering::Acquire);
    if original != 0 {
        let original: InitFn = unsafe { core::mem::transmute(original) };
        unsafe { original() };
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            let saved = load_saved();
            if !saved.is_empty() {
                apply(game, &saved);
            }
        }
    }));
}

unsafe extern "thiscall" fn create_detour(closure: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            let saved = capture(game);
            if !saved.is_empty() {
                store_saved(&saved);
            }
        }
    }));
    let original = ORIGINAL_CREATE.load(Ordering::Acquire);
    if original != 0 {
        let original: CreateFn = unsafe { core::mem::transmute(original) };
        unsafe { original(closure) };
    }
}

fn install_hook(
    symbol: &str,
    detour: *mut c_void,
    original: &AtomicUsize,
) -> Result<W2rHandle, String> {
    let h = host().ok_or("no host")?;
    let target = handle_of(symbol).ok_or_else(|| format!("{symbol}: unknown symbol"))?;
    let mut trampoline: *mut c_void = core::ptr::null_mut();
    let mut hook = 0;
    let status = unsafe { (h.hook_install)(h.token, target, detour, &mut trampoline, &mut hook) };
    if status != W2R_OK {
        return Err(format!("{symbol}: hook_install returned {status}"));
    }
    original.store(trampoline as usize, Ordering::Release);
    Ok(hook)
}

fn remove_hooks() {
    let handles = std::mem::take(&mut *HOOKS.lock().unwrap_or_else(|e| e.into_inner()));
    if let Some(h) = host() {
        for hook in handles.into_iter().rev() {
            unsafe { (h.hook_remove)(h.token, hook) };
        }
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    let game = match resolve() {
        Ok(g) => g,
        Err(name) => {
            log(
                W2R_LOG_ERROR,
                &format!("symbol {name} not available; not remembering settings"),
            );
            return W2R_E_STATE;
        }
    };
    let _ = GAME.set(game);
    let hooks: [(&str, *mut c_void, &AtomicUsize); 2] = [
        (
            "LobbyCreationInit",
            init_detour as *mut c_void,
            &ORIGINAL_INIT,
        ),
        (
            "LobbyCreateButton",
            create_detour as *mut c_void,
            &ORIGINAL_CREATE,
        ),
    ];
    for (symbol, detour, original) in hooks {
        match install_hook(symbol, detour, original) {
            Ok(handle) => HOOKS.lock().unwrap_or_else(|e| e.into_inner()).push(handle),
            Err(e) => {
                log(W2R_LOG_ERROR, &format!("{e}; not remembering settings"));
                remove_hooks();
                return W2R_E_STATE;
            }
        }
    }
    log(W2R_LOG_INFO, "remembering create-game settings");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    remove_hooks();
}

static TABLE: W2rPlugin = W2rPlugin {
    struct_size: W2R_PLUGIN_V1_SIZE,
    on_load: Some(on_load),
    on_event: None,
    on_ui: None,
    on_unload: Some(on_unload),
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &TABLE
}
