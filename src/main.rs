mod audio;
mod packs;
mod pattern;
mod samples;
mod songs;
mod theme;
mod ui;

use std::sync::Arc;

use eframe::egui;

fn main() -> eframe::Result {
    let samples = samples::load_all();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("Usage: sequencer [--play] [--preset demo|rave|late-night]");
        println!("  --play           start playing right away");
        println!("  --preset <name>  start a new song from a preset");
        println!("  --install-pack <pack>  download a sound pack (run without a name to list them)");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--install-pack") {
        let Some(entry) = args.get(i + 1).and_then(|id| packs::CATALOG.iter().find(|e| e.id == id)) else {
            eprintln!("Usage: sequencer --install-pack <pack>. Packs:");
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
            .with_app_id("sequencer")
            .with_transparent(true)
            .with_title("Sequencer")
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([1280.0, 360.0]),
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
