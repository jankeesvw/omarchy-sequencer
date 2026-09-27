use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::audio::{self, Output, Recorder, Shared};
use crate::pattern::{Cell, FilterKind, Fx, MAX_STEPS, MAX_TRACKS, PATTERNS, Song, Track};
use crate::samples::{self, Kind, Sample};
use crate::community;
use crate::packs;
use crate::songs;
use crate::theme::{self, Theme};

const CONTROL_H: f32 = 28.0;
/// Track control columns: header, width.
const COLUMNS: [(&str, f32); 11] = [
    ("", 22.0),
    ("Sample", 150.0),
    ("M", 26.0),
    ("S", 26.0),
    ("Rec", 26.0),
    ("Vol", 56.0),
    ("Pan", 56.0),
    ("FX", 56.0),
    ("Pitch", 40.0),
    ("Len", 36.0),
    ("", 22.0),
];
/// Gap between track controls, the same as between grid cells.
const COL_GAP: f32 = 3.0;
const STRIP_W: f32 = 6.0;
/// Width of the track controls, up to where the grid starts.
const LEFT_W: f32 = {
    let mut w = STRIP_W;
    let mut i = 0;
    while i < COLUMNS.len() {
        w += COLUMNS[i].1 + COL_GAP;
        i += 1;
    }
    w + 1.5
};
/// Inset of a grid cell (and of every track control) inside its slot.
const CELL_INSET: f32 = 1.5;
const LOGO: &str = include_str!("logo.txt");

const CREDITS: &[(&str, &str)] = &[
    ("code and design", "Jankees van Woezik"),
    ("TR-808 samples", "Michael Fischer"),
    ("riffs and stabs", "Ben Burnes"),
    ("rave stab", "Freesound #443931"),
    ("JX-3P stab", "modularsamples"),
    ("hoover", "Chameon"),
    ("hardhouse hoover", "woowah"),
    ("mentasm", "sandizzy"),
    ("M1 organ", "Cloud-10"),
    ("orchestra hit", "BigDumbWeirdo"),
    ("acid line", "makenoisemusic"),
    ("acid bass", "evanjones4"),
    ("house chords", "Slanted"),
    ("rave loops", "Alastair_Pursloe, GENERALMiDiGUy"),
    ("vocals", "Producer Space"),
    ("lo-fi kits, hand percussion", "Patrick Callan"),
    ("bass pack", "Source Guy"),
    ("body percussion", "Karoryfer Samples"),
    ("retro game", "Juhani Junkala"),
    ("found percussion", "Field Recording Working Group"),
    ("built with", "Rust, egui and cpal"),
    ("made for", "Omarchy"),
    ("", ""),
    ("thanks for", "making music"),
];

/// Size of a grid cell; the rows are as tall.
const CELL: f32 = 30.0;
/// Pressing record shorter than this keeps it recording; longer is hold-to-record.
const HOLD_THRESHOLD: Duration = Duration::from_millis(350);

enum ShareState {
    Idle,
    Busy(Arc<Mutex<String>>),
    Done(String),
    Failed(String),
}

/// How long a freshly drawn note takes to pop in.
const POP: Duration = Duration::from_millis(280);

struct Spark {
    pos: Pos2,
    vel: Vec2,
    color: Color32,
    born: Instant,
    life: f32,
    size: f32,
}

/// How long the trigger animation of a cell lasts.
const FLASH: Duration = Duration::from_millis(350);
const PATTERN_NAMES: [&str; PATTERNS] = ["A", "B", "C", "D", "E", "F", "G", "H"];

pub struct App {
    samples: Vec<Arc<Sample>>,
    /// Name of the open song (its file in ~/Music/Sequencer), and the name field while you type.
    song_name: String,
    name_edit: String,
    songs_open: bool,
    about_open: Option<Instant>,
    /// The share window: its fields, and what the upload is doing.
    share_open: bool,
    share_title: String,
    share_artist: String,
    share_description: String,
    share_state: Arc<Mutex<ShareState>>,
    settings_open: bool,
    confirm_delete: Option<String>,
    shared: Arc<Shared>,
    song: Song,
    pushed: Song,
    output: Output,
    theme: Theme,
    watcher: theme::Watcher,
    paint: Option<Cell>,
    wheel: f32,
    meters: [f32; MAX_TRACKS],
    selected: usize,
    view_h: f32,
    started: Instant,
    flash: Option<(String, Instant)>,
    last_save: Instant,
    saved: Song,
    undo: Vec<Song>,
    redo: Vec<Song>,
    committed: Song,
    taps: Vec<Instant>,
    recorder: Recorder,
    rec_track: Option<usize>,
    /// When the recording started, and whether a short click made it keep going (until the next click).
    rec_started: Option<Instant>,
    rec_latched: Option<Instant>,
    export_result: Arc<Mutex<Option<String>>>,
    tab: Option<bool>,
    /// The sound browser: which track it picks for, which list it shows, and the search.
    sounds_open: Option<usize>,
    sounds_source: String,
    sounds_query: String,
    /// Packs being downloaded: id and progress line; finished installs wait in `installed`.
    installing: Arc<Mutex<Vec<(String, Arc<Mutex<String>>)>>>,
    installed: Arc<Mutex<Vec<(String, Result<(), String>)>>>,
    confirm_remove: Option<String>,
    /// Trigger animations: track, step, and when it fired.
    flashes: Vec<(usize, usize, Instant)>,
    /// Click effects: sparks flying off, notes popping in, erased notes shrinking away, accent glints.
    sparks: Vec<Spark>,
    pops: Vec<(usize, usize, usize, Instant)>,
    ghosts: Vec<(Rect, Color32, Instant)>,
    rings: Vec<(Rect, Color32, Instant)>,
    glints: Vec<(Pos2, Color32, Instant)>,
    last_step: Option<usize>,
    /// The track whose effects window is open, and where it opens.
    fx_open: Option<(usize, Pos2)>,
    /// Looping preview in the effects window: when the next one plays, and the pause in between.
    fx_loop: Option<Instant>,
    fx_gap: f32,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, samples: Vec<Arc<Sample>>, shared: Arc<Shared>, song_name: String, song: Song, output: Output) -> Self {
        let theme = Theme::load();
        fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, &theme);
        Self {
            samples,
            name_edit: song_name.clone(),
            song_name,
            songs_open: false,
            about_open: None,
            share_open: false,
            share_title: String::new(),
            share_artist: songs::artist(),
            share_description: String::new(),
            share_state: Arc::new(Mutex::new(ShareState::Idle)),
            settings_open: false,
            confirm_delete: None,
            shared,
            pushed: song.clone(),
            saved: song.clone(),
            committed: song.clone(),
            song,
            output,
            theme,
            watcher: theme::Watcher::new(),
            paint: None,
            wheel: 0.0,
            meters: [0.0; MAX_TRACKS],
            selected: 0,
            view_h: 600.0,
            started: Instant::now(),
            flash: None,
            last_save: Instant::now(),
            undo: Vec::new(),
            redo: Vec::new(),
            taps: Vec::new(),
            recorder: Recorder::new(),
            rec_track: None,
            rec_started: None,
            rec_latched: None,
            export_result: Arc::new(Mutex::new(None)),
            tab: None,
            sounds_open: None,
            sounds_source: "classic:drum".into(),
            sounds_query: String::new(),
            installing: Arc::new(Mutex::new(Vec::new())),
            installed: Arc::new(Mutex::new(Vec::new())),
            confirm_remove: None,
            flashes: Vec::new(),
            sparks: Vec::new(),
            pops: Vec::new(),
            ghosts: Vec::new(),
            rings: Vec::new(),
            glints: Vec::new(),
            last_step: None,
            fx_open: None,
            fx_loop: None,
            fx_gap: 1.0,
        }
    }

    /// Opens the about box (used by `omarchy-sequencer --about`).
    pub fn show_about(&mut self) {
        self.about_open = Some(Instant::now());
    }

    fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    fn toggle_play(&self) {
        self.shared.playing.fetch_xor(true, Ordering::Relaxed);
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), Instant::now()));
    }

    fn kind_color(&self, kind: Kind) -> Color32 {
        match kind {
            Kind::Drum => self.theme.accent,
            Kind::Riff => self.theme.magenta,
            Kind::Rave => self.theme.green,
            Kind::Vox => self.theme.yellow,
            Kind::User => self.theme.cyan,
            Kind::Pack => self.theme.orange,
        }
    }

    fn set_steps(&mut self, steps: usize) {
        let cur = self.song.current;
        self.song.steps[cur] = steps.clamp(1, MAX_STEPS);
    }

    fn select_pattern(&mut self, p: usize, copy: bool) {
        if copy {
            let from = self.song.current;
            for t in &mut self.song.tracks {
                t.lanes[p] = t.lanes[from].clone();
            }
            self.song.steps[p] = self.song.steps[from];
            self.say(format!("copied pattern {} to {}", PATTERN_NAMES[from], PATTERN_NAMES[p]));
            return;
        }
        if self.playing() && p != self.song.current {
            // While playing, the pattern switches at the end of the bar.
            self.song.queued = Some(p);
        } else {
            self.song.current = p;
            self.song.queued = None;
        }
    }

    fn add_track(&mut self) {
        if self.song.tracks.len() >= MAX_TRACKS {
            self.say("at most 16 tracks");
            return;
        }
        let used: Vec<usize> = self.song.tracks.iter().map(|t| t.sample).collect();
        let next = (0..self.samples.len()).find(|i| !used.contains(i)).unwrap_or(0);
        self.song.tracks.push(Track::new(next, self.samples[next].default_len()));
        self.selected = self.song.tracks.len() - 1;
    }

    fn randomize(&mut self) {
        let steps = self.song.steps();
        let cur = self.song.current;
        for t in &mut self.song.tracks {
            let s = &self.samples[t.sample];
            let n = s.name.as_str();
            let lane = &mut t.lanes[cur];
            lane.clear();
            for i in 0..steps {
                let p = match (s.kind, n) {
                    (_, "kick" | "kick_long") => if i % 4 == 0 { 0.9 } else { 0.12 },
                    (_, "snare" | "snare_snappy" | "clap") => if i % 8 == 4 { 0.9 } else { 0.06 },
                    (_, "hat_closed" | "maracas") => if i % 2 == 0 { 0.85 } else { 0.35 },
                    (_, "hat_open") => if i % 4 == 2 { 0.6 } else { 0.03 },
                    (_, n) if n.contains("loop") => if i % 16 == 0 { 0.9 } else { 0.0 },
                    (Kind::Vox, _) => if i % 16 == 12 { 0.4 } else { 0.0 },
                    (Kind::Riff | Kind::Rave, _) => if i % 4 == 0 { 0.3 } else { 0.1 },
                    _ => 0.12,
                };
                if fastrand::f32() < p {
                    let cell = if fastrand::f32() < 0.25 { Cell::Accent } else { Cell::On };
                    let len = if t.note_len > 1 && fastrand::bool() { t.note_len / 2 } else { t.note_len };
                    lane.place(i, cell, len, steps);
                }
            }
        }
        self.say("random pattern generated");
    }

    fn clear_pattern(&mut self) {
        let cur = self.song.current;
        for t in &mut self.song.tracks {
            t.lanes[cur].clear();
        }
        self.say(format!("cleared pattern {}", PATTERN_NAMES[cur]));
    }

    fn tap(&mut self) {
        let now = Instant::now();
        self.taps.retain(|t| now.duration_since(*t) < Duration::from_secs(2));
        self.taps.push(now);
        if self.taps.len() >= 2 {
            let span = now.duration_since(self.taps[0]).as_secs_f32() / (self.taps.len() - 1) as f32;
            self.song.bpm = (60.0 / span).clamp(40.0, 300.0).round();
        }
    }

    fn save_now(&mut self) {
        match songs::write(&self.song_name, &self.song, &self.samples) {
            Ok(()) => {
                self.saved = self.song.clone();
                self.say(format!("saved {}", self.song_name));
            }
            Err(e) => self.say(format!("saving failed: {e}")),
        }
        self.last_save = Instant::now();
    }

    /// Makes `song` the open song. The current one is saved first.
    fn switch_to(&mut self, name: String, song: Song) {
        let _ = songs::write(&self.song_name, &self.song, &self.samples);
        self.song = song;
        self.song_name = name.clone();
        self.name_edit = name;
        self.saved = self.song.clone();
        self.committed = self.song.clone();
        self.undo.clear();
        self.redo.clear();
        self.fx_open = None;
        self.fx_loop = None;
        self.selected = 0;
        let _ = songs::write(&self.song_name, &self.song, &self.samples);
    }

    fn new_song(&mut self, base: &str, song: Song) {
        let name = songs::unique(base);
        self.say(format!("new song: {name}"));
        self.switch_to(name, song);
    }

    fn open_song(&mut self, name: &str) {
        if name == self.song_name {
            return;
        }
        let _ = songs::write(&self.song_name, &self.song, &self.samples);
        match songs::read(name, &self.samples) {
            Some(song) => {
                self.switch_to(name.to_owned(), song);
                self.say(format!("opened {name}"));
            }
            None => self.say(format!("could not open {name}")),
        }
    }

    fn rename_song(&mut self) {
        let wanted = songs::clean(&self.name_edit);
        if wanted == self.song_name {
            self.name_edit = wanted;
            return;
        }
        let _ = songs::write(&self.song_name, &self.song, &self.samples);
        match songs::rename(&self.song_name, &wanted) {
            Ok(name) => {
                self.say(format!("renamed to {name}"));
                self.song_name = name.clone();
                self.name_edit = name;
            }
            Err(e) => {
                self.say(format!("renaming failed: {e}"));
                self.name_edit = self.song_name.clone();
            }
        }
    }

    fn export(&mut self) {
        let samples = self.samples.clone();
        let song = self.song.clone();
        let result = self.export_result.clone();
        self.say("exporting…");
        std::thread::spawn(move || {
            let msg = match audio::export(samples, &song, 4) {
                Ok(path) => format!("exported to {}", path.display()),
                Err(e) => format!("export failed: {e}"),
            };
            *result.lock().unwrap() = Some(msg);
        });
    }

    fn start_recording(&mut self, track: usize) {
        if self.recorder.recording() {
            return;
        }
        match self.recorder.start() {
            Ok(()) => {
                self.rec_track = Some(track);
                self.rec_started = Some(Instant::now());
                self.rec_latched = None;
                self.selected = track;
            }
            Err(e) => self.say(format!("cannot record: {e}")),
        }
    }

    fn stop_recording(&mut self) {
        let Some(track) = self.rec_track.take() else { return };
        self.rec_started = None;
        self.rec_latched = None;
        let (raw, rate) = self.recorder.stop();
        match samples::save_recording(&raw, rate) {
            Ok(sample) => {
                let name = sample.name.clone();
                let sample = Arc::new(sample);
                self.samples.push(sample.clone());
                self.shared.incoming.lock().unwrap().push(sample);
                let idx = self.samples.len() - 1;
                if let Some(t) = self.song.tracks.get_mut(track) {
                    t.sample = idx;
                    t.note_len = 1;
                    t.pitch = 0.0;
                }
                self.shared.preview.lock().unwrap().push((idx, 0.8, Some(track)));
                self.say(format!("recorded {name} (saved in ~/.local/share/omarchy-sequencer/samples)"));
            }
            Err(e) => self.say(format!("recording not saved: {e}")),
        }
    }

    fn history(&mut self, pointer_down: bool) {
        // A change counts as one undo step once the mouse is released; switching patterns is not part of it.
        let mut a = self.song.clone();
        let mut b = self.committed.clone();
        a.current = 0;
        a.queued = None;
        b.current = 0;
        b.queued = None;
        if !pointer_down && a != b {
            self.undo.push(std::mem::replace(&mut self.committed, self.song.clone()));
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self, redo: bool) {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        if let Some(mut prev) = from.pop() {
            prev.current = self.song.current;
            prev.queued = None;
            to.push(self.song.clone());
            self.song = prev.clone();
            self.committed = prev;
            self.say(if redo { "redone" } else { "undone" });
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let wants_text = ctx.egui_wants_keyboard_input();
        // Starts on the press itself, so the tap that stops a recording doesn't start the next one.
        let v_pressed = !wants_text && ctx.input(|i| i.key_pressed(egui::Key::V) && !i.modifiers.ctrl);
        if v_pressed && !self.recorder.recording() {
            self.start_recording(self.selected);
        }
        if wants_text {
            return;
        }
        let pressed = |k| ctx.input(|i| i.key_pressed(k));
        let (ctrl, shift) = ctx.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
        use egui::Key::*;
        if pressed(Space) {
            self.toggle_play();
        }
        // Tab / Shift+Tab selects the track that V records into (intercepted in raw_input_hook).
        if let Some(back) = self.tab.take() {
            let n = self.song.tracks.len();
            self.selected = if back { (self.selected + n - 1) % n } else { (self.selected + 1) % n };
        }
        if pressed(ArrowLeft) {
            self.set_steps(self.song.steps().saturating_sub(1));
        }
        if pressed(ArrowRight) {
            self.set_steps(self.song.steps() + 1);
        }
        if pressed(ArrowUp) {
            self.song.bpm = (self.song.bpm + 1.0).min(300.0);
        }
        if pressed(ArrowDown) {
            self.song.bpm = (self.song.bpm - 1.0).max(40.0);
        }
        if ctrl && pressed(S) {
            self.save_now();
        }
        if ctrl && pressed(N) {
            self.new_song("Untitled", Song::blank(&self.samples));
        }
        if ctrl && pressed(O) {
            self.songs_open = !self.songs_open;
        }
        if ctrl && pressed(E) {
            self.export();
        }
        if ctrl && pressed(Z) {
            self.undo(shift);
        }
        if ctrl && pressed(Y) {
            self.undo(true);
        }
        if !ctrl {
            if pressed(R) {
                self.randomize();
            }
            if pressed(C) {
                self.clear_pattern();
            }
            if pressed(N) {
                self.add_track();
            }
            if pressed(T) {
                self.tap();
            }
        }
        // 1-9 mutes tracks 1 to 9, F1-F8 selects pattern A-H (with shift: copy to it).
        for (i, k) in [Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9].into_iter().enumerate() {
            if !ctrl && pressed(k) {
                if let Some(t) = self.song.tracks.get_mut(i) {
                    t.mute = !t.mute;
                }
            }
        }
        for (i, k) in [F1, F2, F3, F4, F5, F6, F7, F8].into_iter().enumerate() {
            if pressed(k) {
                self.select_pattern(i, shift);
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let th = self.theme.clone();
        ui.spacing_mut().interact_size.y = CONTROL_H;
        let button = |text: &str| egui::Button::new(text).min_size(Vec2::new(CONTROL_H, CONTROL_H));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let playing = self.playing();
            let play = egui::Button::new(egui::RichText::new(if playing { "■  Stop" } else { "▶  Play" }).color(th.on(if playing { th.red } else { th.accent })).strong())
                .fill(if playing { th.red } else { th.accent })
                .min_size(Vec2::new(96.0, CONTROL_H));
            if ui.add(play).on_hover_text("[Space]").clicked() {
                self.toggle_play();
            }
            section_gap(ui);
            label(ui, &th, "BPM");
            ui.add_sized([52.0, CONTROL_H], egui::DragValue::new(&mut self.song.bpm).range(40.0..=300.0).speed(0.5).fixed_decimals(0));
            if ui.add(button("Tap")).on_hover_text("tap the tempo [T]").clicked() {
                self.tap();
            }
            section_gap(ui);
            label(ui, &th, "Pattern");
            let active = self.shared.active.load(Ordering::Relaxed);
            let blink = (self.started.elapsed().as_secs_f32() * 4.0).fract() < 0.5;
            for p in 0..PATTERNS {
                let used = self.song.tracks.iter().any(|t| t.lanes[p].cells.iter().any(|c| *c != Cell::Off));
                let current = self.song.current == p;
                let queued = self.song.queued == Some(p);
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CONTROL_H), Sense::click());
                let painter = ui.painter();
                let fill = if current { th.accent } else if resp.hovered() { th.selection } else { th.bg_light };
                painter.rect_filled(rect, CornerRadius::ZERO, fill);
                if queued && blink {
                    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(2.0, th.accent), StrokeKind::Inside);
                }
                if self.playing() && active == p && !current {
                    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, th.fg_dim), StrokeKind::Inside);
                }
                let text_color = if current { th.on(th.accent) } else if used { th.fg_bright } else { th.fg_dim };
                painter.text(rect.center(), Align2::CENTER_CENTER, PATTERN_NAMES[p], FontId::monospace(13.0), text_color);
                if used && !current {
                    painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() - 5.0), 1.5, th.accent);
                }
                let resp = resp.on_hover_text("click: select pattern [F1-F8] · shift-click or right-click: copy the current pattern here");
                if resp.clicked() {
                    let shift = ui.input(|i| i.modifiers.shift);
                    self.select_pattern(p, shift);
                }
                if resp.secondary_clicked() {
                    self.select_pattern(p, true);
                }
            }
            // More pattern actions.
            egui::containers::menu::MenuButton::from_button(button("⋯")).ui(ui, |ui| {
                if ui.button("Random pattern  [R]").clicked() {
                    self.randomize();
                }
                ui.separator();
                ui.label(egui::RichText::new("Copy this pattern to").color(th.fg_dim).size(11.0));
                ui.horizontal(|ui| {
                    for p in 0..PATTERNS {
                        if p != self.song.current && ui.button(PATTERN_NAMES[p]).clicked() {
                            self.select_pattern(p, true);
                        }
                    }
                });
            });
            section_gap(ui);
            label(ui, &th, "Steps");
            if ui.add(button("−")).on_hover_text("[←]").clicked() {
                self.set_steps(self.song.steps().saturating_sub(1));
            }
            let mut steps = self.song.steps();
            if ui.add_sized([44.0, CONTROL_H], egui::DragValue::new(&mut steps).range(1..=MAX_STEPS).speed(0.2)).changed() {
                self.set_steps(steps);
            }
            if ui.add(button("+")).on_hover_text("[→]").clicked() {
                self.set_steps(self.song.steps() + 1);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if ui.add(button("Clear")).on_hover_text("clear this pattern [C] · undo with Ctrl+Z").clicked() {
                    self.clear_pattern();
                }
                if ui.add(button("Songs")).on_hover_text("open, create and delete songs [Ctrl+O]").clicked() {
                    self.songs_open = !self.songs_open;
                }
                // The song name opens the settings of the whole song.
                let settings_open = self.settings_open;
                let song_button = egui::Button::new(egui::RichText::new(format!("♪  {}", short(&self.song_name, 18))).color(if settings_open { th.on(th.accent) } else { th.fg_bright }))
                    .fill(if settings_open { th.accent } else { th.bg_light })
                    .min_size(Vec2::new(CONTROL_H, CONTROL_H));
                if ui.add(song_button).on_hover_text("song settings: name, tempo, swing and volume").clicked() {
                    self.settings_open = !self.settings_open;
                }
                if ui.add(button("Export")).on_hover_text("the current pattern 4 times to a WAV in ~/Music [Ctrl+E]").clicked() {
                    self.export();
                }
                section_gap(ui);
                if ui.add_enabled(!self.redo.is_empty(), button("↷")).on_hover_text("redo [Ctrl+Shift+Z]").clicked() {
                    self.undo(true);
                }
                if ui.add_enabled(!self.undo.is_empty(), button("↶")).on_hover_text("undo [Ctrl+Z]").clicked() {
                    self.undo(false);
                }
                section_gap(ui);
                if ui.add(button("?")).on_hover_text("about Sequencer").clicked() {
                    self.about_open = if self.about_open.is_some() { None } else { Some(Instant::now()) };
                }
            });
        });
    }

    fn master_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let th = self.theme.clone();
        let mut open = true;
        let mut rename = false;
        let mut tap = false;
        egui::Window::new("Song settings")
            .id(egui::Id::new("settings_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 52.0))
            .frame(egui::Frame::window(&ctx.global_style()).fill(th.bg_dark).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                ui.spacing_mut().slider_width = 150.0;
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                section(ui, &th, "Song");
                egui::Grid::new("song_name").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "Name");
                    let name = ui.add_sized([236.0, CONTROL_H], egui::TextEdit::singleline(&mut self.name_edit).vertical_align(egui::Align::Center));
                    if name.lost_focus() {
                        rename = true;
                    }
                    ui.end_row();
                });
                let song = &mut self.song;
                section(ui, &th, "Tempo");
                egui::Grid::new("song_tempo").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "BPM");
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(&mut song.bpm, 40.0..=300.0).step_by(1.0).fixed_decimals(0));
                        if ui.button("Tap").on_hover_text("[T]").clicked() {
                            tap = true;
                        }
                    });
                    ui.end_row();
                    row_label(ui, &th, "Swing");
                    ui.add(egui::Slider::new(&mut song.swing, 0.0..=0.5).custom_formatter(|v, _| format!("{:.0}%", v * 200.0)));
                    ui.end_row();
                });
                section(ui, &th, "Volume");
                egui::Grid::new("master_out").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "Master");
                    ui.add(amount(&mut song.master));
                    ui.end_row();
                });
            });
        if rename {
            self.rename_song();
        }
        if tap {
            self.tap();
        }
        if !open {
            self.settings_open = false;
            self.rename_song();
        }
    }

    fn grid(&mut self, ui: &mut egui::Ui) {
        let th = self.theme.clone();
        let steps = self.song.steps();
        let cur = self.song.current;
        // Rows sit as close together as the cells in a row: the only gap is the cell inset.
        ui.spacing_mut().item_spacing.y = 0.0;
        // Fixed size cells: when the steps don't fit, the grid scrolls instead of squashing.
        let cell_w = CELL;
        let row_h = CELL;
        let grid_w = cell_w * steps as f32;
        let playing = self.playing();
        let active = self.shared.active.load(Ordering::Relaxed);
        let playhead = if playing && active == cur { Some(self.shared.step.load(Ordering::Relaxed)) } else { None };
        // A new step: every audible note that starts on it gets a flash.
        if playhead != self.last_step {
            if let Some(h) = playhead {
                let solo = self.song.any_solo();
                for (ti, t) in self.song.tracks.iter().enumerate() {
                    let audible = if solo { t.solo } else { !t.mute };
                    if audible && t.lanes[cur].cells.get(h).is_some_and(|c| *c != Cell::Off) {
                        self.flashes.push((ti, h, Instant::now()));
                    }
                }
            }
            self.last_step = playhead;
        }
        self.flashes.retain(|f| f.2.elapsed() < FLASH);
        let mut ripples: Vec<(Rect, Color32, f32)> = Vec::new();

        let mut remove = None;
        let mut move_up = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // The track controls stay put…
            ui.vertical(|ui| {
                let (head, _) = ui.allocate_exact_size(Vec2::new(LEFT_W, 18.0), Sense::hover());
                let p = ui.painter_at(head);
                let mut hx = head.left() + STRIP_W;
                for (name, w) in COLUMNS {
                    let pos = if name == "Sample" { Pos2::new(hx + 6.0, head.center().y) } else { Pos2::new(hx + w / 2.0, head.center().y) };
                    let align = if name == "Sample" { Align2::LEFT_CENTER } else { Align2::CENTER_CENTER };
                    p.text(pos, align, name, FontId::monospace(10.0), th.fg_dim);
                    hx += w + COL_GAP;
                }
                for ti in 0..self.song.tracks.len() {
                    let (row, _) = ui.allocate_exact_size(Vec2::new(LEFT_W, row_h), Sense::hover());
                    self.track_controls(ui, ti, row, &mut remove, &mut move_up);
                }
            });
            // …and the grid scrolls sideways when it is wider than the window.
            egui::ScrollArea::horizontal().id_salt("grid_scroll").auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.vertical(|ui| {
                    let (head, _) = ui.allocate_exact_size(Vec2::new(grid_w, 18.0), Sense::hover());
                    let p = ui.painter_at(head);
                    for s in 0..steps {
                        let x = head.left() + cell_w * (s as f32 + 0.5);
                        let color = if playhead == Some(s) { th.fg_bright } else if s % 4 == 0 { th.fg } else { th.fg_dim };
                        p.text(Pos2::new(x, head.center().y), Align2::CENTER_CENTER, format!("{}", s + 1), FontId::monospace(10.0), color);
                    }
                    for ti in 0..self.song.tracks.len() {
                        let (cells, _) = ui.allocate_exact_size(Vec2::new(grid_w, row_h), Sense::hover());
                        self.cell_row(ui, ti, cells, steps, cur, playhead, &mut ripples);
                    }
                    // Rings last, so they sit on top of the neighbouring cells.
                    let p = ui.painter();
                    for (rect, color, strength) in ripples.drain(..) {
                        p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(2.0, color.gamma_multiply(strength)), StrokeKind::Outside);
                    }
                });
            });
        });

        if let Some(i) = remove {
            if self.song.tracks.len() > 1 {
                self.song.tracks.remove(i);
                self.selected = self.selected.min(self.song.tracks.len() - 1);
            }
        }
        if let Some(i) = move_up {
            if i > 0 {
                self.song.tracks.swap(i, i - 1);
            }
        }
    }

    /// The cells of one track: drawing notes with the mouse, and painting them.
    #[allow(clippy::too_many_arguments)]
    fn cell_row(&mut self, ui: &mut egui::Ui, ti: usize, cells: Rect, steps: usize, cur: usize, playhead: Option<usize>, ripples: &mut Vec<(Rect, Color32, f32)>) {
        let th = self.theme.clone();
        let cell_w = CELL;
        let resp = ui.interact(cells, ui.id().with(("cells", ti)), Sense::click_and_drag());
        let hit = |pos: Pos2| (((pos.x - cells.left()) / cell_w).floor().max(0.0) as usize).min(steps - 1);

        // Click or drag draws notes (as long as the track's L), clicking a note erases it,
        // right-click toggles an accent and scrolling over a note makes it longer or shorter.
        let pointer = ui.input(|i| i.pointer.clone());
        if pointer.primary_pressed() && resp.hovered() {
            if let Some(pos) = pointer.interact_pos() {
                self.selected = ti;
                let empty = self.song.tracks[ti].lanes[cur].note_at(hit(pos)).is_none();
                self.paint = Some(if empty { Cell::On } else { Cell::Off });
            }
        }
        if !pointer.primary_down() {
            self.paint = None;
        }
        let color = self.kind_color(self.samples[self.song.tracks[ti].sample].kind);
        let cell_rect = |s: usize, len: usize| Rect::from_min_size(Pos2::new(cells.left() + cell_w * s as f32, cells.top()), Vec2::new(cell_w * len as f32, cells.height())).shrink(CELL_INSET);
        if let (Some(paint), Some(pos)) = (self.paint, pointer.hover_pos()) {
            if cells.contains(pos) && pointer.primary_down() {
                // A click gets the full burst, a drag a smaller one per cell.
                let amount = if pointer.primary_pressed() { 18 } else { 6 };
                let s = hit(pos);
                let track = &mut self.song.tracks[ti];
                let len = track.note_len;
                let lane = &mut track.lanes[cur];
                if paint == Cell::Off {
                    if let Some(n) = lane.note_at(s) {
                        let bar = cell_rect(n, (lane.lens[n] as usize).min(steps - n));
                        lane.erase(s);
                        self.ghosts.push((bar, color, Instant::now()));
                        self.burst(bar, color, amount / 2, true);
                    }
                } else if lane.note_at(s).is_none() {
                    lane.place(s, Cell::On, len, steps);
                    self.pops.push((ti, cur, s, Instant::now()));
                    let bar = cell_rect(s, lane.lens[s] as usize);
                    self.rings.push((bar, color, Instant::now()));
                    self.burst(bar, color, amount, false);
                }
            }
        }
        if resp.secondary_clicked() {
            if let Some(pos) = pointer.interact_pos() {
                let track = &mut self.song.tracks[ti];
                let len = track.note_len;
                let lane = &mut track.lanes[cur];
                let s = hit(pos);
                let n = match lane.note_at(s) {
                    Some(n) => {
                        let c = &mut lane.cells[n];
                        *c = if *c == Cell::Accent { Cell::On } else { Cell::Accent };
                        n
                    }
                    None => {
                        lane.place(s, Cell::Accent, len, steps);
                        self.pops.push((ti, cur, s, Instant::now()));
                        s
                    }
                };
                let cell = cell_rect(n, 1);
                self.glints.push((cell.center(), color, Instant::now()));
                self.rings.push((cell, Color32::WHITE, Instant::now()));
                self.burst(cell, Color32::WHITE, 12, false);
            }
        }
        if resp.hovered() {
            if let Some(pos) = pointer.hover_pos() {
                let s = hit(pos);
                if self.song.tracks[ti].lanes[cur].note_at(s).is_some() {
                    let dy = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
                    self.wheel += dy;
                    let notches = (self.wheel / 30.0).trunc();
                    if notches != 0.0 {
                        self.wheel -= notches * 30.0;
                        self.song.tracks[ti].lanes[cur].resize(s, notches as i32, steps);
                    }
                }
            }
        }

        let p = ui.painter();
        let track = &self.song.tracks[ti];
        let lane = &track.lanes[cur];
        let color = self.kind_color(self.samples[track.sample].kind);
        let dead = if self.song.any_solo() { !track.solo } else { track.mute };
        for s in 0..steps {
            let r = Rect::from_min_size(Pos2::new(cells.left() + cell_w * s as f32, cells.top()), Vec2::new(cell_w, cells.height())).shrink(CELL_INSET);
            let beat = (s / 4) % 2 == 0;
            let mut base = if beat { th.bg_light } else { th.bg_dark };
            if playhead == Some(s) {
                base = th.accent.gamma_multiply(0.25);
            }
            p.rect_filled(r, CornerRadius::ZERO, base);
        }
        // Rollover: the note you would draw, or an outline around the one you would erase.
        let mut hover_note: Option<Rect> = None;
        if let Some(pos) = pointer.hover_pos().filter(|pos| resp.hovered() && cells.contains(*pos) && self.paint.is_none()) {
            let s = hit(pos);
            let lane = &self.song.tracks[ti].lanes[cur];
            match lane.note_at(s) {
                None => {
                    let room = (s + 1..steps).find(|&n| lane.cells[n] != Cell::Off).unwrap_or(steps) - s;
                    let len = (self.song.tracks[ti].note_len as usize).min(room).max(1);
                    let preview = cell_rect(s, len);
                    p.rect_filled(preview, CornerRadius::ZERO, color.gamma_multiply(0.28));
                    p.rect_stroke(preview, CornerRadius::ZERO, Stroke::new(1.5, color.gamma_multiply(0.9)), StrokeKind::Inside);
                }
                Some(n) => hover_note = Some(cell_rect(n, (lane.lens[n] as usize).min(steps - n))),
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        for n in 0..steps {
            let accent = match lane.cells[n] {
                Cell::Off => continue,
                Cell::On => false,
                Cell::Accent => true,
            };
            let len = (lane.lens[n] as usize).min(steps - n);
            let bar = Rect::from_min_size(Pos2::new(cells.left() + cell_w * n as f32, cells.top()), Vec2::new(cell_w * len as f32, cells.height())).shrink(CELL_INSET);
            // A note that was just drawn pops in with a little overshoot.
            let pop = self.pops.iter().rev().find(|p| p.0 == ti && p.1 == cur && p.2 == n).map(|p| (p.3.elapsed().as_secs_f32() / POP.as_secs_f32()).clamp(0.0, 1.0));
            let bar = match pop {
                Some(t) => {
                    let (c1, c3) = (1.70158f32, 2.70158f32);
                    let k = 1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2);
                    Rect::from_center_size(bar.center(), bar.size() * (0.35 + 0.65 * k))
                }
                None => bar,
            };
            let sounding = playhead.is_some_and(|h| h >= n && h < n + len) && !dead;
            let mut c = if accent { color } else { th.soften(color) };
            if dead {
                c = c.gamma_multiply(0.3);
            }
            p.rect_filled(bar, CornerRadius::ZERO, c);
            if let Some(t) = pop {
                p.rect_filled(bar, CornerRadius::ZERO, th.fg_bright.gamma_multiply((1.0 - t).powi(2) * 0.8));
            }
            // While a long note sounds, a soft highlight follows the playhead across its steps.
            if let (true, Some(h)) = (sounding && len > 1, playhead) {
                let block = Rect::from_min_size(Pos2::new(cells.left() + cell_w * h as f32, cells.top()), Vec2::new(cell_w, cells.height())).shrink(CELL_INSET);
                p.rect_filled(block, CornerRadius::ZERO, th.fg_bright.gamma_multiply(0.35));
            }
            // The trigger: the first step flashes bright and fades, and a ring grows out of it.
            if let Some(age) = self.flashes.iter().filter(|f| f.0 == ti && f.1 == n).map(|f| f.2.elapsed().as_secs_f32()).reduce(f32::min) {
                let t = (age / FLASH.as_secs_f32()).clamp(0.0, 1.0);
                let head = Rect::from_min_size(bar.min, Vec2::new((cell_w - 2.0 * CELL_INSET).min(bar.width()), bar.height()));
                p.rect_filled(head, CornerRadius::ZERO, th.fg_bright.gamma_multiply((1.0 - t).powi(2)));
                ripples.push((head.expand(1.0 + 5.0 * t), c, 1.0 - t));
            }
            // Dividers between the steps of a long note.
            for b in 1..len {
                let x = bar.left() + cell_w * b as f32 - 1.5;
                p.line_segment([Pos2::new(x, bar.top() + 4.0), Pos2::new(x, bar.bottom() - 4.0)], Stroke::new(1.0, th.bg.gamma_multiply(0.6)));
            }
            if accent {
                p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width(), 3.0)), CornerRadius::ZERO, th.fg_bright);
            }
            if len > 1 && cell_w >= 18.0 {
                p.text(bar.left_top() + Vec2::new(4.0, 5.0), Align2::LEFT_TOP, format!("{len}"), FontId::monospace(10.0), th.on(c));
            }
        }
        if let Some(bar) = hover_note {
            p.rect_stroke(bar.expand(1.0), CornerRadius::ZERO, Stroke::new(2.0, th.fg_bright), StrokeKind::Outside);
        }
    }

    fn track_controls(&mut self, ui: &mut egui::Ui, ti: usize, rect: Rect, remove: &mut Option<usize>, move_up: &mut Option<usize>) {
        let th = self.theme.clone();
        let samples = self.samples.clone();
        let kind_color = self.kind_color(samples[self.song.tracks[ti].sample].kind);
        // Every control is exactly as tall as a grid cell and sits on the same line.
        let top = rect.top() + CELL_INSET;
        let h = rect.height() - 2.0 * CELL_INSET;
        let mut x = rect.left() + STRIP_W;
        let mut col = 0;
        let mut next = |_: f32| {
            let w = COLUMNS[col].1;
            col += 1;
            let r = Rect::from_min_size(Pos2::new(x, top), Vec2::new(w, h));
            x += w + COL_GAP;
            r
        };

        // Color strip with level meter.
        let strip = Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(3.0, h));
        let lvl = self.shared.level(ti).max(self.meters[ti]);
        self.meters[ti] = lvl * 0.86;
        self.shared.levels[ti].store(0f32.to_bits(), Ordering::Relaxed);
        let p = ui.painter();
        p.rect_filled(strip, CornerRadius::ZERO, kind_color.gamma_multiply(0.35 + 0.65 * lvl.min(1.0)));

        let num = next(20.0);
        let num_resp = ui.interact(num, ui.id().with(("num", ti)), Sense::click()).on_hover_text("click: select and preview · right-click: move up");
        let selected = self.selected == ti;
        ui.painter().rect_filled(num, CornerRadius::ZERO, if selected { th.accent } else if num_resp.hovered() { th.selection } else { th.bg_light });
        ui.painter().text(num.center(), Align2::CENTER_CENTER, format!("{}", ti + 1), FontId::monospace(12.0), if selected { th.on(th.accent) } else if num_resp.hovered() { th.fg_bright } else { th.fg_dim });
        if num_resp.clicked() {
            self.selected = ti;
            let t = &self.song.tracks[ti];
            self.shared.preview.lock().unwrap().push((t.sample, t.volume, Some(ti)));
        }
        if num_resp.secondary_clicked() {
            *move_up = Some(ti);
        }

        // The sample name opens the sound browser.
        let sample_rect = next(150.0);
        let current = self.song.tracks[ti].sample;
        let open = self.sounds_open == Some(ti);
        let sresp = ui.interact(sample_rect, ui.id().with(("sample", ti)), Sense::click()).on_hover_text("choose a sound");
        let p = ui.painter();
        p.rect_filled(sample_rect, CornerRadius::ZERO, if open { th.accent } else if sresp.hovered() { th.selection } else { th.bg_light });
        let text = if open { th.on(th.accent) } else { th.fg_bright };
        let label = ui.painter().layout_no_wrap(samples[current].name.replace('_', " "), FontId::monospace(12.0), text);
        let clip = sample_rect.shrink2(Vec2::new(8.0, 0.0)).with_max_x(sample_rect.right() - 20.0);
        ui.painter().with_clip_rect(clip).galley(Pos2::new(clip.left(), sample_rect.center().y - label.size().y / 2.0), label, text);
        ui.painter().text(Pos2::new(sample_rect.right() - 10.0, sample_rect.center().y), Align2::CENTER_CENTER, "…", FontId::monospace(12.0), if open { text } else { th.fg_dim });
        if sresp.clicked() {
            if open {
                self.sounds_open = None;
            } else {
                self.sounds_open = Some(ti);
                self.sounds_source = source_of(&samples[current]);
                self.sounds_query.clear();
            }
        }

        // Mute, solo and record.
        let m = next(22.0);
        if toggle(ui, &th, m, "M", self.song.tracks[ti].mute, th.red, ("m", ti)).on_hover_text("mute [1-9]").clicked() {
            let t = &mut self.song.tracks[ti];
            t.mute = !t.mute;
        }
        let so = next(22.0);
        if toggle(ui, &th, so, "S", self.song.tracks[ti].solo, th.yellow, ("s", ti)).on_hover_text("solo").clicked() {
            let t = &mut self.song.tracks[ti];
            t.solo = !t.solo;
        }
        let rec = next(22.0);
        let rec_resp = ui.interact(rec, ui.id().with(("rec", ti)), Sense::click_and_drag()).on_hover_text("click to record, click again to stop · or hold and let go [V]");
        let recording_here = self.rec_track == Some(ti);
        if rec_resp.is_pointer_button_down_on() && !self.recorder.recording() {
            self.start_recording(ti);
        }
        if recording_here && rec_resp.clicked() && self.rec_latched.is_some_and(|t| t.elapsed() > Duration::from_millis(150)) {
            self.stop_recording();
        }
        {
            let p = ui.painter();
            let mic = f32::from_bits(self.recorder.level.load(Ordering::Relaxed));
            p.rect_filled(rec, CornerRadius::ZERO, if recording_here { th.red.gamma_multiply(0.3 + 0.7 * (mic * 3.0).min(1.0)) } else { th.bg_light });
            p.circle_filled(rec.center(), 4.5, if recording_here { th.fg_bright } else if rec_resp.hovered() { th.red } else { th.red.gamma_multiply(0.6) });
        }

        let t = &mut self.song.tracks[ti];
        // Volume, pan and delay send as draggable bars.
        let vol = next(52.0);
        bar_control(ui, &th, vol, &mut t.volume, 0.0, 1.0, th.accent, ("vol", ti), "volume");
        let pan = next(40.0);
        bar_control(ui, &th, pan, &mut t.pan, -1.0, 1.0, th.cyan, ("pan", ti), "pan (double-click = center)");
        // Opens the effects window for this track; lit when any effect is on.
        let fx_rect = next(40.0);
        let active = t.fx.active() || t.send > 0.0;
        let open = self.fx_open.is_some_and(|(o, _)| o == ti);
        let fresp = ui.interact(fx_rect, ui.id().with(("fx", ti)), Sense::click()).on_hover_text("effects for this track");
        let p = ui.painter();
        let fill = if open { th.accent } else if fresp.hovered() { th.selection } else { th.bg_light };
        p.rect_filled(fx_rect, CornerRadius::ZERO, fill);
        let color = if open { th.on(th.accent) } else if active { th.magenta } else { th.fg_dim };
        p.text(fx_rect.center(), Align2::CENTER_CENTER, "FX", FontId::monospace(12.0), color);
        if active && !open {
            p.circle_filled(Pos2::new(fx_rect.right() - 6.0, fx_rect.top() + 6.0), 2.0, th.magenta);
        }
        if fresp.clicked() {
            self.fx_open = if open { None } else { Some((ti, fx_rect.right_top() + Vec2::new(8.0, 0.0))) };
            self.fx_loop = None;
        }
        let t = &mut self.song.tracks[ti];

        // Pitch in semitones: drag vertically, double-click = 0.
        let pitch = next(34.0);
        let presp = ui.interact(pitch, ui.id().with(("pitch", ti)), Sense::click_and_drag()).on_hover_text("pitch in semitones (drag · double-click = 0)");
        if presp.dragged() {
            t.pitch = (t.pitch - presp.drag_delta().y * 0.1).clamp(-24.0, 24.0);
        }
        if presp.double_clicked() {
            t.pitch = 0.0;
        }
        if presp.drag_stopped() {
            t.pitch = t.pitch.round();
        }
        let p = ui.painter();
        p.rect_filled(pitch, CornerRadius::ZERO, if presp.hovered() { th.selection } else { th.bg_light });
        p.text(pitch.center(), Align2::CENTER_CENTER, format!("{:+}", t.pitch.round() as i32), FontId::monospace(12.0), if t.pitch.round() == 0.0 { th.fg_dim } else { th.fg_bright });

        // Length of new notes in steps: click = longer, right-click = shorter.
        let len_rect = next(30.0);
        let lresp = ui.interact(len_rect, ui.id().with(("len", ti)), Sense::click()).on_hover_text("length of new notes in steps (click / right-click)");
        const LENS: [u8; 5] = [1, 2, 4, 8, 16];
        let li = LENS.iter().position(|&l| l >= t.note_len).unwrap_or(0);
        if lresp.clicked() {
            t.note_len = LENS[(li + 1) % LENS.len()];
        }
        if lresp.secondary_clicked() {
            t.note_len = LENS[(li + LENS.len() - 1) % LENS.len()];
        }
        let p = ui.painter();
        p.rect_filled(len_rect, CornerRadius::ZERO, if lresp.hovered() { th.selection } else { th.bg_light });
        p.text(len_rect.center(), Align2::CENTER_CENTER, format!("L{}", t.note_len), FontId::monospace(12.0), if t.note_len > 1 { th.fg_bright } else { th.fg_dim });

        let x_rect = next(18.0);
        let xr = ui.interact(x_rect, ui.id().with(("x", ti)), Sense::click()).on_hover_text("remove track");
        ui.painter().rect_filled(x_rect, CornerRadius::ZERO, if xr.hovered() { th.selection } else { th.bg_light });
        ui.painter().text(x_rect.center(), Align2::CENTER_CENTER, "×", FontId::monospace(15.0), if xr.hovered() { th.red } else { th.fg_dim });
        if xr.clicked() {
            *remove = Some(ti);
        }
    }

    fn fx_window(&mut self, ctx: &egui::Context) {
        let Some((ti, pos)) = self.fx_open else { return };
        if ti >= self.song.tracks.len() {
            self.fx_open = None;
            return;
        }
        let th = self.theme.clone();
        let name = self.samples[self.song.tracks[ti].sample].name.replace('_', " ");
        let mut open = true;
        let mut preview = false;
        egui::Window::new(format!("FX · {} {name}", ti + 1))
            .id(egui::Id::new("fx_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(pos)
            .frame(egui::Frame::window(&ctx.global_style()).fill(th.bg_dark).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                let t = &mut self.song.tracks[ti];
                ui.spacing_mut().slider_width = 150.0;
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                let hz = |v: f64, lo: f64, hi: f64| format!("{:.0} Hz", lo * (hi / lo).powf(v));

                ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                section(ui, &th, "Pitch");
                egui::Grid::new("fx_pitch").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "Semitones");
                    ui.add(egui::Slider::new(&mut t.pitch, -24.0..=24.0).step_by(1.0).suffix(" st"));
                    ui.end_row();
                    row_label(ui, &th, "Fine");
                    ui.add(egui::Slider::new(&mut t.fx.fine, -100.0..=100.0).step_by(1.0).suffix(" ct"));
                    ui.end_row();
                    row_label(ui, &th, "Reverse");
                    ui.checkbox(&mut t.fx.reverse, "play the sample backwards");
                    ui.end_row();
                });

                section(ui, &th, "Filter");
                egui::Grid::new("fx_filter").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "Type");
                    ui.horizontal(|ui| {
                        for (kind, label) in [(FilterKind::Off, "Off"), (FilterKind::Low, "Low"), (FilterKind::High, "High"), (FilterKind::Band, "Band")] {
                            ui.selectable_value(&mut t.fx.filter, kind, label);
                        }
                    });
                    ui.end_row();
                    row_label(ui, &th, "Cutoff");
                    ui.add_enabled(
                        t.fx.filter != FilterKind::Off,
                        egui::Slider::new(&mut t.fx.cutoff, 0.0..=1.0).custom_formatter(move |v, _| hz(v, 40.0, 18_000.0)),
                    );
                    ui.end_row();
                    row_label(ui, &th, "Resonance");
                    ui.add_enabled(t.fx.filter != FilterKind::Off, amount(&mut t.fx.resonance));
                    ui.end_row();
                });

                section(ui, &th, "EQ");
                egui::Grid::new("fx_eq").num_columns(2).show(ui, |ui| {
                    for (label, v) in [("Low", &mut t.fx.eq_low), ("Mid", &mut t.fx.eq_mid), ("High", &mut t.fx.eq_high)] {
                        row_label(ui, &th, label);
                        ui.add(egui::Slider::new(v, -12.0..=12.0).step_by(0.5).suffix(" dB"));
                        ui.end_row();
                    }
                });

                });
                ui.add_space(24.0);
                ui.vertical(|ui| {
                section(ui, &th, "Color");
                egui::Grid::new("fx_color").num_columns(2).show(ui, |ui| {
                    for (label, v) in [("Drive", &mut t.fx.drive), ("Distortion", &mut t.fx.distort), ("Bitcrush", &mut t.fx.crush), ("Sample rate", &mut t.fx.downsample), ("Ring mod", &mut t.fx.ring)] {
                        row_label(ui, &th, label);
                        ui.add(amount(v));
                        ui.end_row();
                    }
                    row_label(ui, &th, "Ring freq");
                    ui.add_enabled(t.fx.ring > 0.0, egui::Slider::new(&mut t.fx.ring_freq, 0.0..=1.0).custom_formatter(move |v, _| hz(v, 30.0, 2000.0)));
                    ui.end_row();
                });

                section(ui, &th, "Rhythm");
                egui::Grid::new("fx_rhythm").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "Chop");
                    ui.add(amount(&mut t.fx.chop));
                    ui.end_row();
                    row_label(ui, &th, "Chop rate");
                    ui.horizontal(|ui| {
                        for (steps, label) in [(1u8, "1/16"), (2, "1/8"), (4, "1/4")] {
                            ui.selectable_value(&mut t.fx.chop_steps, steps, label);
                        }
                    });
                    ui.end_row();
                });

                section(ui, &th, "Delay");
                egui::Grid::new("fx_delay").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "Amount");
                    ui.add(amount(&mut t.send));
                    ui.end_row();
                    row_label(ui, &th, "Time");
                    ui.horizontal(|ui| {
                        for d in [1u8, 2, 3, 4, 6, 8] {
                            ui.selectable_value(&mut t.fx.delay_steps, d, delay_name(d));
                        }
                    });
                    ui.end_row();
                    row_label(ui, &th, "Feedback");
                    ui.add(egui::Slider::new(&mut t.fx.feedback, 0.0..=0.85).custom_formatter(|v, _| format!("{:.0}%", v / 0.85 * 100.0)));
                    ui.end_row();
                });

                section(ui, &th, "Reverb");
                egui::Grid::new("fx_reverb").num_columns(2).show(ui, |ui| {
                    row_label(ui, &th, "Amount");
                    ui.add(amount(&mut t.fx.reverb));
                    ui.end_row();
                });
                });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("▶  Preview").min_size(Vec2::new(0.0, CONTROL_H))).clicked() {
                        preview = true;
                    }
                    let looping = self.fx_loop.is_some();
                    let loop_button = egui::Button::new(egui::RichText::new("⟳  Loop").color(if looping { th.on(th.accent) } else { th.fg }))
                        .fill(if looping { th.accent } else { th.bg_light })
                        .min_size(Vec2::new(0.0, CONTROL_H));
                    if ui.add(loop_button).on_hover_text("play the sample again and again, with a pause in between").clicked() {
                        self.fx_loop = if looping { None } else { Some(Instant::now()) };
                    }
                    ui.label(egui::RichText::new("pause").color(th.fg_dim).size(12.0));
                    for gap in [0.5f32, 1.0, 2.0] {
                        ui.selectable_value(&mut self.fx_gap, gap, format!("{gap} s"));
                    }
                    ui.add_space(12.0);
                    if ui.add(egui::Button::new("Reset").min_size(Vec2::new(0.0, CONTROL_H))).on_hover_text("turn every effect off").clicked() {
                        t.fx = Fx::default();
                        t.send = 0.0;
                        t.pitch = 0.0;
                    }
                });
            });
        // Looping preview: the whole sample (at its pitch), then the pause, then again.
        if let Some(next) = self.fx_loop {
            if Instant::now() >= next {
                preview = true;
                let t = &self.song.tracks[ti];
                let s = &self.samples[t.sample];
                let pitch = t.pitch + t.fx.fine / 100.0;
                let length = s.data.len() as f32 / s.rate as f32 / 2f32.powf(pitch / 12.0);
                self.fx_loop = Some(Instant::now() + Duration::from_secs_f32(length.max(0.1) + self.fx_gap));
            }
        }
        if preview {
            let t = &self.song.tracks[ti];
            self.shared.preview.lock().unwrap().push((t.sample, t.volume, Some(ti)));
        }
        if !open {
            self.fx_open = None;
            self.fx_loop = None;
        }
    }

    /// Throws `amount` sparks from anywhere in `from`; `falling` ones drop instead of bursting outwards.
    fn burst(&mut self, from: Rect, color: Color32, amount: usize, falling: bool) {
        for _ in 0..amount {
            let at = Pos2::new(from.left() + fastrand::f32() * from.width(), from.center().y + (fastrand::f32() - 0.5) * from.height() * 0.5);
            let angle = fastrand::f32() * std::f32::consts::TAU;
            let speed = if falling { 30.0 + fastrand::f32() * 60.0 } else { 110.0 + fastrand::f32() * 170.0 };
            let mut vel = Vec2::angled(angle) * speed;
            if falling {
                vel.y = vel.y.abs() * 0.4;
            }
            self.sparks.push(Spark {
                pos: at,
                vel,
                color: if fastrand::f32() < 0.3 { self.theme.fg_bright } else { color },
                born: Instant::now(),
                life: 0.5 + fastrand::f32() * 0.45,
                size: 3.0 + fastrand::f32() * 3.0,
            });
        }
    }

    /// Paints the click effects on top of everything and forgets the ones that are done.
    fn effects(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("click_effects")));
        for s in &mut self.sparks {
            s.vel.y += 260.0 * dt;
            s.vel *= 1.0 - 2.5 * dt;
            s.pos += s.vel * dt;
            let t = now.duration_since(s.born).as_secs_f32() / s.life;
            let size = s.size * (1.0 - t * 0.6);
            p.rect_filled(Rect::from_center_size(s.pos, Vec2::splat(size)), CornerRadius::ZERO, s.color.gamma_multiply((1.0 - t).clamp(0.0, 1.0)));
        }
        self.sparks.retain(|s| now.duration_since(s.born).as_secs_f32() < s.life);
        for (rect, color, at) in &self.ghosts {
            let t = (now.duration_since(*at).as_secs_f32() / 0.3).min(1.0);
            let ghost = Rect::from_center_size(rect.center(), rect.size() * (1.0 - t * 0.7));
            p.rect_filled(ghost, CornerRadius::ZERO, color.gamma_multiply(0.5 * (1.0 - t)));
            p.rect_stroke(ghost, CornerRadius::ZERO, Stroke::new(1.5, color.gamma_multiply(1.0 - t)), StrokeKind::Outside);
        }
        self.ghosts.retain(|g| now.duration_since(g.2).as_secs_f32() < 0.3);
        // A ring that grows out of a new note and fades.
        for (rect, color, at) in &self.rings {
            let t = (now.duration_since(*at).as_secs_f32() / 0.4).min(1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            p.rect_stroke(rect.expand(2.0 + 12.0 * ease), CornerRadius::ZERO, Stroke::new(2.5 * (1.0 - t) + 0.5, color.gamma_multiply(1.0 - t)), StrokeKind::Outside);
        }
        self.rings.retain(|r| now.duration_since(r.2).as_secs_f32() < 0.4);
        for (center, color, at) in &self.glints {
            let t = (now.duration_since(*at).as_secs_f32() / 0.4).min(1.0);
            let r = 6.0 + 22.0 * t;
            let fade = (1.0 - t).powi(2);
            for d in [Vec2::new(r, 0.0), Vec2::new(0.0, r)] {
                p.line_segment([*center - d, *center + d], Stroke::new(2.0, Color32::WHITE.gamma_multiply(fade)));
            }
            let d = r * 0.45;
            p.line_segment([*center - Vec2::splat(d), *center + Vec2::splat(d)], Stroke::new(1.5, color.gamma_multiply(fade)));
            p.line_segment([*center + Vec2::new(d, -d), *center + Vec2::new(-d, d)], Stroke::new(1.5, color.gamma_multiply(fade)));
        }
        self.glints.retain(|g| now.duration_since(g.2).as_secs_f32() < 0.4);
        self.pops.retain(|p| now.duration_since(p.3) < POP);
    }

    /// While recording: a panel in the bottom right with the live waveform, the time and a stop button.
    fn recording_panel(&mut self, ctx: &egui::Context) {
        if !self.recorder.recording() {
            return;
        }
        let th = self.theme.clone();
        let mut stop = false;
        egui::Area::new(egui::Id::new("recording_panel"))
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-14.0, -42.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new().fill(th.bg_dark).stroke(Stroke::new(1.5, th.red)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(340.0);
                    ui.horizontal(|ui| {
                        let blink = (self.started.elapsed().as_secs_f32() * 2.0).fract() < 0.6;
                        ui.label(egui::RichText::new(if blink { "●" } else { " " }).color(th.red).size(14.0));
                        let secs = self.recorder.length();
                        ui.label(egui::RichText::new(format!("REC  track {}  {:>2}:{:04.1}", self.rec_track.map_or(0, |t| t + 1), (secs / 60.0) as u32, secs % 60.0)).color(th.fg_bright).size(12.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if self.rec_latched.is_some() { "■ Stop" } else { "let go to stop" };
                            let b = egui::Button::new(egui::RichText::new(label).color(th.on(th.red))).fill(th.red);
                            if ui.add_enabled(self.rec_latched.is_some(), b).clicked() {
                                stop = true;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    // The last three seconds, as min/max bars scrolling from right to left.
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(340.0, 70.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, CornerRadius::ZERO, th.bg_darker);
                    p.line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0, th.bg_light));
                    let seconds = 3.0;
                    let data = self.recorder.tail(seconds);
                    let columns = 170usize;
                    let per = ((self.recorder.rate() as f32 * seconds) as usize / columns).max(1);
                    let offset = columns.saturating_sub(data.len() / per);
                    for (c, chunk) in data.chunks(per).enumerate() {
                        let (lo, hi) = chunk.iter().fold((0.0f32, 0.0f32), |(lo, hi), s| (lo.min(*s), hi.max(*s)));
                        let x = rect.left() + (offset + c) as f32 * rect.width() / columns as f32;
                        let y = |v: f32| rect.center().y - (v * 2.5).clamp(-1.0, 1.0) * rect.height() * 0.48;
                        p.line_segment([Pos2::new(x, y(hi)), Pos2::new(x, y(lo).max(y(hi) + 1.0))], Stroke::new(1.5, th.red));
                    }
                });
            });
        if stop {
            self.stop_recording();
        }
    }

    fn start_install(&mut self, id: &str) {
        let Some(entry) = packs::CATALOG.iter().find(|e| e.id == id) else { return };
        let progress = Arc::new(Mutex::new("starting…".to_string()));
        self.installing.lock().unwrap().push((id.to_owned(), progress.clone()));
        let (installing, installed) = (self.installing.clone(), self.installed.clone());
        std::thread::spawn(move || {
            let result = packs::install(entry, &progress);
            installing.lock().unwrap().retain(|(i, _)| i != entry.id);
            installed.lock().unwrap().push((entry.id.to_owned(), result));
        });
    }

    /// Picks up finished downloads: their sounds become available right away.
    fn finish_installs(&mut self) {
        let done = std::mem::take(&mut *self.installed.lock().unwrap());
        for (id, result) in done {
            match result {
                Ok(()) => {
                    let new = samples::load_pack(&id, &packs::dir().join(&id));
                    let count = new.len();
                    for sample in new {
                        let sample = Arc::new(sample);
                        self.samples.push(sample.clone());
                        self.shared.incoming.lock().unwrap().push(sample);
                    }
                    let name = packs::CATALOG.iter().find(|e| e.id == id).map_or(id.as_str(), |e| e.name);
                    self.say(format!("{name} installed: {count} sounds"));
                    if self.sounds_source == "get" {
                        self.sounds_source = id.clone();
                    }
                }
                Err(e) => self.say(format!("installing {id} failed: {e}")),
            }
        }
    }

    fn sounds_window(&mut self, ctx: &egui::Context) {
        let Some(ti) = self.sounds_open else { return };
        if ti >= self.song.tracks.len() {
            self.sounds_open = None;
            return;
        }
        let th = self.theme.clone();
        let current = self.song.tracks[ti].sample;
        let installed_packs = packs::installed();
        let mut sources: Vec<(String, String, &str)> = vec![
            ("classic:drum".into(), "808 drums".into(), "Classic"),
            ("classic:riff".into(), "Riffs".into(), "Classic"),
            ("classic:rave".into(), "90s rave".into(), "Classic"),
            ("classic:vox".into(), "Vocals".into(), "Classic"),
        ];
        for (info, _) in &installed_packs {
            sources.push((info.id.clone(), info.name.clone(), "Packs"));
        }
        sources.push(("user".into(), "Your sounds".into(), "Yours"));
        let mut chosen = None;
        let mut preview = None;
        let mut install = None;
        let mut remove = None;
        let mut open = true;
        egui::Window::new(format!("Sounds · track {}", ti + 1))
            .id(egui::Id::new("sounds_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(Pos2::new(LEFT_W + 8.0, 60.0))
            .frame(egui::Frame::window(&ctx.global_style()).fill(th.bg_dark).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                let search = ui.add_sized([560.0, CONTROL_H], egui::TextEdit::singleline(&mut self.sounds_query).hint_text("search all sounds…").vertical_align(egui::Align::Center));
                if search.changed() && !self.sounds_query.is_empty() && self.sounds_source == "get" {
                    self.sounds_source = "classic:drum".into();
                }
                ui.add_space(4.0);
                ui.horizontal_top(|ui| {
                    // Where the sounds come from.
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        let mut group = "";
                        for (key, name, g) in &sources {
                            if *g != group {
                                section(ui, &th, g);
                                group = g;
                            }
                            let on = self.sounds_query.is_empty() && &self.sounds_source == key;
                            let count = self.samples.iter().filter(|s| &source_of(s) == key).count();
                            if list_row_count(ui, &th, name, count, on).clicked() {
                                self.sounds_source = key.clone();
                                self.sounds_query.clear();
                            }
                        }
                        ui.add_space(8.0);
                        let on = self.sounds_query.is_empty() && self.sounds_source == "get";
                        if list_row(ui, &th, "+ Get more packs", on).clicked() {
                            self.sounds_source = "get".into();
                            self.sounds_query.clear();
                        }
                    });
                    ui.add_space(6.0);
                    // The sounds themselves, or the packs you can download.
                    ui.vertical(|ui| {
                        ui.set_width(380.0);
                        egui::ScrollArea::vertical().id_salt("sounds_list").max_height(420.0).min_scrolled_height(420.0).show(ui, |ui| {
                            ui.set_width(372.0);
                            if self.sounds_query.is_empty() && self.sounds_source == "get" {
                                let busy: Vec<(String, String)> = self.installing.lock().unwrap().iter().map(|(i, p)| (i.clone(), p.lock().unwrap().clone())).collect();
                                ui.label(egui::RichText::new("Sound packs").color(th.fg_bright).size(15.0));
                                ui.label(egui::RichText::new("Free libraries to use in anything you make, CC0 or public domain. One click and they are in the browser.").color(th.fg_dim).size(11.0));
                                ui.add_space(6.0);
                                let tiles = [th.accent, th.magenta, th.green, th.yellow, th.cyan, th.orange];
                                for (n, entry) in packs::CATALOG.iter().enumerate() {
                                    let is_installed = installed_packs.iter().any(|(i, _)| i.id == entry.id);
                                    let progress = busy.iter().find(|(i, _)| i == entry.id).map(|(_, p)| p.clone());
                                    let sounds = self.samples.iter().filter(|s| s.pack == entry.id).count();
                                    let card = PackCard { entry, tile: tiles[n % tiles.len()], installed: is_installed, sounds, progress, asking: self.confirm_remove.as_deref() == Some(entry.id) };
                                    match pack_card(ui, &th, card, self.started.elapsed().as_secs_f32()) {
                                        Some(PackAction::Install) => install = Some(entry.id),
                                        Some(PackAction::Open) => {
                                            self.sounds_source = entry.id.into();
                                            self.confirm_remove = None;
                                        }
                                        Some(PackAction::AskRemove) => self.confirm_remove = Some(entry.id.into()),
                                        Some(PackAction::Remove) => remove = Some(entry.id),
                                        None => {}
                                    }
                                }
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("Packs are saved in ~/.local/share/omarchy-sequencer/packs").color(th.fg_dim).size(10.0));
                                return;
                            }
                            let query = self.sounds_query.to_lowercase();
                            let visible = |s: &Sample| {
                                if !query.is_empty() {
                                    let hidden = !matches!(s.pack.as_str(), "classic" | "user") && !installed_packs.iter().any(|(i, _)| i.id == s.pack);
                                    return !hidden && (s.name.replace('_', " ").contains(&query) || s.id.replace('_', " ").contains(&query));
                                }
                                source_of(s) == self.sounds_source
                            };
                            let mut any = false;
                            for (i, smp) in self.samples.iter().enumerate().filter(|(_, s)| visible(s)) {
                                any = true;
                                let pack = (!query.is_empty()).then(|| source_name(&smp.pack, &installed_packs));
                                let resp = sound_row(ui, &th, &smp.name.replace('_', " "), pack.as_deref(), self.kind_color(smp.kind), current == i);
                                if resp.clicked() {
                                    chosen = Some(i);
                                }
                                if resp.secondary_clicked() {
                                    preview = Some(i);
                                }
                            }
                            if !any {
                                let empty = if query.is_empty() && self.sounds_source == "user" {
                                    "Nothing here yet. Record with the red button on a track, or put .wav files in ~/.local/share/omarchy-sequencer/samples"
                                } else {
                                    "No sounds found"
                                };
                                ui.label(egui::RichText::new(empty).color(th.fg_dim).size(12.0));
                            }
                        });
                    });
                });
                ui.label(egui::RichText::new("click: use it on this track · right-click: just listen").color(th.fg_dim).size(11.0));
            });
        if let Some(i) = chosen {
            let t = &mut self.song.tracks[ti];
            t.sample = i;
            t.note_len = self.samples[i].default_len();
            self.shared.preview.lock().unwrap().push((i, t.volume, Some(ti)));
        }
        if let Some(i) = preview {
            self.shared.preview.lock().unwrap().push((i, 0.8, None));
        }
        if let Some(id) = install {
            self.start_install(id);
        }
        if let Some(id) = remove {
            self.confirm_remove = None;
            match packs::remove(id) {
                Ok(()) => self.say("pack removed; tracks that use it keep their sound until you restart"),
                Err(e) => self.say(format!("removing failed: {e}")),
            }
        }
        if !open {
            self.sounds_open = None;
            self.confirm_remove = None;
        }
    }

    /// An about box like software used to have: a big ASCII logo and credits that scroll by.
    fn about_window(&mut self, ctx: &egui::Context) {
        let Some(opened) = self.about_open else { return };
        let th = self.theme.clone();
        let mut open = true;
        egui::Window::new("About")
            .id(egui::Id::new("about_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(egui::Frame::window(&ctx.global_style()).fill(th.bg_dark).inner_margin(egui::Margin::same(22)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
                // The logo, each line a step further from the accent towards magenta.
                let lines: Vec<&str> = LOGO.lines().collect();
                // Drawn cell by cell instead of as text, so the blocks are solid and the shadow joins up.
                let (cw, ch) = (7.0, 14.0);
                let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(cw * cols as f32, ch * lines.len() as f32), Sense::hover());
                let t = opened.elapsed().as_secs_f32();
                let p = ui.painter();
                for (r, line) in lines.iter().enumerate() {
                    for (c, glyph) in line.chars().enumerate() {
                        let cell = Rect::from_min_size(rect.left_top() + Vec2::new(cw * c as f32, ch * r as f32), Vec2::new(cw, ch));
                        // A colour wave running across the logo.
                        let wave = ((t * 1.4 - c as f32 * 0.08 - r as f32 * 0.25).sin() * 0.5 + 0.5) * 0.5;
                        let color = lerp_color(th.accent, th.magenta, (r as f32 / 5.0 * 0.6 + wave).min(1.0));
                        let shadow = th.fg_dim;
                        let (mx, my) = (cell.center().x, cell.center().y);
                        let line = |a: Pos2, b: Pos2| {
                            p.line_segment([a, b], Stroke::new(1.5, shadow));
                        };
                        match glyph {
                            '█' => {
                                p.rect_filled(cell, CornerRadius::ZERO, color);
                            }
                            '▄' => {
                                p.rect_filled(cell.with_min_y(my), CornerRadius::ZERO, color);
                            }
                            '▀' => {
                                p.rect_filled(cell.with_max_y(my), CornerRadius::ZERO, color);
                            }
                            '═' => line(Pos2::new(cell.left(), my), Pos2::new(cell.right(), my)),
                            '║' => line(Pos2::new(mx, cell.top()), Pos2::new(mx, cell.bottom())),
                            '╔' => {
                                line(Pos2::new(mx, my), Pos2::new(cell.right(), my));
                                line(Pos2::new(mx, my), Pos2::new(mx, cell.bottom()));
                            }
                            '╗' => {
                                line(Pos2::new(cell.left(), my), Pos2::new(mx, my));
                                line(Pos2::new(mx, my), Pos2::new(mx, cell.bottom()));
                            }
                            '╚' => {
                                line(Pos2::new(mx, cell.top()), Pos2::new(mx, my));
                                line(Pos2::new(mx, my), Pos2::new(cell.right(), my));
                            }
                            '╝' => {
                                line(Pos2::new(mx, cell.top()), Pos2::new(mx, my));
                                line(Pos2::new(cell.left(), my), Pos2::new(mx, my));
                            }
                            _ => {}
                        }
                    }
                }
                let width = rect.width();
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("version {}", env!("CARGO_PKG_VERSION"))).color(th.fg_dim).size(12.0));
                    ui.label(egui::RichText::new("·").color(th.fg_dim).size(12.0));
                    ui.label(egui::RichText::new("a step sequencer for Omarchy").color(th.fg).size(12.0));
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("made by Jankees van Woezik ·").color(th.fg).size(12.0));
                    let link = ui.add(egui::Label::new(egui::RichText::new("jankeesvw.com").color(th.accent).size(12.0).underline()).sense(Sense::click()));
                    if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        let _ = std::process::Command::new("xdg-open").arg("https://jankeesvw.com").spawn();
                    }
                });
                ui.add_space(10.0);
                // Credits rolling by, the old-fashioned way.
                let (area, _) = ui.allocate_exact_size(Vec2::new(width, 110.0), Sense::hover());
                let p = ui.painter().with_clip_rect(area);
                p.rect_filled(area, CornerRadius::ZERO, th.bg);
                let line_h = 18.0;
                let total = CREDITS.len() as f32 * line_h + area.height();
                let offset = (t * 16.0) % total;
                for (i, (role, who)) in CREDITS.iter().enumerate() {
                    let y = area.bottom() - offset + i as f32 * line_h;
                    if y < area.top() - line_h || y > area.bottom() {
                        continue;
                    }
                    let fade = ((y - area.top()) / 24.0).min((area.bottom() - y) / 24.0).clamp(0.0, 1.0);
                    let center = area.center().x;
                    p.text(Pos2::new(center - 8.0, y), Align2::RIGHT_TOP, *role, FontId::monospace(11.0), th.fg_dim.gamma_multiply(fade));
                    p.text(Pos2::new(center + 8.0, y), Align2::LEFT_TOP, *who, FontId::monospace(11.0), th.fg_bright.gamma_multiply(fade));
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new("MIT licensed · every bundled sound is CC0 or public domain").color(th.fg_dim).size(10.0));
            });
        if !open {
            self.about_open = None;
        }
    }

    fn open_share(&mut self) {
        self.share_open = true;
        self.share_title = self.song_name.clone();
        if !matches!(*self.share_state.lock().unwrap(), ShareState::Busy(_)) {
            *self.share_state.lock().unwrap() = ShareState::Idle;
        }
    }

    fn start_share(&mut self) {
        songs::set_artist(&self.share_artist);
        let _ = songs::write(&self.song_name, &self.song, &self.samples);
        let progress = Arc::new(Mutex::new("recording…".to_string()));
        *self.share_state.lock().unwrap() = ShareState::Busy(progress.clone());
        let (samples, song, json) = (self.samples.clone(), self.song.clone(), songs::to_json(&self.song, &self.samples));
        let share = community::Share { title: self.share_title.trim().to_owned(), artist: self.share_artist.trim().to_owned(), description: self.share_description.trim().to_owned() };
        let state = self.share_state.clone();
        std::thread::spawn(move || {
            let result = community::share(samples, &song, json, share, &progress);
            *state.lock().unwrap() = match result {
                Ok(url) => ShareState::Done(url),
                Err(e) => ShareState::Failed(e),
            };
        });
    }

    /// Sharing a song with the community site.
    fn share_window(&mut self, ctx: &egui::Context) {
        if !self.share_open {
            return;
        }
        let th = self.theme.clone();
        let mut open = true;
        let mut go = false;
        let state = match &*self.share_state.lock().unwrap() {
            ShareState::Idle => ShareState::Idle,
            ShareState::Busy(p) => ShareState::Busy(p.clone()),
            ShareState::Done(u) => ShareState::Done(u.clone()),
            ShareState::Failed(e) => ShareState::Failed(e.clone()),
        };
        egui::Window::new("Share")
            .id(egui::Id::new("share_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(egui::Frame::window(&ctx.global_style()).fill(th.bg_dark).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                match &state {
                    ShareState::Done(url) => {
                        section(ui, &th, "Shared");
                        ui.label(egui::RichText::new("Your song is on the community site. Anyone can listen, vote, and open it in Sequencer.").color(th.fg));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(url).color(th.accent));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new("Open in browser").color(th.on(th.accent))).fill(th.accent).min_size(Vec2::new(0.0, CONTROL_H))).clicked() {
                                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                            }
                            if ui.add(egui::Button::new("Copy link").min_size(Vec2::new(0.0, CONTROL_H))).clicked() {
                                ui.ctx().copy_text(url.clone());
                            }
                        });
                        return;
                    }
                    ShareState::Busy(progress) => {
                        section(ui, &th, "Sharing");
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new(progress.lock().unwrap().clone()).color(th.fg));
                        });
                        return;
                    }
                    _ => {}
                }
                section(ui, &th, "Share with the community");
                ui.label(egui::RichText::new("Others can listen to it, vote for it and open it in Sequencer.").color(th.fg_dim).size(12.0));
                ui.add_space(4.0);
                egui::Grid::new("share_fields").num_columns(2).spacing(Vec2::new(10.0, 8.0)).show(ui, |ui| {
                    row_label(ui, &th, "Title");
                    ui.add_sized([340.0, CONTROL_H], egui::TextEdit::singleline(&mut self.share_title).char_limit(80).vertical_align(egui::Align::Center));
                    ui.end_row();
                    row_label(ui, &th, "Your name");
                    ui.add_sized([340.0, CONTROL_H], egui::TextEdit::singleline(&mut self.share_artist).char_limit(60).hint_text("optional").vertical_align(egui::Align::Center));
                    ui.end_row();
                    row_label(ui, &th, "About it");
                    ui.add_sized([340.0, 64.0], egui::TextEdit::multiline(&mut self.share_description).char_limit(500).hint_text("optional"));
                    ui.end_row();
                });
                let used: Vec<String> = {
                    let mut ids: Vec<&str> = self.song.tracks.iter().map(|t| self.samples[t.sample].pack.as_str()).filter(|p| !matches!(*p, "classic" | "user")).collect();
                    ids.sort();
                    ids.dedup();
                    ids.iter().filter_map(|id| packs::CATALOG.iter().find(|e| e.id == *id)).map(|e| e.name.to_owned()).collect()
                };
                if !used.is_empty() {
                    ui.label(egui::RichText::new(format!("Uses {}. Whoever opens it gets them installed.", used.join(", "))).color(th.fg_dim).size(11.0));
                }
                if songs::uses_own_sounds(&self.song, &self.samples) {
                    ui.label(egui::RichText::new("Uses your own recordings: others hear them in the recording, but they can't open them.").color(th.yellow).size(11.0));
                }
                if let ShareState::Failed(e) = &state {
                    ui.label(egui::RichText::new(format!("Sharing failed: {e}")).color(th.red).size(12.0));
                }
                ui.add_space(6.0);
                let ready = !self.share_title.trim().is_empty();
                let button = egui::Button::new(egui::RichText::new("Share").color(th.on(th.accent))).fill(th.accent).min_size(Vec2::new(100.0, CONTROL_H));
                if ui.add_enabled(ready, button).clicked() {
                    go = true;
                }
                ui.label(egui::RichText::new(format!("Goes to {}", community::base_url())).color(th.fg_dim).size(10.0));
            });
        if go {
            self.start_share();
        }
        if !open {
            self.share_open = false;
        }
    }

    fn songs_window(&mut self, ctx: &egui::Context) {
        if !self.songs_open {
            return;
        }
        let th = self.theme.clone();
        let mut open = true;
        let mut action: Option<(&str, String)> = None;
        egui::Window::new("Songs")
            .id(egui::Id::new("songs_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 52.0))
            .frame(egui::Frame::window(&ctx.global_style()).fill(th.bg_dark).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                section(ui, &th, "New song");
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("+  Empty").min_size(Vec2::new(0.0, CONTROL_H))).on_hover_text("[Ctrl+N]").clicked() {
                        action = Some(("new", "Untitled".into()));
                    }
                    if ui.add(egui::Button::new("Demo").min_size(Vec2::new(0.0, CONTROL_H))).clicked() {
                        action = Some(("demo", "Demo".into()));
                    }
                    if ui.add(egui::Button::new("Rave").min_size(Vec2::new(0.0, CONTROL_H))).clicked() {
                        action = Some(("rave", "Rave".into()));
                    }
                    if ui.add(egui::Button::new("Late Night").min_size(Vec2::new(0.0, CONTROL_H))).clicked() {
                        action = Some(("late", "Late Night".into()));
                    }
                });
                ui.add_space(6.0);
                section(ui, &th, "Community");
                if ui.add(egui::Button::new(format!("Share “{}”…", short(&self.song_name, 24))).min_size(Vec2::new(0.0, CONTROL_H))).on_hover_text("put this song on the community site, where others can listen, vote and open it").clicked() {
                    action = Some(("share", String::new()));
                }
                ui.add_space(6.0);
                section(ui, &th, "Your songs");
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for info in songs::list() {
                        let current = info.name == self.song_name;
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                        let p = ui.painter();
                        let fill = if current { th.selection } else if resp.hovered() { th.bg_light } else { th.bg_dark };
                        p.rect_filled(rect, CornerRadius::ZERO, fill);
                        if current {
                            p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())), CornerRadius::ZERO, th.accent);
                        }
                        p.text(rect.left_top() + Vec2::new(12.0, 7.0), Align2::LEFT_TOP, &info.name, FontId::monospace(13.0), th.fg_bright);
                        let meta = format!("{:.0} BPM · {} tracks · {}", info.bpm, info.tracks, songs::ago(info.modified));
                        p.text(rect.left_top() + Vec2::new(12.0, 25.0), Align2::LEFT_TOP, meta, FontId::monospace(11.0), th.fg_dim);

                        // Delete asks once more before it removes the file.
                        let del = Rect::from_center_size(Pos2::new(rect.right() - 40.0, rect.center().y), Vec2::new(64.0, CONTROL_H));
                        let asking = self.confirm_delete.as_deref() == Some(info.name.as_str());
                        if !current && (resp.hovered() || asking) {
                            let dresp = ui.interact(del, ui.id().with(("del", &info.name)), Sense::click());
                            let p = ui.painter();
                            p.rect_filled(del, CornerRadius::ZERO, if asking { th.red } else if dresp.hovered() { th.selection } else { th.bg_light });
                            p.text(del.center(), Align2::CENTER_CENTER, if asking { "Sure?" } else { "Delete" }, FontId::monospace(11.0), if asking { th.on(th.red) } else { th.fg_dim });
                            if dresp.clicked() {
                                action = Some((if asking { "delete" } else { "ask" }, info.name.clone()));
                            }
                        }
                        if resp.clicked() && action.is_none() {
                            action = Some(("open", info.name.clone()));
                        }
                        ui.add_space(3.0);
                    }
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(format!("Songs save themselves as you work, in {}", tilde(&songs::dir()))).color(th.fg_dim).size(11.0));
            });
        match action {
            Some(("new", base)) => {
                self.new_song(&base, Song::blank(&self.samples));
                self.songs_open = false;
            }
            Some(("demo", base)) => {
                self.new_song(&base, Song::demo(&self.samples));
                self.songs_open = false;
            }
            Some(("rave", base)) => {
                self.new_song(&base, Song::rave(&self.samples));
                self.songs_open = false;
            }
            Some(("late", base)) => {
                self.new_song(&base, Song::late_night(&self.samples));
                self.songs_open = false;
            }
            Some(("open", name)) => {
                self.confirm_delete = None;
                self.open_song(&name);
                self.songs_open = false;
            }
            Some(("share", _)) => {
                self.songs_open = false;
                self.open_share();
            }
            Some(("ask", name)) => self.confirm_delete = Some(name),
            Some(("delete", name)) => {
                self.confirm_delete = None;
                match songs::delete(&name) {
                    Ok(()) => self.say(format!("deleted {name}")),
                    Err(e) => self.say(format!("deleting failed: {e}")),
                }
            }
            _ => {}
        }
        if !open {
            self.songs_open = false;
            self.confirm_delete = None;
        }
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        let th = self.theme.clone();
        let exported = self.export_result.lock().unwrap().take();
        if let Some(msg) = exported {
            self.say(msg);
        }
        let audio = match &self.output {
            Output::Device(_, rate) => format!("{:.1} kHz", *rate as f32 / 1000.0),
            Output::Silent => "no audio".into(),
        };
        let msg = if self.recorder.recording() {
            format!("● recording on track {}", self.rec_track.map_or(0, |t| t + 1))
        } else {
            match &self.flash {
                Some((m, at)) if at.elapsed() < Duration::from_secs(4) => m.clone(),
                _ => format!(
                    "Pattern {} · step {}/{} · {:.0} BPM · {audio}",
                    PATTERN_NAMES[self.song.current],
                    self.shared.step.load(Ordering::Relaxed) + 1,
                    self.song.steps(),
                    self.song.bpm
                ),
            }
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(msg).color(if self.recorder.recording() { th.red } else { th.fg }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(
                        "Space play · ←→ steps · ↑↓ BPM · F1-F8 pattern · 1-9 mute · Tab track · V record · right-click accent · scroll a note to resize",
                    )
                    .color(th.fg_dim)
                    .size(11.0),
                );
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.watcher.changed() {
            let t = Theme::load();
            if t != self.theme {
                apply_theme(&ctx, &t);
                self.theme = t;
            }
        }
        self.keys(&ctx);

        // Hold to record until you let go; a short click (or tap of V) keeps it going until the next one.
        if self.recorder.recording() {
            match self.rec_latched {
                None => {
                    let held = ctx.input(|i| i.pointer.primary_down() || i.key_down(egui::Key::V));
                    if !held {
                        let short = self.rec_started.is_some_and(|t| t.elapsed() < HOLD_THRESHOLD);
                        if short { self.rec_latched = Some(Instant::now()) } else { self.stop_recording() }
                    }
                }
                Some(since) => {
                    let again = ctx.input(|i| i.key_pressed(egui::Key::V) || i.key_pressed(egui::Key::Escape));
                    if again && since.elapsed() > Duration::from_millis(150) {
                        self.stop_recording();
                    }
                }
            }
            if self.recorder.recording() && self.recorder.length() >= audio::MAX_RECORDING_SECONDS as f32 {
                self.stop_recording();
            }
        }

        // The engine switched patterns at the end of the bar.
        let active = self.shared.active.load(Ordering::Relaxed);
        if self.playing() && self.song.queued == Some(active) {
            self.song.current = active;
            self.song.queued = None;
            self.pushed.current = active;
            self.pushed.queued = None;
        }

        let th = self.theme.clone();
        egui::Panel::top("toolbar")
            .frame(egui::Frame::new().fill(see_through(th.bg_dark)).inner_margin(egui::Margin::symmetric(12, 10)))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(see_through(th.bg_dark)).inner_margin(egui::Margin::symmetric(12, 5)))
            .show(ui, |ui| self.status(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(see_through(th.bg)).inner_margin(egui::Margin::symmetric(12, 10)))
            .show(ui, |ui| {
                self.view_h = ui.available_height();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.grid(ui);
                    ui.add_space(6.0);
                    if ui.button("+ Track").clicked() {
                        self.add_track();
                    }
                });
            });

        self.effects(&ctx);
        self.recording_panel(&ctx);
        self.fx_window(&ctx);
        self.songs_window(&ctx);
        self.share_window(&ctx);
        self.about_window(&ctx);
        self.finish_installs();
        self.sounds_window(&ctx);
        self.master_window(&ctx);

        let pointer_down = ctx.input(|i| i.pointer.any_down());
        self.history(pointer_down);

        // Push changes to the audio thread.
        if self.song != self.pushed {
            *self.shared.song.lock().unwrap() = self.song.clone();
            self.pushed = self.song.clone();
        }
        // Autosave, at most once every few seconds.
        if self.song != self.saved && self.last_save.elapsed() > Duration::from_secs(3) {
            let _ = songs::write(&self.song_name, &self.song, &self.samples);
            self.saved = self.song.clone();
            self.last_save = Instant::now();
        }
        // Smooth playhead while playing; otherwise only poll a few times a second (meters, theme).
        // Input still triggers an immediate repaint.
        let animating = !self.sparks.is_empty() || !self.pops.is_empty() || !self.ghosts.is_empty() || !self.rings.is_empty() || !self.glints.is_empty();
        let sharing = matches!(*self.share_state.lock().unwrap(), ShareState::Busy(_));
        let busy = animating || sharing || self.playing() || self.about_open.is_some() || self.recorder.recording() || self.fx_loop.is_some() || !self.installing.lock().unwrap().is_empty() || self.meters.iter().any(|m| *m > 0.01);
        ctx.request_repaint_after(Duration::from_millis(if busy { 16 } else { 250 }));
    }

    /// Tab selects tracks; otherwise egui would use it to move focus into a text field.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        raw.events.retain(|e| match e {
            egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } => {
                if *pressed {
                    self.tab = Some(modifiers.shift);
                }
                false
            }
            _ => true,
        });
    }

    /// The window itself is transparent; the panels paint the (see-through) background.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = songs::write(&self.song_name, &self.song, &self.samples);
    }
}

/// How opaque the window background is; Hyprland blurs what shows through.
const BACKGROUND_OPACITY: f32 = 0.97;

fn see_through(c: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (BACKGROUND_OPACITY * 255.0) as u8)
}

fn delay_name(steps: u8) -> String {
    match steps {
        1 => "1/16".into(),
        2 => "1/8".into(),
        3 => "3/16".into(),
        4 => "1/4".into(),
        6 => "3/8".into(),
        8 => "1/2".into(),
        n => format!("{n}/16"),
    }
}

fn label(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).color(th.fg_dim).size(12.0));
}

fn toggle(ui: &mut egui::Ui, th: &Theme, rect: Rect, text: &str, on: bool, color: Color32, id: (&str, usize)) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    let p = ui.painter();
    let fill = if on { color } else if resp.hovered() { th.selection } else { th.bg_light };
    p.rect_filled(rect, CornerRadius::ZERO, fill);
    p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::monospace(12.0), if on { th.on(color) } else { th.fg_dim });
    resp
}

/// Horizontal draggable bar for a value between `min` and `max`, also adjustable by scrolling.
#[allow(clippy::too_many_arguments)]
fn bar_control(ui: &mut egui::Ui, th: &Theme, rect: Rect, value: &mut f32, min: f32, max: f32, color: Color32, id: (&str, usize), tip: &str) {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click_and_drag()).on_hover_text(tip);
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            *value = min + (max - min) * ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        }
    }
    if resp.double_clicked() && min < 0.0 {
        *value = 0.0;
    }
    if resp.hovered() {
        let scroll = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
        *value = (*value + scroll * 0.002 * (max - min)).clamp(min, max);
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, if resp.hovered() { th.selection } else { th.bg_light });
    let inner = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 8.0, 10.0));
    let x = |v: f32| inner.left() + inner.width() * (v - min) / (max - min);
    let from = if min < 0.0 { x(0.0) } else { inner.left() };
    if min < 0.0 {
        p.line_segment([Pos2::new(from, inner.top() - 3.0), Pos2::new(from, inner.bottom() + 3.0)], Stroke::new(1.0, th.fg_dim));
    }
    let to = x(*value);
    p.rect_filled(Rect::from_x_y_ranges(from.min(to)..=from.max(to).max(from.min(to) + 2.0), inner.y_range()), CornerRadius::ZERO, color);
}

fn fonts(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    for (name, style) in [("system", "regular"), ("system-bold", "bold")] {
        if let Some(bytes) = theme::system_font(style) {
            defs.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
        }
    }
    if defs.font_data.contains_key("system") {
        for family in [FontFamily::Monospace, FontFamily::Proportional] {
            defs.families.entry(family).or_default().insert(0, "system".into());
        }
    }
    ctx.set_fonts(defs);
}

fn apply_theme(ctx: &egui::Context, th: &Theme) {
    let th = th.clone();
    ctx.all_styles_mut(move |style| {
        for font in style.text_styles.values_mut() {
            font.family = FontFamily::Monospace;
        }
        let v = &mut style.visuals;
        *v = if th.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = th.bg;
        v.window_fill = th.bg_dark;
        v.faint_bg_color = th.bg_dark;
        v.extreme_bg_color = th.bg_darker;
        v.override_text_color = Some(th.fg);
        v.hyperlink_color = th.accent;
        v.selection.bg_fill = th.accent.gamma_multiply(0.5);
        v.selection.stroke = Stroke::new(1.0, th.fg_bright);
        v.window_stroke = Stroke::new(1.0, th.accent);
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = CornerRadius::ZERO;
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow::NONE;
        let ws = &mut v.widgets;
        for w in [&mut ws.noninteractive, &mut ws.inactive, &mut ws.hovered, &mut ws.active, &mut ws.open] {
            w.corner_radius = CornerRadius::ZERO;
            w.expansion = 0.0;
        }
        ws.noninteractive.bg_stroke = Stroke::new(1.0, th.bg_light);
        ws.noninteractive.fg_stroke = Stroke::new(1.0, th.fg);
        ws.inactive.bg_fill = th.bg_light;
        ws.inactive.weak_bg_fill = th.bg_light;
        ws.inactive.bg_stroke = Stroke::NONE;
        ws.inactive.fg_stroke = Stroke::new(1.0, th.fg);
        ws.hovered.bg_fill = th.selection;
        ws.hovered.weak_bg_fill = th.selection;
        ws.hovered.bg_stroke = Stroke::new(1.0, th.accent);
        ws.hovered.fg_stroke = Stroke::new(1.0, th.fg_bright);
        ws.active.bg_fill = th.accent;
        ws.active.weak_bg_fill = th.accent;
        ws.active.bg_stroke = Stroke::new(1.0, th.accent);
        ws.active.fg_stroke = Stroke::new(1.0, th.on(th.accent));
        ws.open.bg_fill = th.selection;
        ws.open.weak_bg_fill = th.selection;
        ws.open.bg_stroke = Stroke::new(1.0, th.accent);
        style.spacing.item_spacing = Vec2::new(6.0, 2.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.button_padding = Vec2::new(10.0, 4.0);
    });
}

fn section_gap(ui: &mut egui::Ui) {
    ui.add_space(8.0);
}

/// A 0..1 slider that shows its value as a percentage.
fn amount(v: &mut f32) -> egui::Slider<'_> {
    egui::Slider::new(v, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
}

fn section(ui: &mut egui::Ui, th: &Theme, title: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(title).color(th.accent).size(12.0));
}

fn row_label(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.allocate_ui_with_layout(Vec2::new(84.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(84.0);
        ui.label(egui::RichText::new(text).color(th.fg_dim).size(12.0));
    });
}

/// Which list of the sound browser a sample belongs to.
fn source_of(s: &Sample) -> String {
    match s.pack.as_str() {
        "classic" => match s.kind {
            Kind::Riff => "classic:riff",
            Kind::Rave => "classic:rave",
            Kind::Vox => "classic:vox",
            _ => "classic:drum",
        }
        .into(),
        pack => pack.into(),
    }
}

fn source_name(pack: &str, installed: &[(packs::PackInfo, std::path::PathBuf)]) -> String {
    match pack {
        "classic" => "Classic".into(),
        "user" => "Yours".into(),
        id => installed.iter().find(|(i, _)| i.id == id).map_or(id.to_owned(), |(i, _)| i.name.clone()),
    }
}

/// A clickable row in the left column of a dialog.
fn list_row(ui: &mut egui::Ui, th: &Theme, text: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, if on { th.accent } else if resp.hovered() { th.selection } else { th.bg_dark });
    p.text(rect.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, text, FontId::monospace(12.0), if on { th.on(th.accent) } else { th.fg });
    resp
}

fn list_row_count(ui: &mut egui::Ui, th: &Theme, text: &str, count: usize, on: bool) -> egui::Response {
    let resp = list_row(ui, th, text, on);
    let color = if on { th.on(th.accent) } else { th.fg_dim };
    ui.painter().text(resp.rect.right_center() - Vec2::new(10.0, 0.0), Align2::RIGHT_CENTER, count.to_string(), FontId::monospace(10.0), color);
    resp
}

fn sound_row(ui: &mut egui::Ui, th: &Theme, name: &str, pack: Option<&str>, color: Color32, current: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, if current { th.selection } else if resp.hovered() { th.bg_light } else { th.bg_dark });
    p.rect_filled(Rect::from_min_size(rect.left_top() + Vec2::new(0.0, 7.0), Vec2::new(3.0, 12.0)), CornerRadius::ZERO, color);
    p.text(rect.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, name, FontId::monospace(12.0), if current { th.fg_bright } else { th.fg });
    if let Some(pack) = pack {
        p.text(rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, pack, FontId::monospace(10.0), th.fg_dim);
    } else if resp.hovered() {
        p.text(rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, "use", FontId::monospace(10.0), th.fg_dim);
    }
    resp
}

enum PackAction {
    Install,
    Open,
    AskRemove,
    Remove,
}

struct PackCard<'a> {
    entry: &'a packs::CatalogEntry,
    tile: Color32,
    installed: bool,
    sounds: usize,
    progress: Option<String>,
    asking: bool,
}

/// One pack in the store: a coloured monogram, what it is, who made it, and Get / Open.
fn pack_card(ui: &mut egui::Ui, th: &Theme, card: PackCard, time: f32) -> Option<PackAction> {
    let entry = card.entry;
    let text_x = 62.0;
    let wrap = ui.available_width() - text_x - 104.0;
    let meta = ui.painter().layout(format!("{} · {}", entry.author, entry.license), FontId::monospace(10.0), th.fg_dim, wrap);
    let about = ui.painter().layout(entry.about.to_owned(), FontId::monospace(11.0), th.fg, wrap);
    let height = (12.0 + 18.0 + meta.size().y + 6.0 + about.size().y + 12.0).max(92.0);
    let (rect, hover) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, if hover.hovered() { th.selection } else { th.bg_light });
    // Monogram tile.
    let tile = Rect::from_min_size(rect.left_top() + Vec2::new(12.0, 12.0), Vec2::splat(38.0));
    p.rect_filled(tile, CornerRadius::ZERO, card.tile);
    let letters: String = entry.name.split(' ').filter_map(|w| w.chars().next()).take(2).collect();
    p.text(tile.center(), Align2::CENTER_CENTER, letters, FontId::monospace(14.0), th.on(card.tile));
    // Name, maker and licence, contents.
    p.text(rect.left_top() + Vec2::new(text_x, 11.0), Align2::LEFT_TOP, entry.name, FontId::monospace(14.0), th.fg_bright);
    let meta_y = 11.0 + 18.0;
    let about_y = meta_y + meta.size().y + 6.0;
    p.galley(rect.left_top() + Vec2::new(text_x, meta_y), meta, th.fg_dim);
    p.galley(rect.left_top() + Vec2::new(text_x, about_y), about, th.fg);

    let button = Rect::from_min_size(Pos2::new(rect.right() - 96.0, rect.top() + 12.0), Vec2::new(84.0, CONTROL_H));
    let mut action = None;
    if let Some(progress) = card.progress {
        // A bar along the bottom of the card: a fraction when we know it, a sweep while we don't.
        let bar = Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 3.0), Vec2::new(rect.width(), 3.0));
        p.rect_filled(bar, CornerRadius::ZERO, th.bg_dark);
        let fraction = progress.split_once('/').and_then(|(a, b)| Some(a.trim().parse::<f32>().ok()? / b.split_whitespace().next()?.parse::<f32>().ok()?));
        let fill = match fraction {
            Some(f) => Rect::from_min_size(bar.min, Vec2::new(bar.width() * f.clamp(0.0, 1.0), bar.height())),
            None => {
                let x = bar.left() + (time * 0.8).fract() * (bar.width() + 80.0) - 80.0;
                Rect::from_x_y_ranges(x.max(bar.left())..=(x + 80.0).min(bar.right()), bar.y_range())
            }
        };
        p.rect_filled(fill, CornerRadius::ZERO, card.tile);
        // Clipped to the button, in case a status line is longer than it.
        p.with_clip_rect(button).text(button.center(), Align2::CENTER_CENTER, progress, FontId::monospace(10.0), th.fg);
    } else if card.installed {
        let resp = ui.interact(button, ui.id().with(("open", entry.id)), Sense::click());
        let p = ui.painter();
        p.rect_filled(button, CornerRadius::ZERO, if resp.hovered() { th.accent } else { th.bg_dark });
        p.text(button.center(), Align2::CENTER_CENTER, "Open", FontId::monospace(12.0), if resp.hovered() { th.on(th.accent) } else { th.fg_bright });
        if resp.clicked() {
            action = Some(PackAction::Open);
        }
        p.text(Pos2::new(button.center().x, button.bottom() + 12.0), Align2::CENTER_CENTER, format!("✓ {} sounds", card.sounds), FontId::monospace(10.0), th.green);
        let remove = Rect::from_center_size(Pos2::new(button.center().x, button.bottom() + 30.0), Vec2::new(84.0, 16.0));
        let rresp = ui.interact(remove, ui.id().with(("remove", entry.id)), Sense::click());
        let (label, color) = if card.asking { ("sure? remove", th.red) } else { ("remove", if rresp.hovered() { th.red } else { th.fg_dim }) };
        ui.painter().text(remove.center(), Align2::CENTER_CENTER, label, FontId::monospace(10.0), color);
        if rresp.clicked() {
            action = Some(if card.asking { PackAction::Remove } else { PackAction::AskRemove });
        }
    } else {
        let resp = ui.interact(button, ui.id().with(("get", entry.id)), Sense::click());
        let fill = if resp.hovered() { th.fg_bright } else { th.accent };
        let p = ui.painter();
        p.rect_filled(button, CornerRadius::ZERO, fill);
        p.text(button.center(), Align2::CENTER_CENTER, "Get", FontId::monospace(12.0), th.on(fill));
        p.text(Pos2::new(button.center().x, button.bottom() + 12.0), Align2::CENTER_CENTER, entry.size, FontId::monospace(10.0), th.fg_dim);
        if resp.clicked() {
            action = Some(PackAction::Install);
        }
    }
    ui.add_space(4.0);
    action
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Cuts a name to `max` characters with an ellipsis.
fn short(name: &str, max: usize) -> String {
    if name.chars().count() <= max { name.to_owned() } else { format!("{}…", name.chars().take(max - 1).collect::<String>()) }
}

/// A path with the home folder written as `~`.
fn tilde(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    match dirs::home_dir() {
        Some(home) => text.replacen(&home.display().to_string(), "~", 1),
        None => text,
    }
}
