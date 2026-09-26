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
}

/// Which song was open last, so the next start continues there.
#[derive(Serialize, Deserialize, Default)]
struct State {
    current: Option<String>,
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
    let file = SongFile { song: song.clone(), names: song.tracks.iter().map(|t| samples[t.sample].id.clone()).collect() };
    std::fs::create_dir_all(dir())?;
    // Write next to it first, so a crash halfway never leaves a broken song behind.
    let tmp = dir().join(format!(".{name}.{EXT}.tmp"));
    std::fs::write(&tmp, serde_json::to_string(&file)?)?;
    std::fs::rename(tmp, path(name))?;
    remember(name);
    Ok(())
}

fn read_file(path: &Path, samples: &[Arc<Sample>]) -> Option<Song> {
    let text = std::fs::read_to_string(path).ok()?;
    let SongFile { mut song, names } = serde_json::from_str(&text).ok()?;
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
    if let Some(dir) = config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let state = State { current: Some(name.to_owned()) };
        let _ = std::fs::write(dir.join("state.json"), serde_json::to_string(&state).unwrap_or_default());
    }
}

/// The song to open at start: the last one, else the old single-song save from earlier versions, else the demo.
pub fn open_last(samples: &[Arc<Sample>]) -> (String, Song) {
    let state: State = config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("state.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
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
