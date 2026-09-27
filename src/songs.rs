//! Songs on disk: every song is a JSON file in `~/Music/Sequencer`, saved as you work.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::pattern::Song;
use crate::samples::Sample;

const EXT: &str = "json";

// Saved with sample names next to the indexes, so the file stays valid when samples are added.
#[derive(Serialize, Deserialize)]
struct SongFile {
    song: Song,
    names: Vec<String>,
    /// The sound packs the song uses, so whoever opens it can get them.
    #[serde(default)]
    packs: Vec<PackRef>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PackRef {
    pub id: String,
    pub name: String,
    pub author: String,
    pub license: String,
    pub url: String,
}

/// The packs `song` uses, from the catalog.
fn packs_of(song: &Song, samples: &[Arc<Sample>]) -> Vec<PackRef> {
    let mut ids: Vec<&str> = song.tracks.iter().map(|t| samples[t.sample].pack.as_str()).collect();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| crate::packs::CATALOG.iter().find(|e| e.id == id))
        .map(|e| PackRef { id: e.id.into(), name: e.name.into(), author: e.author.into(), license: e.license.into(), url: e.url.into() })
        .collect()
}

/// The song file as it is saved and shared.
pub fn to_json(song: &Song, samples: &[Arc<Sample>]) -> String {
    let file = SongFile { song: song.clone(), names: song.tracks.iter().map(|t| samples[t.sample].id.clone()).collect(), packs: packs_of(song, samples) };
    serde_json::to_string(&file).unwrap_or_default()
}

/// Whether the song uses your own recordings or samples, which others won't have.
pub fn uses_own_sounds(song: &Song, samples: &[Arc<Sample>]) -> bool {
    song.tracks.iter().any(|t| samples[t.sample].pack == "user")
}

/// Which song was open last, so the next start continues there, and the name you share under.
#[derive(Serialize, Deserialize, Default)]
struct State {
    current: Option<String>,
    #[serde(default)]
    artist: Option<String>,
}

fn load_state() -> State {
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("state.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(state: &State) {
    if let Some(dir) = config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("state.json"), serde_json::to_string(state).unwrap_or_default());
    }
}

pub fn artist() -> String {
    load_state().artist.unwrap_or_default()
}

pub fn set_artist(name: &str) {
    let mut state = load_state();
    state.artist = Some(name.trim().to_owned()).filter(|n| !n.is_empty());
    save_state(&state);
}

/// Saves a song file someone shared, as it is, and makes it the one that opens. Returns its name.
pub fn import(text: &str, base: &str) -> std::io::Result<String> {
    let name = unique(base);
    std::fs::create_dir_all(dir())?;
    std::fs::write(path(&name), text)?;
    remember(&name);
    Ok(name)
}

/// The packs a song file says it needs.
pub fn packs_in(text: &str) -> Vec<PackRef> {
    serde_json::from_str::<SongFile>(text).map(|f| f.packs).unwrap_or_default()
}

pub struct SongInfo {
    pub name: String,
    pub modified: SystemTime,
    pub bpm: f32,
    pub tracks: usize,
}

pub fn dir() -> PathBuf {
    dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Music")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Sequencer")
}

fn path(name: &str) -> PathBuf {
    dir().join(format!("{name}.{EXT}"))
}

fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(crate::APP))
}

pub fn write(name: &str, song: &Song, samples: &[Arc<Sample>]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir())?;
    // Write next to it first, so a crash halfway never leaves a broken song behind.
    let tmp = dir().join(format!(".{name}.{EXT}.tmp"));
    std::fs::write(&tmp, to_json(song, samples))?;
    std::fs::rename(tmp, path(name))?;
    remember(name);
    Ok(())
}

fn read_file(path: &Path, samples: &[Arc<Sample>]) -> Option<Song> {
    let text = std::fs::read_to_string(path).ok()?;
    let SongFile { mut song, names, .. } = serde_json::from_str(&text).ok()?;
    if song.tracks.is_empty() {
        return None;
    }
    song.sanitize(&names, samples);
    Some(song)
}

pub fn read(name: &str, samples: &[Arc<Sample>]) -> Option<Song> {
    read_file(&path(name), samples)
}

/// All songs, the most recently changed first.
pub fn list() -> Vec<SongInfo> {
    let mut songs: Vec<SongInfo> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != EXT || p.file_name()?.to_string_lossy().starts_with('.') {
                return None;
            }
            let name = p.file_stem()?.to_string_lossy().into_owned();
            let modified = e.metadata().ok()?.modified().ok()?;
            let file: SongFile = serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
            Some(SongInfo { name, modified, bpm: file.song.bpm, tracks: file.song.tracks.len() })
        })
        .collect();
    songs.sort_by(|a, b| b.modified.cmp(&a.modified));
    songs
}

/// Turns what someone typed into a usable file name.
pub fn clean(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | '\0' | ':')).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    if cleaned.is_empty() { "Untitled".into() } else { cleaned.chars().take(60).collect() }
}

/// `base`, or `base 2`, `base 3`… whichever is still free.
pub fn unique(base: &str) -> String {
    let base = clean(base);
    if !path(&base).exists() {
        return base;
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !path(n).exists()).unwrap()
}

pub fn rename(from: &str, to: &str) -> std::io::Result<String> {
    let to = clean(to);
    if to == from {
        return Ok(to);
    }
    let to = unique(&to);
    std::fs::rename(path(from), path(&to))?;
    remember(&to);
    Ok(to)
}

pub fn delete(name: &str) -> std::io::Result<()> {
    std::fs::remove_file(path(name))
}

fn remember(name: &str) {
    let mut state = load_state();
    state.current = Some(name.to_owned());
    save_state(&state);
}

/// The song to open at start: the last one, else the old single-song save from earlier versions, else the demo.
pub fn open_last(samples: &[Arc<Sample>]) -> (String, Song) {
    let state = load_state();
    if let Some(name) = state.current {
        if let Some(song) = read(&name, samples) {
            return (name, song);
        }
    }
    if let Some(song) = config_dir().and_then(|d| read_file(&d.join("song.json"), samples)) {
        let name = unique("My first song");
        let _ = write(&name, &song, samples);
        return (name, song);
    }
    if let Some(latest) = list().first() {
        if let Some(song) = read(&latest.name, samples) {
            return (latest.name.clone(), song);
        }
    }
    let name = unique("Demo");
    let song = Song::demo(samples);
    let _ = write(&name, &song, samples);
    (name, song)
}

/// "just now", "5 min ago", "yesterday"…
pub fn ago(t: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_names() {
        assert_eq!(clean("  My/beat: v2 "), "Mybeat v2");
        assert_eq!(clean("..."), "Untitled");
        assert_eq!(clean(""), "Untitled");
    }
}
