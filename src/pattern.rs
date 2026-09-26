use serde::{Deserialize, Serialize};

use std::sync::Arc;

use crate::samples::Sample;

pub const MAX_STEPS: usize = 64;
pub const MAX_TRACKS: usize = 16;
pub const PATTERNS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Cell {
    #[default]
    Off,
    On,
    Accent,
}

/// De noten van één track in één patroon.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub cells: Vec<Cell>,
    /// Lengte in stappen van de noot die op deze stap begint. 1 = one-shot, langer = afgekapt na zoveel blokjes.
    pub lens: Vec<u8>,
}

impl Default for Lane {
    fn default() -> Self {
        Self { cells: vec![Cell::Off; MAX_STEPS], lens: vec![1; MAX_STEPS] }
    }
}

impl Lane {
    /// De startstap van de noot die stap `s` bedekt, als die er is.
    pub fn note_at(&self, s: usize) -> Option<usize> {
        (0..=s).rev().find(|&n| self.cells[n] != Cell::Off && n + self.lens[n] as usize > s)
    }

    /// Hoeveel stappen er vanaf `s` vrij zijn tot de volgende noot of het einde van het grid.
    fn room(&self, s: usize, steps: usize) -> usize {
        (s + 1..steps).find(|&n| self.cells[n] != Cell::Off).unwrap_or(steps) - s
    }

    /// Zet een noot op `s`, zo lang als past.
    pub fn place(&mut self, s: usize, cell: Cell, len: u8, steps: usize) {
        if s >= steps || self.note_at(s).is_some() {
            return;
        }
        self.cells[s] = cell;
        self.lens[s] = (len as usize).min(self.room(s, steps)).max(1) as u8;
    }

    pub fn erase(&mut self, s: usize) {
        if let Some(n) = self.note_at(s) {
            self.cells[n] = Cell::Off;
            self.lens[n] = 1;
        }
    }

    pub fn resize(&mut self, s: usize, delta: i32, steps: usize) {
        if let Some(n) = self.note_at(s) {
            let max = self.room(n, steps) as i32;
            self.lens[n] = (self.lens[n] as i32 + delta).clamp(1, max.min(16)) as u8;
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::Off);
        self.lens.fill(1);
    }

    fn sanitize(&mut self) {
        self.cells.resize(MAX_STEPS, Cell::Off);
        self.lens.resize(MAX_STEPS, 1);
        for l in &mut self.lens {
            *l = (*l).clamp(1, 16);
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub sample: usize,
    /// Eén lane per patroon (A t/m H).
    pub lanes: Vec<Lane>,
    /// Lengte voor nieuw geplaatste noten.
    pub note_len: u8,
    pub volume: f32,
    pub pitch: f32,
    /// -1 links, 0 midden, 1 rechts.
    pub pan: f32,
    /// Hoeveel van deze track naar de delay gaat.
    pub send: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Track {
    pub fn new(sample: usize, note_len: u8) -> Self {
        Self {
            sample,
            lanes: vec![Lane::default(); PATTERNS],
            note_len,
            volume: 0.8,
            pitch: 0.0,
            pan: 0.0,
            send: 0.0,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub bpm: f32,
    pub swing: f32,
    pub master: f32,
    /// Master lowpass, 0..1 (1 = open).
    pub cutoff: f32,
    /// Delaytijd in zestienden.
    pub delay_steps: u8,
    pub feedback: f32,
    /// Aantal stappen per patroon.
    pub steps: Vec<usize>,
    /// Het patroon dat speelt en dat je bewerkt.
    pub current: usize,
    /// Wisselt naar dit patroon aan het eind van het huidige.
    pub queued: Option<usize>,
    pub tracks: Vec<Track>,
}

impl Song {
    fn empty(tracks: Vec<Track>, bpm: f32, steps: usize) -> Self {
        Self {
            bpm,
            swing: 0.0,
            master: 0.8,
            cutoff: 1.0,
            delay_steps: 3,
            feedback: 0.35,
            steps: vec![steps; PATTERNS],
            current: 0,
            queued: None,
            tracks,
        }
    }

    pub fn steps(&self) -> usize {
        self.steps[self.current]
    }

    pub fn demo(samples: &[Arc<Sample>]) -> Self {
        let idx = |name: &str| samples.iter().position(|s| s.name == name).unwrap_or(0);
        let tracks = ["kick", "snare", "clap", "hat_closed", "hat_open", "cowbell", "bass_hit", "plucks"]
            .iter()
            .map(|n| {
                let s = idx(n);
                Track::new(s, samples[s].default_len())
            })
            .collect();
        let mut song = Self::empty(tracks, 124.0, 16);
        song.swing = 0.08;
        let mut put = |t: usize, p: usize, hits: &[usize], accents: &[usize]| {
            let len = song.tracks[t].note_len;
            for &h in hits {
                let cell = if accents.contains(&h) { Cell::Accent } else { Cell::On };
                song.tracks[t].lanes[p].place(h, cell, len, 16);
            }
        };
        // A: basisgroove.
        put(0, 0, &[0, 4, 8, 12], &[0]);
        put(1, 0, &[4, 12], &[]);
        put(3, 0, &[0, 2, 4, 6, 8, 10, 12, 14], &[2, 6, 10, 14]);
        put(4, 0, &[7, 15], &[]);
        put(6, 0, &[0, 6, 10], &[0]);
        // B: voller, met clap, cowbell en plucks.
        put(0, 1, &[0, 4, 8, 12, 14], &[0]);
        put(1, 1, &[4, 12], &[]);
        put(2, 1, &[12], &[]);
        put(3, 1, &[0, 2, 4, 6, 8, 10, 12, 14], &[2, 6, 10, 14]);
        put(4, 1, &[7, 15], &[]);
        put(5, 1, &[3, 11], &[]);
        put(6, 1, &[0, 3, 6, 10], &[0]);
        put(7, 1, &[8], &[]);
        song.tracks[4].volume = 0.5;
        song.tracks[4].pan = 0.3;
        song.tracks[5].volume = 0.4;
        song.tracks[5].pan = -0.4;
        song.tracks[7].send = 0.4;
        song
    }

    /// Een 135 BPM rave-patroon met lange noten voor loop, stabs, hoover en acid.
    pub fn rave(samples: &[Arc<Sample>]) -> Self {
        let idx = |name: &str| samples.iter().position(|s| s.name == name).unwrap_or(0);
        let row = |name: &str, notes: &[(usize, u8, bool)], volume: f32, pan: f32, send: f32| {
            let sample = idx(name);
            let mut t = Track::new(sample, samples[sample].default_len());
            for &(s, len, accent) in notes {
                t.lanes[0].place(s, if accent { Cell::Accent } else { Cell::On }, len, 32);
            }
            t.volume = volume;
            t.pan = pan;
            t.send = send;
            t
        };
        let four = (0..32).step_by(4).map(|s| (s, 1, s % 16 == 0)).collect::<Vec<_>>();
        let hats = (2..32).step_by(4).map(|s| (s, 1, false)).collect::<Vec<_>>();
        let tracks = vec![
            row("kick", &four, 0.9, 0.0, 0.0),
            row("rave_loop_135", &[(0, 16, false), (16, 16, true)], 0.6, 0.0, 0.0),
            row("hat_open", &hats, 0.4, 0.35, 0.0),
            row("clap", &[(4, 1, false), (12, 1, false), (20, 1, false), (28, 1, true)], 0.6, 0.0, 0.2),
            row("hoover", &[(0, 8, true), (24, 8, false)], 0.5, -0.2, 0.2),
            row("rave_stab", &[(3, 2, true), (6, 2, false), (10, 4, false), (19, 2, true), (22, 2, false)], 0.6, 0.25, 0.45),
            row("acid_line", &[(8, 4, false), (12, 4, true), (16, 8, false)], 0.45, -0.3, 0.3),
            row("orch_hit", &[(0, 1, true), (16, 1, false)], 0.5, 0.0, 0.5),
            row("m1_organ", &[(14, 2, false), (30, 2, false)], 0.5, 0.2, 0.2),
            row("everybody", &[(28, 1, false)], 0.7, 0.0, 0.4),
        ];
        Self::empty(tracks, 135.0, 32)
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }
}

// Opgeslagen met samplenamen naast de indexen, zodat het bestand blijft kloppen als er samples bijkomen.
#[derive(Serialize, Deserialize)]
struct Saved {
    song: Song,
    names: Vec<String>,
}

fn save_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("sequencer").join("song.json"))
}

pub fn save(song: &Song, samples: &[Arc<Sample>]) -> std::io::Result<()> {
    let names = song.tracks.iter().map(|t| samples[t.sample].name.clone()).collect();
    let saved = Saved { song: song.clone(), names };
    let path = save_path().ok_or_else(|| std::io::Error::other("geen config dir"))?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_string(&saved)?)
}

pub fn load(samples: &[Arc<Sample>]) -> Option<Song> {
    let text = std::fs::read_to_string(save_path()?).ok()?;
    let Saved { mut song, names } = serde_json::from_str(&text).ok()?;
    song.tracks.truncate(MAX_TRACKS);
    if song.tracks.is_empty() || names.len() < song.tracks.len() {
        return None;
    }
    for (t, name) in song.tracks.iter_mut().zip(&names) {
        t.sample = samples.iter().position(|s| &s.name == name).unwrap_or(0);
        t.lanes.resize(PATTERNS, Lane::default());
        t.lanes.iter_mut().for_each(Lane::sanitize);
        t.volume = t.volume.clamp(0.0, 1.0);
        t.pitch = t.pitch.clamp(-24.0, 24.0);
        t.pan = t.pan.clamp(-1.0, 1.0);
        t.send = t.send.clamp(0.0, 1.0);
        t.note_len = t.note_len.clamp(1, 16);
    }
    song.steps.resize(PATTERNS, 16);
    for s in &mut song.steps {
        *s = (*s).clamp(1, MAX_STEPS);
    }
    song.current = song.current.min(PATTERNS - 1);
    song.queued = None;
    song.bpm = song.bpm.clamp(40.0, 300.0);
    song.swing = song.swing.clamp(0.0, 0.5);
    song.master = song.master.clamp(0.0, 1.0);
    song.cutoff = song.cutoff.clamp(0.0, 1.0);
    song.feedback = song.feedback.clamp(0.0, 0.9);
    song.delay_steps = song.delay_steps.clamp(1, 16);
    Some(song)
}
