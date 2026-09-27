mod audio;
mod community;
mod packs;
mod samples;
mod songs;
mod theme;
mod ui;

use omarchy_sequencer::{engine, pattern, sound};

use std::sync::Arc;

use eframe::egui;

/// The name of the binary and of its folders.
pub const APP: &str = "omarchy-sequencer";

/// Earlier builds were called "sequencer": move their settings, packs and recordings over once.
fn move_old_folders() {
    for base in [dirs::config_dir(), dirs::data_dir()].into_iter().flatten() {
        let (old, new) = (base.join("sequencer"), base.join(APP));
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(old, new);
        }
    }
}

/// `--open <url>`: downloads a shared song, installs the packs it needs, and makes it the song that opens.
fn open_shared(url: &str) {
    let (text, name) = match community::fetch(url) {
        Ok(found) => found,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    for pack in songs::packs_in(&text) {
        if packs::installed().iter().any(|(info, _)| info.id == pack.id) {
            continue;
        }
        match packs::CATALOG.iter().find(|e| e.id == pack.id) {
            Some(entry) => {
                println!("Installing {}, used in this song…", entry.name);
                let progress = Arc::new(std::sync::Mutex::new(String::new()));
                if let Err(e) = packs::install(entry, &progress) {
                    eprintln!("Installing {} failed: {e}", entry.name);
                }
            }
            None => eprintln!("This song uses the pack {} ({}), which this version doesn't know.", pack.name, pack.url),
        }
    }
    let text = community::fetch_own_sounds(&text);
    match songs::import(&text, &name) {
        Ok(name) => println!("Opening \"{name}\""),
        Err(e) => {
            eprintln!("Saving the song failed: {e}");
            std::process::exit(1);
        }
    }
}

fn main() -> eframe::Result {
    move_old_folders();
    let args: Vec<String> = std::env::args().collect();
    if let Some(url) = args.iter().position(|a| a == "--open").and_then(|i| args.get(i + 1)) {
        open_shared(url);
    }
    // "Open in Sequencer" on the site: omarchy-sequencer://omarchysequencer.com/songs/12-late-night
    if let Some(link) = args.iter().find(|a| a.starts_with(community::SCHEME)) {
        match community::url_from_link(link) {
            Some(url) => open_shared(&url),
            None => {
                let _ = std::process::Command::new("notify-send").args(["Sequencer", "That link is not a song on the community site"]).status();
                std::process::exit(1);
            }
        }
    }
    let samples = samples::load_all();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("Usage: {APP} [--play] [--preset demo|rave|late-night] [--open <url>] [--about] [--install-pack <pack>]");
        println!("  --play           start playing right away");
        println!("  --preset <name>  start a new song from a preset");
        println!("  --open <url>     open a song from the community site, with the packs it needs");
        println!("  --install-pack <pack>  download a sound pack (run without a name to list them)");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--install-pack") {
        let Some(entry) = args.get(i + 1).and_then(|id| packs::CATALOG.iter().find(|e| e.id == id)) else {
            eprintln!("Usage: {APP} --install-pack <pack>. Packs:");
            for e in packs::CATALOG {
                eprintln!("  {:18} {} ({}, {})", e.id, e.name, e.license, e.size);
            }
            std::process::exit(2);
        };
        println!("Installing {}…", entry.name);
        let progress = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        match packs::install(entry, &progress) {
            Ok(()) => println!("Installed {} in {}", entry.name, packs::dir().join(entry.id).display()),
            Err(e) => {
                eprintln!("Installing {} failed: {e}", entry.name);
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let preset = args.iter().position(|a| a == "--preset").and_then(|i| args.get(i + 1));
    let (name, song) = match preset.map(String::as_str) {
        Some(preset) => {
            let (base, song) = match preset {
                "rave" => ("Rave", pattern::Song::rave(&samples)),
                "late" | "late-night" => ("Late Night", pattern::Song::late_night(&samples)),
                _ => ("Demo", pattern::Song::demo(&samples)),
            };
            let name = songs::unique(base);
            let _ = songs::write(&name, &song, &samples);
            (name, song)
        }
        None => songs::open_last(&samples),
    };
    let shared = Arc::new(audio::Shared::new(song.clone()));
    let output = audio::start(samples.clone(), shared.clone());
    let about = args.iter().any(|a| a == "--about");
    if args.iter().any(|a| a == "--play") {
        shared.playing.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP)
            .with_transparent(true)
            .with_title("Sequencer")
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([1200.0, 360.0]),
        ..Default::default()
    };
    // With vsync, Mesa blocks in swap_buffers while the window is hidden (another workspace),
    // so the app stops answering Hyprland's pings and gets flagged as not responding.
    // The UI already paces itself with request_repaint_after.
    options.glow_options.vsync = false;
    eframe::run_native(
        "Sequencer",
        options,
        Box::new(move |cc| {
            let mut app = ui::App::new(cc, samples, shared, name, song, output);
            if about {
                app.show_about();
            }
            Ok(Box::new(app))
        }),
    )
}
