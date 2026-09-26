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

/// The notes of one track in one pattern.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub cells: Vec<Cell>,
    /// Length in steps of the note starting at this step. 1 = one-shot, longer = cut off after that many steps.
    pub lens: Vec<u8>,
}

impl Default for Lane {
    fn default() -> Self {
        Self { cells: vec![Cell::Off; MAX_STEPS], lens: vec![1; MAX_STEPS] }
    }
}

impl Lane {
    /// The start step of the note covering step `s`, if any.
    pub fn note_at(&self, s: usize) -> Option<usize> {
        (0..=s).rev().find(|&n| self.cells[n] != Cell::Off && n + self.lens[n] as usize > s)
    }

    /// How many steps are free from `s` until the next note or the end of the grid.
    fn room(&self, s: usize, steps: usize) -> usize {
        (s + 1..steps).find(|&n| self.cells[n] != Cell::Off).unwrap_or(steps) - s
    }

    /// Places a note at `s`, as long as fits.
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

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FilterKind {
    #[default]
    Off,
    Low,
    High,
    Band,
}

/// Effects on one track, applied to everything the track plays before it goes to the mix.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fx {
    pub filter: FilterKind,
    /// 0..1, mapped exponentially from 40 Hz to 18 kHz.
    pub cutoff: f32,
    pub resonance: f32,
    /// Saturation, 0 = clean.
    pub drive: f32,
    /// Bit reduction, 0 = off, 1 = 2 bits.
    pub crush: f32,
    /// Sample rate reduction, 0 = off.
    pub downsample: f32,
    /// Hard clipping distortion, 0 = off.
    pub distort: f32,
    /// Fine tuning in cents, on top of the track pitch.
    pub fine: f32,
    /// Play the sample backwards.
    pub reverse: bool,
    /// Ring modulator mix and frequency (0..1, mapped from 30 Hz to 2 kHz).
    pub ring: f32,
    pub ring_freq: f32,
    /// Tempo synced gate: depth and length of one on/off cycle in steps.
    pub chop: f32,
    pub chop_steps: u8,
    /// Send to the reverb.
    pub reverb: f32,
    /// Delay time in sixteenth notes and how much of it comes back. The amount is `Track::send`.
    pub delay_steps: u8,
    pub feedback: f32,
    /// Three band EQ in dB: low shelf (100 Hz), mid peak (1 kHz), high shelf (8 kHz).
    pub eq_low: f32,
    pub eq_mid: f32,
    pub eq_high: f32,
}

impl Default for Fx {
    fn default() -> Self {
        Self {
            filter: FilterKind::Off,
            cutoff: 0.6,
            resonance: 0.2,
            drive: 0.0,
            crush: 0.0,
            downsample: 0.0,
            distort: 0.0,
            fine: 0.0,
            reverse: false,
            ring: 0.0,
            ring_freq: 0.4,
            chop: 0.0,
            chop_steps: 1,
            reverb: 0.0,
            delay_steps: 3,
            feedback: 0.35,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
        }
    }
}

impl Fx {
    pub fn active(&self) -> bool {
        self.filter != FilterKind::Off
            || self.drive > 0.0
            || self.crush > 0.0
            || self.downsample > 0.0
            || self.distort > 0.0
            || self.fine != 0.0
            || self.reverse
            || self.ring > 0.0
            || self.chop > 0.0
            || self.reverb > 0.0
            || self.eq_low != 0.0
            || self.eq_mid != 0.0
            || self.eq_high != 0.0
    }

    fn sanitize(&mut self) {
        self.cutoff = self.cutoff.clamp(0.0, 1.0);
        self.resonance = self.resonance.clamp(0.0, 1.0);
        self.drive = self.drive.clamp(0.0, 1.0);
        self.crush = self.crush.clamp(0.0, 1.0);
        self.downsample = self.downsample.clamp(0.0, 1.0);
        self.distort = self.distort.clamp(0.0, 1.0);
        self.fine = self.fine.clamp(-100.0, 100.0);
        self.ring = self.ring.clamp(0.0, 1.0);
        self.ring_freq = self.ring_freq.clamp(0.0, 1.0);
        self.chop = self.chop.clamp(0.0, 1.0);
        self.chop_steps = self.chop_steps.clamp(1, 8);
        self.reverb = self.reverb.clamp(0.0, 1.0);
        self.delay_steps = self.delay_steps.clamp(1, 16);
        self.feedback = self.feedback.clamp(0.0, 0.9);
        self.eq_low = self.eq_low.clamp(-12.0, 12.0);
        self.eq_mid = self.eq_mid.clamp(-12.0, 12.0);
        self.eq_high = self.eq_high.clamp(-12.0, 12.0);
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub sample: usize,
    /// One lane per pattern (A to H).
    pub lanes: Vec<Lane>,
    /// Length for newly placed notes.
    pub note_len: u8,
    pub volume: f32,
    pub pitch: f32,
    /// -1 left, 0 center, 1 right.
    pub pan: f32,
    /// How much of this track goes to the delay.
    pub send: f32,
    #[serde(default)]
    pub fx: Fx,
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
            fx: Fx::default(),
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
    /// Number of steps per pattern.
    pub steps: Vec<usize>,
    /// The pattern that plays and that you edit.
    pub current: usize,
    /// Switches to this pattern at the end of the current one.
    pub queued: Option<usize>,
    pub tracks: Vec<Track>,
}

impl Song {
    fn empty(tracks: Vec<Track>, bpm: f32, steps: usize) -> Self {
        Self {
            bpm,
            swing: 0.0,
            master: 0.8,
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
        // A: basic groove.
        put(0, 0, &[0, 4, 8, 12], &[0]);
        put(1, 0, &[4, 12], &[]);
        put(3, 0, &[0, 2, 4, 6, 8, 10, 12, 14], &[2, 6, 10, 14]);
        put(4, 0, &[7, 15], &[]);
        put(6, 0, &[0, 6, 10], &[0]);
        // B: fuller, with clap, cowbell and plucks.
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

    /// A 135 BPM rave pattern with long notes for loop, stabs, hoover and acid.
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

    /// An empty song to start from: a few drum tracks and nothing on the grid.
    pub fn blank(samples: &[Arc<Sample>]) -> Self {
        let idx = |name: &str| samples.iter().position(|s| s.name == name).unwrap_or(0);
        let tracks = ["kick", "snare", "clap", "hat_closed", "hat_open", "bass_hit"]
            .iter()
            .map(|n| {
                let s = idx(n);
                Track::new(s, samples[s].default_len())
            })
            .collect();
        Self::empty(tracks, 120.0, 16)
    }

    /// Clamps everything that came from a file into range and resolves sample names to indexes.
    pub fn sanitize(&mut self, names: &[String], samples: &[Arc<Sample>]) {
        self.tracks.truncate(MAX_TRACKS);
        for (t, name) in self.tracks.iter_mut().zip(names) {
            t.sample = samples.iter().position(|s| &s.name == name).unwrap_or(0);
        }
        for t in &mut self.tracks {
            t.sample = t.sample.min(samples.len() - 1);
            t.lanes.resize(PATTERNS, Lane::default());
            t.lanes.iter_mut().for_each(Lane::sanitize);
            t.volume = t.volume.clamp(0.0, 1.0);
            t.pitch = t.pitch.clamp(-24.0, 24.0);
            t.pan = t.pan.clamp(-1.0, 1.0);
            t.send = t.send.clamp(0.0, 1.0);
            t.note_len = t.note_len.clamp(1, 16);
            t.fx.sanitize();
        }
        self.steps.resize(PATTERNS, 16);
        for s in &mut self.steps {
            *s = (*s).clamp(1, MAX_STEPS);
        }
        self.current = self.current.min(PATTERNS - 1);
        self.queued = None;
        self.bpm = self.bpm.clamp(40.0, 300.0);
        self.swing = self.swing.clamp(0.0, 0.5);
        self.master = self.master.clamp(0.0, 1.0);
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }
}
