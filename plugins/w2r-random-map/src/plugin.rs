//! The host-facing half: registers one callback for the Create Multiplayer Game screen and hands
//! the host's rows and map index to the [`Controller`]. No game memory is touched; everything goes
//! through the host's create-screen functions (docs/abi.md, "Create screen" in the w2rpm repo).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

use w2r_abi::*;

use crate::model::{Controller, FOLDER_CAP, Maps, Rows, Unavailable, valid_players};

/// The settings key holding the slider value (0 = any number of players).
const KEY: &str = "players";
/// The settings key holding the folder limit (empty = every folder).
const FOLDER_KEY: &str = "folder";

static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static STATE: Mutex<Option<Controller>> = Mutex::new(None);
/// The settings page's edit box: the folder text, NUL-terminated.
static EDIT: Mutex<[u8; FOLDER_CAP]> = Mutex::new([0; FOLDER_CAP]);

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

/// The host's map index for the length of one callback.
struct HostMaps(&'static W2rHost);

impl HostMaps {
    /// Tells the host which folder this callback's map calls are limited to. A host that cannot
    /// limit them must not be asked for a limited pick, or the limit would be silently ignored.
    fn limit(&self, folder: &str) -> Result<(), Unavailable> {
        if self.0.struct_size < W2R_HOST_MAP_FOLDER_SIZE {
            return if folder.is_empty() {
                Ok(())
            } else {
                Err(Unavailable::Failed)
            };
        }
        match unsafe { (self.0.create_map_folder)(self.0.token, s(folder)) } {
            W2R_OK => Ok(()),
            _ => Err(Unavailable::Failed),
        }
    }
}

impl Maps for HostMaps {
    fn count(&mut self, players: u32, folder: &str) -> Result<u32, Unavailable> {
        self.limit(folder)?;
        let mut count = 0u32;
        match unsafe { (self.0.create_map_count)(self.0.token, players, &mut count) } {
            W2R_OK => Ok(count),
            W2R_E_BUSY => Err(Unavailable::Busy),
            _ => Err(Unavailable::Failed),
        }
    }

    fn select(&mut self, players: u32, folder: &str, nth: u32) -> bool {
        if self.limit(folder).is_err() {
            return false;
        }
        unsafe { (self.0.create_map_select)(self.0.token, players, nth) == W2R_OK }
    }
}

/// The host's `W2rUi` for the length of one callback.
struct HostRows<'a>(&'a W2rUi);

impl Rows for HostRows<'_> {
    fn label(&mut self, text: &str) {
        unsafe { (self.0.label)(self.0.ctx, s(text)) };
    }

    fn slider(&mut self, text: &str, value: &mut i32, min: i32, max: i32) -> bool {
        unsafe { (self.0.slider_int)(self.0.ctx, s(text), value, min, max) != 0 }
    }

    fn button(&mut self, text: &str) -> bool {
        unsafe { (self.0.button)(self.0.ctx, s(text)) != 0 }
    }
}

fn save(players: i32) {
    if let Some(h) = host() {
        unsafe { (h.settings_set_i64)(h.token, s(KEY), i64::from(players)) };
    }
}

fn load() -> i32 {
    let Some(h) = host() else { return 0 };
    let mut value = 0i64;
    match unsafe { (h.settings_get_i64)(h.token, s(KEY), &mut value) } {
        W2R_OK => valid_players(value),
        _ => 0,
    }
}

fn save_folder(folder: &str) {
    if let Some(h) = host() {
        unsafe { (h.settings_set_str)(h.token, s(FOLDER_KEY), s(folder)) };
    }
}

fn load_folder() -> String {
    let Some(h) = host() else {
        return String::new();
    };
    let mut buf = [0u8; FOLDER_CAP];
    let mut len = 0u32;
    let status = unsafe {
        (h.settings_get_str)(
            h.token,
            s(FOLDER_KEY),
            buf.as_mut_ptr().cast(),
            FOLDER_CAP as u32,
            &mut len,
        )
    };
    if status != W2R_OK {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..(len as usize).min(buf.len())]).into_owned()
}

fn fill_edit(text: &str) {
    let mut edit = EDIT.lock().unwrap_or_else(|e| e.into_inner());
    *edit = [0; FOLDER_CAP];
    let n = text.len().min(FOLDER_CAP - 1);
    edit[..n].copy_from_slice(&text.as_bytes()[..n]);
}

/// The plugin's page in the settings: the folder limit.
unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(ui) = (unsafe { ui.as_ref() }) else {
            return;
        };
        if ui.struct_size < W2R_UI_TEXT_EDIT_SIZE {
            unsafe { (ui.label)(ui.ctx, s("Needs a newer w2rpm to set a folder.")) };
            return;
        }
        let mut buf = *EDIT.lock().unwrap_or_else(|e| e.into_inner());
        let label =
            "Only pick maps from this folder (inside your maps folder, e.g. mp; empty = all)";
        if unsafe { (ui.text_edit)(ui.ctx, s(label), buf.as_mut_ptr(), buf.len() as u32) } != 0 {
            *EDIT.lock().unwrap_or_else(|e| e.into_inner()) = buf;
            let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let text = String::from_utf8_lossy(&buf[..n]).into_owned();
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = state.as_mut() {
                if c.set_folder(&text) {
                    save_folder(c.folder());
                }
            }
        }
    }));
}

fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    nanos ^ (std::process::id() as u64).rotate_left(32)
}

unsafe extern "C" fn create_ui(ui: *const W2rUi) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let (Some(host), Some(ui)) = (host(), unsafe { ui.as_ref() }) else {
            return;
        };
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(controller) = state.as_mut() else {
            return;
        };
        let changed = controller.frame(&mut HostRows(ui), &mut HostMaps(host));
        if changed {
            save(controller.players());
        }
    }));
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    let Some(h) = (unsafe { host.as_ref() }) else {
        return W2R_E_INVALID;
    };
    if h.struct_size < W2R_HOST_CREATE_UI_SIZE {
        log(
            W2R_LOG_ERROR,
            "this version of w2rpm has no create-screen support; update w2rpm",
        );
        return W2R_E_UNSUPPORTED;
    }
    let mut controller = Controller::new(load(), seed());
    controller.set_folder(&load_folder());
    fill_edit(controller.folder());
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(controller);
    let status = unsafe { (h.create_ui_register)(h.token, Some(create_ui)) };
    if status != W2R_OK {
        log(
            W2R_LOG_ERROR,
            &format!("create_ui_register returned {status}"),
        );
        return W2R_E_STATE;
    }
    log(W2R_LOG_INFO, "random map button ready");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    if let Some(h) = host() {
        unsafe { (h.create_ui_register)(h.token, None) };
    }
}

static TABLE: W2rPlugin = W2rPlugin {
    struct_size: W2R_PLUGIN_V1_SIZE,
    on_load: Some(on_load),
    on_event: None,
    on_ui: Some(on_ui),
    on_unload: Some(on_unload),
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &TABLE
}
