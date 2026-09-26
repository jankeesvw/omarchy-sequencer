#!/usr/bin/env bash
# Installs Sequencer for your user.
#   From a checkout it builds with cargo; piped from curl it downloads the latest release:
#   curl -fsSL https://raw.githubusercontent.com/jankeesvw/sequencer/main/install.sh | bash
set -euo pipefail

repo=jankeesvw/sequencer

if [[ -f Cargo.toml ]] && grep -q '^name = "sequencer"' Cargo.toml 2>/dev/null; then
  if command -v cargo >/dev/null; then
    cargo build --release
  fi
  binary=target/release/sequencer
  data=packaging
else
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  echo "Downloading the latest release of Sequencer…"
  curl -fsSL "https://github.com/$repo/releases/latest/download/sequencer-x86_64-linux.tar.gz" | tar xz -C "$tmp"
  dir=$(echo "$tmp"/sequencer-*)
  binary=$dir/sequencer
  data=$dir
fi

install -Dm755 "$binary" ~/.local/bin/sequencer
install -Dm644 "$data/sequencer.desktop" ~/.local/share/applications/sequencer.desktop
install -Dm644 "$data/sequencer.svg" ~/.local/share/icons/hicolor/scalable/apps/sequencer.svg
mkdir -p ~/.local/share/sequencer/samples
update-desktop-database ~/.local/share/applications 2>/dev/null || true

echo "Sequencer installed. Open it from the launcher (Super + Space), or run 'sequencer'."
