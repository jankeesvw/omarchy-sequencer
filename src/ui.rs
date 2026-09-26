use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::audio::{self, Output, Recorder, Shared};
use crate::pattern::{self, Cell, MAX_STEPS, MAX_TRACKS, PATTERNS, Song, Track};
use crate::samples::{self, Kind, Sample};
use crate::theme::{self, Theme};

const LEFT_W: f32 = 540.0;
const PATTERN_NAMES: [&str; PATTERNS] = ["A", "B", "C", "D", "E", "F", "G", "H"];

pub struct App {
    samples: Vec<Arc<Sample>>,
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
}

impl App {
    pub fn new(cc: &eframe::CreationContext, samples: Vec<Arc<Sample>>, shared: Arc<Shared>, song: Song, output: Output) -> Self {
        let theme = Theme::load();
        fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, &theme);
        Self {
            samples,
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
            self.say(format!("patroon {} gekopieerd naar {}", PATTERN_NAMES[from], PATTERN_NAMES[p]));
            return;
        }
        if self.playing() && p != self.song.current {
            // Tijdens het spelen wisselt het patroon op de maatgrens.
            self.song.queued = Some(p);
        } else {
            self.song.current = p;
            self.song.queued = None;
        }
    }

    fn add_track(&mut self) {
        if self.song.tracks.len() >= MAX_TRACKS {
            self.say("maximaal 16 tracks");
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
        self.say("willekeurig patroon gemaakt");
    }

    fn clear_pattern(&mut self) {
        let cur = self.song.current;
        for t in &mut self.song.tracks {
            t.lanes[cur].clear();
        }
        self.say(format!("patroon {} gewist", PATTERN_NAMES[cur]));
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
        match pattern::save(&self.song, &self.samples) {
            Ok(()) => {
                self.saved = self.song.clone();
                self.say("opgeslagen in ~/.config/sequencer/song.json");
            }
            Err(e) => self.say(format!("opslaan mislukt: {e}")),
        }
        self.last_save = Instant::now();
    }

    fn export(&mut self) {
        let samples = self.samples.clone();
        let song = self.song.clone();
        let result = self.export_result.clone();
        self.say("exporteren…");
        std::thread::spawn(move || {
            let msg = match audio::export(samples, &song, 4) {
                Ok(path) => format!("geëxporteerd naar {}", path.display()),
                Err(e) => format!("export mislukt: {e}"),
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
            Err(e) => self.say(format!("opnemen lukt niet: {e}")),
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
                self.shared.preview.lock().unwrap().push((idx, 0.8));
                self.say(format!("opgenomen: {name} (staat in ~/.local/share/sequencer/samples)"));
            }
            Err(e) => self.say(format!("opname niet bewaard: {e}")),
        }
    }

    fn history(&mut self, pointer_down: bool) {
        // Een wijziging telt als één stap zodra je de muis loslaat; wisselen van patroon hoort er niet bij.
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
            self.say(if redo { "opnieuw" } else { "ongedaan gemaakt" });
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
        // Tab / Shift+Tab kiest de track waar V naartoe opneemt (onderschept in raw_input_hook).
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
        // 1-9 zet track 1 t/m 9 op mute, F1-F8 kiest patroon A-H (met shift: kopieer ernaartoe).
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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let playing = self.playing();
            let play = egui::Button::new(egui::RichText::new(if playing { "■  Stop" } else { "▶  Play" }).color(th.bg).strong())
                .fill(if playing { th.red } else { th.accent })
                .min_size(Vec2::new(84.0, 26.0));
            if ui.add(play).clicked() {
                self.toggle_play();
            }
            ui.add_space(10.0);
            label(ui, &th, "BPM");
            ui.add(egui::DragValue::new(&mut self.song.bpm).range(40.0..=300.0).speed(0.5).fixed_decimals(0));
            if ui.button("Tap").on_hover_text("tik het tempo in [T]").clicked() {
                self.tap();
            }
            label(ui, &th, "Swing");
            let mut swing = (self.song.swing * 200.0).round();
            if ui.add(egui::DragValue::new(&mut swing).range(0.0..=100.0).speed(0.5).fixed_decimals(0).suffix("%")).changed() {
                self.song.swing = swing / 200.0;
            }

            ui.add_space(14.0);
            label(ui, &th, "Pattern");
            let active = self.shared.active.load(Ordering::Relaxed);
            let blink = (self.started.elapsed().as_secs_f32() * 4.0).fract() < 0.5;
            for p in 0..PATTERNS {
                let used = self.song.tracks.iter().any(|t| t.lanes[p].cells.iter().any(|c| *c != Cell::Off));
                let current = self.song.current == p;
                let queued = self.song.queued == Some(p);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::click());
                let painter = ui.painter();
                let fill = if current { th.accent } else if resp.hovered() { th.selection } else { th.bg_light };
                painter.rect_filled(rect, CornerRadius::ZERO, fill);
                if queued && blink {
                    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(2.0, th.accent), StrokeKind::Inside);
                }
                let text_color = if current { th.bg } else if used { th.fg_bright } else { th.fg_dim };
                painter.text(rect.center(), Align2::CENTER_CENTER, PATTERN_NAMES[p], FontId::monospace(13.0), text_color);
                if used && !current {
                    painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() - 4.0), 1.5, th.accent);
                }
                if self.playing() && active == p && !current {
                    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, th.fg_dim), StrokeKind::Inside);
                }
                let resp = resp.on_hover_text("klik: kies patroon [F1-F8] · shift-klik of rechtsklik: kopieer het huidige hierheen");
                if resp.clicked() {
                    let shift = ui.input(|i| i.modifiers.shift);
                    self.select_pattern(p, shift);
                }
                if resp.secondary_clicked() {
                    self.select_pattern(p, true);
                }
            }

            ui.add_space(14.0);
            label(ui, &th, "Steps");
            if ui.small_button("−").clicked() {
                self.set_steps(self.song.steps().saturating_sub(1));
            }
            let mut steps = self.song.steps();
            if ui.add(egui::DragValue::new(&mut steps).range(1..=MAX_STEPS).speed(0.2)).changed() {
                self.set_steps(steps);
            }
            if ui.small_button("+").clicked() {
                self.set_steps(self.song.steps() + 1);
            }
            for n in [8, 16, 32, 64] {
                if ui.selectable_label(self.song.steps() == n, n.to_string()).clicked() {
                    self.set_steps(n);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.scope(ui));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().slider_width = 110.0;
            label(ui, &th, "Filter");
            ui.add(egui::Slider::new(&mut self.song.cutoff, 0.0..=1.0).show_value(false)).on_hover_text("master lowpass");
            label(ui, &th, "Delay");
            egui::ComboBox::from_id_salt("delay")
                .width(64.0)
                .selected_text(delay_name(self.song.delay_steps))
                .show_ui(ui, |ui| {
                    for d in [1, 2, 3, 4, 6, 8] {
                        ui.selectable_value(&mut self.song.delay_steps, d, delay_name(d));
                    }
                });
            label(ui, &th, "Feedback");
            ui.add(egui::Slider::new(&mut self.song.feedback, 0.0..=0.85).show_value(false));
            label(ui, &th, "Volume");
            ui.add(egui::Slider::new(&mut self.song.master, 0.0..=1.0).show_value(false));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Export WAV").on_hover_text("4 keer het huidige patroon naar ~/Music [Ctrl+E]").clicked() {
                    self.export();
                }
                if ui.button("Save").on_hover_text("[Ctrl+S], er wordt ook automatisch bewaard").clicked() {
                    self.save_now();
                }
                ui.menu_button("Presets", |ui| {
                    if ui.button("Demo, 124 BPM").clicked() {
                        self.song = Song::demo(&self.samples);
                        ui.close();
                    }
                    if ui.button("Rave, 135 BPM").clicked() {
                        self.song = Song::rave(&self.samples);
                        ui.close();
                    }
                });
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo")).on_hover_text("[Ctrl+Shift+Z]").clicked() {
                    self.undo(true);
                }
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).on_hover_text("[Ctrl+Z]").clicked() {
                    self.undo(false);
                }
                if ui.button("Clear").on_hover_text("wis het huidige patroon [C]").clicked() {
                    self.clear_pattern();
                }
                if ui.button("Random").on_hover_text("[R]").clicked() {
                    self.randomize();
                }
                if ui.button("+ Track").on_hover_text("[N]").clicked() {
                    self.add_track();
                }
            });
        });
    }

    fn scope(&self, ui: &mut egui::Ui) {
        let th = &self.theme;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(180.0, 26.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::ZERO, th.bg_darker);
        let data = self.shared.scope.lock().unwrap().clone();
        let pts: Vec<Pos2> = data
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Pos2::new(
                    rect.left() + rect.width() * i as f32 / (data.len() - 1) as f32,
                    rect.center().y - (s * 2.5).clamp(-1.0, 1.0) * rect.height() * 0.45,
                )
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.0, th.accent)));
    }

    fn grid(&mut self, ui: &mut egui::Ui) {
        let th = self.theme.clone();
        let steps = self.song.steps();
        let cur = self.song.current;
        let avail = ui.available_width() - LEFT_W - 12.0;
        let cell_w = (avail / steps as f32).floor().clamp(14.0, 96.0);
        let rows = self.song.tracks.len() as f32;
        let row_h = ((self.view_h - 70.0) / rows - ui.spacing().item_spacing.y).floor().clamp(30.0, 56.0).min(cell_w.max(30.0) + 4.0);
        let width = LEFT_W + cell_w * steps as f32 + 8.0;
        let playing = self.playing();
        let active = self.shared.active.load(Ordering::Relaxed);
        let playhead = if playing && active == cur { Some(self.shared.step.load(Ordering::Relaxed)) } else { None };

        // Stapnummers.
        let (num_rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
        let p = ui.painter_at(num_rect);
        for s in 0..steps {
            let x = num_rect.left() + LEFT_W + cell_w * (s as f32 + 0.5);
            let color = if playhead == Some(s) { th.fg_bright } else if s % 4 == 0 { th.fg } else { th.fg_dim };
            if cell_w >= 20.0 || s % 4 == 0 {
                p.text(Pos2::new(x, num_rect.center().y), Align2::CENTER_CENTER, format!("{}", s + 1), FontId::monospace(10.0), color);
            }
        }

        let mut remove = None;
        let mut move_up = None;
        for ti in 0..self.song.tracks.len() {
            let (row, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
            if ti == self.selected {
                ui.painter().rect_filled(Rect::from_min_size(row.min, Vec2::new(LEFT_W - 8.0, row_h - 4.0)), CornerRadius::ZERO, th.selection);
            }
            self.track_controls(ui, ti, Rect::from_min_size(row.min, Vec2::new(LEFT_W - 8.0, row_h - 4.0)), &mut remove, &mut move_up);

            let cells = Rect::from_min_size(row.min + Vec2::new(LEFT_W, 0.0), Vec2::new(cell_w * steps as f32, row_h - 4.0));
            let resp = ui.interact(cells, ui.id().with(("cells", ti)), Sense::click_and_drag());
            let hit = |pos: Pos2| (((pos.x - cells.left()) / cell_w).floor().max(0.0) as usize).min(steps - 1);

            // Klikken of slepen tekent noten (zo lang als de L van de track), klikken op een noot wist hem,
            // rechtsklik zet een accent en scrollen boven een noot maakt hem langer of korter.
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
                let r = Rect::from_min_size(Pos2::new(cells.left() + cell_w * s as f32, cells.top()), Vec2::new(cell_w, cells.height())).shrink(1.5);
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
                let bar = Rect::from_min_size(Pos2::new(cells.left() + cell_w * n as f32, cells.top()), Vec2::new(cell_w * len as f32, cells.height())).shrink(1.5);
                let sounding = playhead.is_some_and(|h| h >= n && h < n + len) && !dead;
                let mut c = if accent { color } else { color.gamma_multiply(0.7) };
                if dead {
                    c = c.gamma_multiply(0.3);
                }
                p.rect_filled(bar, CornerRadius::ZERO, if sounding { th.fg_bright } else { c });
                // Voegen tussen de blokjes van een lange noot.
                for b in 1..len {
                    let x = bar.left() + cell_w * b as f32 - 1.5;
                    p.line_segment([Pos2::new(x, bar.top() + 4.0), Pos2::new(x, bar.bottom() - 4.0)], Stroke::new(1.0, th.bg.gamma_multiply(0.6)));
                }
                if accent {
                    p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width(), 3.0)), CornerRadius::ZERO, th.fg_bright);
                }
                if len > 1 && cell_w >= 18.0 {
                    p.text(bar.left_top() + Vec2::new(4.0, 5.0), Align2::LEFT_TOP, format!("{len}"), FontId::monospace(10.0), th.bg);
                }
            }
        }

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

    fn track_controls(&mut self, ui: &mut egui::Ui, ti: usize, rect: Rect, remove: &mut Option<usize>, move_up: &mut Option<usize>) {
        let th = self.theme.clone();
        let samples = self.samples.clone();
        let kind_color = self.kind_color(samples[self.song.tracks[ti].sample].kind);
        let cy = rect.center().y;
        let mut x = rect.left() + 8.0;
        let mut next = |w: f32| {
            let r = Rect::from_min_size(Pos2::new(x, cy - 11.0), Vec2::new(w, 22.0));
            x += w + 6.0;
            r
        };

        // Kleurstrook met level-meter.
        let strip = Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height()));
        let lvl = self.shared.level(ti).max(self.meters[ti]);
        self.meters[ti] = lvl * 0.86;
        self.shared.levels[ti].store(0f32.to_bits(), Ordering::Relaxed);
        let p = ui.painter();
        p.rect_filled(strip, CornerRadius::ZERO, kind_color.gamma_multiply(0.35 + 0.65 * lvl.min(1.0)));

        let num = next(20.0);
        let num_resp = ui.interact(num, ui.id().with(("num", ti)), Sense::click()).on_hover_text("klik: selecteer en luister · rechtsklik: omhoog");
        ui.painter().text(num.center(), Align2::CENTER_CENTER, format!("{}", ti + 1), FontId::monospace(12.0), if num_resp.hovered() { th.fg_bright } else { th.fg_dim });
        if num_resp.clicked() {
            self.selected = ti;
            let t = &self.song.tracks[ti];
            self.shared.preview.lock().unwrap().push((t.sample, t.volume));
        }
        if num_resp.secondary_clicked() {
            *move_up = Some(ti);
        }

        let combo_rect = next(150.0);
        let mut chosen = None;
        let current = self.song.tracks[ti].sample;
        let mut combo_ui = ui.new_child(egui::UiBuilder::new().max_rect(combo_rect).id_salt(("combo", ti)));
        egui::ComboBox::from_id_salt(("sample", ti))
            .width(146.0)
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
                                Kind::User => "Eigen samples en opnames",
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
            self.shared.preview.lock().unwrap().push((i, t.volume));
        }

        // Mute, solo en opnemen.
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
        let rec_resp = ui.interact(rec, ui.id().with(("rec", ti)), Sense::click_and_drag()).on_hover_text("ingedrukt houden om op te nemen van je microfoon [V]");
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
        // Volume, pan en delay-send als sleepbare balkjes.
        let vol = next(52.0);
        bar_control(ui, &th, vol, &mut t.volume, 0.0, 1.0, th.accent, ("vol", ti), "volume");
        let pan = next(40.0);
        bar_control(ui, &th, pan, &mut t.pan, -1.0, 1.0, th.cyan, ("pan", ti), "pan (dubbelklik = midden)");
        let send = next(40.0);
        bar_control(ui, &th, send, &mut t.send, 0.0, 1.0, th.magenta, ("send", ti), "delay send");

        // Toonhoogte in halve tonen: verticaal slepen, dubbelklik = 0.
        let pitch = next(34.0);
        let presp = ui.interact(pitch, ui.id().with(("pitch", ti)), Sense::click_and_drag()).on_hover_text("pitch in halve tonen (slepen · dubbelklik = 0)");
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

        // Lengte van nieuwe noten in blokjes: klik = langer, rechtsklik = korter.
        let len_rect = next(30.0);
        let lresp = ui.interact(len_rect, ui.id().with(("len", ti)), Sense::click()).on_hover_text("lengte van nieuwe noten in blokjes (klik / rechtsklik)");
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
        let xr = ui.interact(x_rect, ui.id().with(("x", ti)), Sense::click()).on_hover_text("track verwijderen");
        ui.painter().text(x_rect.center(), Align2::CENTER_CENTER, "×", FontId::monospace(15.0), if xr.hovered() { th.red } else { th.fg_dim });
        if xr.clicked() {
            *remove = Some(ti);
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
            Output::Silent => "geen audio".into(),
        };
        let msg = if self.recorder.recording() {
            format!("● opnemen op track {}…  laat los om te stoppen", self.rec_track.map_or(0, |t| t + 1))
        } else {
            match &self.flash {
                Some((m, at)) if at.elapsed() < Duration::from_secs(4) => m.clone(),
                _ => String::new(),
            }
        };
        let step = self.shared.step.load(Ordering::Relaxed) + 1;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(msg).color(if self.recorder.recording() { th.red } else { th.fg }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} · stap {step}/{} · {:.0} BPM · {audio}   ·   Space play · ←→ steps · ↑↓ BPM · F1-F8 patroon · 1-9 mute · Tab track · V opnemen · R random · Ctrl+Z undo · rechtsklik accent · scroll op noot = lengte",
                        PATTERN_NAMES[self.song.current],
                        self.song.steps(),
                        self.song.bpm
                    ))
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

        // Push-to-talk: loslaten van knop of V stopt de opname.
        if self.recorder.recording() {
            let held = ctx.input(|i| i.pointer.primary_down() || i.key_down(egui::Key::V));
            if !held {
                self.stop_recording();
            }
        }

        // Het engine-patroon is gewisseld op de maatgrens.
        let active = self.shared.active.load(Ordering::Relaxed);
        if self.playing() && self.song.queued == Some(active) {
            self.song.current = active;
            self.song.queued = None;
            self.pushed.current = active;
            self.pushed.queued = None;
        }

        let th = self.theme.clone();
        egui::Panel::top("toolbar")
            .frame(egui::Frame::new().fill(th.bg_dark).inner_margin(egui::Margin::symmetric(12, 10)))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(th.bg_dark).inner_margin(egui::Margin::symmetric(12, 5)))
            .show(ui, |ui| self.status(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(th.bg).inner_margin(egui::Margin::symmetric(12, 10)))
            .show(ui, |ui| {
                self.view_h = ui.available_height();
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    self.grid(ui);
                    ui.add_space(6.0);
                    if ui.button("+ Track").clicked() {
                        self.add_track();
                    }
                });
            });

        let pointer_down = ctx.input(|i| i.pointer.any_down());
        self.history(pointer_down);

        // Wijzigingen naar de audio-thread.
        if self.song != self.pushed {
            *self.shared.song.lock().unwrap() = self.song.clone();
            self.pushed = self.song.clone();
        }
        // Autosave, maximaal eens per paar seconden.
        if self.song != self.saved && self.last_save.elapsed() > Duration::from_secs(3) {
            let _ = pattern::save(&self.song, &self.samples);
            self.saved = self.song.clone();
            self.last_save = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /// Tab gebruiken we voor trackselectie; egui zou er anders de focus mee naar een invoerveld verplaatsen.
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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = pattern::save(&self.song, &self.samples);
    }
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
    p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::monospace(12.0), if on { th.bg } else { th.fg_dim });
    resp
}

/// Horizontaal sleepbaar balkje voor een waarde tussen `min` en `max`, ook met scrollen.
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
    let inner = rect.shrink2(Vec2::new(0.0, 7.0));
    let x = |v: f32| inner.left() + inner.width() * (v - min) / (max - min);
    let from = if min < 0.0 { x(0.0) } else { inner.left() };
    if min < 0.0 {
        p.line_segment([Pos2::new(from, rect.top() + 4.0), Pos2::new(from, rect.bottom() - 4.0)], Stroke::new(1.0, th.fg_dim));
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
        ws.active.fg_stroke = Stroke::new(1.0, th.bg);
        ws.open.bg_fill = th.selection;
        ws.open.weak_bg_fill = th.selection;
        ws.open.bg_stroke = Stroke::new(1.0, th.accent);
        style.spacing.item_spacing = Vec2::new(6.0, 2.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.button_padding = Vec2::new(10.0, 4.0);
    });
}
