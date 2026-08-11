//! TOML configuration loading, defaults and key bindings.

use anyhow::{anyhow, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;
use std::path::PathBuf;

/// Initial split direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// Left/right split.
    Horizontal,
    /// Top/bottom split.
    Vertical,
}

impl Layout {
    pub fn flipped(self) -> Layout {
        match self {
            Layout::Horizontal => Layout::Vertical,
            Layout::Vertical => Layout::Horizontal,
        }
    }
}

/// A parsed key binding, e.g. `Ctrl+Right` or `Alt+x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBind {
    pub mods: KeyModifiers,
    pub code: KeyCode,
}

impl KeyBind {
    pub fn new(mods: KeyModifiers, code: KeyCode) -> KeyBind {
        KeyBind { mods, code }
    }

    /// Parse strings like `"Ctrl+g"`, `"Ctrl+Shift+Left"`, `"Alt+x"`, `"F5"`.
    pub fn parse(s: &str) -> Result<KeyBind> {
        let mut mods = KeyModifiers::empty();
        let mut code: Option<KeyCode> = None;
        for part in s.split('+') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            let lower = p.to_ascii_lowercase();
            match lower.as_str() {
                "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
                "alt" | "meta" => mods |= KeyModifiers::ALT,
                "shift" => mods |= KeyModifiers::SHIFT,
                "enter" | "return" => code = Some(KeyCode::Enter),
                "esc" | "escape" => code = Some(KeyCode::Esc),
                "tab" => code = Some(KeyCode::Tab),
                "backtab" => code = Some(KeyCode::BackTab),
                "backspace" => code = Some(KeyCode::Backspace),
                "space" => code = Some(KeyCode::Char(' ')),
                "up" => code = Some(KeyCode::Up),
                "down" => code = Some(KeyCode::Down),
                "left" => code = Some(KeyCode::Left),
                "right" => code = Some(KeyCode::Right),
                "home" => code = Some(KeyCode::Home),
                "end" => code = Some(KeyCode::End),
                "pageup" => code = Some(KeyCode::PageUp),
                "pagedown" => code = Some(KeyCode::PageDown),
                "delete" | "del" => code = Some(KeyCode::Delete),
                "insert" | "ins" => code = Some(KeyCode::Insert),
                _ => {
                    if lower.len() > 1 && lower.starts_with('f') {
                        if let Ok(n) = lower[1..].parse::<u8>() {
                            if (1..=12).contains(&n) {
                                code = Some(KeyCode::F(n));
                                continue;
                            }
                        }
                    }
                    let mut chars = p.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => code = Some(KeyCode::Char(c.to_ascii_lowercase())),
                        _ => return Err(anyhow!("unknown key '{p}' in binding '{s}'")),
                    }
                }
            }
        }
        let code = code.ok_or_else(|| anyhow!("binding '{s}' has no key"))?;
        Ok(KeyBind { mods, code })
    }

    pub fn matches(&self, ev: &KeyEvent) -> bool {
        if self.mods != ev.modifiers {
            return false;
        }
        match (self.code, ev.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        }
    }
}

impl<'de> Deserialize<'de> for KeyBind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        KeyBind::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Key bindings for the TUI chrome (everything else is forwarded to the
/// focused pane).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    pub focus_toggle: KeyBind,
    pub layout_toggle: KeyBind,
    pub scroll_mode: KeyBind,
    pub ratio_increase: KeyBind,
    pub ratio_decrease: KeyBind,
    pub toggle_agent: KeyBind,
    pub quit: KeyBind,
}

impl Default for KeyBindings {
    fn default() -> Self {
        KeyBindings {
            focus_toggle: KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('g')),
            layout_toggle: KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('t')),
            scroll_mode: KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('s')),
            ratio_increase: KeyBind::new(KeyModifiers::CONTROL, KeyCode::Right),
            ratio_decrease: KeyBind::new(KeyModifiers::CONTROL, KeyCode::Left),
            toggle_agent: KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('n')),
            quit: KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('q')),
        }
    }
}

/// Top-level configuration (TOML).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Command used to launch the agent pane (default `kimi`). May contain
    /// arguments, split on whitespace.
    pub agent: String,
    /// Shell for the user pane. Defaults to `$SHELL` (Unix) or
    /// `%COMSPEC%`/`powershell.exe` (Windows).
    pub shell: Option<String>,
    /// Initial split direction.
    pub layout: Layout,
    /// Initial split ratio for the left (or top) pane, 0.1..=0.9.
    pub ratio: f32,
    /// Maximum scrollback lines kept per pane.
    pub scrollback_lines: usize,
    pub keybindings: KeyBindings,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            agent: "kimi".to_string(),
            shell: None,
            layout: Layout::Horizontal,
            ratio: 0.5,
            scrollback_lines: 10_000,
            keybindings: KeyBindings::default(),
        }
    }
}

impl Config {
    /// Per-OS config file path (`~/.config/termassist/config.toml` on Linux).
    pub fn default_path() -> PathBuf {
        directories::ProjectDirs::from("", "", "termassist")
            .map(|p| p.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    }

    /// Load the config file, falling back to defaults (with a warning) on any
    /// error.
    pub fn load() -> Config {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &std::path::Path) -> Config {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Config::default(),
            Err(e) => {
                eprintln!("termassist: cannot read {}: {e}; using defaults", path.display());
                return Config::default();
            }
        };
        match toml::from_str(&text) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("termassist: invalid config {}: {e}; using defaults", path.display());
                Config::default()
            }
        }
    }

    /// Shell to launch in the user pane.
    pub fn resolved_shell(&self) -> String {
        self.shell.clone().unwrap_or_else(crate::pty::default_shell)
    }

    pub fn clamped_ratio(&self) -> f32 {
        self.ratio.clamp(0.1, 0.9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_on_empty_toml() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.agent, "kimi");
        assert_eq!(c.shell, None);
        assert_eq!(c.layout, Layout::Horizontal);
        assert!((c.ratio - 0.5).abs() < f32::EPSILON);
        assert_eq!(c.scrollback_lines, 10_000);
        assert_eq!(
            c.keybindings.focus_toggle,
            KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('g'))
        );
        assert_eq!(
            c.keybindings.toggle_agent,
            KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('n'))
        );
    }

    #[test]
    fn parses_full_config() {
        let text = r#"
agent = "kimi --verbose"
shell = "/bin/zsh"
layout = "vertical"
ratio = 0.7
scrollback_lines = 5000

[keybindings]
focus_toggle = "Ctrl+b"
layout_toggle = "Alt+l"
scroll_mode = "Ctrl+u"
ratio_increase = "Ctrl+Up"
ratio_decrease = "Ctrl+Down"
toggle_agent = "Alt+n"
quit = "Ctrl+F12"
"#;
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.agent, "kimi --verbose");
        assert_eq!(c.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(c.layout, Layout::Vertical);
        assert!((c.ratio - 0.7).abs() < f32::EPSILON);
        assert_eq!(c.scrollback_lines, 5000);
        assert_eq!(
            c.keybindings.focus_toggle,
            KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('b'))
        );
        assert_eq!(
            c.keybindings.layout_toggle,
            KeyBind::new(KeyModifiers::ALT, KeyCode::Char('l'))
        );
        assert_eq!(
            c.keybindings.ratio_increase,
            KeyBind::new(KeyModifiers::CONTROL, KeyCode::Up)
        );
        assert_eq!(
            c.keybindings.toggle_agent,
            KeyBind::new(KeyModifiers::ALT, KeyCode::Char('n'))
        );
        assert_eq!(
            c.keybindings.quit,
            KeyBind::new(KeyModifiers::CONTROL, KeyCode::F(12))
        );
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let c: Config = toml::from_str("ratio = 0.3\n").unwrap();
        assert!((c.ratio - 0.3).abs() < f32::EPSILON);
        assert_eq!(c.agent, "kimi");
        assert_eq!(c.scrollback_lines, 10_000);
    }

    #[test]
    fn keybind_parse_variants() {
        assert_eq!(
            KeyBind::parse("ctrl+shift+Left").unwrap(),
            KeyBind::new(KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::Left)
        );
        assert_eq!(
            KeyBind::parse("Alt+X").unwrap(),
            KeyBind::new(KeyModifiers::ALT, KeyCode::Char('x'))
        );
        assert_eq!(
            KeyBind::parse("F5").unwrap(),
            KeyBind::new(KeyModifiers::empty(), KeyCode::F(5))
        );
        assert!(KeyBind::parse("Ctrl+").is_err());
        assert!(KeyBind::parse("banana").is_err());
    }

    #[test]
    fn keybind_matches_event() {
        let kb = KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('g'));
        let ev = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert!(kb.matches(&ev));
        let ev2 = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        assert!(!kb.matches(&ev2));
    }
}
