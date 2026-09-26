# Sequencer

Een step sequencer voor Omarchy in Rust (egui voor de interface, cpal voor audio via PipeWire). De app neemt kleuren en font over van het actieve Omarchy-thema en verandert live mee bij `omarchy theme set` of `omarchy font set` (font na een herstart).

## Installeren

```bash
./install.sh
```

Dit bouwt een release, zet de binary in `~/.local/bin/sequencer` en installeert een `.desktop`-bestand en icoon, zodat hij in de Omarchy launcher staat (Super + Space → "Sequencer"). Met `sequencer --play` start hij direct met afspelen.

## Wat erin zit

- Grid van 1 tot 64 stappen en 1 tot 16 tracks, 8 patronen (A t/m H) die tijdens het spelen op de maatgrens wisselen.
- Lange noten: elke track heeft een `L`-knop (1, 2, 4, 8 of 16 blokjes) voor nieuwe noten. Een lange noot klinkt precies zo lang als hij in het grid staat. Scroll boven een noot om hem langer of korter te maken.
- Per track: sample, mute, solo, volume, pan, delay-send, toonhoogte in halve tonen.
- Master: tempo met tap tempo, swing, lowpass-filter, ping-pong delay op tempo met feedback, volume.
- Opnemen, zoals Voxtype: houd de rode knop van een track ingedrukt (of `V` voor de geselecteerde track) en neem op van je standaard microfoon. Stilte gaat eraf, de opname wordt genormaliseerd, bewaard als `rec_NN.wav` en meteen op die track gezet.
- Undo en redo, automatisch opslaan in `~/.config/sequencer/song.json`, export van vier keer het huidige patroon naar een stereo WAV in `~/Music`.
- Eigen samples: zet `.wav` bestanden in `~/.local/share/sequencer/samples`.

## Bediening

| Actie | Muis | Toets |
|---|---|---|
| Afspelen / stoppen | Play | `Space` |
| Noot tekenen of wissen | klikken / slepen | |
| Accent | rechtsklik | |
| Notelengte aanpassen | scrollen boven een noot | |
| Stappen | − / + | `←` `→` |
| Tempo | BPM, Tap | `↑` `↓`, `T` |
| Patroon kiezen | A t/m H | `F1` t/m `F8` |
| Patroon kopiëren | shift-klik of rechtsklik op een letter | `Shift+F1` t/m `F8` |
| Track muten | M | `1` t/m `9` |
| Track selecteren | tracknummer | `Tab` / `Shift+Tab` |
| Opnemen | rode knop ingedrukt houden | `V` ingedrukt houden |
| Random / wissen | Random / Clear | `R` / `C` |
| Track erbij | + Track | `N` |
| Undo / redo | Undo / Redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Opslaan / exporteren | Save / Export WAV | `Ctrl+S` / `Ctrl+E` |

## Samples

Alle meegeleverde samples zijn CC0 (public domain), omgezet naar 44.1 kHz mono en in de binary ingebakken.

- Drums: Roland TR-808 Sound Sample Set van Michael Fischer (1994), via [tidalcycles/sounds-tr808-fischer](https://github.com/tidalcycles/sounds-tr808-fischer), CC0 1.0.
- 90s rave (echte opnames, allemaal CC0 op Freesound): rave stab ([443931](https://freesound.org/s/443931/)), Roland JX-3P stab door modularsamples ([307131](https://freesound.org/s/307131/)), hoover door Chameon ([399542](https://freesound.org/s/399542/)), hardhouse hoover door woowah ([11512](https://freesound.org/s/11512/)), mentasm door sandizzy ([25393](https://freesound.org/s/25393/)), M1 organ door Cloud-10 ([680995](https://freesound.org/s/680995/)), orchestra hit door BigDumbWeirdo ([90741](https://freesound.org/s/90741/)), acid line door makenoisemusic ([402949](https://freesound.org/s/402949/)), acid bass door evanjones4 ([243999](https://freesound.org/s/243999/)), house chords door Slanted ([510229](https://freesound.org/s/510229/)), rave loops door Alastair_Pursloe ([183441](https://freesound.org/s/183441/)) en GENERALMiDiGUy ([490703](https://freesound.org/s/490703/)).
- Vocals: dance shouts, soulful house vocals en computerstemmen uit de [Producer Space CC0 library](https://archive.org/details/producer-space-cc0-sample-library).
- Riffs en stabs: "2HTC Samples Vol 4 Addendum" van Ben Burnes (Abstraction Music), via [lavenderdotpet/CC0-Public-Domain-Sounds](https://github.com/lavenderdotpet/CC0-Public-Domain-Sounds), CC0.
