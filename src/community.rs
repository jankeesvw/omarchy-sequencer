//! The community site: sharing a song from the app, and opening one someone else shared.

use std::path::{Path, PathBuf};
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

/// Uploads the song file with the sounds the site doesn't have (your own and those from packs), and
/// returns the page it got. The site plays the song itself, with the same engine as the app.
pub fn share(samples: Vec<Arc<Sample>>, song: &Song, json: String, share: Share, progress: &Arc<Mutex<String>>) -> Result<String, String> {
    let say = |s: &str| *progress.lock().unwrap() = s.to_owned();
    let dir = std::env::temp_dir().join(format!("{}-share-{}", crate::APP, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        say("preparing…");
        let file = dir.join("song.json");
        std::fs::write(&file, json).map_err(|e| e.to_string())?;
        let mut sounds = Vec::new();
        for track in &song.tracks {
            let sample = &samples[track.sample];
            if sample.pack == "classic" || sounds.iter().any(|(id, _)| id == &sample.id) {
                continue;
            }
            let wav = dir.join(format!("sound-{}.wav", sounds.len()));
            std::fs::write(&wav, crate::sound::to_wav(sample)).map_err(|e| e.to_string())?;
            sounds.push((sample.id.clone(), wav));
        }
        say("uploading…");
        upload(&share, &file, &sounds)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn upload(share: &Share, file: &Path, sounds: &[(String, PathBuf)]) -> Result<String, String> {
    let mut curl = Command::new("curl");
    curl.args(["-sS", "--max-time", "120", "-w", "\n%{http_code}"])
        .args(["--form-string", &format!("title={}", share.title)])
        .args(["--form-string", &format!("artist={}", share.artist)])
        .args(["--form-string", &format!("description={}", share.description)])
        .arg("-F").arg(format!("song=@{};type=application/json", file.display()));
    for (id, wav) in sounds {
        curl.args(["--form-string", &format!("sound_names[]={id}")]).arg("-F").arg(format!("sounds[]=@{};type=audio/wav", wav.display()));
    }
    let out = curl.arg(format!("{}/api/songs", base_url())).output().map_err(|e| format!("curl: {e}"))?;
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

/// Downloads the own sounds a shared song uses (the site lists them under "sounds") into your sample
/// folder. A name you already use for another sound gets a number, and the song file follows along.
pub fn fetch_own_sounds(text: &str) -> String {
    let Ok(mut file) = serde_json::from_str::<serde_json::Value>(text) else { return text.to_owned() };
    let Some(dir) = crate::samples::user_sample_dir() else { return text.to_owned() };
    let sounds: Vec<(String, String)> = file["sounds"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, url)| Some((name.clone(), url.as_str()?.to_owned())))
        // Pack sounds come with their pack; built-in ones are here already.
        .filter(|(name, _)| !name.contains('/') && !name.contains("..") && !name.is_empty())
        .collect();
    for (name, url) in sounds {
        let tmp = dir.join(format!(".{name}.download"));
        let _ = std::fs::create_dir_all(&dir);
        let ok = Command::new("curl").args(["-fsSL", "--max-time", "60", "-o"]).arg(&tmp).arg(&url).status().is_ok_and(|s| s.success());
        let bytes = std::fs::read(&tmp).ok().filter(|_| ok);
        let _ = std::fs::remove_file(&tmp);
        let Some(bytes) = bytes else {
            eprintln!("Could not download the sound {name}");
            continue;
        };
        // Same name and same sound: nothing to do. Same name, other sound: find a free name.
        let mut local = name.clone();
        for n in 2.. {
            match std::fs::read(dir.join(format!("{local}.wav"))) {
                Ok(existing) if existing == bytes => break,
                Ok(_) => local = format!("{name}_{n}"),
                Err(_) => {
                    if std::fs::write(dir.join(format!("{local}.wav")), &bytes).is_ok() {
                        println!("Saved the sound {local}");
                    }
                    break;
                }
            }
        }
        if local != name {
            for n in file["names"].as_array_mut().into_iter().flatten() {
                if n.as_str() == Some(&name) {
                    *n = serde_json::Value::String(local.clone());
                }
            }
        }
    }
    serde_json::to_string(&file).unwrap_or_else(|_| text.to_owned())
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
    use std::sync::{Arc, Mutex};

    use crate::sound::{Kind, Sample};

    /// Shares a song with an own recording to a running site and opens it again:
    /// `OMARCHY_SEQUENCER_COMMUNITY=http://localhost:3000 cargo test -- --ignored shares_and_opens`
    #[test]
    #[ignore]
    fn shares_and_opens_a_song_with_an_own_sound() {
        let mut samples = crate::samples::load_all();
        let data: Vec<f32> = (0..22_050).map(|i| (i as f32 * 0.06).sin() * (1.0 - i as f32 / 22_050.0)).collect();
        samples.push(Arc::new(Sample { name: "rec_test".into(), pack: "user".into(), id: "rec_test".into(), kind: Kind::User, data, rate: 44_100 }));
        let mut song = crate::pattern::Song::demo(&samples);
        song.tracks[0].sample = samples.len() - 1;
        let json = crate::songs::to_json(&song, &samples);
        let share = super::Share { title: "Own sound test".into(), artist: "test".into(), description: String::new() };
        let url = super::share(samples, &song, json, share, &Arc::new(Mutex::new(String::new()))).expect("shared");
        println!("{url}");
        let (text, name) = super::fetch(&url).expect("fetched");
        assert_eq!(name, "Own sound test");
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(file["sounds"]["rec_test"].as_str().is_some_and(|u| u.ends_with(".wav")), "{}", file["sounds"]);
    }

    #[test]
    fn names_a_shared_song_after_its_address() {
        let slug = "12-late-night";
        let name = slug.split_once('-').map_or(slug, |(id, rest)| if id.chars().all(|c| c.is_ascii_digit()) { rest } else { slug }).replace('-', " ");
        assert_eq!(name, "late night");
    }
}
