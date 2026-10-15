# Instructions for AI coding agents

You are probably helping someone write or change a plugin for Warcraft II Remastered. Read this
file first, then follow the links. A person is responsible for what you produce and will open the
pull request; your job is to make that PR correct, small and easy to review.

## Read, in this order

1. [CONTRIBUTING.md](CONTRIBUTING.md): the workflow rules and the rules for plugins. They are
   binding.
2. [docs/PLUGIN-GUIDE.md](docs/PLUGIN-GUIDE.md): how a plugin is built, with the ABI rules that
   matter, a skeleton, testing, and the mistakes agents make most often.
3. An existing plugin as a pattern: `plugins/w2r-random-map` (UI and settings),
   `plugins/w2r-player-colors` (reads game state), `plugins/w2r-chat` (events and chat).

## Non-negotiable rules

- **Never commit to `main` and never push to it.** Work on a feature branch (`plugin/<id>` for a new
  plugin, `fix/<id>-<what>` for a fix) and finish with a pull request. Do not open the PR or push
  unless the person asked you to.
- **One plugin per PR.** Do not reformat, rename or "tidy" other plugins, the tools, the workflows
  or the root `Cargo.toml` as part of a plugin change.
- **Do not edit** `.github/`, `tools/w2r-publish/`, the `rev` in the root `Cargo.toml`, or any
  other plugin's `plugin.json` or `Cargo.toml`, unless the task is exactly that. Those files are
  maintainer-owned (CODEOWNERS).
- **Least capability.** Start from `"capabilities": []` and add one only when the code calls a host
  function that needs it. Never add `hooks.raw` or `net.send` to make something easier.
- **Source only.** No prebuilt DLLs, no vendored binaries, no downloads at build time.
- **Do not invent the API.** If you need a host function, symbol, event or field, find it in the
  ABI header or docs (see the guide for where). If it is not there, say so and stop; do not guess
  an address, a struct layout or a function name.
- **Game memory is dangerous.** Do not read or write game memory with raw pointers. `game.read` goes
  through `symbol_lookup` and `symbol_read`. Do not guess offsets for a game build; use named
  symbols the host provides, or state clearly what is unverified.
- **Be honest about testing.** You usually cannot run the game. Run what you can (unit tests, the
  32-bit build, `w2r-publish check`) and tell the person plainly what was and was not verified in
  the game. Never claim a UI or behavior works without seeing it.
- **Do not add attribution lines** (such as "Co-Authored-By") to commits or PR text. Disclose AI
  help in the PR template's "AI assistance" section instead.

## Before you say you are done

```bash
cargo run --locked -p w2r-publish -- check
cargo test --locked -p <your-crate>
cargo build --locked --release --target i686-pc-windows-msvc -p <your-crate>
```

Then re-read your diff: version bumped in both `Cargo.toml` and `plugin.json` when a published plugin
changed, README updated, capabilities justified, no stray files (`out/`, `target/`, logs,
`.cargo/config.toml`).
