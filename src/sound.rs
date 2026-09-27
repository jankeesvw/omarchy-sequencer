//! A sample in memory, and reading one from a WAV. No files or folders here, so it also runs in the browser.

pub struct Sample {
    pub name: String,
    /// Where it comes from: "classic" (built in), "user" (your own and recordings) or a pack id.
    pub pack: String,
    /// Unique across packs, used in song files: the name for built-in and own samples, "pack/name" otherwise.
    pub id: String,
    pub kind: Kind,
    pub data: Vec<f32>,
    pub rate: u32,
}

impl Sample {
    /// How many steps a new note spans by default: loops a whole bar, riffs one beat.
    pub fn default_len(&self) -> u8 {
        match self.kind {
            _ if self.name.contains("loop") => 16,
            Kind::Riff | Kind::Rave => 4,
            Kind::Drum | Kind::Vox | Kind::User | Kind::Pack => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Drum,
    Riff,
    Rave,
    Vox,
    User,
    /// A sound from a downloaded pack that is not clearly a drum or a bass.
    Pack,
}

/// Longest sample kept in memory: packs sometimes contain long soundscapes, your own recordings can be longer.
const MAX_SECONDS: u32 = 12;
const MAX_USER_SECONDS: u32 = 60;


/// Reads a WAV into a mono sample; the id says where it comes from.
pub fn decode<R: std::io::Read>(name: &str, kind: Kind, pack: &str, reader: R) -> Option<Sample> {
    let mut wav = hound::WavReader::new(reader).ok()?;
    let spec = wav.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => wav.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            wav.samples::<i32>().filter_map(Result::ok).map(|s| s as f32 * scale).collect()
        }
    };
    let data: Vec<f32> = interleaved
        .chunks(channels)
        .take((spec.sample_rate * if pack == "user" { MAX_USER_SECONDS } else { MAX_SECONDS }) as usize)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    if data.len() < 2 {
        return None;
    }
    let id = if matches!(pack, "classic" | "user") { name.to_string() } else { format!("{pack}/{name}") };
    Some(Sample { name: name.to_string(), pack: pack.to_string(), id, kind, data, rate: spec.sample_rate })
}

/// The sample as a mono 16 bit WAV, exactly as the engine plays it.
pub fn to_wav(sample: &Sample) -> Vec<u8> {
    let spec = hound::WavSpec { channels: 1, sample_rate: sample.rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(Vec::new());
    if let Ok(mut w) = hound::WavWriter::new(&mut out, spec) {
        for s in &sample.data {
            let _ = w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16);
        }
        let _ = w.finalize();
    }
    out.into_inner()
}
