use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

use crate::audio::{Output, Shared};
use crate::pattern::{self, Cell, MAX_STEPS, MAX_TRACKS, Pattern, Track};
use crate::samples::{Kind, Sample};

const BG: Color32 = Color32::from_rgb(6, 4, 14);
const PANEL: Color32 = Color32::from_rgb(14, 10, 28);
const DIM: Color32 = Color32::from_rgb(40, 28, 72);
const CYAN: Color32 = Color32::from_rgb(0, 240, 255);
const MAGENTA: Color32 = Color32::from_rgb(255, 43, 214);
const GREEN: Color32 = Color32::from_rgb(57, 255, 20);
const YELLOW: Color32 = Color32::from_rgb(255, 242, 0);
const TEXT: Color32 = Color32::from_rgb(200, 190, 255);

const LEFT_W: f32 = 400.0;

pub struct App {
    samples: Arc<Vec<Sample>>,
    shared: Arc<Shared>,
    pat: Pattern,
    pushed: Pattern,
    output: Output,
    paint: Option<Cell>,
    wheel: f32,
    meters: [f32; MAX_TRACKS],
    last_save: Instant,
    saved_pat: Pattern,
    started: Instant,
    flash: Option<(String, Instant)>,
    view_h: f32,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext,
        samples: Arc<Vec<Sample>>,
        shared: Arc<Shared>,
        pat: Pattern,
        output: Output,
    ) -> Self {
        style(&cc.egui_ctx);
        Self {
            samples,
            shared,
            pushed: pat.clone(),
            saved_pat: pat.clone(),
            pat,
            output,
            paint: None,
            wheel: 0.0,
            meters: [0.0; MAX_TRACKS],
            last_save: Instant::now(),
            started: Instant::now(),
            flash: None,
            view_h: 600.0,
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

    fn set_steps(&mut self, steps: usize) {
        self.pat.steps = steps.clamp(1, MAX_STEPS);
    }

    fn add_track(&mut self) {
        if self.pat.tracks.len() >= MAX_TRACKS {
            self.say("MAX 16 TRACKS // GEHEUGEN VOL");
            return;
        }
        let used: Vec<usize> = self.pat.tracks.iter().map(|t| t.sample).collect();
        let next = (0..self.samples.len()).find(|i| !used.contains(i)).unwrap_or(0);
        self.pat.tracks.push(Track::new(next, self.samples[next].default_len()));
    }

    fn randomize(&mut self) {
        let steps = self.pat.steps;
        for t in &mut self.pat.tracks {
            let s = &self.samples[t.sample];
            let n = s.name.as_str();
            t.clear();
            for i in 0..steps {
                let r = fastrand::f32();
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
                if r < p {
                    let cell = if fastrand::f32() < 0.25 { Cell::Accent } else { Cell::On };
                    let len = if t.note_len > 1 && fastrand::bool() { t.note_len / 2 } else { t.note_len };
                    t.place(i, cell, len, steps);
                }
            }
        }
        self.say("RANDOMIZE.EXE // PATROON GEGENEREERD");
    }

    fn save_now(&mut self) {
        match pattern::save(&self.pat, &self.samples) {
            Ok(()) => {
                self.saved_pat = self.pat.clone();
                self.say("SAVED >> ~/.config/sequencer/pattern.json");
            }
            Err(e) => self.say(format!("SAVE ERROR: {e}")),
        }
        self.last_save = Instant::now();
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (space, left, right, up, down, save, rnd, clear, plus, rave) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.modifiers.ctrl && i.key_pressed(egui::Key::S),
                !i.modifiers.ctrl && i.key_pressed(egui::Key::R),
                !i.modifiers.ctrl && i.key_pressed(egui::Key::C),
                i.key_pressed(egui::Key::T),
                i.key_pressed(egui::Key::Num9),
            )
        });
        if space {
            self.toggle_play();
        }
        if left {
            self.set_steps(self.pat.steps.saturating_sub(1));
        }
        if right {
            self.set_steps(self.pat.steps + 1);
        }
        if up {
            self.pat.bpm = (self.pat.bpm + 1.0).min(300.0);
        }
        if down {
            self.pat.bpm = (self.pat.bpm - 1.0).max(40.0);
        }
        if save {
            self.save_now();
        }
        if rnd {
            self.randomize();
        }
        if clear {
            clear_cells(&mut self.pat);
        }
        if rave {
            self.pat = Pattern::rave(&self.samples);
            self.say("LOADING RAVE.MOD // 135 BPM // HARDCORE UPROAR");
        }
        if plus {
            self.add_track();
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let t = self.started.elapsed().as_secs_f32();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 74.0), Sense::hover());
        let p = ui.painter_at(rect);

        // Logo met chromatische aberratie en af en toe een glitch.
        let glitch = if (t * 0.7).fract() > 0.93 { ((t * 60.0).sin() * 4.0).round() } else { 0.0 };
        let font = FontId::new(40.0, FontFamily::Monospace);
        let pos = rect.left_top() + Vec2::new(10.0, 4.0);
        let title = "CYBERSEQ//2000";
        p.text(pos + Vec2::new(-3.0 + glitch, 0.0), Align2::LEFT_TOP, title, font.clone(), MAGENTA.gamma_multiply(0.8));
        p.text(pos + Vec2::new(3.0 - glitch, 1.0), Align2::LEFT_TOP, title, font.clone(), CYAN.gamma_multiply(0.8));
        let title_rect = p.text(pos, Align2::LEFT_TOP, title, font, Color32::WHITE);
        let blink = if (t * 1.5).fract() < 0.6 { "█" } else { " " };
        p.text(
            title_rect.left_bottom() + Vec2::new(2.0, 2.0),
            Align2::LEFT_TOP,
            format!("NEURAL BEATWORKS (c) 1997 :: {} SAMPLES LOADED :: CC0 AUDIO {blink}", self.samples.len()),
            FontId::monospace(12.0),
            GREEN,
        );

        // Oscilloscoop.
        let scope = Rect::from_min_max(Pos2::new(rect.right() - 330.0, rect.top() + 6.0), rect.right_bottom() - Vec2::new(10.0, 4.0));
        if scope.left() > title_rect.right() + 20.0 {
            p.rect_filled(scope, CornerRadius::ZERO, Color32::from_rgb(2, 14, 6));
            for i in 1..4 {
                let y = scope.top() + scope.height() * i as f32 / 4.0;
                p.line_segment([Pos2::new(scope.left(), y), Pos2::new(scope.right(), y)], Stroke::new(1.0, Color32::from_rgb(10, 50, 20)));
            }
            for i in 1..8 {
                let x = scope.left() + scope.width() * i as f32 / 8.0;
                p.line_segment([Pos2::new(x, scope.top()), Pos2::new(x, scope.bottom())], Stroke::new(1.0, Color32::from_rgb(10, 50, 20)));
            }
            let data = self.shared.scope.lock().unwrap().clone();
            let pts: Vec<Pos2> = data
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    Pos2::new(
                        scope.left() + scope.width() * i as f32 / (data.len() - 1) as f32,
                        scope.center().y - (s * 3.0).clamp(-1.0, 1.0) * scope.height() * 0.45,
                    )
                })
                .collect();
            p.add(egui::Shape::line(pts.clone(), Stroke::new(4.0, GREEN.gamma_multiply(0.2))));
            p.add(egui::Shape::line(pts, Stroke::new(1.5, GREEN)));
            bevel(&p, scope, GREEN.gamma_multiply(0.6));
            p.text(scope.left_top() + Vec2::new(4.0, 2.0), Align2::LEFT_TOP, "SCOPE", FontId::monospace(9.0), GREEN.gamma_multiply(0.7));
        }
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let playing = self.playing();
            if neon_button(ui, if playing { "■ STOP" } else { "▶ PLAY" }, if playing { MAGENTA } else { GREEN }, playing, 96.0).clicked() {
                self.toggle_play();
            }
            ui.add_space(8.0);
            label(ui, "BPM", CYAN);
            ui.add(egui::DragValue::new(&mut self.pat.bpm).range(40.0..=300.0).speed(0.5).fixed_decimals(0));
            label(ui, "SWING", CYAN);
            let mut swing = (self.pat.swing * 200.0).round();
            if ui.add(egui::DragValue::new(&mut swing).range(0.0..=100.0).speed(0.5).fixed_decimals(0).suffix("%")).changed() {
                self.pat.swing = swing / 200.0;
            }
            label(ui, "VOL", CYAN);
            let mut vol = (self.pat.master * 100.0).round();
            if ui.add(egui::DragValue::new(&mut vol).range(0.0..=100.0).speed(0.5).fixed_decimals(0)).changed() {
                self.pat.master = vol / 100.0;
            }

            ui.add_space(12.0);
            label(ui, "GRID", YELLOW);
            if neon_button(ui, "-", YELLOW, false, 26.0).clicked() {
                self.set_steps(self.pat.steps.saturating_sub(1));
            }
            let mut steps = self.pat.steps;
            if ui.add(egui::DragValue::new(&mut steps).range(1..=MAX_STEPS).speed(0.2).suffix(" STP")).changed() {
                self.set_steps(steps);
            }
            if neon_button(ui, "+", YELLOW, false, 26.0).clicked() {
                self.set_steps(self.pat.steps + 1);
            }
            for n in [8, 12, 16, 24, 32, 64] {
                if neon_button(ui, &n.to_string(), YELLOW, self.pat.steps == n, 30.0).clicked() {
                    self.set_steps(n);
                }
            }
            ui.add_space(12.0);
            if neon_button(ui, "+ TRACK", CYAN, false, 80.0).clicked() {
                self.add_track();
            }
            if neon_button(ui, "RND", MAGENTA, false, 48.0).clicked() {
                self.randomize();
            }
            if neon_button(ui, "CLR", MAGENTA, false, 48.0).clicked() {
                clear_cells(&mut self.pat);
                self.say("PATROON GEWIST");
            }
            if neon_button(ui, "90S RAVE", GREEN, false, 84.0).on_hover_text("laad een 135 BPM rave-patroon [9]").clicked() {
                self.pat = Pattern::rave(&self.samples);
                self.say("LOADING RAVE.MOD // 135 BPM // HARDCORE UPROAR");
            }
            if neon_button(ui, "SAVE", GREEN, false, 56.0).clicked() {
                self.save_now();
            }
        });
    }

    fn grid(&mut self, ui: &mut egui::Ui) {
        let steps = self.pat.steps;
        let avail = ui.available_width() - LEFT_W - 12.0;
        let cell_w = (avail / steps as f32).floor().clamp(14.0, 96.0);
        // Rijen groeien mee met het venster, zodat het grid de ruimte vult.
        let rows = self.pat.tracks.len() as f32;
        let row_h = ((self.view_h - 70.0) / rows - ui.spacing().item_spacing.y).floor().clamp(30.0, 84.0).min(cell_w.max(30.0) + 4.0);
        let width = LEFT_W + cell_w * steps as f32 + 8.0;
        let playhead = self.shared.step.load(Ordering::Relaxed);
        let playing = self.playing();
        let t = self.started.elapsed().as_secs_f32();

        // Stap-nummers.
        let (num_rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
        let p = ui.painter_at(num_rect);
        for s in 0..steps {
            let x = num_rect.left() + LEFT_W + cell_w * (s as f32 + 0.5);
            let on_head = playing && s == playhead;
            let color = if on_head { YELLOW } else if s % 4 == 0 { CYAN } else { DIM.gamma_multiply(2.5) };
            if cell_w >= 20.0 || s % 4 == 0 {
                p.text(Pos2::new(x, num_rect.center().y), Align2::CENTER_CENTER, format!("{:02}", s + 1), FontId::monospace(10.0), color);
            }
        }

        let mut remove = None;
        let mut move_up = None;
        for ti in 0..self.pat.tracks.len() {
            let (row, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
            self.track_controls(ui, ti, Rect::from_min_size(row.min, Vec2::new(LEFT_W - 8.0, row_h - 4.0)), &mut remove, &mut move_up);

            let cells = Rect::from_min_size(row.min + Vec2::new(LEFT_W, 0.0), Vec2::new(cell_w * steps as f32, row_h - 4.0));
            let resp = ui.interact(cells, ui.id().with(("cells", ti)), Sense::click_and_drag());
            let hit = |pos: Pos2| ((pos.x - cells.left()) / cell_w).floor() as usize;

            // Links klikken of slepen tekent noten (zo lang als de L-instelling van de track),
            // klikken op een bestaande noot wist hem, rechts zet een accent, scrollen maakt hem langer of korter.
            let pointer = ui.input(|i| i.pointer.clone());
            if pointer.primary_pressed() && resp.hovered() {
                if let Some(pos) = pointer.interact_pos() {
                    let s = hit(pos).min(steps - 1);
                    self.paint = Some(if self.pat.tracks[ti].note_at(s).is_none() { Cell::On } else { Cell::Off });
                }
            }
            if !pointer.primary_down() {
                self.paint = None;
            }
            if let (Some(paint), Some(pos)) = (self.paint, pointer.hover_pos()) {
                if cells.contains(pos) && pointer.primary_down() {
                    let s = hit(pos).min(steps - 1);
                    let track = &mut self.pat.tracks[ti];
                    if paint == Cell::Off {
                        track.erase(s);
                    } else {
                        track.place(s, Cell::On, track.note_len, steps);
                    }
                }
            }
            if resp.secondary_clicked() {
                if let Some(pos) = pointer.interact_pos() {
                    let s = hit(pos).min(steps - 1);
                    let track = &mut self.pat.tracks[ti];
                    match track.note_at(s) {
                        Some(n) => {
                            let c = &mut track.cells[n];
                            *c = if *c == Cell::Accent { Cell::On } else { Cell::Accent };
                        }
                        None => track.place(s, Cell::Accent, track.note_len, steps),
                    }
                }
            }
            if resp.hovered() {
                if let Some(pos) = pointer.hover_pos() {
                    let s = hit(pos).min(steps - 1);
                    if self.pat.tracks[ti].note_at(s).is_some() {
                        let dy = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
                        self.wheel += dy;
                        let notches = (self.wheel / 30.0).trunc();
                        if notches != 0.0 {
                            self.wheel -= notches * 30.0;
                            self.pat.tracks[ti].resize(s, notches as i32, steps);
                        }
                    }
                }
            }

            let p = ui.painter();
            let track = &self.pat.tracks[ti];
            let dead = if self.pat.any_solo() { !track.solo } else { track.mute };
            for s in 0..steps {
                let r = Rect::from_min_size(Pos2::new(cells.left() + cell_w * s as f32, cells.top()), Vec2::new(cell_w, cells.height())).shrink(2.0);
                let beat = (s / 4) % 2 == 0;
                let base = if beat { Color32::from_rgb(22, 16, 44) } else { Color32::from_rgb(14, 10, 30) };
                let on_head = playing && s == playhead;
                p.rect_filled(r, CornerRadius::same(2), if on_head { Color32::from_rgb(50, 44, 20) } else { base });
                p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, DIM), StrokeKind::Inside);
            }
            // Noten: één blokje, of een rij aaneengesloten blokjes voor lange noten.
            for n in 0..steps {
                let mut c = match track.cells[n] {
                    Cell::Off => continue,
                    Cell::On => CYAN,
                    Cell::Accent => MAGENTA,
                };
                if dead {
                    c = c.gamma_multiply(0.25);
                }
                let len = (track.lens[n] as usize).min(steps - n);
                let bar = Rect::from_min_size(
                    Pos2::new(cells.left() + cell_w * n as f32, cells.top()),
                    Vec2::new(cell_w * len as f32, cells.height()),
                )
                .shrink(2.0);
                let sounding = playing && !dead && playhead >= n && playhead < n + len;
                p.rect_filled(bar.expand(2.0), CornerRadius::same(3), c.gamma_multiply(if sounding { 0.3 } else { 0.1 }));
                for b in 0..len {
                    let block = Rect::from_min_size(Pos2::new(bar.left() + cell_w * b as f32, bar.top()), Vec2::new(cell_w - 4.0, bar.height()));
                    let block = if b + 1 == len { block } else { block.with_max_x(block.right() + 4.0) };
                    let fill = if sounding && n + b == playhead {
                        Color32::WHITE
                    } else if b == 0 {
                        c
                    } else {
                        c.gamma_multiply(0.6)
                    };
                    p.rect_filled(block, CornerRadius::ZERO, fill);
                    // Voegen tussen de blokjes, zodat je ze kunt tellen.
                    if b > 0 {
                        let x = block.left();
                        p.line_segment([Pos2::new(x, bar.top() + 3.0), Pos2::new(x, bar.bottom() - 3.0)], Stroke::new(2.0, BG.gamma_multiply(0.8)));
                    }
                }
                p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width(), 3.0)), CornerRadius::ZERO, Color32::from_white_alpha(90));
                if len > 1 && cell_w >= 18.0 {
                    p.text(bar.left_top() + Vec2::new(4.0, 5.0), Align2::LEFT_TOP, format!("{len}"), FontId::monospace(10.0), BG);
                }
            }
            // Playhead lijn.
            if playing && playhead < steps {
                let x = cells.left() + cell_w * playhead as f32;
                let r = Rect::from_min_size(Pos2::new(x, cells.top() - 2.0), Vec2::new(cell_w, cells.height() + 4.0));
                p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, YELLOW.gamma_multiply(0.5 + 0.3 * (t * 8.0).sin())), StrokeKind::Inside);
            }
        }

        if let Some(i) = remove {
            if self.pat.tracks.len() > 1 {
                self.pat.tracks.remove(i);
            }
        }
        if let Some(i) = move_up {
            if i > 0 {
                self.pat.tracks.swap(i, i - 1);
            }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            if neon_button(ui, "+ ADD TRACK", CYAN, false, 120.0).clicked() {
                self.add_track();
            }
        });
    }

    fn track_controls(&mut self, ui: &mut egui::Ui, ti: usize, rect: Rect, remove: &mut Option<usize>, move_up: &mut Option<usize>) {
        let samples = self.samples.clone();
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(2), PANEL);
        bevel(p, rect, DIM.gamma_multiply(2.0));

        // Level-meter die knippert bij elke hit.
        let lvl = self.shared.level(ti).max(self.meters[ti]);
        self.meters[ti] = lvl * 0.86;
        self.shared.levels[ti].store(0f32.to_bits(), Ordering::Relaxed);
        let meter = Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::new(5.0, rect.height() - 8.0));
        p.rect_filled(meter, CornerRadius::ZERO, Color32::from_rgb(20, 20, 20));
        let fill = Rect::from_min_max(Pos2::new(meter.left(), meter.bottom() - meter.height() * lvl.min(1.0)), meter.max);
        p.rect_filled(fill, CornerRadius::ZERO, if lvl > 0.8 { MAGENTA } else { GREEN });

        let track = &mut self.pat.tracks[ti];
        let s = &samples[track.sample];
        let num = Rect::from_min_size(rect.min + Vec2::new(12.0, 0.0), Vec2::new(22.0, rect.height()));
        let num_resp = ui.interact(num, ui.id().with(("num", ti)), Sense::click());
        p.text(num.center(), Align2::CENTER_CENTER, format!("{:02}", ti + 1), FontId::monospace(12.0), if num_resp.hovered() { YELLOW } else { TEXT });
        let num_resp = num_resp.on_hover_text("klik: voorluisteren · rechts: omhoog");
        if num_resp.clicked() {
            self.shared.preview.lock().unwrap().push((track.sample, track.volume));
        }
        if num_resp.secondary_clicked() {
            *move_up = Some(ti);
        }

        let kind_color = match s.kind {
            Kind::Drum => CYAN,
            Kind::Riff => MAGENTA,
            Kind::Rave => GREEN,
            Kind::Vox => Color32::from_rgb(255, 140, 0),
            Kind::User => YELLOW,
        };
        let mut chosen = None;
        let combo_rect = Rect::from_center_size(Pos2::new(rect.left() + 100.0, rect.center().y), Vec2::new(128.0, 24.0));
        // Een losse child-ui, zodat de dropdown de layout-cursor van het grid niet verschuift.
        let mut combo_ui = ui.new_child(egui::UiBuilder::new().max_rect(combo_rect).id_salt(("combo", ti)));
        {
            let ui = &mut combo_ui;
            egui::ComboBox::from_id_salt(("sample", ti))
                .width(124.0)
                .height(420.0)
                .selected_text(egui::RichText::new(format!("{} {}", s.kind.label(), s.name.to_uppercase())).color(kind_color))
                .show_ui(ui, |ui| {
                    for kind in [Kind::Drum, Kind::Riff, Kind::Rave, Kind::Vox, Kind::User] {
                        let mut first = true;
                        for (i, smp) in samples.iter().enumerate().filter(|(_, x)| x.kind == kind) {
                            if first {
                                ui.label(egui::RichText::new(match kind {
                                    Kind::Drum => "── TR-808 DRUMS ──",
                                    Kind::Riff => "── RIFFS & STABS ──",
                                    Kind::Rave => "── 90S RAVE ──",
                                    Kind::Vox => "── VOX & SHOUTS ──",
                                    Kind::User => "── USER SAMPLES ──",
                                }).color(GREEN).size(10.0));
                                first = false;
                            }
                            if ui.selectable_label(track.sample == i, smp.name.to_uppercase()).clicked() {
                                chosen = Some(i);
                            }
                        }
                    }
                });
        }
        if let Some(i) = chosen {
            track.sample = i;
            track.note_len = samples[i].default_len();
            self.shared.preview.lock().unwrap().push((i, track.volume));
        }

        // Mute / solo.
        let x = rect.left() + 170.0;
        let m = Rect::from_min_size(Pos2::new(x, rect.center().y - 11.0), Vec2::new(22.0, 22.0));
        if toggle_box(ui, m, "M", track.mute, MAGENTA, ("m", ti)).clicked() {
            track.mute = !track.mute;
        }
        let so = m.translate(Vec2::new(25.0, 0.0));
        if toggle_box(ui, so, "S", track.solo, YELLOW, ("s", ti)).clicked() {
            track.solo = !track.solo;
        }

        // Volume als sleepbare balk.
        let vol = Rect::from_min_size(Pos2::new(so.right() + 6.0, rect.center().y - 8.0), Vec2::new(56.0, 16.0));
        let resp = ui.interact(vol, ui.id().with(("vol", ti)), Sense::click_and_drag()).on_hover_text("volume (slepen, scroll)");
        if resp.dragged() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                track.volume = ((pos.x - vol.left()) / vol.width()).clamp(0.0, 1.0);
            }
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            track.volume = (track.volume + scroll * 0.002).clamp(0.0, 1.0);
        }
        let p = ui.painter();
        p.rect_filled(vol, CornerRadius::ZERO, Color32::from_rgb(20, 14, 36));
        let segs = 8;
        for i in 0..segs {
            let seg = Rect::from_min_size(
                Pos2::new(vol.left() + vol.width() * i as f32 / segs as f32 + 1.0, vol.top() + 1.0),
                Vec2::new(vol.width() / segs as f32 - 2.0, vol.height() - 2.0),
            );
            let lit = (i as f32 + 0.5) / segs as f32 <= track.volume;
            let c = if i >= 6 { MAGENTA } else { CYAN };
            p.rect_filled(seg, CornerRadius::ZERO, if lit { c } else { c.gamma_multiply(0.12) });
        }

        // Toonhoogte in halve tonen, scrollen of slepen.
        let pitch = Rect::from_min_size(Pos2::new(vol.right() + 6.0, rect.center().y - 11.0), Vec2::new(40.0, 22.0));
        let presp = ui.interact(pitch, ui.id().with(("pitch", ti)), Sense::click_and_drag()).on_hover_text("pitch in semitonen (slepen · dubbelklik = 0)");
        if presp.dragged() {
            track.pitch = (track.pitch - presp.drag_delta().y * 0.1).clamp(-24.0, 24.0);
        }
        if presp.double_clicked() {
            track.pitch = 0.0;
        }
        if presp.drag_stopped() {
            track.pitch = track.pitch.round();
        }
        let p = ui.painter();
        p.rect_filled(pitch, CornerRadius::ZERO, Color32::from_rgb(20, 14, 36));
        bevel(p, pitch, if presp.hovered() { YELLOW } else { DIM.gamma_multiply(2.0) });
        p.text(pitch.center(), Align2::CENTER_CENTER, format!("{:+}", track.pitch.round() as i32), FontId::monospace(12.0), if track.pitch.round() == 0.0 { TEXT } else { YELLOW });

        // Lengte van nieuwe noten in blokjes: klik = langer, rechtsklik = korter.
        let len_rect = Rect::from_min_size(Pos2::new(pitch.right() + 4.0, rect.center().y - 11.0), Vec2::new(34.0, 22.0));
        let lresp = ui.interact(len_rect, ui.id().with(("len", ti)), Sense::click()).on_hover_text("lengte van nieuwe noten in blokjes (klik / rechtsklik)");
        const LENS: [u8; 5] = [1, 2, 4, 8, 16];
        let li = LENS.iter().position(|&l| l >= track.note_len).unwrap_or(0);
        if lresp.clicked() {
            track.note_len = LENS[(li + 1) % LENS.len()];
        }
        if lresp.secondary_clicked() {
            track.note_len = LENS[(li + LENS.len() - 1) % LENS.len()];
        }
        let p = ui.painter();
        p.rect_filled(len_rect, CornerRadius::ZERO, Color32::from_rgb(20, 14, 36));
        bevel(p, len_rect, if lresp.hovered() { GREEN } else { DIM.gamma_multiply(2.0) });
        p.text(len_rect.center(), Align2::CENTER_CENTER, format!("L{}", track.note_len), FontId::monospace(12.0), if track.note_len > 1 { GREEN } else { TEXT });

        let x_rect = Rect::from_min_size(Pos2::new(len_rect.right() + 4.0, rect.center().y - 11.0), Vec2::new(20.0, 22.0));
        let xr = ui.interact(x_rect, ui.id().with(("x", ti)), Sense::click()).on_hover_text("track verwijderen");
        ui.painter().text(x_rect.center(), Align2::CENTER_CENTER, "×", FontId::monospace(16.0), if xr.hovered() { MAGENTA } else { DIM.gamma_multiply(3.0) });
        if xr.clicked() {
            *remove = Some(ti);
        }
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        let t = self.started.elapsed().as_secs_f32();
        let cursor = if (t * 2.0).fract() < 0.5 { "_" } else { " " };
        let audio = match &self.output {
            Output::Device(_, rate) => format!("{:.1}kHz", *rate as f32 / 1000.0),
            Output::Silent => "NO AUDIO DEV".into(),
        };
        let msg = match &self.flash {
            Some((m, at)) if at.elapsed() < Duration::from_secs(3) => m.clone(),
            _ => if self.playing() { "SEQ RUNNING".into() } else { "SYS READY".into() },
        };
        let step = self.shared.step.load(Ordering::Relaxed) + 1;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("C:\\CYBERSEQ> {msg}{cursor}")).color(GREEN));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "STEP {step:02}/{:02} │ {:.0} BPM │ {} TRK │ {audio} │ [SPC] play [←→] grid [↑↓] bpm [R]nd [C]lr [T]rack [9] rave [^S]ave │ rechtsklik = accent │ scroll op noot = lengte",
                        self.pat.steps,
                        self.pat.bpm,
                        self.pat.tracks.len()
                    ))
                    .color(TEXT.gamma_multiply(0.7))
                    .size(11.0),
                );
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.keys(&ctx);

        egui::Panel::top("header")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(8, 6)))
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(4.0);
                self.transport(ui);
                ui.add_space(4.0);
                let r = ui.max_rect();
                ui.painter().line_segment([Pos2::new(r.left(), r.bottom() + 5.0), Pos2::new(r.right(), r.bottom() + 5.0)], Stroke::new(1.0, MAGENTA.gamma_multiply(0.6)));
            });
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(Color32::from_rgb(10, 6, 20)).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| self.status(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(8, 8)))
            .show(ui, |ui| {
                backdrop(ui);
                self.view_h = ui.available_height();
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| self.grid(ui));
            });

        scanlines(&ctx);

        // Wijzigingen naar de audio-thread.
        if self.pat != self.pushed {
            *self.shared.pattern.lock().unwrap() = self.pat.clone();
            self.pushed = self.pat.clone();
        }
        // Autosave, maximaal eens per paar seconden.
        if self.pat != self.saved_pat && self.last_save.elapsed() > Duration::from_secs(3) {
            let _ = pattern::save(&self.pat, &self.samples);
            self.saved_pat = self.pat.clone();
            self.last_save = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = pattern::save(&self.pat, &self.samples);
    }
}

fn clear_cells(p: &mut Pattern) {
    for t in &mut p.tracks {
        t.clear();
    }
}

fn label(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.label(egui::RichText::new(text).color(color).size(11.0));
}

/// Win95-achtige bevel, maar dan in neon.
fn bevel(p: &egui::Painter, r: Rect, c: Color32) {
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, c));
    p.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, c));
    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, c.gamma_multiply(0.3)));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, c.gamma_multiply(0.3)));
}

fn neon_button(ui: &mut egui::Ui, text: &str, color: Color32, active: bool, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click());
    let p = ui.painter();
    let hot = resp.hovered();
    let pressed = resp.is_pointer_button_down_on();
    if active || hot {
        p.rect_filled(rect.expand(3.0), CornerRadius::same(2), color.gamma_multiply(if active { 0.25 } else { 0.12 }));
    }
    p.rect_filled(rect, CornerRadius::ZERO, if active { color.gamma_multiply(0.35) } else { PANEL });
    if pressed {
        p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, color.gamma_multiply(0.3)));
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, color));
    } else {
        bevel(p, rect, color);
    }
    let offset = if pressed { Vec2::new(1.0, 1.0) } else { Vec2::ZERO };
    p.text(rect.center() + offset, Align2::CENTER_CENTER, text, FontId::monospace(12.0), if active || hot { Color32::WHITE } else { color });
    resp
}

fn toggle_box(ui: &mut egui::Ui, rect: Rect, text: &str, on: bool, color: Color32, id: (&str, usize)) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, if on { color } else { Color32::from_rgb(20, 14, 36) });
    bevel(p, rect, if resp.hovered() { Color32::WHITE } else { color.gamma_multiply(0.7) });
    p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::monospace(12.0), if on { BG } else { color });
    resp
}

/// Een vaag cybergrid achter alles.
fn backdrop(ui: &egui::Ui) {
    let r = ui.max_rect();
    let p = ui.painter();
    let c = Color32::from_rgba_unmultiplied(80, 40, 160, 14);
    let mut x = r.left();
    while x < r.right() {
        p.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(1.0, c));
        x += 24.0;
    }
    let mut y = r.top();
    while y < r.bottom() {
        p.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new(1.0, c));
        y += 24.0;
    }
}

fn scanlines(ctx: &egui::Context) {
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("scanlines")));
    let r = ctx.content_rect();
    let mut y = r.top();
    while y < r.bottom() {
        p.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new(1.0, Color32::from_black_alpha(55)));
        y += 3.0;
    }
}

fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        for font in style.text_styles.values_mut() {
            font.family = FontFamily::Monospace;
        }
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = Color32::from_rgb(20, 14, 36);
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = MAGENTA.gamma_multiply(0.5);
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.window_stroke = Stroke::new(1.0, CYAN);
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = CornerRadius::ZERO;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
            w.corner_radius = CornerRadius::ZERO;
        }
        v.widgets.inactive.bg_fill = Color32::from_rgb(20, 14, 36);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 14, 36);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, DIM.gamma_multiply(2.0));
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, CYAN);
        v.widgets.hovered.bg_fill = Color32::from_rgb(34, 20, 60);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(34, 20, 60);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, CYAN);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.active.bg_fill = MAGENTA.gamma_multiply(0.4);
        v.widgets.active.bg_stroke = Stroke::new(1.0, MAGENTA);
        v.widgets.open.bg_fill = Color32::from_rgb(34, 20, 60);
        v.widgets.open.bg_stroke = Stroke::new(1.0, MAGENTA);
        style.spacing.item_spacing = Vec2::new(6.0, 2.0);
        style.spacing.interact_size.y = 22.0;
    });
}
