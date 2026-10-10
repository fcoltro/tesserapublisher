#!/bin/bash
# Prepares a Claude Code cloud session to build, lint and test Tessera.
# Local machines are left alone: there the README is the setup guide.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"

# The same GUI headers CI installs on ubuntu-latest. eframe links GTK on
# Linux, so without them `cargo clippy --all-targets` fails in a build script.
if ! dpkg -s libgtk-3-dev libxkbcommon-dev libwayland-dev >/dev/null 2>&1; then
  SUDO=""
  [ "$(id -u)" -ne 0 ] && SUDO="sudo -n"
  $SUDO apt-get update -qq
  DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y -qq \
    libgtk-3-dev libxkbcommon-dev libwayland-dev
fi

# rust-toolchain.toml pins the exact compiler; this installs it if missing.
rustup show active-toolchain >/dev/null

# Fetch crates now so the first build does not stall on the network. Building
# is left to the session: the container is cached after this hook, and a
# warm `target/` is not worth minutes on every start.
cargo fetch --locked --quiet
