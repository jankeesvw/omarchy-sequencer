//! The parts of Sequencer that make the sound: songs, samples and the engine. They build without the app
//! around them, so the community site can play songs in the browser with the same engine (see `player/`).

pub mod engine;
pub mod pattern;
pub mod sound;
