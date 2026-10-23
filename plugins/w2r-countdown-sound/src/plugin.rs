//! The game-facing half (32-bit only). Nothing is hooked: a helper thread reads the game's
//! countdown flag and seconds-left counter (plain globals, written by the main thread) every
//! 20 ms and plays a generated tone through `PlaySoundW` when they change, but only while the game is not the
//! foreground application (the game has its own countdown sound, which is silent otherwise). The thread is joined
//! on unload so no plugin code runs after the DLL is gone.

use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use w2r_abi::*;

use crate::model::{self, Cue, PcmFormat, Tracker};

const SND_NODEFAULT: u32 = 0x0002;
const SND_MEMORY: u32 = 0x0004;

#[link(name = "winmm")]
unsafe extern "system" {
    fn PlaySoundW(sound: *const u16, module: *mut c_void, flags: u32) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentProcess() -> *mut c_void;
    fn ReadProcessMemory(
        process: *mut c_void,
        base: *const c_void,
        buf: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
}

/// Reads game memory without risking a fault: a bad address is a `false`, not a crash.
fn safe_read(addr: usize, out: &mut [u8]) -> bool {
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(GetCurrentProcess(), addr as *const c_void, out.as_mut_ptr().cast(), out.len(), &mut got)
    };
    ok != 0 && got == out.len()
}

fn read_u32(addr: usize) -> Option<usize> {
    let mut b = [0u8; 4];
    safe_read(addr, &mut b).then(|| u32::from_le_bytes(b) as usize)
}

// The game's sound table (`SoundTable`, stride 0x18, id 0 is the countdown tick).
const ENTRY_LEN: usize = 0x00; // data length in bytes
const ENTRY_BUFFER: usize = 0x14; // the IDirectSoundBuffer, once the sound has been played

// IDirectSoundBuffer vtable offsets.
const VT_GET_FORMAT: usize = 0x14;
const VT_LOCK: usize = 0x2c;
const VT_UNLOCK: usize = 0x4c;

type GetFormatFn = unsafe extern "system" fn(*mut c_void, *mut u8, u32, *mut u32) -> i32;
type LockFn = unsafe extern "system" fn(
    *mut c_void,
    u32,
    u32,
    *mut *mut u8,
    *mut u32,
    *mut *mut u8,
    *mut u32,
    u32,
) -> i32;
type UnlockFn = unsafe extern "system" fn(*mut c_void, *mut u8, u32, *mut u8, u32) -> i32;

/// The game's countdown sound, copied out of its DirectSound buffer.
struct Stock {
    format: PcmFormat,
    data: Vec<u8>,
}

const STATUS_UNTRIED: u32 = 0;
const STATUS_CAPTURED: u32 = 1;
const STATUS_FAILED: u32 = 2;
const CAPTURE_ATTEMPTS: u32 = 3;

static SOUND_TABLE: AtomicUsize = AtomicUsize::new(0);
static STATUS: AtomicU32 = AtomicU32::new(STATUS_UNTRIED);
static STOCK: Mutex<Option<Stock>> = Mutex::new(None);

/// Copies the sound out of the game's buffer. The buffer exists once the game has played the
/// sound (it creates it on first use, even when it is muted by a lost focus).
unsafe fn capture(table: usize) -> Option<Stock> {
    let len = read_u32(table + ENTRY_LEN)?;
    if len == 0 || len > 1 << 20 {
        return None;
    }
    let buffer = read_u32(table + ENTRY_BUFFER)?;
    if buffer == 0 {
        return None;
    }
    let vtable = read_u32(buffer)?;
    let get_format: GetFormatFn = unsafe { core::mem::transmute(read_u32(vtable + VT_GET_FORMAT)?) };
    let lock: LockFn = unsafe { core::mem::transmute(read_u32(vtable + VT_LOCK)?) };
    let unlock: UnlockFn = unsafe { core::mem::transmute(read_u32(vtable + VT_UNLOCK)?) };
    let this = buffer as *mut c_void;

    let mut fmt = [0u8; 18];
    let mut written = 0u32;
    if unsafe { get_format(this, fmt.as_mut_ptr(), fmt.len() as u32, &mut written) } < 0 {
        return None;
    }
    let format = PcmFormat::parse(&fmt)?;
    if !format.usable() {
        return None;
    }

    let (mut p1, mut p2) = (core::ptr::null_mut::<u8>(), core::ptr::null_mut::<u8>());
    let (mut l1, mut l2) = (0u32, 0u32);
    if unsafe { lock(this, 0, len as u32, &mut p1, &mut l1, &mut p2, &mut l2, 0) } < 0 {
        return None;
    }
    let mut data = Vec::with_capacity(len);
    if !p1.is_null() && l1 as usize <= len {
        data.extend_from_slice(unsafe { core::slice::from_raw_parts(p1, l1 as usize) });
    }
    if !p2.is_null() && data.len() + l2 as usize <= len {
        data.extend_from_slice(unsafe { core::slice::from_raw_parts(p2, l2 as usize) });
    }
    unsafe { unlock(this, p1, l1, p2, l2) };
    (!data.is_empty()).then_some(Stock { format, data })
}

/// Tries to have the game's sound; gives up for good after a few failed cues.
fn ensure_stock(attempts: &mut u32) {
    if STATUS.load(Ordering::Acquire) != STATUS_UNTRIED {
        return;
    }
    let table = SOUND_TABLE.load(Ordering::Acquire);
    if table != 0 && *attempts < CAPTURE_ATTEMPTS {
        *attempts += 1;
        // The game creates the buffer in the same frame as the cue; give it a moment.
        for _ in 0..5 {
            if let Some(stock) = unsafe { capture(table) } {
                *STOCK.lock().unwrap_or_else(|e| e.into_inner()) = Some(stock);
                STATUS.store(STATUS_CAPTURED, Ordering::Release);
                return;
            }
            thread::sleep(Duration::from_millis(30));
        }
    }
    if table == 0 || *attempts >= CAPTURE_ATTEMPTS {
        STATUS.store(STATUS_FAILED, Ordering::Release);
    }
}

/// True when the foreground window belongs to this process (the game has focus). False when it is
/// in the background, minimised, or nothing has focus; the game plays its own sound only while
/// focused.
fn game_has_focus() -> bool {
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        pid == GetCurrentProcessId()
    }
}

static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static RUNNING: AtomicBool = AtomicBool::new(false);
static VOLUME: AtomicI32 = AtomicI32::new(model::DEFAULT_VOLUME);
static WORKER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

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

fn load_volume() {
    let Some(h) = host() else { return };
    let mut v = 0i64;
    let ok = unsafe { (h.settings_get_i64)(h.token, s(model::KEY), &mut v) } == W2R_OK;
    let v = if ok {
        v.clamp(0, i64::from(model::MAX_VOLUME)) as i32
    } else {
        model::DEFAULT_VOLUME
    };
    VOLUME.store(v, Ordering::Relaxed);
}

fn play(cue: Cue) {
    let volume = VOLUME.load(Ordering::Relaxed);
    if volume <= 0 || game_has_focus() {
        return;
    }
    let stock = match cue {
        Cue::Tick => STOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|st| model::wav_from_pcm(&st.format, &st.data, volume)),
        Cue::Go => None,
    };
    let wav = stock.unwrap_or_else(|| {
        let (freq, ms) = model::tone(cue);
        model::wav(freq, ms, volume)
    });
    // Synchronous, so the buffer outlives the playback; this thread has nothing else to do.
    unsafe { PlaySoundW(wav.as_ptr().cast(), core::ptr::null_mut(), SND_MEMORY | SND_NODEFAULT) };
}

fn watch(flag: usize) {
    let mut tracker = Tracker::default();
    let mut attempts = 0u32;
    while RUNNING.load(Ordering::Acquire) {
        // SAFETY: statics in the game image, valid for the life of the process; byte and aligned
        // word reads cannot tear.
        let (active, seconds) = unsafe {
            (
                core::ptr::read_volatile(flag as *const u8) != 0,
                core::ptr::read_volatile((flag + model::SECONDS_OFFSET) as *const u32),
            )
        };
        if let Some(cue) = tracker.step(active, seconds) {
            if cue == Cue::Tick {
                ensure_stock(&mut attempts);
            }
            play(cue);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    if event.id == W2R_EVT_SETTINGS_CHANGED {
        load_volume();
    }
}

/// Set when the host took the volume as a declaration: its options screen draws and stores it, so
/// `on_ui` only shows which sound is in use.
static DECLARED: AtomicBool = AtomicBool::new(false);

/// Declares the volume to a host that has `settings_declare`; an older host keeps the slider
/// `on_ui` draws.
fn declare_setting() {
    let Some(h) = host() else { return };
    if h.struct_size < W2R_HOST_SETTINGS_DECLARE_SIZE {
        return;
    }
    let def = W2rSettingDef {
        struct_size: W2R_SETTING_DEF_V1_SIZE,
        kind: W2R_SETTING_INT,
        default_int: i64::from(model::DEFAULT_VOLUME),
        min: 0,
        max: i64::from(model::MAX_VOLUME),
        flags: 0,
        choice_count: 0,
        choices: core::ptr::null(),
        reserved: 0,
        key: s(model::KEY),
        label: s(model::LABEL),
        description: s(model::DESCRIPTION),
        category: s(model::CATEGORY),
        subcategory: s(model::SUBCATEGORY),
        default_str: s(""),
        active_when: s(""),
    };
    let status = unsafe { (h.settings_declare)(h.token, &def) };
    if status == W2R_OK {
        DECLARED.store(true, Ordering::Release);
    } else {
        log(
            W2R_LOG_WARN,
            &format!("settings_declare returned {status}; drawing the slider"),
        );
    }
}

unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let Some(ui) = (unsafe { ui.as_ref() }) else {
        return;
    };
    let declared = DECLARED.load(Ordering::Acquire);
    let mut value = VOLUME.load(Ordering::Relaxed);
    unsafe {
        if !declared {
            (ui.label)(
                ui.ctx,
                s("Sounds during the lobby start countdown. 0 is silent."),
            );
        }
        let status = match STATUS.load(Ordering::Acquire) {
            STATUS_CAPTURED => "Using the game's own countdown sound.",
            STATUS_FAILED => "Could not copy the game's sound; using a generated beep.",
            _ => "Copies the game's countdown sound at the first countdown.",
        };
        (ui.label)(ui.ctx, s(status));
        if declared {
            return;
        }
        if (ui.slider_int)(ui.ctx, s("Volume"), &mut value, 0, model::MAX_VOLUME) != 0 {
            let value = value.clamp(0, model::MAX_VOLUME);
            VOLUME.store(value, Ordering::Relaxed);
            if let Some(h) = host() {
                (h.settings_set_i64)(h.token, s(model::KEY), value as i64);
            }
        }
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    let Some(h) = self::host() else {
        return W2R_E_STATE;
    };
    let mut handle = 0;
    if unsafe { (h.symbol_lookup)(h.token, s("MpLobbyCountdownActive"), &mut handle) } != W2R_OK {
        log(W2R_LOG_ERROR, "symbol MpLobbyCountdownActive not available; no countdown sound");
        return W2R_E_STATE;
    }
    let mut flag = 0usize;
    if unsafe { (h.symbol_address)(h.token, handle, &mut flag) } != W2R_OK || flag == 0 {
        log(W2R_LOG_ERROR, "no address for MpLobbyCountdownActive; no countdown sound");
        return W2R_E_STATE;
    }
    // Optional: without the sound table (an older host) the generated beep is used.
    let mut table = 0usize;
    let mut table_handle = 0;
    if unsafe { (h.symbol_lookup)(h.token, s("SoundTable"), &mut table_handle) } == W2R_OK {
        unsafe { (h.symbol_address)(h.token, table_handle, &mut table) };
    }
    SOUND_TABLE.store(table, Ordering::Release);
    load_volume();
    declare_setting();
    unsafe { (h.subscribe)(h.token, W2R_EVT_SETTINGS_CHANGED) };
    RUNNING.store(true, Ordering::Release);
    let worker = thread::Builder::new()
        .name("w2r-countdown-sound".into())
        .spawn(move || watch(flag));
    match worker {
        Ok(w) => *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = Some(w),
        Err(e) => {
            RUNNING.store(false, Ordering::Release);
            log(W2R_LOG_ERROR, &format!("could not start the watcher thread: {e}"));
            return W2R_E_STATE;
        }
    }
    log(W2R_LOG_INFO, "countdown sound ready");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    RUNNING.store(false, Ordering::Release);
    let worker = WORKER.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(w) = worker {
        let _ = w.join();
    }
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
