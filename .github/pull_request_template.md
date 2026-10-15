## What and why

<!-- One or two sentences: what the plugin or change does and why it is worth shipping. -->

## Checklist

- [ ] This PR is from a feature branch, not `main`, and touches one plugin (or one concern).
- [ ] `plugin.json` is valid, `id` equals the folder name, and `version` equals the one in `Cargo.toml`
      (`cargo run -p w2r-publish -- check` passes).
- [ ] Version bumped if this changes a plugin that is already published.
- [ ] `cargo test -p <crate>` passes and the 32-bit release build succeeds.
- [ ] Capabilities are the least that work; each one is explained below.
- [ ] `affects_simulation` is correct (see CONTRIBUTING.md).
- [ ] `README.md` says what the plugin does, its settings, and why it needs each capability.
- [ ] No prebuilt binaries, no vendored blobs, no build scripts that download anything.
- [ ] I tested it in the game (say how below), or I say clearly that I could not.

## Capabilities and why

<!-- One line per capability. Write "none" if the list is empty. Explain every hook or message if you use hooks.raw or net.send. -->

## How it was tested

<!-- Unit tests, and what you saw in the game. Paste the relevant lines of w2r.log if useful. -->

## AI assistance

<!-- If an LLM wrote or helped with this code, say so. You are still responsible for reading and understanding it. -->
