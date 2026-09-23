#!/bin/sh
# Builds codexSwitch and installs it into ~/.local/bin (override with BIN_DIR).
#
#   ./install.sh
#   BIN_DIR=/usr/local/bin ./install.sh
set -eu

cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "install.sh: cargo が見つかりません。https://rustup.rs から Rust を入れてください" >&2
  exit 1
fi

BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

cargo build --release --locked
mkdir -p "$BIN_DIR"
install -m 755 target/release/codexSwitch "$BIN_DIR/codexSwitch"

echo "installed: $BIN_DIR/codexSwitch ($("$BIN_DIR/codexSwitch" --version))"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "注意: $BIN_DIR が PATH に入っていません" >&2 ;;
esac
