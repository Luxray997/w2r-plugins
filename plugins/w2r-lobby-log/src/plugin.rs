//! The game-facing half (32-bit only). "Start logging" swaps 45 vtable slots in the exe (the
//! wrapper the game uses to call Battle.net's game master service, and the handler that receives
//! the service's pushes) for stubs (`stubs.rs`) that call [`log_entry`] and then jump to the
//! original function. Lines collect in memory and are written to `lobby-log.txt` in the plugin's
//! data folder at lobby, match and shutdown events or when the button is pressed. "Stop" puts the
//! original pointers back. Nothing is sent anywhere.
//!
//! `log_entry` runs on whatever thread the game calls from, outside the host's guard: it only
//! reads memory (with `ReadProcessMemory`, so a bad pointer is a miss, not a crash), formats a
//! line and pushes it under a mutex. It never calls a host function.

use core::ffi::c_void;
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use w2r_abi::*;

use crate::model::{self, Dir, ENTRIES};
use crate::stubs;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn ReadProcessMemory(
        process: *mut c_void,
        base: *const c_void,
        buf: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    fn GetLastError() -> u32;
}

const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const MAX_LINES: usize = 6000;
const FILE_NAME: &str = "lobby-log.txt";

static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
/// The original function of each patched slot, indexed like [`ENTRIES`]. The stubs jump through it.
pub static ORIG: [AtomicU32; model::STATIC_COUNT + model::DYN_MAX] =
    [const { AtomicU32::new(0) }; model::STATIC_COUNT + model::DYN_MAX];
/// Lines written so far per patched slot, for the per-slot limit.
static COUNTS: [AtomicU32; model::STATIC_COUNT + model::DYN_MAX] =
    [const { AtomicU32::new(0) }; model::STATIC_COUNT + model::DYN_MAX];

/// A slot found at run time in one of the other service wrappers.
#[derive(Clone)]
struct DynEntry {
    at: u32,
    svc: &'static str,
    slot: u32,
}
static DYN: Mutex<Vec<DynEntry>> = Mutex::new(Vec::new());
/// Win32 error of the last failed [`poke`], for the log.
static LAST_POKE_ERROR: AtomicU32 = AtomicU32::new(0);
/// Lobbies the service has told us about (kept while logging is on).
static GAMES: Mutex<model::GameTable> = Mutex::new(model::GameTable::new());
/// Stub id of each patched slot of the SDK game master object, `u32::MAX` if unknown.
static SDK_STUB: [AtomicU32; 20] = [const { AtomicU32::new(u32::MAX) }; 20];
/// The original function of SDK slot 9 (stop finding), so it can be put back.
static SDK_STOP_ORIG: AtomicU32 = AtomicU32::new(0);
/// A copy of the game's own search-info object from its first find request, if seen.
static SNAPSHOT: Mutex<Option<[u8; model::SEARCH_INFO_SIZE]>> = Mutex::new(None);
/// Milliseconds (since logging started) of the last lobby event, 0 if none.
static LAST_FEED_MS: AtomicU32 = AtomicU32::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static FILE: AtomicU32 = AtomicU32::new(0);
static START: OnceLock<Instant> = OnceLock::new();
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static DROPPED: AtomicU32 = AtomicU32::new(0);
static WRITTEN: AtomicU32 = AtomicU32::new(0);
static STATUS: Mutex<String> = Mutex::new(String::new());
static AUTO_START: AtomicBool = AtomicBool::new(false);
static EXE_BASE: AtomicU32 = AtomicU32::new(0);
static SDK_BASE: AtomicU32 = AtomicU32::new(0);

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
        let m: String = msg.chars().take(500).collect();
        unsafe { (h.log)(h.token, level, s(&m)) };
    }
}

fn set_status(text: &str) {
    let t: String = text.chars().take(110).collect();
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = t;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn read_bytes(addr: u32, buf: &mut [u8]) -> bool {
    if addr < 0x1_0000 {
        return false;
    }
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as usize as *const c_void,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut got,
        )
    };
    ok != 0 && got == buf.len()
}

fn rd(addr: u32) -> Option<u32> {
    let mut b = [0u8; 4];
    read_bytes(addr, &mut b).then(|| u32::from_le_bytes(b))
}

/// A NUL-terminated string of printable text at `addr`, at most 96 bytes, or `None`.
fn cstr(addr: u32) -> Option<String> {
    let mut out = Vec::new();
    for chunk in 0..12u32 {
        let mut b = [0u8; 8];
        if !read_bytes(addr.wrapping_add(chunk * 8), &mut b) {
            break;
        }
        for &c in &b {
            if c == 0 {
                return Some(model::clean(&out));
            }
            out.push(c);
        }
    }
    None
}

fn in_sdk(addr: u32) -> bool {
    let base = SDK_BASE.load(Ordering::Relaxed);
    base != 0 && addr >= base && addr < base.wrapping_add(0x0100_0000)
}

type Get32 = unsafe extern "fastcall" fn(*mut c_void, usize) -> u32;
type Get64 = unsafe extern "fastcall" fn(*mut c_void, usize) -> u64;

/// The SDK function at `slot_off` of `obj`'s vtable, if the object and function look right.
fn sdk_fn(obj: u32, slot_off: u32) -> Option<usize> {
    let vt = rd(obj)?;
    if !in_sdk(vt) {
        return None;
    }
    let f = rd(vt.wrapping_add(slot_off))?;
    in_sdk(f).then_some(f as usize)
}

/// A decoded `IGameInfoV1`: id, name and the attribute list.
struct Decoded {
    id: u64,
    name: String,
    attrs: Vec<(String, String)>,
}

/// Decodes an `IGameInfoV1*` the same way the game's own handler does: id (+4), name (+0x10),
/// attribute count (+0x34), attribute array (+0x38), each attribute's key (+4) and value (+8).
fn decode_game(obj: u32) -> Option<Decoded> {
    let f_id = sdk_fn(obj, 0x04)?;
    let f_name = sdk_fn(obj, 0x10)?;
    let f_count = sdk_fn(obj, 0x34)?;
    let f_attrs = sdk_fn(obj, 0x38)?;
    let this = obj as usize as *mut c_void;
    // SAFETY: these are the getters the game's own GameFound handler calls on this object.
    let (id, name, count, attrs) = unsafe {
        let id = core::mem::transmute::<usize, Get64>(f_id)(this, 0);
        let name = core::mem::transmute::<usize, Get32>(f_name)(this, 0);
        let count = core::mem::transmute::<usize, Get32>(f_count)(this, 0) & 0xffff;
        let attrs = core::mem::transmute::<usize, Get32>(f_attrs)(this, 0);
        (id, name, count, attrs)
    };
    let mut out = Decoded {
        id,
        name: cstr(name).unwrap_or_default(),
        attrs: Vec::new(),
    };
    for i in 0..count.min(48) {
        let Some(a) = rd(attrs.wrapping_add(i * 4)) else { break };
        let (Some(fk), Some(fv)) = (sdk_fn(a, 0x04), sdk_fn(a, 0x08)) else { break };
        let (k, v) = unsafe {
            (
                core::mem::transmute::<usize, Get32>(fk)(a as usize as *mut c_void, 0),
                core::mem::transmute::<usize, Get32>(fv)(a as usize as *mut c_void, 0),
            )
        };
        out.attrs.push((cstr(k).unwrap_or_default(), cstr(v).unwrap_or_default()));
    }
    Some(out)
}

fn describe_game(g: &Decoded) -> String {
    let mut s = format!("game id={:#x} name={:?}", g.id, g.name);
    for (k, v) in &g.attrs {
        s.push_str(&format!(" {k}={v}"));
    }
    s
}

/// Keeps the open-games table current from the service's pushes. Runs for every call, before the
/// per-slot log limit, so the table is right even when the log has stopped taking lines.
fn feed(id: usize, args: *const u32) {
    let arg = |i: u32| rd((args as u32).wrapping_add(i * 4)).unwrap_or(0);
    if id == model::GAME_FOUND {
        for i in 0..3 {
            if let Some(g) = decode_game(arg(i)) {
                let game = model::open_game_from(g.id, &g.name, &g.attrs, elapsed_ms());
                if let Ok(mut t) = GAMES.try_lock() {
                    t.upsert(game);
                }
                LAST_FEED_MS.store(elapsed_ms() as u32, Ordering::Relaxed);
                break;
            }
        }
    } else if id == model::GAME_REMOVED {
        let gid = arg(2) as u64 | (arg(3) as u64) << 32;
        if let Ok(mut t) = GAMES.try_lock() {
            t.remove(gid);
        }
        LAST_FEED_MS.store(elapsed_ms() as u32, Ordering::Relaxed);
    } else if id == SDK_STUB[model::SDK_FIND_SLOT].load(Ordering::Relaxed) as usize {
        // The game started a new find: the service answers with the full current list.
        if let Ok(mut t) = GAMES.try_lock() {
            t.clear();
        }
        let info = arg(2);
        if rd(info) == Some(model::runtime(model::SEARCH_INFO_VTABLE_VA, model::EXE_IMAGE_BASE, EXE_BASE.load(Ordering::Relaxed))) {
            let mut buf = [0u8; model::SEARCH_INFO_SIZE];
            if read_bytes(info, &mut buf)
                && let Ok(mut s) = SNAPSHOT.try_lock()
            {
                *s = Some(buf);
            }
        }
    }
}

/// A short description of an argument that might be a pointer: first dwords and a string if any.
fn describe_arg(i: usize, a: u32) -> Option<String> {
    let mut b = [0u8; 24];
    if !read_bytes(a, &mut b) {
        return None;
    }
    let words: Vec<String> = b
        .chunks_exact(4)
        .map(|c| format!("{:#x}", u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
        .collect();
    let mut note = format!("a{i}->[{}]", words.join(","));
    if let Some(t) = cstr(a) {
        note.push_str(&format!(" {t:?}"));
    }
    Some(note)
}

fn push_line(line: String) {
    let mut v = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if v.len() >= MAX_LINES {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    v.push(line);
}

fn elapsed_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Called by every stub, on the game's thread.
pub extern "C" fn log_entry(id: u32, this: u32, args: *const u32) {
    let _ = catch_unwind(|| {
        if !ACTIVE.load(Ordering::Acquire) {
            return;
        }
        let id = id as usize;
        if id >= COUNTS.len() {
            return;
        }
        feed(id, args);
        let n = COUNTS[id].fetch_add(1, Ordering::Relaxed);
        if n > model::LINES_PER_SLOT {
            return;
        }
        let (name, dir, nargs) = if let Some(e) = ENTRIES.get(id) {
            (e.name.to_string(), e.dir, e.nargs)
        } else {
            let Ok(d) = DYN.try_lock() else { return };
            let Some(e) = d.get(id - model::STATIC_COUNT) else { return };
            (format!("{}.slot{}", e.svc, e.slot), Dir::Out, 5)
        };
        let mut a = Vec::with_capacity(nargs as usize);
        for i in 0..nargs {
            a.push(rd((args as u32).wrapping_add(i * 4)).unwrap_or(0));
        }
        let mut notes = Vec::new();
        if id == model::GAME_FOUND {
            for &x in &a {
                if let Some(g) = decode_game(x) {
                    notes.push(describe_game(&g));
                    break;
                }
            }
        }
        if notes.is_empty() && dir != Dir::Lan {
            for (i, &x) in a.iter().enumerate() {
                if x >= 0x1_0000
                    && let Some(n) = describe_arg(i, x)
                {
                    notes.push(n);
                }
            }
        }
        if n == model::LINES_PER_SLOT {
            notes.push("(further calls of this slot are not logged)".into());
        }
        push_line(model::format_call(elapsed_ms(), dir, &name, this, &a, &notes));
    });
}

fn exe_base() -> u32 {
    unsafe { GetModuleHandleW(core::ptr::null()) as usize as u32 }
}

/// Swaps in the stubs. Refuses unless every slot holds the function the analysis expects.
fn install() -> Result<usize, String> {
    if ACTIVE.load(Ordering::Acquire) {
        return Ok(0);
    }
    let exe = exe_base();
    let sdk = unsafe { GetModuleHandleW(wide("ClientSdk.dll").as_ptr()) } as usize as u32;
    if exe == 0 || sdk == 0 {
        return Err("game module or ClientSdk.dll not found".into());
    }
    EXE_BASE.store(exe, Ordering::Relaxed);
    SDK_BASE.store(sdk, Ordering::Relaxed);
    let wrapper = model::runtime(model::CONTAINER_VA, model::EXE_IMAGE_BASE, exe) + model::INTERNET_WRAPPER_OFFSET;
    let want = model::runtime(model::WRAPPER_VTABLE_VA, model::EXE_IMAGE_BASE, exe);
    match rd(wrapper) {
        Some(v) if v == want => {}
        Some(v) => return Err(format!("game master wrapper vtable is {v:#x}, expected {want:#x} (not initialised yet, or another build)")),
        None => return Err("cannot read the service wrapper".into()),
    }
    for (i, e) in ENTRIES.iter().enumerate() {
        let at = model::slot_address(e, exe);
        let expect = model::runtime(e.fn_va, model::EXE_IMAGE_BASE, exe);
        match rd(at) {
            Some(v) if v == expect => {}
            Some(v) => return Err(format!("slot {} ({}) holds {v:#x}, expected {expect:#x}", i, e.name)),
            None => return Err(format!("slot {} ({}) unreadable", i, e.name)),
        }
    }
    START.get_or_init(Instant::now);
    ACTIVE.store(true, Ordering::Release);
    let mut done = 0;
    for (i, e) in ENTRIES.iter().enumerate() {
        let at = model::slot_address(e, exe);
        let orig = rd(at).unwrap_or(0);
        ORIG[i].store(orig, Ordering::Release);
        if !poke(at, stubs::stub(i) as u32) {
            // Put back what was changed so far and stop.
            ACTIVE.store(false, Ordering::Release);
            restore_range(exe, i);
            return Err(format!("could not write slot {} ({})", i, e.name));
        }
        done += 1;
    }
    done += patch_services(exe);
    done += patch_sdk(exe, sdk);
    Ok(done)
}

/// The SDK refuses writes to its own vtable memory (the page is execute-read and `VirtualProtect`
/// fails), so its game master object is redirected instead: a copy of the vtable lives here, with
/// the slots pointing at stubs, and the heap object's vtable pointer is switched to the copy.
/// The copy is never freed: a call that is already inside a stub may still return through it.
static SDK_COPY: [AtomicU32; 21] = [const { AtomicU32::new(0) }; 21];
/// `(object, original vtable pointer)` while the redirect is in place.
static SDK_REDIRECT: Mutex<Option<(u32, u32)>> = Mutex::new(None);

fn patch_sdk(exe: u32, sdk: u32) -> usize {
    let vt = model::runtime(model::SDK_INET_VTABLE_VA, model::SDK_IMAGE_BASE, sdk);
    let wrapper = model::runtime(model::CONTAINER_VA, model::EXE_IMAGE_BASE, exe) + model::INTERNET_WRAPPER_OFFSET;
    let Some(obj) = rd(wrapper + 4) else { return 0 };
    if rd(obj) != Some(vt) {
        log(W2R_LOG_WARN, &format!("lobby log: SDK object {obj:#x} has vtable {:?}, expected {vt:#x}; not redirected", rd(obj)));
        return 0;
    }
    for (i, &fn_va) in model::SDK_INET_SLOTS.iter().enumerate() {
        let slot = i as u32 + 1;
        let expect = model::runtime(fn_va, model::SDK_IMAGE_BASE, sdk);
        if rd(vt + slot * 4) != Some(expect) {
            log(W2R_LOG_WARN, &format!("lobby log: SDK slot {slot} differs; SDK object not redirected"));
            return 0;
        }
    }
    let mut dyn_entries = DYN.lock().unwrap_or_else(|e| e.into_inner());
    // Slot -1 (the type information pointer) through slot 19.
    for (k, cell) in SDK_COPY.iter().enumerate() {
        cell.store(rd(vt - 4 + k as u32 * 4).unwrap_or(0), Ordering::Release);
    }
    let mut done = 0;
    for (i, _) in model::SDK_INET_SLOTS.iter().enumerate() {
        let slot = i + 1;
        let n = model::STATIC_COUNT + dyn_entries.len();
        if n >= model::STATIC_COUNT + model::DYN_MAX {
            log(W2R_LOG_WARN, "lobby log: stub pool full");
            break;
        }
        let at = &SDK_COPY[slot + 1] as *const AtomicU32 as u32;
        ORIG[n].store(SDK_COPY[slot + 1].load(Ordering::Acquire), Ordering::Release);
        SDK_COPY[slot + 1].store(stubs::stub(n) as u32, Ordering::Release);
        if slot < SDK_STUB.len() {
            SDK_STUB[slot].store(n as u32, Ordering::Release);
        }
        if slot == model::SDK_STOP_FIND_SLOT {
            SDK_STOP_ORIG.store(ORIG[n].load(Ordering::Acquire), Ordering::Release);
        }
        dyn_entries.push(DynEntry { at, svc: "sdkInet", slot: slot as u32 });
        done += 1;
    }
    let copy_vt = &SDK_COPY[1] as *const AtomicU32 as u32;
    // SAFETY: the object is a live heap object whose first word is its vtable pointer (checked above).
    let ok = unsafe {
        core::ptr::write_volatile(obj as usize as *mut u32, copy_vt);
        core::ptr::read_volatile(obj as usize as *const u32) == copy_vt
    };
    if !ok {
        return 0;
    }
    *SDK_REDIRECT.lock().unwrap_or_else(|e| e.into_inner()) = Some((obj, vt));
    log(W2R_LOG_INFO, &format!("lobby log: SDK game master object {obj:#x} redirected to a logging vtable ({done} slots)"));
    done
}

/// Finds the other service wrappers in the container, walks their vtables and patches every code
/// slot. Anything that does not look like `{vftable -> code pointers}` is skipped.
fn patch_services(exe: u32) -> usize {
    let container = model::runtime(model::CONTAINER_VA, model::EXE_IMAGE_BASE, exe);
    let to_va = |p: u32| p.wrapping_sub(exe).wrapping_add(model::EXE_IMAGE_BASE);
    let mut dyn_entries = DYN.lock().unwrap_or_else(|e| e.into_inner());
    dyn_entries.clear();
    let mut done = 0;
    for (off, svc) in model::SERVICES {
        let Some(vt) = rd(container + off) else { continue };
        // A vtable lies in the exe image, not in code, and its first slot is code.
        let first = rd(vt).map(to_va);
        if !(model::RDATA_START_VA..model::RDATA_END_VA).contains(&to_va(vt)) || !first.is_some_and(model::is_code) {
            log(W2R_LOG_INFO, &format!("lobby log: {svc} (+{off:#x}) has no vtable: {vt:#x}"));
            continue;
        }
        for (slot, f) in model::walk_vtable(vt, rd, to_va) {
            let n = model::STATIC_COUNT + dyn_entries.len();
            if n >= model::STATIC_COUNT + model::DYN_MAX {
                log(W2R_LOG_WARN, "lobby log: stub pool full");
                return done;
            }
            let at = vt + slot * 4;
            ORIG[n].store(f, Ordering::Release);
            if poke(at, stubs::stub(n) as u32) {
                dyn_entries.push(DynEntry { at, svc, slot });
                done += 1;
            }
        }
        log(W2R_LOG_INFO, &format!("lobby log: {svc} (+{off:#x}) vtable {vt:#x}"));
    }
    done
}

/// Writes one pointer into read-only data. The page is made writable for the write (read-write,
/// or read-write-execute for pages the SDK maps as code) and put back afterwards.
fn poke(at: u32, value: u32) -> bool {
    for prot in [PAGE_READWRITE, PAGE_EXECUTE_READWRITE] {
        let mut old = 0u32;
        unsafe {
            if VirtualProtect(at as usize as *mut c_void, 4, prot, &mut old) == 0 {
                LAST_POKE_ERROR.store(GetLastError(), Ordering::Relaxed);
                continue;
            }
            core::ptr::write_volatile(at as usize as *mut u32, value);
            let mut tmp = 0u32;
            VirtualProtect(at as usize as *mut c_void, 4, old, &mut tmp);
        }
        return true;
    }
    false
}

fn restore_range(exe: u32, upto: usize) {
    for (i, e) in ENTRIES.iter().enumerate().take(upto) {
        let at = model::slot_address(e, exe);
        let orig = ORIG[i].load(Ordering::Acquire);
        if orig != 0 && rd(at) == Some(stubs::stub(i) as u32) {
            poke(at, orig);
        }
    }
}

/// Puts the original pointers back. A call already inside a stub still finishes through `ORIG`,
/// which is never cleared.
fn uninstall() {
    if !ACTIVE.swap(false, Ordering::AcqRel) {
        return;
    }
    restore_range(EXE_BASE.load(Ordering::Relaxed), ENTRIES.len());
    if let Some((obj, vt)) = SDK_REDIRECT.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let copy_vt = &SDK_COPY[1] as *const AtomicU32 as u32;
        if rd(obj) == Some(copy_vt) {
            // SAFETY: still the live object we redirected; this puts its own vtable pointer back.
            unsafe { core::ptr::write_volatile(obj as usize as *mut u32, vt) };
        }
    }
    let d = DYN.lock().unwrap_or_else(|e| e.into_inner());
    for (k, e) in d.iter().enumerate() {
        let n = model::STATIC_COUNT + k;
        let orig = ORIG[n].load(Ordering::Acquire);
        if orig != 0 && rd(e.at) == Some(stubs::stub(n) as u32) {
            poke(e.at, orig);
        }
    }
}

fn open_file() {
    if FILE.load(Ordering::Acquire) != 0 {
        return;
    }
    let Some(h) = host() else { return };
    let mut handle: W2rHandle = 0;
    let st = unsafe { (h.data_open)(h.token, s(FILE_NAME), W2R_FILE_WRITE, &mut handle) };
    if st == W2R_OK {
        FILE.store(handle, Ordering::Release);
    } else {
        log(W2R_LOG_WARN, &format!("lobby log: cannot open {FILE_NAME}: status {st}"));
    }
}

/// Main thread only. Appends the collected lines to the data file and the host log summary.
fn flush(why: &str) {
    let lines: Vec<String> = std::mem::take(&mut *LINES.lock().unwrap_or_else(|e| e.into_inner()));
    let Some(h) = host() else { return };
    if lines.is_empty() {
        return;
    }
    open_file();
    let file = FILE.load(Ordering::Acquire);
    if file == 0 {
        // Keep them for a later attempt.
        let mut v = LINES.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = lines;
        all.append(&mut v);
        *v = all;
        return;
    }
    let mut text = format!("# {why}\n");
    for l in &lines {
        text.push_str(l);
        text.push('\n');
    }
    let dropped = DROPPED.swap(0, Ordering::Relaxed);
    if dropped > 0 {
        text.push_str(&format!("# {dropped} lines dropped (buffer full)\n"));
    }
    for chunk in text.as_bytes().chunks(60_000) {
        let st = unsafe { (h.data_write)(h.token, file, chunk.as_ptr().cast(), chunk.len() as u32) };
        if st != W2R_OK {
            log(W2R_LOG_WARN, &format!("lobby log: write failed: status {st}"));
            break;
        }
    }
    WRITTEN.fetch_add(lines.len() as u32, Ordering::Relaxed);
    log(W2R_LOG_INFO, &format!("lobby log: wrote {} lines to {FILE_NAME} ({why})", lines.len()));
}

fn marker(text: &str) {
    push_line(format!("{:>8} == {text}", elapsed_ms()));
}

fn try_start() {
    match install() {
        Ok(0) => {}
        Ok(n) => {
            log(W2R_LOG_INFO, &format!("lobby log: {n} slots patched"));
            marker("logging started");
            set_status(&format!("Logging ({n} slots). Written to {FILE_NAME} at lobby/match events."));
        }
        Err(why) => {
            log(W2R_LOG_WARN, &format!("lobby log: not started: {why}"));
            set_status(&format!("Not started: {why}"));
        }
    }
}

fn setting_i64(key: &str) -> Option<i64> {
    let h = host()?;
    let mut v = 0i64;
    (unsafe { (h.settings_get_i64)(h.token, s(key), &mut v) } == W2R_OK).then_some(v)
}

unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let Some(ui) = (unsafe { ui.as_ref() }) else { return };
    // Retry a start that came too early in the game's start-up.
    if AUTO_START.load(Ordering::Relaxed) && !ACTIVE.load(Ordering::Acquire) {
        try_start();
    }
    let status = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let pending = LINES.lock().unwrap_or_else(|e| e.into_inner()).len();
    unsafe {
        (ui.label)(ui.ctx, s("Research: records the game's lobby-list traffic with Battle.net."));
        if !status.is_empty() {
            (ui.label)(ui.ctx, s(&status));
        }
        (ui.label)(
            ui.ctx,
            s(&format!("{} lines waiting, {} written", pending, WRITTEN.load(Ordering::Relaxed))),
        );
        if ACTIVE.load(Ordering::Acquire) {
            if (ui.button)(ui.ctx, s("Stop logging (restore the game)")) != 0 {
                uninstall();
                marker("logging stopped");
                set_status("Stopped; original code restored.");
            }
        } else if (ui.button)(ui.ctx, s("Start logging")) != 0 {
            try_start();
        }
        if (ui.button)(ui.ctx, s("Write the log file now")) != 0 {
            flush("button");
        }
        let mut auto = AUTO_START.load(Ordering::Relaxed) as i32;
        if (ui.checkbox)(ui.ctx, s("Start logging when the game starts"), &mut auto) != 0 {
            AUTO_START.store(auto != 0, Ordering::Relaxed);
            if let Some(h) = host() {
                (h.settings_set_i64)(h.token, s("auto_start"), auto as i64);
            }
        }
    }
}

unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let Some(ev) = (unsafe { event.as_ref() }) else { return };
    let name = match ev.id {
        W2R_EVT_LOBBY_ENTER => "lobby enter",
        W2R_EVT_LOBBY_LEAVE => "lobby leave",
        W2R_EVT_MATCH_START => "match start",
        W2R_EVT_MATCH_END => "match end",
        W2R_EVT_SHUTDOWN => "shutdown",
        _ => return,
    };
    if ev.id == W2R_EVT_SHUTDOWN {
        uninstall();
        flush(name);
        let file = FILE.swap(0, Ordering::AcqRel);
        if file != 0
            && let Some(h) = host()
        {
            unsafe { (h.data_close)(h.token, file) };
        }
        return;
    }
    if ACTIVE.load(Ordering::Acquire) {
        marker(name);
        flush(name);
    } else if AUTO_START.load(Ordering::Relaxed) {
        try_start();
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    for ev in [
        W2R_EVT_LOBBY_ENTER,
        W2R_EVT_LOBBY_LEAVE,
        W2R_EVT_MATCH_START,
        W2R_EVT_MATCH_END,
        W2R_EVT_SHUTDOWN,
    ] {
        unsafe { ((*host).subscribe)((*host).token, ev) };
    }
    AUTO_START.store(setting_i64("auto_start").unwrap_or(0) != 0, Ordering::Relaxed);
    set_status("Not logging. Press Start logging, or tick the box to start with the game.");
    log(W2R_LOG_INFO, "lobby log loaded");
    if AUTO_START.load(Ordering::Relaxed) {
        try_start();
    }
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    uninstall();
    flush("unload");
    let file = FILE.swap(0, Ordering::AcqRel);
    if file != 0
        && let Some(h) = host()
    {
        unsafe { (h.data_close)(h.token, file) };
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
