use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::pattern::{Cell, MAX_TRACKS, Pattern};
use crate::samples::Sample;

pub const SCOPE_LEN: usize = 1024;
const FADE_FRAMES: f32 = 96.0;

/// Alles wat UI en audio-thread delen.
pub struct Shared {
    pub pattern: Mutex<Pattern>,
    pub playing: AtomicBool,
    pub step: AtomicUsize,
    /// Per track een piekniveau (f32 bits) voor de knipperende meters.
    pub levels: [AtomicU32; MAX_TRACKS],
    pub scope: Mutex<Vec<f32>>,
    /// Sample-previews die de UI aanvraagt (sample index, gain).
    pub preview: Mutex<Vec<(usize, f32)>>,
}

impl Shared {
    pub fn new(pattern: Pattern) -> Self {
        Self {
            pattern: Mutex::new(pattern),
            playing: AtomicBool::new(false),
            step: AtomicUsize::new(0),
            levels: std::array::from_fn(|_| AtomicU32::new(0)),
            scope: Mutex::new(vec![0.0; SCOPE_LEN]),
            preview: Mutex::new(Vec::new()),
        }
    }

    pub fn level(&self, track: usize) -> f32 {
        f32::from_bits(self.levels[track].load(Ordering::Relaxed))
    }
}

struct Voice {
    sample: usize,
    track: Option<usize>,
    pos: f64,
    rate: f64,
    gain: f32,
    fade: Option<f32>,
}

pub struct Engine {
    samples: Arc<Vec<Sample>>,
    shared: Arc<Shared>,
    out_rate: f64,
    voices: Vec<Voice>,
    until_next: f64,
    next_step: usize,
    was_playing: bool,
    scope_buf: Vec<f32>,
}

impl Engine {
    fn new(samples: Arc<Vec<Sample>>, shared: Arc<Shared>, out_rate: u32) -> Self {
        Self {
            samples,
            shared,
            out_rate: out_rate as f64,
            voices: Vec::with_capacity(64),
            until_next: 0.0,
            next_step: 0,
            was_playing: false,
            scope_buf: Vec::with_capacity(SCOPE_LEN),
        }
    }

    fn trigger(&mut self, sample: usize, track: Option<usize>, gain: f32, pitch: f32) {
        // Eén stem per track: de vorige wordt kort uitgefade (choke), dat klinkt strak bij hats en riffs.
        if track.is_some() {
            for v in self.voices.iter_mut().filter(|v| v.track == track && v.fade.is_none()) {
                v.fade = Some(1.0);
            }
        }
        if self.voices.len() >= 64 {
            self.voices.remove(0);
        }
        let s = &self.samples[sample];
        let rate = s.rate as f64 / self.out_rate * 2f64.powf(pitch as f64 / 12.0);
        self.voices.push(Voice { sample, track, pos: 0.0, rate, gain, fade: None });
    }

    fn step(&mut self, pattern: &Pattern) {
        let step = self.next_step % pattern.steps.max(1);
        self.shared.step.store(step, Ordering::Relaxed);
        let solo = pattern.any_solo();
        for (i, t) in pattern.tracks.iter().enumerate() {
            let cell = t.cells[step];
            let audible = if solo { t.solo } else { !t.mute };
            if cell != Cell::Off && audible {
                let accent = if cell == Cell::Accent { 1.0 } else { 0.62 };
                self.trigger(t.sample, Some(i), t.volume * accent, t.pitch);
                self.shared.levels[i].store(accent.to_bits(), Ordering::Relaxed);
            }
        }
        // Swing: even stappen lang, oneven kort.
        let len = self.out_rate * 60.0 / pattern.bpm as f64 / 4.0;
        let swing = pattern.swing as f64;
        self.until_next += if step % 2 == 0 { len * (1.0 + swing) } else { len * (1.0 - swing) };
        self.next_step = step + 1;
    }

    pub fn render(&mut self, out: &mut [f32]) {
        let previews = match self.shared.preview.try_lock() {
            Ok(mut p) if !p.is_empty() => std::mem::take(&mut *p),
            _ => Vec::new(),
        };
        for (sample, gain) in previews {
            self.trigger(sample, None, gain, 0.0);
        }
        let shared = self.shared.clone();
        let pattern = shared.pattern.lock().unwrap();
        let playing = shared.playing.load(Ordering::Relaxed);
        if playing && !self.was_playing {
            self.next_step = 0;
            self.until_next = 0.0;
        }
        if !playing && self.was_playing {
            self.shared.step.store(0, Ordering::Relaxed);
        }
        self.was_playing = playing;
        let master = pattern.master;

        for frame in out.iter_mut() {
            if playing {
                while self.until_next <= 0.0 {
                    self.step(&pattern);
                }
                self.until_next -= 1.0;
            }
            let mut mix = 0.0f32;
            for v in self.voices.iter_mut() {
                let data = &self.samples[v.sample].data;
                let i = v.pos as usize;
                if i + 1 >= data.len() {
                    v.gain = 0.0;
                    continue;
                }
                let frac = (v.pos - i as f64) as f32;
                let s = data[i] + (data[i + 1] - data[i]) * frac;
                let mut g = v.gain;
                if let Some(f) = v.fade.as_mut() {
                    *f -= 1.0 / FADE_FRAMES;
                    g *= f.max(0.0);
                }
                mix += s * g;
                v.pos += v.rate;
            }
            self.voices.retain(|v| v.gain > 0.0 && v.fade.is_none_or(|f| f > 0.0));
            // Zachte clipper, voor dat warme overstuurde 90s gevoel.
            *frame = (mix * master * 1.4).tanh();
            if self.scope_buf.len() < SCOPE_LEN {
                self.scope_buf.push(*frame);
            }
        }
        drop(pattern);

        if self.scope_buf.len() >= SCOPE_LEN {
            if let Ok(mut s) = self.shared.scope.try_lock() {
                s.copy_from_slice(&self.scope_buf);
            }
            self.scope_buf.clear();
        }
    }
}

pub enum Output {
    Device(#[allow(dead_code)] cpal::Stream, u32),
    /// Geen geluidskaart: de sequencer loopt stil door op een eigen klok.
    Silent,
}

pub fn start(samples: Arc<Vec<Sample>>, shared: Arc<Shared>) -> Output {
    match open_device(samples.clone(), shared.clone()) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("sequencer: geen audio ({e}), ik draai stil door");
            std::thread::spawn(move || {
                let mut engine = Engine::new(samples, shared, 44_100);
                let mut buf = vec![0.0; 441];
                loop {
                    engine.render(&mut buf);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            });
            Output::Silent
        }
    }
}

fn open_device(samples: Arc<Vec<Sample>>, shared: Arc<Shared>) -> Result<Output, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("geen output device")?;
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let rate = config.sample_rate;
    let engine = Engine::new(samples, shared, rate);
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, engine)?,
        cpal::SampleFormat::I16 => build::<i16>(&device, config, engine)?,
        cpal::SampleFormat::I32 => build::<i32>(&device, config, engine)?,
        cpal::SampleFormat::U16 => build::<u16>(&device, config, engine)?,
        other => return Err(format!("sample format {other} niet ondersteund").into()),
    };
    stream.play()?;
    Ok(Output::Device(stream, rate))
}

fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, mut engine: Engine) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut mono = Vec::new();
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            mono.resize(data.len() / channels, 0.0);
            engine.render(&mut mono);
            for (frame, s) in data.chunks_mut(channels).zip(&mono) {
                frame.fill(T::from_sample(*s));
            }
        },
        |e| eprintln!("sequencer: audio fout: {e}"),
        None,
    )
}
