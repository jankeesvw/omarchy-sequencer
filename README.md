# CYBERSEQ//2000

Een step sequencer voor Omarchy in 90s cyber-stijl: neon op zwart, scanlines, oscilloscoop, Win95-bevels. Geschreven in Rust met egui (UI) en cpal (audio via ALSA/PipeWire).

## Installeren

```bash
./install.sh
```

Dit bouwt een release, zet de binary in `~/.local/bin/sequencer` en installeert een `.desktop`-bestand en icoon, zodat hij in de Omarchy launcher staat (Super + Space → "CyberSeq"). Met `sequencer --play` start hij direct met afspelen.

## Bediening

- Links klikken of slepen over het grid: noten tekenen, klik op een noot om hem te wissen. Rechtsklik: accent (magenta, harder).
- Lange noten: elke track heeft een `L`-knop (1, 2, 4, 8 of 16 blokjes) voor nieuwe noten. Riffs en 90s-samples staan standaard op 4, loops op 16. Een lange noot klinkt precies zo lang als hij in het grid staat. Scroll boven een noot om hem langer of korter te maken.
- `90S RAVE` (of toets `9`) laadt een 135 BPM rave-patroon.
- GRID: aantal stappen van 1 tot 64 (`-`/`+`, slepen, of de knoppen 8/12/16/24/32/64). Tracks toevoegen met `+ TRACK` (max 16), verwijderen met `×`.
- Per track: sample kiezen (dropdown, speelt meteen af), mute, solo, volume (slepen of scrollen), pitch in halve tonen (verticaal slepen, dubbelklik = 0). Klik op het tracknummer om te previewen, rechtsklik schuift de track omhoog.
- Toetsen: `Space` play/stop, `←`/`→` grid kleiner/groter, `↑`/`↓` BPM, `R` random patroon, `C` wissen, `T` track erbij, `Ctrl+S` opslaan.
- Het patroon wordt automatisch bewaard in `~/.config/sequencer/pattern.json`.
- Eigen samples: zet `.wav` bestanden in `~/.local/share/sequencer/samples`; ze verschijnen onder USER SAMPLES.

## Samples

Alle meegeleverde samples zijn CC0 (public domain), omgezet naar 44.1 kHz mono en in de binary ingebakken.

- Drums: Roland TR-808 Sound Sample Set van Michael Fischer (1994), via [tidalcycles/sounds-tr808-fischer](https://github.com/tidalcycles/sounds-tr808-fischer), CC0 1.0.
- 90s rave (echte opnames, allemaal CC0 op Freesound): rave stab ([443931](https://freesound.org/s/443931/)), Roland JX-3P stab door modularsamples ([307131](https://freesound.org/s/307131/)), hoover door Chameon ([399542](https://freesound.org/s/399542/)), hardhouse hoover door woowah ([11512](https://freesound.org/s/11512/)), mentasm door sandizzy ([25393](https://freesound.org/s/25393/)), M1 organ door Cloud-10 ([680995](https://freesound.org/s/680995/)), orchestra hit door BigDumbWeirdo ([90741](https://freesound.org/s/90741/)), acid line door makenoisemusic ([402949](https://freesound.org/s/402949/)), acid bass door evanjones4 ([243999](https://freesound.org/s/243999/)), house chords door Slanted ([510229](https://freesound.org/s/510229/)), rave loops door Alastair_Pursloe ([183441](https://freesound.org/s/183441/)) en GENERALMiDiGUy ([490703](https://freesound.org/s/490703/)).
- Vocals: dance shouts, soulful house vocals en computerstemmen uit de [Producer Space CC0 library](https://archive.org/details/producer-space-cc0-sample-library).
- Riffs en stabs: "2HTC Samples Vol 4 Addendum" van Ben Burnes (Abstraction Music), via [lavenderdotpet/CC0-Public-Domain-Sounds](https://github.com/lavenderdotpet/CC0-Public-Domain-Sounds), CC0.
