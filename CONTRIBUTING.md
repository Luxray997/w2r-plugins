# Contributing a plugin

A plugin is a folder `plugins/<id>/` with:

| File | Required | Notes |
|---|---|---|
| `plugin.json` | yes | the manifest, see `docs/manifest.md` in the [w2rpm repo](https://github.com/hastinbe/w2r-mod-loader); `id` equals the folder name |
| `Cargo.toml` | yes | `cdylib`, `version` equal to the manifest version, `w2r-abi.workspace = true` |
| `src/` | yes | the source; everything in the DLL must be built from this repo |
| `README.md` | recommended | packaged and shown to users |
| `LICENSE` | if you use others' code | packaged; keep the original notices |

Start from an existing plugin, such as `plugins/w2r-random-map` (no capabilities needed).

## Rules

- **The source is the plugin.** CI builds the DLL from your PR; prebuilt binaries are never accepted.
- **Ask for the least capability that works.** The manifest lists `game.read`, `chat.read`,
  `chat.write`, `net.send`, `hooks.raw`. Every one is shown to users at install and explained in the
  PR description.
- **`hooks.raw` and `net.send`** (the plugin can run code inside the game or send over the
  network): two maintainer approvals, a written explanation of every hook or message, and no
  vendored binary blobs.
- **`affects_simulation: true`** only if the plugin changes anything that must match between
  players; it is flagged to users and in the lobby.
- **Dependencies:** Cargo dependencies must be small, pinned by `Cargo.lock`, and justified in the
  PR. No build scripts that download anything.
- **Versions are immutable.** A published `(id, version)` never changes. Bump `version` in
  `Cargo.toml` and `plugin.json` for any change that should ship; `w2r-publish check` fails if they
  differ.
- Credit original authors in `author` and `LICENSE` when you port code.

## Local checks

```bash
cargo run -p w2r-publish -- check
cargo build --release --target i686-pc-windows-msvc -p <crate>
cargo run -p w2r-publish -- package --target-dir target/i686-pc-windows-msvc/release --out out/packages
```

## Building against a local w2rpm checkout

The workspace pins `w2r-abi`, `w2r-formats` and `w2r-loader-lib` to a w2rpm commit. To work against a
sibling checkout, create the untracked `.cargo/config.toml`:

```toml
[patch."https://github.com/hastinbe/w2r-mod-loader"]
w2r-abi = { path = "../w2rpm/crates/w2r-abi" }
w2r-formats = { path = "../w2rpm/crates/w2r-formats" }
w2r-loader-lib = { path = "../w2rpm/crates/w2r-loader-lib" }
```

When the ABI changes, bump the `rev` in the root `Cargo.toml` in its own PR.

With the patch active, cargo writes a `Cargo.lock` that CI (which has no patch) rejects under
`--locked`. Run `tools/sync-lock.sh` after changing dependencies or the `rev`; it regenerates the
lock with the patch moved aside. To check automatically before each commit, install the hook:

```sh
printf '#!/usr/bin/env bash\ngit diff --cached --name-only | grep -qE "(^|/)Cargo\.(toml|lock)$" && exec tools/sync-lock.sh --check\nexit 0\n' > .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
```
