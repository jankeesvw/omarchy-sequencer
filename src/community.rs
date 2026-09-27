//! The community site: sharing a song from the app, and opening one someone else shared.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::pattern::Song;
use crate::samples::Sample;

const DEFAULT_URL: &str = "https://omarchysequencer.com";

/// Where the community site lives; `OMARCHY_SEQUENCER_COMMUNITY` points it elsewhere (for testing).
pub fn base_url() -> String {
    std::env::var("OMARCHY_SEQUENCER_COMMUNITY").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| DEFAULT_URL.into()).trim_end_matches('/').to_owned()
}

pub struct Share {
    pub title: String,
    pub artist: String,
    pub description: String,
}

/// Renders the song, uploads it with its song file and returns the page it got.
pub fn share(samples: Vec<Arc<Sample>>, song: &Song, json: String, share: Share, progress: &Arc<Mutex<String>>) -> Result<String, String> {
    let say = |s: &str| *progress.lock().unwrap() = s.to_owned();
    let dir = std::env::temp_dir().join(format!("{}-share-{}", crate::APP, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        say("recording…");
        // At least twenty seconds, so there is something to listen to.
        let bar = song.steps() as f32 * 60.0 / song.bpm / 4.0;
        let loops = ((20.0 / bar).ceil() as usize).clamp(2, 16);
        let wav = dir.join("song.wav");
        crate::audio::render(samples, song, loops, &wav)?;
        let file = dir.join("song.json");
        std::fs::write(&file, json).map_err(|e| e.to_string())?;
        say("uploading…");
        upload(&share, &file, &wav)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn upload(share: &Share, file: &Path, wav: &Path) -> Result<String, String> {
    let out = Command::new("curl")
        .args(["-sS", "--max-time", "120", "-w", "\n%{http_code}"])
        .args(["--form-string", &format!("title={}", share.title)])
        .args(["--form-string", &format!("artist={}", share.artist)])
        .args(["--form-string", &format!("description={}", share.description)])
        .arg("-F").arg(format!("song=@{};type=application/json", file.display()))
        .arg("-F").arg(format!("audio=@{};type=audio/wav", wav.display()))
        .arg(format!("{}/api/songs", base_url()))
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or((&text, ""));
    let reply: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    if code == "201" {
        reply["url"].as_str().map(str::to_owned).ok_or_else(|| "the site answered without a link".into())
    } else if let Some(error) = reply["error"].as_str() {
        Err(error.to_owned())
    } else if !out.status.success() {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    } else {
        Err(format!("the site answered {code}"))
    }
}

/// Downloads a shared song: its song file, and a name made from its address.
pub fn fetch(url: &str) -> Result<(String, String), String> {
    let url = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches('/');
    let url = if url.ends_with(".json") { url.to_owned() } else { format!("{url}.json") };
    let out = Command::new("curl").args(["-fsSL", "--max-time", "60", &url]).output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("could not download {url}"));
    }
    let text = String::from_utf8(out.stdout).map_err(|e| e.to_string())?;
    // "/songs/12-late-night.json" becomes "late night".
    let slug = url.rsplit('/').next().unwrap_or("").trim_end_matches(".json");
    let slug = slug.split_once('-').map_or(slug, |(id, rest)| if id.chars().all(|c| c.is_ascii_digit()) { rest } else { slug });
    let name = slug.replace('-', " ");
    // The site sends the title along; the address is only a fallback.
    let title = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["title"].as_str().map(str::to_owned));
    let name = title.filter(|t| !t.trim().is_empty()).unwrap_or(name);
    let name = if name.trim().is_empty() { "Shared song".to_owned() } else { name };
    Ok((text, name))
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_a_shared_song_after_its_address() {
        let slug = "12-late-night";
        let name = slug.split_once('-').map_or(slug, |(id, rest)| if id.chars().all(|c| c.is_ascii_digit()) { rest } else { slug }).replace('-', " ");
        assert_eq!(name, "late night");
    }
}
