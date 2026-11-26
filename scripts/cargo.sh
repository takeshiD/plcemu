#!/bin/sh
# Runs cargo with a usable toolchain even in a fresh environment (empty HOME, rustup without default).
# Order: working `cargo` on PATH -> rustup installs minimal stable into $HOME -> fail with guidance.
set -eu
if cargo --version >/dev/null 2>&1; then exec cargo "$@"; fi
if command -v rustup >/dev/null 2>&1; then
  rustup toolchain install stable --profile minimal -c rustfmt -c clippy >&2
  rustup default stable >&2
  exec cargo "$@"
fi
echo "cargo not found: install Rust (https://rustup.rs)" >&2
exit 127
