# Contributing

Thank you for adding to the plugin collection. Writing a plugin with an AI assistant is fine; the
person who opens the pull request is responsible for it. If you use an AI agent, point it at
[AGENTS.md](AGENTS.md) and [docs/PLUGIN-GUIDE.md](docs/PLUGIN-GUIDE.md) first.

## Workflow

1. **Open an issue first** for a new plugin or anything that needs a new host function or a risky
   capability (`hooks.raw`, `net.send`). Say what the plugin does and which capabilities it would
   need. Small fixes to an existing plugin can go straight to a PR.
2. **Work on a branch, never on `main`.** Fork the repository (or, if you have write access, branch
   from the latest `main`). Name the branch `plugin/<id>` for a new plugin or
   `fix/<id>-<what>` / `docs/<what>` for other work. Nobody pushes to `main` directly, maintainers
   included.
3. **One plugin or one concern per pull request.** Do not mix a new plugin with changes to other
   plugins, the tools, the workflows or the pinned w2rpm `rev`. Those go in their own PRs.
4. **Keep commits readable.** A short imperative subject (`Random map: add a folder setting`), a body
   that says why. Do not commit build output, DLLs, `out/`, `target/`, or your local
   `.cargo/config.toml`.
5. **Open a pull request against `main`** and fill in the template. CI must pass. A maintainer
   reviews it; expect questions about every capability. Push fixes to the same branch; do not open a
   new PR for review feedback.
6. **Merging** is done by a maintainer, normally as a squash. Merging a change that touches a plugin
   publishes it: the release workflow builds it and adds it to the index, so a version, once
   merged, is permanent.
7. **Security problems** (a plugin that could harm players, a way around the capability checks): do
   not open a public issue. Use GitHub's private vulnerability reporting (the repository's Security tab).

Reviewers will not merge a plugin they cannot read. Keep the code small, commented where the game
requires a surprising thing, and split logic from the host glue so it can be tested.

### AI-assisted contributions

- Say so in the PR ("AI assistance" in the template). Name the tool if you like; do not add
  tool-attribution lines to commit messages.
- Read and understand what the agent wrote before you open the PR. "The AI wrote it" is not an answer
  to a review question.
- Be honest about testing. If you could not run the plugin in the game, say that. An agent's claim that
  it works is not evidence.
- Agents must not guess game addresses, struct layouts or host functions. If the ABI does not offer
  it, the answer is an issue, not a hook.

# Contributing a plugin

A plugin is a folder `plugins/<id>/` with:

| File | Required | Notes |
|---|---|---|
| `plugin.json` | yes | the manifest, see `docs/manifest.md` in the [w2rpm repo](https://github.com/hastinbe/w2rpm); `id` equals the folder name |
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

The workspace pins `w2r-abi`, `w2r-formats` and `w2rpm-lib` to a w2rpm commit. To work against a
sibling checkout, create the untracked `.cargo/config.toml`:

```toml
[patch."https://github.com/hastinbe/w2rpm"]
w2r-abi = { path = "../w2rpm/crates/w2r-abi" }
w2r-formats = { path = "../w2rpm/crates/w2r-formats" }
w2rpm-lib = { path = "../w2rpm/crates/w2rpm-lib" }
```

When the ABI changes, bump the `rev` in the root `Cargo.toml` in its own PR.

With the patch active, cargo writes a `Cargo.lock` that CI (which has no patch) rejects under
`--locked`. Run `tools/sync-lock.sh` after changing dependencies or the `rev`; it regenerates the
lock with the patch moved aside. To check automatically before each commit, install the hook:

```sh
printf '#!/usr/bin/env bash\ngit diff --cached --name-only | grep -qE "(^|/)Cargo\.(toml|lock)$" && exec tools/sync-lock.sh --check\nexit 0\n' > .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
```
