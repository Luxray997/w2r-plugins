//! The game-facing half (32-bit only). Two buttons in the plugin list: "Check" reads the game's
//! ClientSdk local web server object (read only) and lists this process's listening TCP ports;
//! "Start" repeats the check and, only if every pointer matches what the analysis found for build
//! 2818, calls the object's vtable slot 17 once (it takes no arguments) and lists the ports again.
//! Results go to the host log and a status line. Nothing is hooked, written or sent anywhere.

use core::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use w2r_abi::*;

use crate::model::{self, Observed};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn ReadProcessMemory(
        process: *mut c_void,
        base: *const c_void,
        buf: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
}

#[link(name = "iphlpapi")]
unsafe extern "system" {
    fn GetExtendedTcpTable(
        table: *mut c_void,
        size: *mut u32,
        order: i32,
        family: u32,
        class: u32,
        reserved: u32,
    ) -> u32;
}

const AF_INET: u32 = 2;
const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;

static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static STATUS: Mutex<String> = Mutex::new(String::new());

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

fn set_status(text: &str) {
    let mut t: String = text.chars().take(110).collect();
    if t.is_empty() {
        t.push('-');
    }
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = t;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Reads game memory without risking a fault: a bad address is a `None`, not a crash.
fn read_u32(addr: u32) -> Option<u32> {
    if addr == 0 {
        return None;
    }
    let mut b = [0u8; 4];
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as usize as *const c_void,
            b.as_mut_ptr().cast(),
            4,
            &mut got,
        )
    };
    (ok != 0 && got == 4).then(|| u32::from_le_bytes(b))
}

fn observe() -> Observed {
    let exe_base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize as u32;
    let sdk_base = unsafe { GetModuleHandleW(wide("ClientSdk.dll").as_ptr()) } as usize as u32;
    let mut o = Observed {
        exe_base,
        sdk_base,
        ..Observed::default()
    };
    if exe_base == 0 {
        return o;
    }
    let wrapper =
        model::runtime(model::CONTAINER_VA, model::EXE_IMAGE_BASE, exe_base) + model::WRAPPER_OFFSET;
    o.wrapper_vtable = read_u32(wrapper).unwrap_or(0);
    o.sdk_object = read_u32(wrapper + model::SDK_OBJECT_FIELD).unwrap_or(0);
    o.object_vtable = read_u32(o.sdk_object).unwrap_or(0);
    o.start_fn = read_u32(o.object_vtable.wrapping_add(model::START_SLOT * 4)).unwrap_or(0);
    o
}

/// Listening IPv4 TCP sockets owned by this process, as `a.b.c.d:port`.
fn listeners() -> Vec<String> {
    let mut size = 0u32;
    unsafe {
        GetExtendedTcpTable(core::ptr::null_mut(), &mut size, 0, AF_INET, TCP_TABLE_OWNER_PID_LISTENER, 0)
    };
    if size == 0 || size > 1 << 20 {
        return Vec::new();
    }
    let mut buf = vec![0u32; (size as usize).div_ceil(4)];
    let rc = unsafe {
        GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut size, 0, AF_INET, TCP_TABLE_OWNER_PID_LISTENER, 0)
    };
    if rc != 0 {
        return Vec::new();
    }
    let pid = unsafe { GetCurrentProcessId() };
    let rows = buf[0] as usize;
    let mut out = Vec::new();
    for i in 0..rows.min(buf.len().saturating_sub(1) / 6) {
        let row = &buf[1 + i * 6..1 + i * 6 + 6];
        if row[5] != pid {
            continue;
        }
        let a = row[1].to_le_bytes();
        let port = u16::from_be((row[2] & 0xffff) as u16);
        out.push(format!("{}.{}.{}.{}:{port}", a[0], a[1], a[2], a[3]));
    }
    out
}

fn describe(o: &Observed) -> String {
    format!(
        "exe {:#x} sdk {:#x} wrapper vtable {:#x} object {:#x} object vtable {:#x} slot17 {:#x}",
        o.exe_base, o.sdk_base, o.wrapper_vtable, o.sdk_object, o.object_vtable, o.start_fn
    )
}

fn ports_text(ports: &[String]) -> String {
    if ports.is_empty() {
        "nothing".into()
    } else {
        ports.join(", ")
    }
}

fn check() {
    let o = observe();
    log(W2R_LOG_INFO, &describe(&o));
    let verdict = model::verify(&o);
    let ports = listeners();
    log(W2R_LOG_INFO, &format!("verify: {verdict:?}; listening: {ports:?}"));
    match verdict {
        Ok(()) => set_status(&format!("Object ready. Listening: {}", ports_text(&ports))),
        Err(why) => set_status(&format!("Not safe to call: {why}")),
    }
}

/// The experiments, each allowed once per session. `Slot17` registers the `web-api` routes (it ran
/// fine and left the game idle); the others are the candidates for what opens the listening socket.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Call {
    Slot17,
    Slot4,
    Slot5Port0,
    Slot6,
}

impl Call {
    fn index(self) -> usize {
        self as usize
    }
    fn name(self) -> &'static str {
        match self {
            Call::Slot17 => "slot 17 ()",
            Call::Slot4 => "slot 4 () -> bool",
            Call::Slot5Port0 => "slot 5 (port 0)",
            Call::Slot6 => "slot 6 ()",
        }
    }
}

static DONE: [AtomicBool; 4] = [const { AtomicBool::new(false) }; 4];

// `this` in ECX, as thiscall; the callee pops its own stack argument, as fastcall does.
type NoArgFn = unsafe extern "fastcall" fn(*mut c_void, usize) -> u32;
type PortFn = unsafe extern "fastcall" fn(*mut c_void, usize, u32) -> u32;

fn run(call: Call) {
    let done = &DONE[call.index()];
    if done.load(Ordering::Acquire) {
        set_status(&format!("{} already called this session; use Check.", call.name()));
        return;
    }
    let o = observe();
    log(W2R_LOG_INFO, &describe(&o));
    if let Err(why) = model::verify(&o) {
        log(W2R_LOG_WARN, &format!("refusing to call {}: {why}", call.name()));
        set_status(&format!("Refused: {why}"));
        return;
    }
    let slot = match call {
        Call::Slot17 => 17,
        Call::Slot4 => 4,
        Call::Slot5Port0 => 5,
        Call::Slot6 => 6,
    };
    let Some(target) = read_u32(o.object_vtable.wrapping_add(slot * 4)) else {
        set_status("Refused: vtable slot unreadable");
        return;
    };
    let expected = model::runtime(model::SLOT_VAS[slot as usize], model::SDK_IMAGE_BASE, o.sdk_base);
    if target != expected {
        log(W2R_LOG_WARN, &format!("refusing {}: slot {slot} is {target:#x}, analysed {expected:#x}", call.name()));
        set_status("Refused: slot function differs from the analysis");
        return;
    }
    let before = listeners();
    done.store(true, Ordering::Release);
    log(
        W2R_LOG_INFO,
        &format!("calling {} on {:#x} at {target:#x}; listening before: {before:?}", call.name(), o.sdk_object),
    );
    // SAFETY: the object, its vtable and the target were checked against the analysed layout above;
    // the signatures are the ones the exe's own wrapper uses for these slots.
    let ret = unsafe {
        match call {
            Call::Slot5Port0 => {
                let f: PortFn = core::mem::transmute(target as usize);
                f(o.sdk_object as *mut c_void, 0, 0)
            }
            _ => {
                let f: NoArgFn = core::mem::transmute(target as usize);
                f(o.sdk_object as *mut c_void, 0)
            }
        }
    };
    let after = listeners();
    log(W2R_LOG_INFO, &format!("{} returned {ret:#x}; listening after: {after:?}", call.name()));
    set_status(&format!("{} returned {ret:#x}. Listening: {}", call.name(), ports_text(&after)));
}

unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let Some(ui) = (unsafe { ui.as_ref() }) else {
        return;
    };
    let status = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    unsafe {
        (ui.label)(ui.ctx, s("Research probe for the game ClientSdk local web server."));
        (ui.label)(ui.ctx, s("Check is read only. The others call the SDK once and could crash the game."));
        if !status.is_empty() {
            (ui.label)(ui.ctx, s(&status));
        }
        if (ui.button)(ui.ctx, s("Check (read only)")) != 0 {
            check();
        }
        for (call, text) in [
            (Call::Slot4, "Call slot 4 (returns a bool)"),
            (Call::Slot5Port0, "Call slot 5 with port 0"),
            (Call::Slot6, "Call slot 6 (no arguments)"),
            (Call::Slot17, "Call slot 17 (routes; already tried)"),
        ] {
            if (ui.button)(ui.ctx, s(text)) != 0 {
                run(call);
            }
        }
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    set_status("Press Check.");
    log(W2R_LOG_INFO, "sdk probe loaded; open the plugin list and press Check");
    W2R_OK
}

static TABLE: W2rPlugin = W2rPlugin {
    struct_size: W2R_PLUGIN_V1_SIZE,
    on_load: Some(on_load),
    on_event: None,
    on_ui: Some(on_ui),
    on_unload: None,
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &TABLE
}
