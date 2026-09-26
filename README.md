# CYBERSEQ//2000

Een step sequencer voor Omarchy in 90s cyber-stijl: neon op zwart, scanlines, oscilloscoop, Win95-bevels. Geschreven in Rust met egui (UI) en cpal (audio via ALSA/PipeWire).

## Installeren

```bash
./install.sh
```

Dit bouwt een release, zet de binary in `~/.local/bin/sequencer` en installeert een `.desktop`-bestand en icoon, zodat hij in de Omarchy launcher staat (Super + Space → "CyberSeq"). Met `sequencer --play` start hij direct met afspelen.

## Bediening

- Links klikken of slepen over het grid: noten tekenen, klik op een noot om hem te wissen. Rechtsklik: accent (magenta, harder).
- Lange noten: elke track heeft een `L`-knop (1, 2, 4, 8 of 16 blokjes) voor nieuwe noten. Riffs en 90s-synths staan standaard op 4. Een lange noot klinkt precies zo lang als hij in het grid staat. Scroll boven een noot om hem langer of korter te maken.
- `90S RAVE` (of toets `9`) laadt een 138 BPM rave-patroon.
- GRID: aantal stappen van 1 tot 64 (`-`/`+`, slepen, of de knoppen 8/12/16/24/32/64). Tracks toevoegen met `+ TRACK` (max 16), verwijderen met `×`.
- Per track: sample kiezen (dropdown, speelt meteen af), mute, solo, volume (slepen of scrollen), pitch in halve tonen (verticaal slepen, dubbelklik = 0). Klik op het tracknummer om te previewen, rechtsklik schuift de track omhoog.
- Toetsen: `Space` play/stop, `←`/`→` grid kleiner/groter, `↑`/`↓` BPM, `R` random patroon, `C` wissen, `T` track erbij, `Ctrl+S` opslaan.
- Het patroon wordt automatisch bewaard in `~/.config/sequencer/pattern.json`.
- Eigen samples: zet `.wav` bestanden in `~/.local/share/sequencer/samples`; ze verschijnen onder USER SAMPLES.

## Samples

Alle meegeleverde samples zijn CC0 (public domain), omgezet naar 44.1 kHz mono en in de binary ingebakken.

- Drums: Roland TR-808 Sound Sample Set van Michael Fischer (1994), via [tidalcycles/sounds-tr808-fischer](https://github.com/tidalcycles/sounds-tr808-fischer), CC0 1.0.
- 90s rave-synths (hoover, rave stab, acid 303, reese, M1-orgel, house piano): zelf gesynthetiseerd met `tools/synth90s.py`, CC0.
- Riffs en stabs: "2HTC Samples Vol 4 Addendum" van Ben Burnes (Abstraction Music), via [lavenderdotpet/CC0-Public-Domain-Sounds](https://github.com/lavenderdotpet/CC0-Public-Domain-Sounds), CC0.
