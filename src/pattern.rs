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
    pub volume: f32,
    pub pitch: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Track {
    pub fn new(sample: usize) -> Self {
        Self { sample, cells: vec![Cell::Off; MAX_STEPS], volume: 0.8, pitch: 0.0, mute: false, solo: false }
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
            let mut t = Track::new(idx(name));
            for &h in hits {
                t.cells[h] = Cell::On;
            }
            for &a in accents {
                t.cells[a] = Cell::Accent;
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

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }
}

#[derive(Serialize, Deserialize)]
struct SavedTrack {
    sample: String,
    cells: Vec<Cell>,
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
            Track {
                sample: samples.iter().position(|s| s.name == t.sample).unwrap_or(0),
                cells,
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
