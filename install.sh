#!/usr/bin/env bash
# Installs Sequencer as a regular app, so it shows up in the Omarchy launcher (Super + Space).
set -euo pipefail
cd "$(dirname "$0")"

if [[ ! -x target/release/sequencer ]] || command -v cargo >/dev/null; then
  cargo build --release
fi

install -Dm755 target/release/sequencer ~/.local/bin/sequencer
install -Dm644 packaging/sequencer.desktop ~/.local/share/applications/sequencer.desktop
install -Dm644 packaging/sequencer.svg ~/.local/share/icons/hicolor/scalable/apps/sequencer.svg
mkdir -p ~/.local/share/sequencer/samples
update-desktop-database ~/.local/share/applications 2>/dev/null || true

echo "Sequencer installed. Launch it with Super + Space → 'Sequencer', or run 'sequencer' in a terminal."
echo "Your own .wav samples go in ~/.local/share/sequencer/samples"
