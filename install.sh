#!/usr/bin/env bash
# Installeert CyberSeq 2000 als gewone app, zodat hij in de Omarchy launcher (Super + Space) staat.
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

echo "CyberSeq 2000 geïnstalleerd. Start via Super + Space → 'CyberSeq', of 'sequencer' in een terminal."
echo "Eigen .wav samples? Zet ze in ~/.local/share/sequencer/samples"
