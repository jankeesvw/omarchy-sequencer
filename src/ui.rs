use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::audio::{self, Output, Recorder, Shared};
use crate::pattern::{Cell, FilterKind, Fx, MAX_STEPS, MAX_TRACKS, PATTERNS, Song, Track};
use crate::samples::{self, Kind, Sample};
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
/// Size of a grid cell; the rows are as tall.
const CELL: f32 = 30.0;
/// How long the trigger animation of a cell lasts.
const FLASH: Duration = Duration::from_millis(350);
const PATTERN_NAMES: [&str; PATTERNS] = ["A", "B", "C", "D", "E", "F", "G", "H"];

pub struct App {
    samples: Vec<Arc<Sample>>,
    /// Name of the open song (its file in ~/Music/Sequencer), and the name field while you type.
    song_name: String,
    name_edit: String,
    songs_open: bool,
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
    export_result: Arc<Mutex<Option<String>>>,
    tab: Option<bool>,
    /// Trigger animations: track, step, and when it fired.
    flashes: Vec<(usize, usize, Instant)>,
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
            export_result: Arc::new(Mutex::new(None)),
            tab: None,
            flashes: Vec::new(),
            last_step: None,
            fx_open: None,
            fx_loop: None,
            fx_gap: 1.0,
        }
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
                self.selected = track;
            }
            Err(e) => self.say(format!("cannot record: {e}")),
        }
    }

    fn stop_recording(&mut self) {
        let Some(track) = self.rec_track.take() else { return };
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
                self.say(format!("recorded {name} (saved in ~/.local/share/sequencer/samples)"));
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
        let v_down = !wants_text && ctx.input(|i| i.key_down(egui::Key::V) && !i.modifiers.ctrl);
        if v_down && !self.recorder.recording() {
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
            if ui.add(button("Clear")).on_hover_text("clear this pattern [C] · undo with Ctrl+Z").clicked() {
                self.clear_pattern();
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
                if ui.add(button("Export")).on_hover_text("the current pattern 4 times to a WAV in ~/Music [Ctrl+E]").clicked() {
                    self.export();
                }
                if ui.add(button("Songs")).on_hover_text("open, create and delete songs [Ctrl+O]").clicked() {
                    self.songs_open = !self.songs_open;
                }
                // The song name opens the settings of the whole song.
                let settings_open = self.settings_open;
                let song_button = egui::Button::new(egui::RichText::new(format!("♪  {}", self.song_name)).color(if settings_open { th.on(th.accent) } else { th.fg_bright }))
                    .fill(if settings_open { th.accent } else { th.bg_light })
                    .min_size(Vec2::new(CONTROL_H, CONTROL_H));
                if ui.add(song_button).on_hover_text("song settings: name, tempo, swing and volume").clicked() {
                    self.settings_open = !self.settings_open;
                }
                section_gap(ui);
                if ui.add_enabled(!self.redo.is_empty(), button("↷")).on_hover_text("redo [Ctrl+Shift+Z]").clicked() {
                    self.undo(true);
                }
                if ui.add_enabled(!self.undo.is_empty(), button("↶")).on_hover_text("undo [Ctrl+Z]").clicked() {
                    self.undo(false);
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
        if let (Some(paint), Some(pos)) = (self.paint, pointer.hover_pos()) {
            if cells.contains(pos) && pointer.primary_down() {
                let track = &mut self.song.tracks[ti];
                let len = track.note_len;
                let lane = &mut track.lanes[cur];
                if paint == Cell::Off {
                    lane.erase(hit(pos));
                } else {
                    lane.place(hit(pos), Cell::On, len, steps);
                }
            }
        }
        if resp.secondary_clicked() {
            if let Some(pos) = pointer.interact_pos() {
                let track = &mut self.song.tracks[ti];
                let len = track.note_len;
                let lane = &mut track.lanes[cur];
                match lane.note_at(hit(pos)) {
                    Some(n) => {
                        let c = &mut lane.cells[n];
                        *c = if *c == Cell::Accent { Cell::On } else { Cell::Accent };
                    }
                    None => lane.place(hit(pos), Cell::Accent, len, steps),
                }
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
        for n in 0..steps {
            let accent = match lane.cells[n] {
                Cell::Off => continue,
                Cell::On => false,
                Cell::Accent => true,
            };
            let len = (lane.lens[n] as usize).min(steps - n);
            let bar = Rect::from_min_size(Pos2::new(cells.left() + cell_w * n as f32, cells.top()), Vec2::new(cell_w * len as f32, cells.height())).shrink(CELL_INSET);
            let sounding = playhead.is_some_and(|h| h >= n && h < n + len) && !dead;
            let mut c = if accent { color } else { th.soften(color) };
            if dead {
                c = c.gamma_multiply(0.3);
            }
            p.rect_filled(bar, CornerRadius::ZERO, c);
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

        let combo_rect = next(150.0);
        let mut chosen = None;
        let current = self.song.tracks[ti].sample;
        let mut combo_ui = ui.new_child(egui::UiBuilder::new().max_rect(combo_rect).id_salt(("combo", ti)));
        combo_ui.spacing_mut().interact_size.y = h;
        egui::ComboBox::from_id_salt(("sample", ti))
            .width(COLUMNS[1].1)
            .height(480.0)
            .selected_text(egui::RichText::new(samples[current].name.replace('_', " ")).color(th.fg_bright))
            .show_ui(&mut combo_ui, |ui| {
                for kind in [Kind::Drum, Kind::Riff, Kind::Rave, Kind::Vox, Kind::User] {
                    let mut first = true;
                    for (i, smp) in samples.iter().enumerate().filter(|(_, x)| x.kind == kind) {
                        if first {
                            let title = match kind {
                                Kind::Drum => "TR-808 drums",
                                Kind::Riff => "Riffs",
                                Kind::Rave => "90s rave",
                                Kind::Vox => "Vocals",
                                Kind::User => "Your samples and recordings",
                            };
                            ui.label(egui::RichText::new(title).color(self.kind_color(kind)).size(11.0));
                            first = false;
                        }
                        if ui.selectable_label(current == i, smp.name.replace('_', " ")).clicked() {
                            chosen = Some(i);
                        }
                    }
                }
            });
        if let Some(i) = chosen {
            let t = &mut self.song.tracks[ti];
            t.sample = i;
            t.note_len = samples[i].default_len();
            self.shared.preview.lock().unwrap().push((i, t.volume, Some(ti)));
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
        let rec_resp = ui.interact(rec, ui.id().with(("rec", ti)), Sense::click_and_drag()).on_hover_text("hold to record from your microphone [V]");
        let recording_here = self.rec_track == Some(ti);
        if rec_resp.is_pointer_button_down_on() && !self.recorder.recording() {
            self.start_recording(ti);
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
                });
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
                ui.label(egui::RichText::new(format!("Songs save themselves as you work, in {}", songs::dir().display())).color(th.fg_dim).size(11.0));
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
            Some(("open", name)) => {
                self.confirm_delete = None;
                self.open_song(&name);
                self.songs_open = false;
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
            format!("● recording on track {}…  release to stop", self.rec_track.map_or(0, |t| t + 1))
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

        // Push-to-talk: releasing the button or V stops the recording.
        if self.recorder.recording() {
            let held = ctx.input(|i| i.pointer.primary_down() || i.key_down(egui::Key::V));
            if !held {
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

        self.fx_window(&ctx);
        self.songs_window(&ctx);
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
        let busy = self.playing() || self.recorder.recording() || self.fx_loop.is_some() || self.meters.iter().any(|m| *m > 0.01);
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
    ui.add_space(18.0);
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
