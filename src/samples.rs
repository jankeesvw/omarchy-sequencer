use std::path::Path;

pub struct Sample {
    pub name: String,
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
            Kind::Drum | Kind::Vox | Kind::User => 1,
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
}


macro_rules! embed {
    ($kind:expr, $($name:literal),* $(,)?) => {
        [$(($name, $kind, include_bytes!(concat!("../assets/samples/", $name, ".wav")).as_slice())),*]
    };
}

const DRUMS: [(&str, Kind, &[u8]); 16] = embed!(
    Kind::Drum,
    "kick", "kick_long", "snare", "snare_snappy", "clap", "hat_closed", "hat_open", "cymbal",
    "rimshot", "cowbell", "clave", "maracas", "tom_low", "tom_high", "conga_low", "conga_high",
);

const RIFFS: [(&str, Kind, &[u8]); 10] = embed!(
    Kind::Riff,
    "bass_hit", "bass_run", "bass_walk", "thunk", "note_run", "plucks", "rhodes_chord",
    "rhodes_tone", "stab", "neon_pad",
);

// Real recordings from Freesound (CC0), see README.
const RAVE: [(&str, Kind, &[u8]); 12] = embed!(
    Kind::Rave,
    "rave_stab", "jx3p_stab", "orch_hit", "hoover", "hardhouse_hoover", "mentasm", "m1_organ",
    "house_chords", "acid_line", "acid_bass", "rave_loop_135", "rave_loop_128",
);

// Producer Space (CC0): dance shouts, house vocals and computer voices.
const VOX: [(&str, Kind, &[u8]); 10] = embed!(
    Kind::Vox,
    "everybody", "here_we_go", "lets_go", "hey", "feel_the_rhythm", "clap_your_hands",
    "access_granted", "security_breach", "system_error", "were_in",
);

pub fn load_all() -> Vec<std::sync::Arc<Sample>> {
    let mut out: Vec<Sample> = DRUMS
        .iter()
        .chain(RIFFS.iter())
        .chain(RAVE.iter())
        .chain(VOX.iter())
        .filter_map(|(name, kind, bytes)| decode(name, *kind, std::io::Cursor::new(*bytes)))
        .collect();

    // Your own samples: drop .wav files in ~/.local/share/sequencer/samples
    if let Some(dir) = user_sample_dir() {
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
            .collect();
        paths.sort();
        for path in paths {
            if let Some(s) = load_file(&path) {
                out.push(s);
            }
        }
    }
    out.into_iter().map(std::sync::Arc::new).collect()
}

pub fn user_sample_dir() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("sequencer").join("samples"))
}

fn load_file(path: &Path) -> Option<Sample> {
    let name = path.file_stem()?.to_string_lossy().to_lowercase();
    let file = std::fs::File::open(path).ok()?;
    decode(&name, Kind::User, std::io::BufReader::new(file))
}

fn decode<R: std::io::Read>(name: &str, kind: Kind, reader: R) -> Option<Sample> {
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
    let data = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Some(Sample { name: name.to_string(), kind, data, rate: spec.sample_rate })
}

/// Turns a raw microphone recording into a usable sample: silence trimmed, normalized,
/// saved as `rec_NN.wav` in the user sample folder.
pub fn save_recording(raw: &[f32], rate: u32) -> Result<Sample, String> {
    let peak = raw.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak < 0.01 {
        return Err("nothing heard, is the microphone on?".into());
    }
    let threshold = (peak * 0.06).max(0.005);
    let first = raw.iter().position(|s| s.abs() > threshold).unwrap_or(0);
    let last = raw.iter().rposition(|s| s.abs() > threshold).unwrap_or(raw.len() - 1);
    let pre = (rate as usize) / 200;
    let post = (rate as usize) / 20;
    let mut data: Vec<f32> = raw[first.saturating_sub(pre)..(last + post).min(raw.len())].iter().map(|s| s / peak * 0.9).collect();
    let fade = data.len().min(rate as usize / 100);
    let n = data.len();
    for i in 0..fade {
        data[n - 1 - i] *= i as f32 / fade as f32;
    }

    let dir = user_sample_dir().ok_or("no data dir")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = (1..)
        .map(|i| format!("rec_{i:02}"))
        .find(|n| !dir.join(format!("{n}.wav")).exists())
        .unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(dir.join(format!("{name}.wav")), spec).map_err(|e| e.to_string())?;
    for s in &data {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())?;
    Ok(Sample { name, kind: Kind::User, data, rate })
}
