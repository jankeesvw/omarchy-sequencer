# Changelog

## 1.2.0

- **Open in Sequencer** on [omarchysequencer.com](https://omarchysequencer.com) opens a song straight in the app: Sequencer handles `omarchy-sequencer://` links.
- Sharing sends your own recordings as Opus, about a tenth of the size, and no longer sends pack sounds: the site gets the packs itself.
- Shares are signed, so the site only takes songs from the app. Sequencer 1.1 can no longer share.

## 1.1.0

- **Share** in the toolbar puts your song on [omarchysequencer.com](https://omarchysequencer.com), where others listen to it, vote for it and open it in the app. The site plays songs with the app's own engine, compiled to WebAssembly (`player/`), with every pattern.
- `omarchy-sequencer --open <song url>` opens a shared song, installs the sound packs it needs and downloads the own recordings it uses.

## 1.0.0

The first release.

- A grid of 1 to 64 steps and up to 16 tracks, with eight patterns that switch on the bar.
- Notes as long as you want: 1 to 16 steps, drawn with the track's length and resized by scrolling.
- Effects on every track: pitch, fine tuning, reverse, filter, three band EQ, drive, distortion, bitcrush, sample rate reduction, ring modulator, chop, delay and reverb, with a looping preview.
- A sound browser with a built-in TR-808 kit, riffs, real 90s rave sounds and vocals, and search across everything.
- Six free sound packs to download from inside the app, all CC0 or public domain.
- Recording from the microphone: click to start and stop, or hold and let go, with a live waveform. Up to 60 seconds.
- Click effects: notes pop in with sparks, and a preview of the note shows where you hover.
- Songs saved as files in `~/Music/Sequencer`, with presets, undo and redo, and export to WAV.
- Follows your Omarchy theme and font, and keeps every theme readable.
- An about box with an ASCII logo and rolling credits.
