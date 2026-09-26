use std::path::Path;

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

/// Longest sample kept in memory; packs sometimes contain long soundscapes.
const MAX_SECONDS: u32 = 12;


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
        .filter_map(|(name, kind, bytes)| decode(name, *kind, "classic", std::io::Cursor::new(*bytes)))
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
    for (info, dir) in crate::packs::installed() {
        out.extend(load_pack(&info.id, &dir));
    }
    out.into_iter().map(std::sync::Arc::new).collect()
}

/// Every WAV in an installed pack.
pub fn load_pack(pack: &str, dir: &Path) -> Vec<Sample> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
        .collect();
    paths.sort_by_key(|p| natural_key(&p.to_string_lossy().to_lowercase()));
    paths
        .iter()
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().to_lowercase().replace(' ', "_");
            let file = std::fs::File::open(path).ok()?;
            let mut sample = decode(&stem, guess_kind(&stem), pack, std::io::BufReader::new(file))?;
            // The id keeps the file name (songs refer to it); the name is for people.
            sample.name = display_name(&stem);
            Some(sample)
        })
        .collect()
}

/// Tidies a file name from a pack for the browser: "SourceGuy - Funny 808 1" becomes "funny 808 1",
/// "bellycenter_l_vl2_rr1" becomes "bellycenter L" and "sfx_wpn_laser10" becomes "wpn laser 10".
pub fn display_name(stem: &str) -> String {
    let mut name = stem.to_lowercase().replace('-', " - ");
    // Drop a maker's name in front ("SourceGuy - …").
    if let Some((_, rest)) = name.rsplit_once(" - ") {
        name = rest.to_owned();
    }
    let mut words: Vec<String> = name.split(['_', ' ']).filter(|w| !w.is_empty()).map(str::to_owned).collect();
    // Recording details at the end: velocity layer and round robin.
    while words.last().is_some_and(|w| is_take(w)) {
        words.pop();
    }
    if words.first().is_some_and(|w| w == "sfx") && words.len() > 1 {
        words.remove(0);
    }
    let words: Vec<String> = words
        .into_iter()
        .map(|w| match w.as_str() {
            "l" => "L".into(),
            "r" => "R".into(),
            _ => split_number(&w),
        })
        .collect();
    let name = words.join(" ");
    if name.is_empty() { stem.to_owned() } else { name }
}

/// "vl2", "rr1": which recording of a multisampled sound it is.
fn is_take(w: &str) -> bool {
    ["vl", "rr"].iter().any(|p| w.strip_prefix(p).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())))
}

/// "laser10" becomes "laser 10", "808" stays "808".
fn split_number(w: &str) -> String {
    let digits = w.len() - w.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits == w.len() {
        return w.to_owned();
    }
    let (word, number) = w.split_at(w.len() - digits);
    format!("{word} {number}")
}

/// Sort key that puts "laser2" before "laser10": runs of digits compare as numbers.
fn natural_key(s: &str) -> Vec<(String, u64)> {
    let mut key = Vec::new();
    let mut text = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut n = c.to_digit(10).unwrap() as u64;
            while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                n = n.saturating_mul(10).saturating_add(d as u64);
                chars.next();
            }
            key.push((std::mem::take(&mut text), n));
        } else {
            text.push(c);
        }
    }
    key.push((text, 0));
    key
}

/// Sorts a pack sound into drums, bass or the rest by its name, for its colour and note length.
fn guess_kind(name: &str) -> Kind {
    const DRUMS: [&str; 23] = [
        "kick", "snare", "hat", "clap", "tom", "rim", "perc", "cajon", "conga", "bongo", "shaker", "tamb", "stomp", "cymbal",
        "crash", "ride", "snap", "slap", "bodhran", "bodrhain", "guiro", "drum", "beatbox",
    ];
    if DRUMS.iter().any(|d| name.contains(d)) {
        Kind::Drum
    } else if ["bass", "808", "reese", "donk"].iter().any(|b| name.contains(b)) {
        Kind::Riff
    } else {
        Kind::Pack
    }
}

pub fn user_sample_dir() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("sequencer").join("samples"))
}

fn load_file(path: &Path) -> Option<Sample> {
    let name = path.file_stem()?.to_string_lossy().to_lowercase();
    let file = std::fs::File::open(path).ok()?;
    decode(&name, Kind::User, "user", std::io::BufReader::new(file))
}

fn decode<R: std::io::Read>(name: &str, kind: Kind, pack: &str, reader: R) -> Option<Sample> {
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
        .take((spec.sample_rate * MAX_SECONDS) as usize)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    if data.len() < 2 {
        return None;
    }
    let id = if matches!(pack, "classic" | "user") { name.to_string() } else { format!("{pack}/{name}") };
    Some(Sample { name: name.to_string(), pack: pack.to_string(), id, kind, data, rate: spec.sample_rate })
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
    Ok(Sample { id: name.clone(), name, pack: "user".into(), kind: Kind::User, data, rate })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidies_pack_names() {
        assert_eq!(display_name("sourceguy_-_funny_808_1"), "funny 808 1");
        assert_eq!(display_name("bellycenter_l_vl2_rr1"), "bellycenter L");
        assert_eq!(display_name("sfx_wpn_laser10"), "wpn laser 10");
        assert_eq!(display_name("cajon_kick_1"), "cajon kick 1");
        assert_eq!(display_name("snap_r_rr1"), "snap R");
    }

    #[test]
    fn sorts_numbers_naturally() {
        let mut names = vec!["laser10", "laser2", "laser1", "kick"];
        names.sort_by_key(|n| natural_key(n));
        assert_eq!(names, ["kick", "laser1", "laser2", "laser10"]);
    }
}
