#!/usr/bin/env bash
# Keep Cargo.lock consistent with what CI sees: no .cargo/config.toml patch, git sources for the
# w2rpm crates. Run from anywhere. `sync-lock.sh` regenerates the lock; `sync-lock.sh --check`
# fails if it would change (used by the pre-commit hook).
set -euo pipefail
cd "$(dirname "$0")/.."

cfg=.cargo/config.toml
if [ -z "${RUSTUP_HOME:-}" ] && [ -d /e/rust/rustup ]; then
  export RUSTUP_HOME=E:/rust/rustup CARGO_HOME=E:/rust/cargo
fi

if [ -f "$cfg" ]; then
  mv "$cfg" "$cfg.off"
  trap 'mv "$cfg.off" "$cfg"' EXIT
fi

if [ "${1:-}" = "--check" ]; then
  if ! cargo metadata --locked --format-version 1 >/dev/null 2>&1; then
    echo "Cargo.lock is out of date for CI (probably generated with the local .cargo patch)." >&2
    echo "Run tools/sync-lock.sh and stage Cargo.lock." >&2
    exit 1
  fi
else
  cargo metadata --format-version 1 >/dev/null
  echo "Cargo.lock synced."
fi
