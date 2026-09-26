use serde::{Deserialize, Serialize};

use crate::samples::Sample;

pub const MAX_STEPS: usize = 64;
pub const MAX_TRACKS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Cell {
    #[default]
    Off,
    On,
    Accent,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub sample: usize,
    pub cells: Vec<Cell>,
    /// Lengte in stappen van de noot die op deze stap begint. 1 = one-shot, langer = afgekapt na zoveel blokjes.
    pub lens: Vec<u8>,
    /// Lengte voor nieuw geplaatste noten.
    pub note_len: u8,
    pub volume: f32,
    pub pitch: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Track {
    pub fn new(sample: usize, note_len: u8) -> Self {
        Self {
            sample,
            cells: vec![Cell::Off; MAX_STEPS],
            lens: vec![1; MAX_STEPS],
            note_len,
            volume: 0.8,
            pitch: 0.0,
            mute: false,
            solo: false,
        }
    }

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
}

#[derive(Clone, PartialEq)]
pub struct Pattern {
    pub bpm: f32,
    pub swing: f32,
    pub steps: usize,
    pub master: f32,
    pub tracks: Vec<Track>,
}

impl Pattern {
    pub fn demo(samples: &[Sample]) -> Self {
        let idx = |name: &str| samples.iter().position(|s| s.name == name).unwrap_or(0);
        let row = |name: &str, hits: &[usize], accents: &[usize]| {
            let sample = idx(name);
            let len = samples[sample].default_len();
            let mut t = Track::new(sample, len);
            for &h in hits {
                t.place(h, if accents.contains(&h) { Cell::Accent } else { Cell::On }, len, 16);
            }
            t
        };
        let mut tracks = vec![
            row("kick", &[0, 4, 8, 12], &[0]),
            row("snare", &[4, 12], &[]),
            row("clap", &[12], &[]),
            row("hat_closed", &[0, 2, 4, 6, 8, 10, 12, 14], &[2, 6, 10, 14]),
            row("hat_open", &[7, 15], &[]),
            row("cowbell", &[3, 11], &[]),
            row("bass_hit", &[0, 3, 6, 10], &[0]),
            row("plucks", &[8], &[]),
        ];
        tracks[5].volume = 0.4;
        tracks[4].volume = 0.5;
        Self { bpm: 124.0, swing: 0.08, steps: 16, master: 0.8, tracks }
    }

    /// Een 135 BPM rave-patroon met lange noten voor loop, stabs, hoover en acid.
    pub fn rave(samples: &[Sample]) -> Self {
        let idx = |name: &str| samples.iter().position(|s| s.name == name).unwrap_or(0);
        let row = |name: &str, notes: &[(usize, u8, bool)], volume: f32| {
            let sample = idx(name);
            let mut t = Track::new(sample, samples[sample].default_len());
            for &(s, len, accent) in notes {
                t.place(s, if accent { Cell::Accent } else { Cell::On }, len, 32);
            }
            t.volume = volume;
            t
        };
        let four = (0..32).step_by(4).map(|s| (s, 1, s % 16 == 0)).collect::<Vec<_>>();
        let tracks = vec![
            row("kick", &four, 0.9),
            row("rave_loop_135", &[(0, 16, false), (16, 16, true)], 0.6),
            row("hat_open", &(2..32).step_by(4).map(|s| (s, 1, false)).collect::<Vec<_>>(), 0.4),
            row("clap", &[(4, 1, false), (12, 1, false), (20, 1, false), (28, 1, true)], 0.6),
            row("hoover", &[(0, 8, true), (24, 8, false)], 0.5),
            row("rave_stab", &[(3, 2, true), (6, 2, false), (10, 4, false), (19, 2, true), (22, 2, false)], 0.6),
            row("acid_line", &[(8, 4, false), (12, 4, true), (16, 8, false)], 0.45),
            row("orch_hit", &[(0, 1, true), (16, 1, false)], 0.5),
            row("m1_organ", &[(14, 2, false), (30, 2, false)], 0.5),
            row("everybody", &[(28, 1, false)], 0.7),
        ];
        Self { bpm: 135.0, swing: 0.0, steps: 32, master: 0.8, tracks }
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }
}

#[derive(Serialize, Deserialize)]
struct SavedTrack {
    sample: String,
    cells: Vec<Cell>,
    #[serde(default)]
    lens: Vec<u8>,
    #[serde(default)]
    note_len: u8,
    volume: f32,
    pitch: f32,
    mute: bool,
    solo: bool,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    bpm: f32,
    swing: f32,
    steps: usize,
    master: f32,
    tracks: Vec<SavedTrack>,
}

fn save_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("sequencer").join("pattern.json"))
}

pub fn save(pattern: &Pattern, samples: &[Sample]) -> std::io::Result<()> {
    let saved = Saved {
        bpm: pattern.bpm,
        swing: pattern.swing,
        steps: pattern.steps,
        master: pattern.master,
        tracks: pattern
            .tracks
            .iter()
            .map(|t| SavedTrack {
                sample: samples[t.sample].name.clone(),
                cells: t.cells.clone(),
                lens: t.lens.clone(),
                note_len: t.note_len,
                volume: t.volume,
                pitch: t.pitch,
                mute: t.mute,
                solo: t.solo,
            })
            .collect(),
    };
    let path = save_path().ok_or_else(|| std::io::Error::other("geen config dir"))?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_string_pretty(&saved)?)
}

pub fn load(samples: &[Sample]) -> Option<Pattern> {
    let text = std::fs::read_to_string(save_path()?).ok()?;
    let saved: Saved = serde_json::from_str(&text).ok()?;
    let tracks: Vec<Track> = saved
        .tracks
        .into_iter()
        .take(MAX_TRACKS)
        .map(|t| {
            let mut cells = t.cells;
            cells.resize(MAX_STEPS, Cell::Off);
            let mut lens = t.lens;
            lens.resize(MAX_STEPS, 1);
            let sample = samples.iter().position(|s| s.name == t.sample).unwrap_or(0);
            Track {
                sample,
                cells,
                lens: lens.into_iter().map(|l| l.clamp(1, 16)).collect(),
                note_len: if t.note_len == 0 { samples[sample].default_len() } else { t.note_len.clamp(1, 16) },
                volume: t.volume.clamp(0.0, 1.0),
                pitch: t.pitch.clamp(-24.0, 24.0),
                mute: t.mute,
                solo: t.solo,
            }
        })
        .collect();
    if tracks.is_empty() {
        return None;
    }
    Some(Pattern {
        bpm: saved.bpm.clamp(40.0, 300.0),
        swing: saved.swing.clamp(0.0, 0.5),
        steps: saved.steps.clamp(1, MAX_STEPS),
        master: saved.master.clamp(0.0, 1.0),
        tracks,
    })
}
