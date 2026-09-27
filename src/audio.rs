use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub use crate::engine::*;
use crate::pattern::Song;
use crate::samples::Sample;


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
    let dir = dirs::audio_dir().or_else(dirs::home_dir).ok_or("no music folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let letter = (b'a' + song.current as u8) as char;
    let path = (1..)
        .map(|i| dir.join(format!("sequencer-{letter}-{:.0}bpm-{i:02}.wav", song.bpm)))
        .find(|p| !p.exists())
        .unwrap();
    render(samples, song, loops, &path)?;
    Ok(path)
}

/// Renders the current pattern `loops` times, plus the tail of the effects, to a stereo WAV at `path`.
pub fn render(samples: Vec<Arc<Sample>>, song: &Song, loops: usize, path: &std::path::Path) -> Result<(), String> {
    const RATE: u32 = 44_100;
    let spec = hound::WavSpec { channels: 2, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for s in render_frames(samples, song, loops, RATE) {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
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
