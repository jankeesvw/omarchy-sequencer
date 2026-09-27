#!/usr/bin/env bash
# Installs Sequencer for your user.
#   From a checkout it builds with cargo; piped from curl it downloads the latest release:
#   curl -fsSL https://raw.githubusercontent.com/jankeesvw/omarchy-sequencer/main/install.sh | bash
set -euo pipefail

name=omarchy-sequencer
repo=jankeesvw/$name

if [[ -f Cargo.toml ]] && grep -q "^name = \"$name\"" Cargo.toml 2>/dev/null; then
  if command -v cargo >/dev/null; then
    cargo build --release
  fi
  binary=target/release/$name
  data=packaging
else
  case $(uname -m) in
    x86_64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) echo "Sequencer has no release for $(uname -m); build it from a checkout with cargo." >&2; exit 1 ;;
  esac
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  echo "Downloading the latest release of Sequencer…"
  curl -fsSL "https://github.com/$repo/releases/latest/download/$name-$arch-linux.tar.gz" | tar xz -C "$tmp"
  dir=$(echo "$tmp/$name"-*)
  binary=$dir/$name
  data=$dir
fi

install -Dm755 "$binary" ~/.local/bin/$name
install -Dm644 "$data/$name.desktop" ~/.local/share/applications/$name.desktop
install -Dm644 "$data/$name.svg" ~/.local/share/icons/hicolor/scalable/apps/$name.svg
update-desktop-database ~/.local/share/applications 2>/dev/null || true
# "Open in Sequencer" on omarchysequencer.com.
xdg-mime default $name.desktop x-scheme-handler/$name 2>/dev/null || true

echo "Sequencer installed. Open it from the launcher (Super + Space), or run '$name'."
