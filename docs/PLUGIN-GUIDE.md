# Writing a plugin: guide for contributors and their AI agents

A plugin is a 32-bit Windows DLL that Warcraft II Remastered loads through the W2R Mod Loader host. It is
written in Rust here (the ABI is C, so other languages work, but this repository builds Rust).
This guide is the short version of what you need; the normative documents are in the w2r-mod-loader
repository and are linked at the end.

Read [CONTRIBUTING.md](../CONTRIBUTING.md) first for the workflow and the rules. This file is about
how to build the thing.

## 1. What a plugin can and cannot do

- The plugin runs **inside the game process**, on the game's main thread. A crash in your code
  crashes the game. Capabilities are a consent prompt, not a sandbox.
- It talks to the game only through the **host table** (`W2rHost`) it receives in `on_load`: logging,
  its own settings, its own data files, events, UI rows, and (with capabilities) game reads, chat and
  peer messages. There is no other supported way.
- The game build is fixed (`1.0.2.2818` today). Anything that depends on a build must be named in
  `game_builds`; the host refuses to load a plugin on a build it does not list.

| Capability | Lets the plugin | Notes |
|---|---|---|
| (none) | log, own settings, own data files, UI rows in its settings page, lobby/match events | most plugins need nothing else |
| `game.read` | look up named game symbols and read their memory | reads that fault return an error, they do not crash |
| `chat.read` | receive chat lines and player names | names are empty without it |
| `chat.write` | send chat lines | rate limited |
| `net.send` | send and receive plugin messages to other players' plugins | needs two maintainer approvals |
| `hooks.raw` | get raw addresses and install code hooks | shown to users as dangerous; two maintainer approvals |

If you think you need `hooks.raw`, first check whether a host function or event already does it. The
maintainers would rather add a safe host function than review another hook.

## 2. Layout of a plugin

```
plugins/<id>/
  plugin.json     manifest (all twelve fields, no extras)
  Cargo.toml      cdylib + rlib, version equal to plugin.json
  README.md       what it does, settings, capabilities and why
  src/lib.rs      declares the modules
  src/model.rs    pure logic, no host calls, unit tested on any machine
  src/plugin.rs   the thin layer over the host (the C ABI)
```

Copy `plugins/w2r-random-map` and rename. Keep this split: the **model** is ordinary Rust that
takes small traits for what it needs from the host (see `Maps` and `Rows` in
`w2r-random-map/src/model.rs`), so tests use fakes and run on a normal 64-bit `cargo test`. The
**plugin** file is only glue. A plugin that touches game memory or Windows APIs gates its glue with
`#[cfg(target_arch = "x86")]` (see `w2r-countdown-sound/src/lib.rs`) so the workspace still builds
and tests everywhere.

### `plugin.json`

```json
{
  "schema": 1,
  "id": "w2r-example",
  "name": "Example",
  "version": "0.1.0",
  "author": "Your Name",
  "description": "One sentence a player understands.",
  "entry": "w2r_example.dll",
  "abi": 1,
  "game_builds": ["1.0.2.2818"],
  "capabilities": [],
  "affects_simulation": false,
  "dependencies": []
}
```

- `id`: lowercase letters, digits and `-`, 3 to 64 characters, equal to the folder name. Use the
  `w2r-` prefix.
- `entry`: `<crate name with underscores>.dll`, equal to the `[lib] name` in `Cargo.toml`.
- `version`: SemVer, and **equal** to `version` in `Cargo.toml` (CI fails otherwise).
- `capabilities`: only `game.read`, `chat.read`, `chat.write`, `net.send`, `hooks.raw`. Do not
  list `log`, `settings`, `ui` or `fs.data`: they are implicit and listing them is an error.
- `affects_simulation`: `true` only if the plugin changes anything that must be identical on every
  player's machine (unit data, game rules, anything that makes two clients diverge). A plugin that
  only draws or reads is `false`. When unsure, say so in the PR. Players are warned about `true`.
- `abi`: the minimum ABI version needed. Use `1` unless you use a function added later.
- Strings are limited (`name` and `author` 64 bytes, `description` 256). Strict JSON, no comments.

### `Cargo.toml`

```toml
[package]
name = "w2r-example"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish = false
description = "Same idea as the manifest description"

[lib]
name = "w2r_example"
crate-type = ["cdylib", "rlib"]

[dependencies]
w2r-abi.workspace = true

[lints.clippy]
# Callbacks are called by the host; the ABI header documents the contract.
missing_safety_doc = "allow"
```

New workspace members are picked up automatically (`members = ["plugins/*", ...]`). Do not add
dependencies without a reason you can write in the PR; the only one most plugins need is `w2r-abi`.

## 3. The ABI rules that matter

Source of truth: `docs/abi.md` and `crates/w2r-abi` in the w2r-mod-loader repository (section 8 says how to
read them locally). The rules agents break most:

1. **Entry point.** Export exactly one function, `w2r_plugin_main`, returning a pointer to a
   `static W2rPlugin` table (null refuses to load). The host then calls `on_load(host)`; a non-zero
   return fails the load.
2. **Everything runs on the game's main thread.** Host functions called from another thread return
   `W2R_E_STATE`. If you start a thread, it cannot call the host, and you must join it in
   `on_unload` so no plugin code runs after the DLL is gone.
3. **Check `struct_size`.** `W2rHost`, `W2rPlugin` and `W2rUi` only ever grow at the end. Before using a
   field added after the first release, check `host.struct_size >= W2R_HOST_..._SIZE` (and the
   function pointer is non-null) and degrade or return an error with a log line. `random-map` shows
   the pattern.
4. **Pass `host.token` back** as the first argument of every host call.
5. **Never let a panic cross the FFI boundary.** Wrap every callback body in
   `catch_unwind(AssertUnwindSafe(|| ...))`, as the existing plugins do. Do not `unwrap()` a mutex:
   use `lock().unwrap_or_else(|e| e.into_inner())`.
6. **Strings** are `W2rStr { ptr, len }`: UTF-8, length-delimited, **not** NUL-terminated.
   Pointers handed to you in an event or UI call are valid **only during that call**; copy what you
   keep. Player names and chat reach you as printable ASCII only.
7. **Status codes**: `0` is success, failures are negative (`W2R_E_DENIED`, `_INVALID`, `_LIMIT`,
   `_STATE`, `_UNSUPPORTED`, `_NOT_FOUND`, `_RATE`, `_CONFLICT`, `_BUSY`). Check every return. `W2R_E_BUSY`
   means "ask again on a later frame".
8. **Limits** (selected): log lines 512 bytes and 20 per second; settings keys 1 to 64 bytes of
   `[a-z0-9_.]`, at most 128 keys, string values at most 1,024 bytes; data files 64 files and 16 MiB
   in total, names `[A-Za-z0-9_.-]`; chat 200 printable ASCII bytes; UI strings 128 bytes.
   Hitting a limit is an error code, not a crash. The full table is in `docs/abi.md`.

### Skeleton of `plugin.rs`

```rust
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicPtr, Ordering};

use w2r_abi::*;

static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());

fn host() -> Option<&'static W2rHost> {
    unsafe { HOST.load(Ordering::Acquire).as_ref() }
}

fn s(text: &str) -> W2rStr {
    W2rStr { ptr: text.as_ptr().cast(), len: text.len() as u32 }
}

fn log(level: u32, msg: &str) {
    if let Some(h) = host() {
        unsafe { (h.log)(h.token, level, s(msg)) };
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    log(W2R_LOG_INFO, "example loaded");
    W2R_OK
}

unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(ui) = (unsafe { ui.as_ref() }) else { return };
        unsafe { (ui.label)(ui.ctx, s("Hello from the example plugin")) };
    }));
}

unsafe extern "C" fn on_unload() {}

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
```

Check the field names against the `w2r-abi` source for the ABI revision this repository pins before
you rely on this skeleton; it is a starting point, not the contract.

## 4. Common tasks

- **Settings.** `settings_get_i64` / `settings_set_i64` / `_str` store small values under
  `config\<id>.json`. A missing key is `W2R_E_NOT_FOUND`: use a default. Validate whatever you read
  back (a user can edit the file) and clamp it. Writes are immediate.
- **UI.** `on_ui` draws rows in your plugin's page in the game's plugin list: `label`, `button`,
  `checkbox`, `slider_int`, and (newer hosts) `text_edit` and `set_active`. The call returns whether
  the value changed this frame. Anything the player types is untrusted; validate it. Some plugins
  draw on other screens through dedicated registration functions (for example
  `create_ui_register`): those are in `docs/abi.md`.
- **Events.** Set `on_event` and `subscribe` in `on_load` to the ids you want: lobby enter or leave,
  player join or leave, chat received (needs `chat.read`), peer message (needs `net.send`), match
  start and end, settings changed, shutdown. Subscribing to an event whose capability is not
  approved returns `W2R_E_DENIED`: handle it, do not unwrap.
- **Reading game state** (`game.read`). `symbol_lookup(name)` gives a handle valid for the session;
  `symbol_read(handle, offset, dst, len)` copies at most 4,096 bytes and never crashes the game on a
  bad read. Only symbols the host knows by name exist. If the data you need has no symbol, ask in
  an issue; do not hard-code an address (see `w2r-sdk-probe`, a research tool the maintainers
  keep out of the index).
- **Files.** `data_open`, `data_read`, `data_write` and `data_close` work on flat files in your
  plugin's own `data\` folder. You never see a path.

## 5. Testing

```bash
cargo test --locked -p w2r-example              # model tests, any machine
cargo build --locked --release --target i686-pc-windows-msvc -p w2r-example   # the real thing
cargo run --locked -p w2r-publish -- check      # manifests
```

- Install the 32-bit target once: `rustup target add i686-pc-windows-msvc` (needs the Visual
  Studio C++ build tools on Windows).
- Put every decision in the model and test it. A plugin whose only logic lives in `extern "C"`
  callbacks cannot be reviewed or tested and will be sent back.
- CI builds every plugin and runs the unit tests of all of them (`cargo test --workspace`). Run
  `cargo test -p <your-crate>` yourself first and paste the result in the PR.
- To try it in the game: build with `--release --target i686-pc-windows-msvc`, package it with
  `w2r-publish package` (see CONTRIBUTING.md) or copy the DLL and `plugin.json` into
  `plugins\<id>\` under the w2r folder, enable it in the W2R Mod Loader and read
  `logs\w2r.log` (in `Saved Games\Warcraft2Remastered\w2r\`). A plugin that faults is disabled by the
  host; the log says why.

## 6. Pitfalls checklist

- Version in `Cargo.toml` and `plugin.json` differ, or a published plugin changed without a bump.
- `id`, folder name, crate name and `entry` do not agree.
- Listing implicit capabilities, or a capability the code never uses.
- Calling a host function whose capability was not declared (it returns `W2R_E_DENIED`).
- Using `unwrap()`, `expect()`, indexing that can panic, or unguarded arithmetic inside a callback.
- Holding a pointer from a callback after it returns.
- Assuming a newer host function exists without checking `struct_size`.
- Doing real work on every frame (UI callbacks run every frame). Cache, and bail out early.
- Logging every frame (throttled to 20 lines per second; the rest are dropped).
- Marking a plugin `affects_simulation: false` when it changes something both clients must agree on.
- Treating `game_builds` as a formality. Memory layouts differ per build.
- Adding a crate for something `std` does.
- Committing `out/`, `target/`, `.cargo/config.toml`, DLLs, or editor files.

## 7. The README of a plugin

Players see it in the manager. Say what it does in plain words, list every setting and what it
changes, list each capability with the reason, say what game build and what W2R Mod Loader version it needs,
and give credit when you ported code (and include the original `LICENSE`).

## 8. Where the real documentation is

The ABI header, `docs/abi.md`, `docs/manifest.md`, `docs/load-policy.md` and `docs/layout.md` live in
the w2r-mod-loader repository (`hastinbe/w2r-mod-loader`, public). This repository depends on it through Cargo, pinned by
`rev` in the root `Cargo.toml`. After a first `cargo fetch` (or any build), Cargo has checked out
the **whole** repository at that revision on your machine, under
`~/.cargo/git/checkouts/w2r-mod-loader-*/<short rev>/`. Read the documents there; they are guaranteed to
match the ABI you are compiling against:

- `docs/abi.md`: every host function, limit, event and rule.
- `crates/w2r-abi/include/w2r_abi.h` and `crates/w2r-abi/src/lib.rs`: the exact types.
- `docs/manifest.md`: every manifest rule and error code.
- `docs/load-policy.md`: how and when the host loads or refuses a plugin.

If you need something the host does not offer, open an issue describing the plugin and the missing
function. Do not work around it with a hook.
