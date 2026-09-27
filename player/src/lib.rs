//! The Sequencer engine for the browser: give it the song file and the sounds it uses, and it renders
//! the song the way the app plays it. Plain exports without bindings; `player.js` on the site drives it.

use std::cell::RefCell;
use std::sync::Arc;

use omarchy_sequencer::engine::render_frames;
use omarchy_sequencer::pattern::Song;
use omarchy_sequencer::sound::{Kind, Sample, decode};
use serde::Deserialize;

#[derive(Deserialize)]
struct SongFile {
    song: Song,
    names: Vec<String>,
}

thread_local! {
    static SOUNDS: RefCell<Vec<(String, Arc<Sample>)>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

/// Room for JavaScript to write into; it hands the pointer back to the calls below.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// Forgets the sounds of the previous song.
#[unsafe(no_mangle)]
pub extern "C" fn clear() {
    SOUNDS.with_borrow_mut(Vec::clear);
}

/// Adds a sound under the name the song file uses for it; `own` for your own recordings, which the app
/// lets run longer than built-in and pack sounds. Returns 0 when it is not a WAV we can read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn add_sound(name: *const u8, name_len: usize, wav: *const u8, wav_len: usize, own: i32) -> i32 {
    let name = String::from_utf8_lossy(unsafe { bytes(name, name_len) }).into_owned();
    let pack = if own != 0 { "user" } else { "classic" };
    match decode(&name, Kind::User, pack, std::io::Cursor::new(unsafe { bytes(wav, wav_len) })) {
        Some(sample) => {
            SOUNDS.with_borrow_mut(|s| s.push((name, Arc::new(sample))));
            1
        }
        None => 0,
    }
}

/// Adds a sound the browser decoded itself (your own recordings come as Opus): mono samples at `rate`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn add_pcm(name: *const u8, name_len: usize, data: *const f32, len: usize, rate: u32) -> i32 {
    let name = String::from_utf8_lossy(unsafe { bytes(name, name_len) }).into_owned();
    // As long as the app lets your own recordings be.
    let data: Vec<f32> = unsafe { std::slice::from_raw_parts(data, len) }.iter().take(rate as usize * 60).copied().collect();
    if data.len() < 2 || rate == 0 {
        return 0;
    }
    let sample = Sample { name: name.clone(), pack: "user".into(), id: name.clone(), kind: Kind::User, data, rate };
    SOUNDS.with_borrow_mut(|s| s.push((name, Arc::new(sample))));
    1
}

/// Renders the song `loops` times at `rate`, as interleaved stereo; returns the number of frames,
/// or 0 when the song file can't be read. Tracks whose sound is missing stay silent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn render(json: *const u8, json_len: usize, loops: usize, rate: u32) -> usize {
    let Ok(file) = serde_json::from_slice::<SongFile>(unsafe { bytes(json, json_len) }) else {
        return 0;
    };
    let mut song = file.song;
    let samples: Vec<Arc<Sample>> = SOUNDS.with_borrow(|sounds| {
        for (i, track) in song.tracks.iter_mut().enumerate() {
            let name = file.names.get(i).map(String::as_str).unwrap_or("");
            track.sample = sounds.iter().position(|(n, _)| n == name).unwrap_or(usize::MAX);
        }
        sounds.iter().map(|(_, s)| s.clone()).collect()
    });
    let out = render_frames(samples, &song, loops.clamp(1, 64), rate.clamp(8_000, 192_000));
    let frames = out.len() / 2;
    OUTPUT.with_borrow_mut(|o| *o = out);
    frames
}

/// Where the last render is: `2 * frames` floats, left and right interleaved.
#[unsafe(no_mangle)]
pub extern "C" fn output() -> *const f32 {
    OUTPUT.with_borrow(|o| o.as_ptr())
}
