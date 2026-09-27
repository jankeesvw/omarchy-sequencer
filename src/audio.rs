use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::pattern::{Cell, FilterKind, Fx, MAX_TRACKS, Song};
use crate::samples::Sample;

pub const SCOPE_LEN: usize = 1024;
/// Longest recording; it stops growing after this.
pub const MAX_RECORDING_SECONDS: usize = 60;
const FADE_FRAMES: f32 = 96.0;
const MAX_VOICES: usize = 64;

/// Everything the UI and the audio thread share.
pub struct Shared {
    pub song: Mutex<Song>,
    pub playing: AtomicBool,
    pub step: AtomicUsize,
    /// The pattern the engine is playing right now (after a switch at the end of a bar).
    pub active: AtomicUsize,
    /// Peak level per track (f32 bits) for the meters.
    pub levels: [AtomicU32; MAX_TRACKS],
    pub scope: Mutex<Vec<f32>>,
    /// Sample previews requested by the UI (sample index, gain, track to play through).
    pub preview: Mutex<Vec<(usize, f32, Option<usize>)>>,
    /// New samples (recordings) the engine should add to its list.
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
    fade: Option<f32>,
    /// Frames until the note is cut off; None plays the whole sample.
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
    /// One ping-pong delay line per track.
    delays: Vec<Delay>,
    /// Per track: what the voices played this frame, and the state of its effects.
    bus: [[f32; 2]; MAX_TRACKS],
    fx: [FxState; MAX_TRACKS],
    reverb: Reverb,
    /// Frames since playback started, for tempo synced effects.
    clock: f64,
}

/// One biquad section (RBJ cookbook), stereo.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
}

impl Biquad {
    fn set(&mut self, kind: u8, hz: f32, db: f32, rate: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * std::f32::consts::PI * hz / rate;
        let (sin, cos) = w.sin_cos();
        let (b0, b1, b2, a0, a1, a2);
        match kind {
            // Low shelf and high shelf with a slope of 1.
            0 | 2 => {
                let alpha = sin / 2.0 * std::f32::consts::SQRT_2;
                let s = if kind == 0 { 1.0 } else { -1.0 };
                let sq = 2.0 * a.sqrt() * alpha;
                b0 = a * ((a + 1.0) - s * (a - 1.0) * cos + sq);
                b1 = s * 2.0 * a * ((a - 1.0) - s * (a + 1.0) * cos);
                b2 = a * ((a + 1.0) - s * (a - 1.0) * cos - sq);
                a0 = (a + 1.0) + s * (a - 1.0) * cos + sq;
                a1 = -s * 2.0 * ((a - 1.0) + s * (a + 1.0) * cos);
                a2 = (a + 1.0) + s * (a - 1.0) * cos - sq;
            }
            // Peak with a Q of 0.8.
            _ => {
                let alpha = sin / (2.0 * 0.8);
                b0 = 1.0 + alpha * a;
                b1 = -2.0 * cos;
                b2 = 1.0 - alpha * a;
                a0 = 1.0 + alpha / a;
                a1 = -2.0 * cos;
                a2 = 1.0 - alpha / a;
            }
        }
        self.b = [b0 / a0, b1 / a0, b2 / a0];
        self.a = [a1 / a0, a2 / a0];
    }

    fn process(&mut self, x: [f32; 2]) -> [f32; 2] {
        let mut y = [0.0; 2];
        for ch in 0..2 {
            let [z1, z2] = &mut self.z[ch];
            y[ch] = self.b[0] * x[ch] + *z1;
            *z1 = self.b[1] * x[ch] - self.a[0] * y[ch] + *z2;
            *z2 = self.b[2] * x[ch] - self.a[1] * y[ch];
            if !y[ch].is_finite() {
                *z1 = 0.0;
                *z2 = 0.0;
                y[ch] = 0.0;
            }
        }
        y
    }
}

#[derive(Clone, Copy, Default)]
struct FxState {
    /// State variable filter: [low, band] per channel.
    svf: [[f32; 2]; 2],
    hold: [f32; 2],
    phase: f32,
    ring_phase: f32,
    chop_gain: f32,
    eq: [Biquad; 3],
    /// The EQ settings the biquads were computed for.
    eq_for: [f32; 3],
}

impl FxState {
    /// `beat` is the position within the chop cycle, 0..1.
    fn process(&mut self, fx: &Fx, mut x: [f32; 2], rate: f32, beat: f32) -> [f32; 2] {
        let eq = [fx.eq_low, fx.eq_mid, fx.eq_high];
        if eq != [0.0; 3] {
            if eq != self.eq_for {
                for (band, (hz, db)) in [(100.0, eq[0]), (1000.0, eq[1]), (8000.0, eq[2])].into_iter().enumerate() {
                    self.eq[band].set(band as u8, hz, db, rate);
                }
                self.eq_for = eq;
            }
            for band in &mut self.eq {
                x = band.process(x);
            }
        }
        if fx.drive > 0.0 {
            let g = 1.0 + fx.drive * 12.0;
            for s in &mut x {
                *s = (*s * g).tanh() / g.tanh() * (1.0 - fx.drive * 0.4);
            }
        }
        if fx.distort > 0.0 {
            // Hard clipping with lots of gain in front, then brought back down.
            let g = 1.0 + fx.distort * 40.0;
            let ceiling = 0.35;
            for s in &mut x {
                *s = (*s * g).clamp(-ceiling, ceiling) * (1.0 / (1.0 + fx.distort * 1.5));
            }
        }
        if fx.ring > 0.0 {
            let hz = 30.0 * (2000.0f32 / 30.0).powf(fx.ring_freq);
            self.ring_phase = (self.ring_phase + hz / rate).fract();
            let m = (self.ring_phase * std::f32::consts::TAU).sin();
            for s in &mut x {
                *s = *s * (1.0 - fx.ring) + *s * m * fx.ring;
            }
        }
        if fx.downsample > 0.0 {
            let n = 1.0 + fx.downsample * 23.0;
            self.phase += 1.0;
            if self.phase >= n {
                self.phase -= n;
                self.hold = x;
            }
            x = self.hold;
        }
        if fx.crush > 0.0 {
            let levels = 2f32.powf(15.0 - fx.crush * 13.0);
            for s in &mut x {
                *s = (*s * levels).round() / levels;
            }
        }
        if fx.filter != FilterKind::Off {
            let hz = 40.0 * (18_000.0f32 / 40.0).powf(fx.cutoff);
            let f = 2.0 * (std::f32::consts::PI * hz.min(rate / 6.0) / rate).sin();
            let q = 1.5 - fx.resonance * 1.4;
            for (ch, s) in x.iter_mut().enumerate() {
                let [low, band] = &mut self.svf[ch];
                *low += f * *band;
                let high = *s - *low - q * *band;
                *band += f * high;
                *s = match fx.filter {
                    FilterKind::Low => *low,
                    FilterKind::High => high,
                    FilterKind::Band => *band,
                    FilterKind::Off => *s,
                };
                if !low.is_finite() || !band.is_finite() {
                    *low = 0.0;
                    *band = 0.0;
                    *s = 0.0;
                }
            }
        }
        // Chop: on for the first half of each cycle, with short ramps so it doesn't click.
        let target = if fx.chop > 0.0 && beat >= 0.5 { 1.0 - fx.chop } else { 1.0 };
        self.chop_gain += (target - self.chop_gain) * 0.004;
        if fx.chop > 0.0 || self.chop_gain < 0.999 {
            x[0] *= self.chop_gain;
            x[1] *= self.chop_gain;
        }
        x
    }
}

/// Tempo synced ping-pong delay for one track.
struct Delay {
    buf: Vec<[f32; 2]>,
    pos: usize,
    /// Frames since anything went in, so an idle delay costs nothing.
    idle: usize,
}

impl Delay {
    fn new(rate: u32) -> Self {
        Self { buf: vec![[0.0; 2]; rate as usize * 3], pos: 0, idle: usize::MAX }
    }

    fn process(&mut self, input: [f32; 2], len: usize, feedback: f32) -> [f32; 2] {
        if input == [0.0; 2] {
            // Silent input and the echoes have died out: nothing to do.
            if self.idle > self.buf.len() * 8 {
                return [0.0; 2];
            }
            self.idle = self.idle.saturating_add(1);
        } else {
            self.idle = 0;
        }
        let len = len.clamp(1, self.buf.len() - 1);
        let read = (self.pos + self.buf.len() - len) % self.buf.len();
        let [dl, dr] = self.buf[read];
        self.buf[self.pos] = [input[0] + dr * feedback, input[1] + dl * feedback];
        self.pos = (self.pos + 1) % self.buf.len();
        [dl * 0.8, dr * 0.8]
    }
}

/// A small Freeverb style reverb: parallel combs into series allpasses, per channel.
struct Reverb {
    combs: [[(Vec<f32>, usize, f32); 4]; 2],
    allpasses: [[(Vec<f32>, usize); 2]; 2],
}

impl Reverb {
    fn new(rate: u32) -> Self {
        let scale = rate as f32 / 44_100.0;
        let len = |n: usize, ch: usize| ((n + ch * 23) as f32 * scale) as usize;
        Self {
            combs: std::array::from_fn(|ch| std::array::from_fn(|i| (vec![0.0; len([1116, 1188, 1277, 1356][i], ch)], 0, 0.0))),
            allpasses: std::array::from_fn(|ch| std::array::from_fn(|i| (vec![0.0; len([556, 441][i], ch)], 0))),
        }
    }

    fn process(&mut self, x: [f32; 2]) -> [f32; 2] {
        let mut out = [0.0; 2];
        for ch in 0..2 {
            let input = x[ch] * 0.2;
            let mut sum = 0.0;
            for (buf, pos, store) in &mut self.combs[ch] {
                let y = buf[*pos];
                *store = y * 0.75 + *store * 0.25;
                buf[*pos] = input + *store * 0.84;
                *pos = (*pos + 1) % buf.len();
                sum += y;
            }
            for (buf, pos) in &mut self.allpasses[ch] {
                let b = buf[*pos];
                buf[*pos] = sum + b * 0.5;
                sum = b - sum;
                *pos = (*pos + 1) % buf.len();
            }
            out[ch] = sum;
        }
        out
    }
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
            delays: (0..MAX_TRACKS).map(|_| Delay::new(out_rate)).collect(),
            bus: [[0.0; 2]; MAX_TRACKS],
            fx: [FxState::default(); MAX_TRACKS],
            reverb: Reverb::new(out_rate),
            clock: 0.0,
        }
    }

    fn step_len(&self, bpm: f32) -> f64 {
        self.out_rate * 60.0 / bpm as f64 / 4.0
    }

    #[allow(clippy::too_many_arguments)]
    fn trigger(&mut self, sample: usize, track: Option<usize>, gain: f32, pitch: f32, pan: f32, reverse: bool, gate: Option<f64>) {
        if sample >= self.samples.len() {
            return;
        }
        // One voice per track: the previous one gets a short fade out (choke), which keeps hats and riffs tight.
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
        // Backwards: start at the end and step down.
        let (pos, rate) = if reverse { ((s.data.len().saturating_sub(2)) as f64, -rate) } else { (0.0, rate) };
        // Equal-power panning.
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        self.voices.push(Voice {
            sample,
            track,
            pos,
            rate,
            gain_l: gain * angle.cos(),
            gain_r: gain * angle.sin(),
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
                // Long notes (more than one step) sound exactly as long as they are in the grid.
                let blocks = lane.lens[step] as f64;
                let gate = (blocks > 1.0).then_some(blocks * len);
                self.trigger(t.sample, Some(i), t.volume * accent, t.pitch + t.fx.fine / 100.0, t.pan, t.fx.reverse, gate);
                self.shared.levels[i].store(accent.to_bits(), Ordering::Relaxed);
            }
        }
        // Swing: even steps long, odd steps short.
        let swing = song.swing as f64;
        self.until_next += if step % 2 == 0 { len * (1.0 + swing) } else { len * (1.0 - swing) };
        self.next_step = step + 1;
    }

    /// Renders interleaved stereo.
    pub fn render(&mut self, out: &mut [f32]) {
        let previews = match self.shared.preview.try_lock() {
            Ok(mut p) if !p.is_empty() => std::mem::take(&mut *p),
            _ => Vec::new(),
        };
        if let Ok(mut inc) = self.shared.incoming.try_lock() {
            self.samples.append(&mut inc);
        }
        let shared = self.shared.clone();
        let song = shared.song.lock().unwrap();
        for (sample, gain, track) in previews {
            // A preview through a track uses its pitch, pan and effects.
            match track.and_then(|i| song.tracks.get(i).map(|t| (i, t))) {
                Some((i, t)) => self.trigger(sample, Some(i), gain, t.pitch + t.fx.fine / 100.0, t.pan, t.fx.reverse, None),
                None => self.trigger(sample, None, gain, 0.0, 0.0, false, None),
            }
        }
        let playing = shared.playing.load(Ordering::Relaxed);
        if playing && !self.was_playing {
            self.clock = 0.0;
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
        let step_len = self.step_len(song.bpm);

        for frame in out.chunks_mut(2) {
            if playing {
                while self.until_next <= 0.0 {
                    self.step(&song);
                }
                self.until_next -= 1.0;
            }
            // Runs while stopped too, so chop is heard in a preview.
            self.clock += 1.0;
            let (mut l, mut r) = (0.0f32, 0.0f32);
            self.bus = [[0.0; 2]; MAX_TRACKS];
            for v in self.voices.iter_mut() {
                let data = &self.samples[v.sample].data;
                if v.pos < 0.0 {
                    v.gain_l = 0.0;
                    v.gain_r = 0.0;
                    continue;
                }
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
                match v.track {
                    Some(t) if t < MAX_TRACKS => {
                        self.bus[t][0] += s * v.gain_l;
                        self.bus[t][1] += s * v.gain_r;
                    }
                    _ => {
                        l += s * v.gain_l;
                        r += s * v.gain_r;
                    }
                }
                v.pos += v.rate;
            }
            // Each track through its own effects; the sends are taken after them.
            let (mut rev_l, mut rev_r) = (0.0f32, 0.0f32);
            for (i, t) in song.tracks.iter().enumerate().take(MAX_TRACKS) {
                let cycle = step_len * t.fx.chop_steps.max(1) as f64 * 2.0;
                let beat = ((self.clock / cycle).fract()) as f32;
                let [tl, tr] = self.fx[i].process(&t.fx, self.bus[i], self.out_rate as f32, beat);
                l += tl;
                r += tr;
                let [dl, dr] = self.delays[i].process([tl * t.send, tr * t.send], (t.fx.delay_steps as f64 * step_len) as usize, t.fx.feedback);
                l += dl;
                r += dr;
                rev_l += tl * t.fx.reverb;
                rev_r += tr * t.fx.reverb;
            }
            let [wl, wr] = self.reverb.process([rev_l, rev_r]);
            l += wl;
            r += wr;
            self.voices.retain(|v| (v.gain_l > 0.0 || v.gain_r > 0.0) && v.fade.is_none_or(|f| f > 0.0));

            // Soft clipper on the master.
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
    /// No sound card: the sequencer keeps running silently on its own clock.
    Silent,
}

pub fn start(samples: Vec<Arc<Sample>>, shared: Arc<Shared>) -> Output {
    match open_device(samples.clone(), shared.clone()) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("omarchy-sequencer: no audio ({e}), running silently");
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
    let device = host.default_output_device().ok_or("no output device")?;
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
        other => return Err(format!("sample format {other} not supported").into()),
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
        |e| eprintln!("omarchy-sequencer: audio error: {e}"),
        None,
    )
}

/// Renders the current pattern a number of times offline to a stereo WAV in ~/Music.
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

    let dir = dirs::audio_dir().or_else(dirs::home_dir).ok_or("no music folder")?;
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
        // After the last loop the sequencer stops, so only the tail of the delay and samples remains.
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

/// Push-to-talk recording from the default microphone, like Voxtype: the input stream is only open while recording.
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
        let device = host.default_input_device().ok_or("no microphone found")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        self.rate = config.sample_rate;
        self.buf.lock().unwrap().clear();
        let stream = match format {
            cpal::SampleFormat::F32 => self.build::<f32>(&device, config),
            cpal::SampleFormat::I16 => self.build::<i16>(&device, config),
            cpal::SampleFormat::I32 => self.build::<i32>(&device, config),
            other => return Err(format!("microphone format {other} not supported")),
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
        let max = config.sample_rate as usize * MAX_RECORDING_SECONDS;
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
                    if b.len() < max {
                        b.push(s);
                    }
                }
                level.store(peak.to_bits(), Ordering::Relaxed);
            },
            |e| eprintln!("omarchy-sequencer: microphone error: {e}"),
            None,
        )
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The last `seconds` of the recording, for the live waveform.
    pub fn tail(&self, seconds: f32) -> Vec<f32> {
        let buf = self.buf.lock().unwrap();
        let n = ((self.rate as f32 * seconds) as usize).min(buf.len());
        buf[buf.len() - n..].to_vec()
    }

    /// How long it has been recording.
    pub fn length(&self) -> f32 {
        self.buf.lock().unwrap().len() as f32 / self.rate as f32
    }

    /// Stops recording and returns the raw audio.
    pub fn stop(&mut self) -> (Vec<f32>, u32) {
        self.stream = None;
        self.level.store(0, Ordering::Relaxed);
        (std::mem::take(&mut *self.buf.lock().unwrap()), self.rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::FilterKind;

    fn render(song: Song, seconds: f32) -> Vec<f32> {
        let samples = crate::samples::load_all();
        let shared = Arc::new(Shared::new(song));
        shared.playing.store(true, Ordering::Relaxed);
        let mut engine = Engine::new(samples, shared, 44_100);
        let mut out = vec![0.0; (44_100.0 * seconds) as usize * 2];
        for chunk in out.chunks_mut(1024) {
            engine.render(chunk);
        }
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    #[test]
    fn every_effect_at_once_stays_finite_and_audible() {
        let samples = crate::samples::load_all();
        let mut song = Song::rave(&samples);
        for (i, t) in song.tracks.iter_mut().enumerate() {
            let fx = &mut t.fx;
            fx.filter = [FilterKind::Low, FilterKind::High, FilterKind::Band][i % 3];
            fx.cutoff = 0.9;
            fx.resonance = 1.0;
            fx.drive = 1.0;
            fx.distort = 1.0;
            fx.crush = 1.0;
            fx.downsample = 1.0;
            fx.ring = 1.0;
            fx.chop = 1.0;
            fx.reverb = 1.0;
            fx.reverse = i % 2 == 0;
            fx.fine = 50.0;
            fx.eq_low = 12.0;
            fx.eq_mid = -12.0;
            fx.eq_high = 12.0;
            t.send = 1.0;
        }
        let out = render(song, 4.0);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(peak(&out) > 0.05, "silent: {}", peak(&out));
    }

    #[test]
    fn a_filter_takes_out_the_highs() {
        let samples = crate::samples::load_all();
        let dry = Song::rave(&samples);
        let mut wet = dry.clone();
        for t in &mut wet.tracks {
            t.fx.filter = FilterKind::Low;
            t.fx.cutoff = 0.2;
        }
        // Sum of absolute differences between neighbours: a rough measure of high frequencies.
        let roughness = |x: &[f32]| x.chunks(2).map(|f| f[0]).collect::<Vec<_>>().windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
        assert!(roughness(&render(wet, 2.0)) < roughness(&render(dry, 2.0)) * 0.5);
    }
}
