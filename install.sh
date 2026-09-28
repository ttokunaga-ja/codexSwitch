#!/bin/sh
# Installs the latest codexSwitch release into ~/.local/bin (override with BIN_DIR).
#
#   curl -fsSL https://raw.githubusercontent.com/ttokunaga-ja/codexSwitch/main/install.sh | sh
#
# After this, `codexSwitch update` keeps it up to date.
set -eu

REPO=ttokunaga-ja/codexSwitch
ASSET=codexSwitch-macos
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

if [ "$(uname -s)" != Darwin ]; then
  echo "install.sh: macOS 用です。Windows は README の手順を見てください" >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
base="https://github.com/$REPO/releases/latest/download"
curl -fsSL -o "$tmp/$ASSET" "$base/$ASSET"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"
if ! (cd "$tmp" && grep " \*\{0,1\}$ASSET\$" SHA256SUMS | shasum -a 256 -c - >/dev/null); then
  echo "install.sh: ダウンロードしたファイルのハッシュが一致しません" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
chmod 755 "$tmp/$ASSET"
mv -f "$tmp/$ASSET" "$BIN_DIR/codexSwitch"

echo "installed: $BIN_DIR/codexSwitch ($("$BIN_DIR/codexSwitch" --version))"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "注意: $BIN_DIR が PATH に入っていません。~/.zshrc に次の行を足してください: export PATH=\"$BIN_DIR:\$PATH\"" >&2 ;;
esac
