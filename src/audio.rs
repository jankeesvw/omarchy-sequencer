use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::pattern::{Cell, MAX_TRACKS, Song};
use crate::samples::Sample;

pub const SCOPE_LEN: usize = 1024;
const FADE_FRAMES: f32 = 96.0;
const MAX_VOICES: usize = 64;

/// Alles wat UI en audio-thread delen.
pub struct Shared {
    pub song: Mutex<Song>,
    pub playing: AtomicBool,
    pub step: AtomicUsize,
    /// Het patroon dat de engine op dit moment speelt (na een wissel op de maatgrens).
    pub active: AtomicUsize,
    /// Per track een piekniveau (f32 bits) voor de meters.
    pub levels: [AtomicU32; MAX_TRACKS],
    pub scope: Mutex<Vec<f32>>,
    /// Sample-previews die de UI aanvraagt (sample index, gain).
    pub preview: Mutex<Vec<(usize, f32)>>,
    /// Nieuwe samples (opnames) die de engine aan zijn lijst moet toevoegen.
    pub incoming: Mutex<Vec<Arc<Sample>>>,
}

impl Shared {
    pub fn new(song: Song) -> Self {
        Self {
            active: AtomicUsize::new(song.current),
            song: Mutex::new(song),
            playing: AtomicBool::new(false),
            step: AtomicUsize::new(0),
            levels: std::array::from_fn(|_| AtomicU32::new(0)),
            scope: Mutex::new(vec![0.0; SCOPE_LEN]),
            preview: Mutex::new(Vec::new()),
            incoming: Mutex::new(Vec::new()),
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
    gain_l: f32,
    gain_r: f32,
    send: f32,
    fade: Option<f32>,
    /// Frames tot de noot wordt afgekapt; None speelt de sample helemaal uit.
    gate: Option<f64>,
}

pub struct Engine {
    samples: Vec<Arc<Sample>>,
    shared: Arc<Shared>,
    out_rate: f64,
    voices: Vec<Voice>,
    until_next: f64,
    next_step: usize,
    active: usize,
    was_playing: bool,
    scope_buf: Vec<f32>,
    delay: Vec<[f32; 2]>,
    delay_pos: usize,
    filter: [[f32; 2]; 2],
    cutoff: f32,
}

impl Engine {
    fn new(samples: Vec<Arc<Sample>>, shared: Arc<Shared>, out_rate: u32) -> Self {
        Self {
            samples,
            shared,
            out_rate: out_rate as f64,
            voices: Vec::with_capacity(MAX_VOICES),
            until_next: 0.0,
            next_step: 0,
            active: 0,
            was_playing: false,
            scope_buf: Vec::with_capacity(SCOPE_LEN),
            delay: vec![[0.0; 2]; out_rate as usize * 3],
            delay_pos: 0,
            filter: [[0.0; 2]; 2],
            cutoff: 1.0,
        }
    }

    fn step_len(&self, bpm: f32) -> f64 {
        self.out_rate * 60.0 / bpm as f64 / 4.0
    }

    #[allow(clippy::too_many_arguments)]
    fn trigger(&mut self, sample: usize, track: Option<usize>, gain: f32, pitch: f32, pan: f32, send: f32, gate: Option<f64>) {
        if sample >= self.samples.len() {
            return;
        }
        // Eén stem per track: de vorige wordt kort uitgefade (choke), dat klinkt strak bij hats en riffs.
        if track.is_some() {
            for v in self.voices.iter_mut().filter(|v| v.track == track && v.fade.is_none()) {
                v.fade = Some(1.0);
            }
        }
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        let s = &self.samples[sample];
        let rate = s.rate as f64 / self.out_rate * 2f64.powf(pitch as f64 / 12.0);
        // Equal-power panning.
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        self.voices.push(Voice {
            sample,
            track,
            pos: 0.0,
            rate,
            gain_l: gain * angle.cos(),
            gain_r: gain * angle.sin(),
            send,
            fade: None,
            gate,
        });
    }

    fn step(&mut self, song: &Song) {
        if self.next_step >= song.steps[self.active].max(1) {
            self.next_step = 0;
            if let Some(q) = song.queued {
                self.active = q;
                self.shared.active.store(q, Ordering::Relaxed);
            }
        }
        let step = self.next_step.min(song.steps[self.active].max(1) - 1);
        self.shared.step.store(step, Ordering::Relaxed);
        let solo = song.any_solo();
        let len = self.step_len(song.bpm);
        for (i, t) in song.tracks.iter().enumerate() {
            let lane = &t.lanes[self.active];
            let cell = lane.cells[step];
            let audible = if solo { t.solo } else { !t.mute };
            if cell != Cell::Off && audible {
                let accent = if cell == Cell::Accent { 1.0 } else { 0.62 };
                // Lange noten (meer dan één blokje) klinken precies zo lang als ze in het grid staan.
                let blocks = lane.lens[step] as f64;
                let gate = (blocks > 1.0).then_some(blocks * len);
                self.trigger(t.sample, Some(i), t.volume * accent, t.pitch, t.pan, t.send, gate);
                self.shared.levels[i].store(accent.to_bits(), Ordering::Relaxed);
            }
        }
        // Swing: even stappen lang, oneven kort.
        let swing = song.swing as f64;
        self.until_next += if step % 2 == 0 { len * (1.0 + swing) } else { len * (1.0 - swing) };
        self.next_step = step + 1;
    }

    /// Rendert interleaved stereo.
    pub fn render(&mut self, out: &mut [f32]) {
        let previews = match self.shared.preview.try_lock() {
            Ok(mut p) if !p.is_empty() => std::mem::take(&mut *p),
            _ => Vec::new(),
        };
        if let Ok(mut inc) = self.shared.incoming.try_lock() {
            self.samples.append(&mut inc);
        }
        for (sample, gain) in previews {
            self.trigger(sample, None, gain, 0.0, 0.0, 0.0, None);
        }
        let shared = self.shared.clone();
        let song = shared.song.lock().unwrap();
        let playing = shared.playing.load(Ordering::Relaxed);
        if playing && !self.was_playing {
            self.next_step = 0;
            self.until_next = 0.0;
            self.active = song.current;
            self.shared.active.store(self.active, Ordering::Relaxed);
        }
        if !playing {
            self.active = song.current;
            if self.was_playing {
                self.shared.step.store(0, Ordering::Relaxed);
            }
        }
        self.was_playing = playing;

        let master = song.master;
        let delay_len = ((song.delay_steps as f64 * self.step_len(song.bpm)) as usize).clamp(1, self.delay.len() - 1);
        let feedback = song.feedback;
        let target_cutoff = song.cutoff;
        let nyquist_guard = self.out_rate as f32 / 6.0;

        for frame in out.chunks_mut(2) {
            if playing {
                while self.until_next <= 0.0 {
                    self.step(&song);
                }
                self.until_next -= 1.0;
            }
            let (mut l, mut r, mut send_l, mut send_r) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for v in self.voices.iter_mut() {
                let data = &self.samples[v.sample].data;
                let i = v.pos as usize;
                if i + 1 >= data.len() {
                    v.gain_l = 0.0;
                    v.gain_r = 0.0;
                    continue;
                }
                let frac = (v.pos - i as f64) as f32;
                let mut s = data[i] + (data[i + 1] - data[i]) * frac;
                if let Some(gate) = v.gate.as_mut() {
                    *gate -= 1.0;
                    if *gate <= 0.0 && v.fade.is_none() {
                        v.fade = Some(1.0);
                    }
                }
                if let Some(f) = v.fade.as_mut() {
                    *f -= 1.0 / FADE_FRAMES;
                    s *= f.max(0.0);
                }
                l += s * v.gain_l;
                r += s * v.gain_r;
                send_l += s * v.gain_l * v.send;
                send_r += s * v.gain_r * v.send;
                v.pos += v.rate;
            }
            self.voices.retain(|v| (v.gain_l > 0.0 || v.gain_r > 0.0) && v.fade.is_none_or(|f| f > 0.0));

            // Ping-pong delay op tempo.
            let read = (self.delay_pos + self.delay.len() - delay_len) % self.delay.len();
            let [dl, dr] = self.delay[read];
            self.delay[self.delay_pos] = [send_l + dr * feedback, send_r + dl * feedback];
            self.delay_pos = (self.delay_pos + 1) % self.delay.len();
            l += dl * 0.8;
            r += dr * 0.8;

            // Master lowpass (state variable); de cutoff glijdt mee om klikken te voorkomen.
            self.cutoff += (target_cutoff - self.cutoff) * 0.001;
            if self.cutoff < 0.995 {
                let hz = 60.0 * (20_000.0f32 / 60.0).powf(self.cutoff);
                let f = 2.0 * (std::f32::consts::PI * hz.min(nyquist_guard) / self.out_rate as f32).sin();
                for (ch, x) in [&mut l, &mut r].into_iter().enumerate() {
                    let [low, band] = &mut self.filter[ch];
                    *low += f * *band;
                    let high = *x - *low - 0.6 * *band;
                    *band += f * high;
                    *x = *low;
                }
            }
            // Zachte clipper op de master.
            frame[0] = (l * master * 1.4).tanh();
            frame[1] = (r * master * 1.4).tanh();
            if self.scope_buf.len() < SCOPE_LEN {
                self.scope_buf.push((frame[0] + frame[1]) * 0.5);
            }
        }
        drop(song);

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

pub fn start(samples: Vec<Arc<Sample>>, shared: Arc<Shared>) -> Output {
    match open_device(samples.clone(), shared.clone()) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("sequencer: geen audio ({e}), ik draai stil door");
            std::thread::spawn(move || {
                let mut engine = Engine::new(samples, shared, 44_100);
                let mut buf = vec![0.0; 882];
                loop {
                    engine.render(&mut buf);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            });
            Output::Silent
        }
    }
}

fn open_device(samples: Vec<Arc<Sample>>, shared: Arc<Shared>) -> Result<Output, Box<dyn std::error::Error>> {
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
    let mut stereo = Vec::new();
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels;
            stereo.resize(frames * 2, 0.0);
            engine.render(&mut stereo);
            for (frame, s) in data.chunks_mut(channels).zip(stereo.chunks(2)) {
                if frame.len() == 1 {
                    frame[0] = T::from_sample((s[0] + s[1]) * 0.5);
                } else {
                    frame[0] = T::from_sample(s[0]);
                    frame[1] = T::from_sample(s[1]);
                    for x in &mut frame[2..] {
                        *x = T::from_sample(0.0);
                    }
                }
            }
        },
        |e| eprintln!("sequencer: audio fout: {e}"),
        None,
    )
}

/// Rendert het huidige patroon een aantal keer offline naar een stereo WAV in ~/Music.
pub fn export(samples: Vec<Arc<Sample>>, song: &Song, loops: usize) -> Result<std::path::PathBuf, String> {
    const RATE: u32 = 44_100;
    let mut song = song.clone();
    song.queued = None;
    let shared = Arc::new(Shared::new(song.clone()));
    shared.playing.store(true, Ordering::Relaxed);
    let mut engine = Engine::new(samples, shared.clone(), RATE);
    let bar = song.steps() as f64 * RATE as f64 * 60.0 / song.bpm as f64 / 4.0;
    let music = bar * loops as f64;
    let frames = music as usize + RATE as usize * 2;

    let dir = dirs::audio_dir().or_else(dirs::home_dir).ok_or("geen muziekmap")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let letter = (b'a' + song.current as u8) as char;
    let path = (1..)
        .map(|i| dir.join(format!("sequencer-{letter}-{:.0}bpm-{i:02}.wav", song.bpm)))
        .find(|p| !p.exists())
        .unwrap();
    let spec = hound::WavSpec { channels: 2, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
    let mut buf = vec![0.0f32; 1024];
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(512);
        // Na de laatste loop stopt de sequencer, zodat alleen de staart van delay en samples nog klinkt.
        if done as f64 >= music {
            shared.playing.store(false, Ordering::Relaxed);
        }
        engine.render(&mut buf[..n * 2]);
        for s in &buf[..n * 2] {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).map_err(|e| e.to_string())?;
        }
        done += n;
    }
    w.finalize().map_err(|e| e.to_string())?;
    Ok(path)
}

/// Push-to-talk opname van de standaard microfoon, net als Voxtype: de input stream staat alleen open zolang je opneemt.
pub struct Recorder {
    stream: Option<cpal::Stream>,
    buf: Arc<Mutex<Vec<f32>>>,
    pub level: Arc<AtomicU32>,
    rate: u32,
}

impl Recorder {
    pub fn new() -> Self {
        Self { stream: None, buf: Arc::new(Mutex::new(Vec::new())), level: Arc::new(AtomicU32::new(0)), rate: 44_100 }
    }

    pub fn recording(&self) -> bool {
        self.stream.is_some()
    }

    pub fn start(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("geen microfoon gevonden")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        self.rate = config.sample_rate;
        self.buf.lock().unwrap().clear();
        let stream = match format {
            cpal::SampleFormat::F32 => self.build::<f32>(&device, config),
            cpal::SampleFormat::I16 => self.build::<i16>(&device, config),
            cpal::SampleFormat::I32 => self.build::<i32>(&device, config),
            other => return Err(format!("microfoonformaat {other} niet ondersteund")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }

    fn build<T>(&self, device: &cpal::Device, config: cpal::StreamConfig) -> Result<cpal::Stream, cpal::Error>
    where
        T: cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let channels = config.channels as usize;
        let max = config.sample_rate as usize * 20;
        let buf = self.buf.clone();
        let level = self.level.clone();
        device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut b = buf.lock().unwrap();
                let mut peak = 0.0f32;
                for frame in data.chunks(channels) {
                    let s = frame.iter().map(|x| <f32 as cpal::FromSample<T>>::from_sample_(*x)).sum::<f32>() / channels as f32;
                    peak = peak.max(s.abs());
                    // Maximaal 20 seconden.
                    if b.len() < max {
                        b.push(s);
                    }
                }
                level.store(peak.to_bits(), Ordering::Relaxed);
            },
            |e| eprintln!("sequencer: microfoon fout: {e}"),
            None,
        )
    }

    /// Stopt de opname en geeft de ruwe audio terug.
    pub fn stop(&mut self) -> (Vec<f32>, u32) {
        self.stream = None;
        self.level.store(0, Ordering::Relaxed);
        (std::mem::take(&mut *self.buf.lock().unwrap()), self.rate)
    }
}
