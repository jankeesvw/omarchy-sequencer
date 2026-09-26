mod audio;
mod pattern;
mod samples;
mod theme;
mod ui;

use std::sync::Arc;

use eframe::egui;

fn main() -> eframe::Result {
    let samples = samples::load_all();
    let song = pattern::load(&samples).unwrap_or_else(|| pattern::Song::demo(&samples));
    let shared = Arc::new(audio::Shared::new(song.clone()));
    let output = audio::start(samples.clone(), shared.clone());
    if std::env::args().any(|a| a == "--play") {
        shared.playing.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("sequencer")
            .with_title("Sequencer")
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([900.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sequencer",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, samples, shared, song, output)))),
    )
}
