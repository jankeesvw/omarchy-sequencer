//! Sound packs: free sample libraries you can download from inside the app.
//!
//! An installed pack is a folder in `~/.local/share/sequencer/packs/<id>/` with its WAV files
//! and a `pack.json` that says where it came from. Downloading goes through `curl` and zips are
//! unpacked with `bsdtar`, both part of every Arch install.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub enum Source {
    /// An archive.org item: every original `.wav` in it.
    Archive(&'static str),
    /// A zip file; `take` picks which of the WAV files inside to keep.
    Zip(&'static str, Take),
}

#[derive(Clone, Copy)]
pub enum Take {
    All,
    /// Multisampled instruments: one take per sound (the first round robin of the loudest layer).
    OnePerSound,
}

pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub author: &'static str,
    pub license: &'static str,
    pub url: &'static str,
    pub about: &'static str,
    pub size: &'static str,
    pub source: Source,
}

/// Packs that can be installed. Every licence was checked at the source.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "lofi-kits",
        name: "Lo-fi Kits",
        author: "Patrick Callan (Mailbox Badger)",
        license: "Public Domain Mark",
        url: "https://archive.org/details/HeatDish",
        about: "Casio SA-75 and Yamaha Portasound drums, beatbox, sine kicks, tin can and shakers",
        size: "8 MB",
        source: Source::Archive("HeatDish"),
    },
    CatalogEntry {
        id: "hand-percussion",
        name: "Hand Percussion",
        author: "Patrick Callan",
        license: "Public Domain Mark",
        url: "https://archive.org/details/public_domain_drum_samples_pack_for_upcoming_2026_album",
        about: "Cajon, bodhrán, guiro, shaker, tambourine and snare",
        size: "2 MB",
        source: Source::Archive("public_domain_drum_samples_pack_for_upcoming_2026_album"),
    },
    CatalogEntry {
        id: "bass",
        name: "Bass",
        author: "Source Guy",
        license: "CC0",
        url: "https://archive.org/details/source-guy-bass-collection",
        about: "808s, donks and reese basses",
        size: "6 MB",
        source: Source::Archive("source-guy-bass-collection"),
    },
    CatalogEntry {
        id: "body-percussion",
        name: "Body Percussion",
        author: "Karoryfer Samples",
        license: "CC0",
        url: "https://github.com/sfzinstruments/body_percussion",
        about: "Claps, snaps, chest and belly slaps, stomps",
        size: "61 MB download",
        source: Source::Zip("https://github.com/sfzinstruments/body_percussion/archive/refs/heads/main.zip", Take::OnePerSound),
    },
    CatalogEntry {
        id: "retro-game",
        name: "Retro Game",
        author: "Juhani Junkala",
        license: "CC0",
        url: "https://opengameart.org/content/512-sound-effects-8-bit-style",
        about: "512 8-bit sound effects: blips, jumps, lasers, explosions, voices",
        size: "21 MB",
        source: Source::Zip(
            "https://opengameart.org/sites/default/files/The%20Essential%20Retro%20Video%20Game%20Sound%20Effects%20Collection%20%5B512%20sounds%5D.zip",
            Take::All,
        ),
    },
    CatalogEntry {
        id: "found-percussion",
        name: "Found Percussion",
        author: "Field Recording Working Group",
        license: "Public Domain Mark",
        url: "https://archive.org/details/MicroblocksVol.1ASoundscapePercussionSamplePack",
        about: "Percussion recorded from everyday objects",
        size: "88 MB",
        source: Source::Archive("MicroblocksVol.1ASoundscapePercussionSamplePack"),
    },
];

/// What an installed pack says about itself.
#[derive(Serialize, Deserialize, Clone)]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub author: String,
    pub license: String,
    pub url: String,
}

pub fn dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("sequencer").join("packs")
}

/// Installed packs, with the folder their sounds are in.
pub fn installed() -> Vec<(PackInfo, PathBuf)> {
    let mut packs: Vec<(PackInfo, PathBuf)> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let info: PackInfo = serde_json::from_str(&std::fs::read_to_string(path.join("pack.json")).ok()?).ok()?;
            Some((info, path))
        })
        .collect();
    packs.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    packs
}

pub fn remove(id: &str) -> std::io::Result<()> {
    std::fs::remove_dir_all(dir().join(id))
}

/// Downloads and unpacks a pack; `progress` gets short status lines for the UI.
pub fn install(entry: &CatalogEntry, progress: &Arc<Mutex<String>>) -> Result<(), String> {
    let say = |s: String| *progress.lock().unwrap() = s;
    let tmp = dir().join(format!(".{}.partial", entry.id));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        match &entry.source {
            Source::Archive(item) => {
                say("reading the file list…".into());
                let meta = fetch_text(&format!("https://archive.org/metadata/{item}"))?;
                let meta: serde_json::Value = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
                let files: Vec<String> = meta["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| f["source"] == "original")
                    .filter_map(|f| f["name"].as_str())
                    .filter(|n| n.to_lowercase().ends_with(".wav"))
                    .map(str::to_owned)
                    .collect();
                if files.is_empty() {
                    return Err("no WAV files found".into());
                }
                for (i, name) in files.iter().enumerate() {
                    say(format!("{}/{} files", i + 1, files.len()));
                    let url = format!("https://archive.org/download/{item}/{}", encode(name));
                    let file = unique_path(&tmp, name);
                    download(&url, &file)?;
                }
            }
            Source::Zip(url, take) => {
                say("downloading…".into());
                let zip = tmp.join("pack.zip");
                download(url, &zip)?;
                say("unpacking…".into());
                let unpacked = tmp.join("unpacked");
                std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
                let ok = Command::new("bsdtar").arg("-xf").arg(&zip).arg("-C").arg(&unpacked).status().map_err(|e| e.to_string())?;
                if !ok.success() {
                    return Err("could not unpack the download".into());
                }
                let mut wavs = Vec::new();
                collect_wavs(&unpacked, &mut wavs);
                for wav in choose(wavs, *take) {
                    let name = wav.file_name().unwrap().to_string_lossy().into_owned();
                    std::fs::rename(&wav, unique_path(&tmp, &name)).map_err(|e| e.to_string())?;
                }
                let _ = std::fs::remove_dir_all(&unpacked);
                let _ = std::fs::remove_file(&zip);
            }
        }
        let info = PackInfo {
            id: entry.id.into(),
            name: entry.name.into(),
            author: entry.author.into(),
            license: entry.license.into(),
            url: entry.url.into(),
        };
        std::fs::write(tmp.join("pack.json"), serde_json::to_string_pretty(&info).unwrap()).map_err(|e| e.to_string())?;
        let target = dir().join(entry.id);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let ok = Command::new("curl").args(["-fsSL", "--retry", "2", "-o"]).arg(to).arg(url).status().map_err(|e| format!("curl: {e}"))?;
    if ok.success() { Ok(()) } else { Err(format!("download failed: {url}")) }
}

fn fetch_text(url: &str) -> Result<String, String> {
    let out = Command::new("curl").args(["-fsSL", "--retry", "2", url]).output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("download failed: {url}"));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// Percent-encodes a path for a URL, keeping the slashes.
fn encode(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A flat file name in `dir` for `name` (which may contain folders), not taken yet.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let file = Path::new(name).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| name.into());
    let (stem, ext) = file.rsplit_once('.').unwrap_or((&file, "wav"));
    let mut path = dir.join(&file);
    let mut i = 2;
    while path.exists() {
        path = dir.join(format!("{stem} {i}.{ext}"));
        i += 1;
    }
    path
}

fn collect_wavs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_wavs(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            out.push(path);
        }
    }
}

/// For multisampled instruments named like `clap_l_vl2_rr1.wav`: keep the first round robin
/// of the loudest velocity layer of every sound.
fn choose(wavs: Vec<PathBuf>, take: Take) -> Vec<PathBuf> {
    if let Take::All = take {
        return wavs;
    }
    let mut best: std::collections::BTreeMap<String, (u32, PathBuf)> = Default::default();
    for wav in wavs {
        let stem = wav.file_stem().unwrap().to_string_lossy().to_lowercase();
        let Some(base) = stem.strip_suffix("_rr1") else { continue };
        let (sound, layer) = match base.rsplit_once("_vl") {
            Some((sound, layer)) => (sound.to_owned(), layer.parse().unwrap_or(0)),
            None => (base.to_owned(), 0),
        };
        if best.get(&sound).is_none_or(|(l, _)| layer > *l) {
            best.insert(sound, (layer, wav));
        }
    }
    best.into_values().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_one_take_per_sound() {
        let files = ["clap_l_vl1_rr1.wav", "clap_l_vl2_rr1.wav", "clap_l_vl2_rr2.wav", "stomp_vl1_rr1.wav", "stomp_vl1_rr3.wav"];
        let picked: Vec<String> = choose(files.iter().map(PathBuf::from).collect(), Take::OnePerSound)
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert_eq!(picked, ["clap_l_vl2_rr1.wav", "stomp_vl1_rr1.wav"]);
    }

    #[test]
    fn encodes_urls() {
        assert_eq!(encode("Bass/808s/SourceGuy - Funny 808 1.wav"), "Bass/808s/SourceGuy%20-%20Funny%20808%201.wav");
    }
}
