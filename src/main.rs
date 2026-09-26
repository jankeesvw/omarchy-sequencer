mod audio;
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
        println!("Usage: sequencer [--play] [--preset demo|rave]");
        println!("  --play           start playing right away");
        println!("  --preset <name>  start a new song from a preset");
        return Ok(());
    }
    let preset = args.iter().position(|a| a == "--preset").and_then(|i| args.get(i + 1));
    let (name, song) = match preset.map(String::as_str) {
        Some(preset) => {
            let (base, song) = if preset == "rave" { ("Rave", pattern::Song::rave(&samples)) } else { ("Demo", pattern::Song::demo(&samples)) };
            let name = songs::unique(base);
            let _ = songs::write(&name, &song, &samples);
            (name, song)
        }
        None => songs::open_last(&samples),
    };
    let shared = Arc::new(audio::Shared::new(song.clone()));
    let output = audio::start(samples.clone(), shared.clone());
    if args.iter().any(|a| a == "--play") {
        shared.playing.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("sequencer")
            .with_title("Sequencer")
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([900.0, 420.0]),
        ..Default::default()
    };
    // With vsync, Mesa blocks in swap_buffers while the window is hidden (another workspace),
    // so the app stops answering Hyprland's pings and gets flagged as not responding.
    // The UI already paces itself with request_repaint_after.
    options.glow_options.vsync = false;
    eframe::run_native(
        "Sequencer",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, samples, shared, name, song, output)))),
    )
}
