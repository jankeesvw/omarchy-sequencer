use std::path::Path;

pub struct Sample {
    pub name: String,
    pub kind: Kind,
    pub data: Vec<f32>,
    pub rate: u32,
}

impl Sample {
    /// Hoeveel blokjes een nieuwe noot standaard beslaat: loops een hele maat, riffs een tel.
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

impl Kind {

    pub fn label(self) -> &'static str {
        match self {
            Kind::Drum => "DRM",
            Kind::Riff => "RIF",
            Kind::Rave => "90S",
            Kind::Vox => "VOX",
            Kind::User => "USR",
        }
    }
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

// Echte opnames van Freesound (CC0), zie README.
const RAVE: [(&str, Kind, &[u8]); 12] = embed!(
    Kind::Rave,
    "rave_stab", "jx3p_stab", "orch_hit", "hoover", "hardhouse_hoover", "mentasm", "m1_organ",
    "house_chords", "acid_line", "acid_bass", "rave_loop_135", "rave_loop_128",
);

// Producer Space (CC0): dance shouts, house vocals en computerstemmen.
const VOX: [(&str, Kind, &[u8]); 10] = embed!(
    Kind::Vox,
    "everybody", "here_we_go", "lets_go", "hey", "feel_the_rhythm", "clap_your_hands",
    "access_granted", "security_breach", "system_error", "were_in",
);

pub fn load_all() -> Vec<Sample> {
    let mut out: Vec<Sample> = DRUMS
        .iter()
        .chain(RIFFS.iter())
        .chain(RAVE.iter())
        .chain(VOX.iter())
        .filter_map(|(name, kind, bytes)| decode(name, *kind, std::io::Cursor::new(*bytes)))
        .collect();

    // Eigen samples: gooi .wav bestanden in ~/.local/share/sequencer/samples
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
    out
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
