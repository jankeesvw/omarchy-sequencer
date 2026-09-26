//! Colors and font from the active Omarchy theme, so the app follows `omarchy theme set`.

use std::path::PathBuf;

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
    /// Tokyo Night, the Omarchy default.
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
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))?;
    Some(state.join("omarchy/current/theme/colors.toml"))
}

impl Theme {
    pub fn load() -> Self {
        Self::read().readable()
    }

    fn read() -> Self {
        match colors_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => Self::parse(&text),
            None => Self::default(),
        }
    }

    /// Reads `colors.toml`: `key = "#rrggbb"` lines and `mode = "dark"`.
    fn parse(text: &str) -> Self {
        let mut t = Self::default();
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

/// Relative luminance (WCAG).
fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

/// Contrast ratio between two colours, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Moves `c` away from `against` (towards black or white, whichever side it is on)
/// just far enough to reach `min` contrast.
fn ensure(c: Color32, against: Color32, min: f32) -> Color32 {
    if contrast(c, against) >= min {
        return c;
    }
    let lighter = luminance(c) >= luminance(against);
    // Go the other way when there is no room left on this side.
    let target = if lighter && contrast(Color32::WHITE, against) >= min || !lighter && contrast(Color32::BLACK, against) < min {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    (1..=20).map(|i| mix(c, target, i as f32 / 20.0)).find(|m| contrast(*m, against) >= min).unwrap_or(target)
}

impl Theme {
    /// Some Omarchy themes are too subtle for small text and empty cells; this nudges only
    /// the colours that fall short, so every theme stays readable.
    fn readable(mut self) -> Self {
        // Empty cells must stand out from the background, and the two beat shades from each other.
        self.bg_light = ensure(self.bg_light, self.bg, 1.22);
        self.bg_dark = ensure(self.bg_dark, self.bg, 1.08);
        self.selection = ensure(self.selection, self.bg_light, 1.15);
        // Text on cells and on the background.
        self.fg = ensure(self.fg, self.bg_light, 7.0);
        self.fg_bright = ensure(self.fg_bright, self.bg_light, 9.0);
        self.fg_dim = ensure(self.fg_dim, self.bg_light, 4.5);
        // Notes, meters and accents have to be visible on the cells.
        for c in [&mut self.accent, &mut self.red, &mut self.yellow, &mut self.orange, &mut self.green, &mut self.cyan, &mut self.magenta] {
            *c = ensure(*c, self.bg_light, 3.0);
        }
        self
    }

    /// A quieter version of a note colour (for notes without accent) that still stands out on a cell.
    pub fn soften(&self, c: Color32) -> Color32 {
        ensure(mix(c, self.bg_light, 0.3), self.bg_light, 2.2)
    }

    /// Text colour for on top of `fill`: the background or the bright foreground, whichever reads better.
    pub fn on(&self, fill: Color32) -> Color32 {
        let dark = if self.dark { self.bg } else { self.fg_bright };
        let light = if self.dark { self.fg_bright } else { self.bg };
        let best = if contrast(dark, fill) >= contrast(light, fill) { dark } else { light };
        ensure(best, fill, 4.5)
    }
}

/// Tracks whether the theme changed. Compares the contents of `colors.toml` rather than its
/// modification time, because a theme switch can copy files with their original timestamps.
pub struct Watcher {
    last: Option<String>,
    checked: std::time::Instant,
}

impl Watcher {
    pub fn new() -> Self {
        Self { last: Self::read(), checked: std::time::Instant::now() }
    }

    fn read() -> Option<String> {
        colors_path().and_then(|p| std::fs::read_to_string(p).ok())
    }

    pub fn changed(&mut self) -> bool {
        if self.checked.elapsed().as_millis() < 500 {
            return false;
        }
        self.checked = std::time::Instant::now();
        let now = Self::read();
        if now.is_none() || now == self.last {
            return false;
        }
        self.last = now;
        true
    }
}

/// The system monospace font (`omarchy font set` sets it through fontconfig).
pub fn system_font(style: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}", &format!("monospace:{style}")]).output().ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    std::fs::read(path.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every theme that ships with Omarchy (when it is installed) must come out readable.
    #[test]
    fn every_omarchy_theme_is_readable() {
        let Ok(dirs) = std::fs::read_dir("/usr/share/omarchy/themes") else { return };
        for dir in dirs.flatten() {
            let Ok(text) = std::fs::read_to_string(dir.path().join("colors.toml")) else { continue };
            let name = dir.file_name().to_string_lossy().into_owned();
            let t = Theme::parse(&text).readable();
            let check = |what: &str, a: Color32, b: Color32, min: f32| {
                let c = contrast(a, b);
                assert!(c >= min - 0.01, "{name}: {what} contrast {c:.2} < {min}");
            };
            check("dim text on a cell", t.fg_dim, t.bg_light, 4.5);
            check("text on a cell", t.fg, t.bg_light, 7.0);
            check("cell on background", t.bg_light, t.bg, 1.2);
            check("accent on a cell", t.accent, t.bg_light, 3.0);
            check("magenta on a cell", t.magenta, t.bg_light, 3.0);
            check("green on a cell", t.green, t.bg_light, 3.0);
            check("text on accent", t.on(t.accent), t.accent, 4.5);
            check("text on red", t.on(t.red), t.red, 4.5);
            check("soft note on a cell", t.soften(t.accent), t.bg_light, 2.2);
        }
    }
}
