//! The host-facing half: registers one callback for the Create Multiplayer Game screen and hands
//! the host's rows and map index to the [`Controller`]. No game memory is touched; everything goes
//! through the host's create-screen functions (docs/abi.md, "Create screen" in the w2rpm repo).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use w2r_abi::*;

use crate::model::{Controller, Maps, Rows, Unavailable, valid_players};

/// The settings key holding the slider value (0 = any number of players).
const KEY: &str = "players";

static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static STATE: Mutex<Option<Controller>> = Mutex::new(None);

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

impl Maps for HostMaps {
    fn count(&mut self, players: u32) -> Result<u32, Unavailable> {
        let mut count = 0u32;
        match unsafe { (self.0.create_map_count)(self.0.token, players, &mut count) } {
            W2R_OK => Ok(count),
            W2R_E_BUSY => Err(Unavailable::Busy),
            _ => Err(Unavailable::Failed),
        }
    }

    fn select(&mut self, players: u32, nth: u32) -> bool {
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
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Controller::new(load(), seed()));
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
    on_ui: None,
    on_unload: Some(on_unload),
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &TABLE
}
