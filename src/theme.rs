//! Kleuren en font uit het actieve Omarchy-thema, zodat de app meeverandert met `omarchy theme set`.

use std::path::PathBuf;
use std::time::SystemTime;

use eframe::egui::Color32;

#[derive(Clone, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub accent: Color32,
    pub selection: Color32,
    pub muted: Color32,
    pub bg: Color32,
    pub bg_dark: Color32,
    pub bg_darker: Color32,
    pub bg_light: Color32,
    pub fg: Color32,
    pub fg_dim: Color32,
    pub fg_bright: Color32,
    pub red: Color32,
    pub yellow: Color32,
    pub orange: Color32,
    pub green: Color32,
    pub cyan: Color32,
    pub magenta: Color32,
}

impl Default for Theme {
    /// Tokyo Night, de standaard van Omarchy.
    fn default() -> Self {
        let c = |h: &str| hex(h).unwrap();
        Self {
            dark: true,
            accent: c("#7aa2f7"),
            selection: c("#292e42"),
            muted: c("#414868"),
            bg: c("#1a1b26"),
            bg_dark: c("#13141c"),
            bg_darker: c("#0e0e14"),
            bg_light: c("#24283b"),
            fg: c("#a9b1d6"),
            fg_dim: c("#565f89"),
            fg_bright: c("#c0caf5"),
            red: c("#f7768e"),
            yellow: c("#e0af68"),
            orange: c("#eb927b"),
            green: c("#9ece6a"),
            cyan: c("#449dab"),
            magenta: c("#ad8ee6"),
        }
    }
}

fn hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_matches('"').trim_start_matches('#');
    if s.len() < 6 {
        return None;
    }
    let v = u32::from_str_radix(&s[..6], 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn colors_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".local/state/omarchy/current/theme/colors.toml"))
}

impl Theme {
    pub fn load() -> Self {
        let mut t = Self::default();
        let Some(text) = colors_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return t;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            if key == "mode" {
                t.dark = !value.contains("light");
                continue;
            }
            let Some(c) = hex(value) else { continue };
            match key {
                "accent" => t.accent = c,
                "selection" => t.selection = c,
                "muted" => t.muted = c,
                "background" => t.bg = c,
                "dark_background" => t.bg_dark = c,
                "darker_background" => t.bg_darker = c,
                "lighter_background" => t.bg_light = c,
                "foreground" => t.fg = c,
                "dark_foreground" => t.fg_dim = c,
                "bright_foreground" => t.fg_bright = c,
                "red" => t.red = c,
                "yellow" => t.yellow = c,
                "orange" => t.orange = c,
                "green" => t.green = c,
                "cyan" => t.cyan = c,
                "magenta" => t.magenta = c,
                _ => {}
            }
        }
        t
    }
}

/// Houdt bij of `colors.toml` is veranderd (Omarchy vervangt het bestand bij een themawissel).
pub struct Watcher {
    last: Option<SystemTime>,
    checked: std::time::Instant,
}

impl Watcher {
    pub fn new() -> Self {
        Self { last: Self::mtime(), checked: std::time::Instant::now() }
    }

    fn mtime() -> Option<SystemTime> {
        colors_path().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
    }

    pub fn changed(&mut self) -> bool {
        if self.checked.elapsed().as_millis() < 1000 {
            return false;
        }
        self.checked = std::time::Instant::now();
        let now = Self::mtime();
        let changed = now != self.last;
        self.last = now;
        changed
    }
}

/// Het monospace-font van het systeem (`omarchy font set` zet dat via fontconfig).
pub fn system_font(style: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}", &format!("monospace:{style}")]).output().ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    std::fs::read(path.trim()).ok()
}
