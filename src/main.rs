mod audio;
mod pattern;
mod samples;
mod ui;

use std::sync::Arc;

use eframe::egui;

fn main() -> eframe::Result {
    let samples = Arc::new(samples::load_all());
    let pattern = pattern::load(&samples).unwrap_or_else(|| pattern::Pattern::demo(&samples));
    let shared = Arc::new(audio::Shared::new(pattern.clone()));
    let output = audio::start(samples.clone(), shared.clone());
    if std::env::args().any(|a| a == "--play") {
        shared.playing.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("sequencer")
            .with_title("CYBERSEQ 2000")
            .with_inner_size([1280.0, 780.0])
            .with_min_inner_size([720.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "CYBERSEQ 2000",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, samples, shared, pattern, output)))),
    )
}
