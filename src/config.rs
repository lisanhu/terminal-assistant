//! TOML configuration loading, defaults and key bindings.

use anyhow::{anyhow, Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

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

// ---------------------------------------------------------------------------
// First-run wizard
// ---------------------------------------------------------------------------

/// Pure decision: the wizard runs only when there is no config file yet and
/// both stdin and stdout are terminals.
pub fn should_run_wizard(config_exists: bool, interactive: bool) -> bool {
    !config_exists && interactive
}

/// Full default config file with `agent` substituted in.
pub fn default_config_toml(agent: &str) -> String {
    let agent = agent.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"# termassist configuration
# Agent command for the agent pane (arguments allowed, split on whitespace).
agent = "{agent}"
# shell = "/bin/zsh"      # default: $SHELL (Unix) / %COMSPEC% or powershell (Windows)
layout = "horizontal"     # "horizontal" (left/right) or "vertical" (top/bottom)
ratio = 0.5               # fraction for the left/top pane, 0.1..=0.9
scrollback_lines = 10000  # per pane

[keybindings]
focus_toggle = "Ctrl+g"
layout_toggle = "Ctrl+t"
scroll_mode = "Ctrl+s"
ratio_increase = "Ctrl+Right"
ratio_decrease = "Ctrl+Left"
toggle_agent = "Ctrl+n"
quit = "Ctrl+q"
"#
    )
}

/// Write the default config (chosen agent, everything else default) to
/// `path`, creating parent directories.
pub fn write_default_config(path: &Path, agent: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, default_config_toml(agent))
        .with_context(|| format!("cannot write {}", path.display()))
}

/// Ask (on `input`/`output`) for the agent command. Tri-state result:
/// `Some(cmd)` for a valid (non-empty) entry, `None` for empty/blank input
/// or EOF. I/O is injected so the flow is unit-testable.
pub fn prompt_agent_command(
    input: &mut impl std::io::BufRead,
    output: &mut impl std::io::Write,
    config_path: &Path,
) -> Option<String> {
    writeln!(
        output,
        "termassist: first run — no config file at {}.",
        config_path.display()
    )
    .ok()?;
    write!(output, "Agent command for the agent pane (arguments allowed): ").ok()?;
    output.flush().ok()?;
    let mut line = String::new();
    input.read_line(&mut line).ok()?;
    let line = line.trim();
    if line.is_empty() { None } else { Some(line.to_string()) }
}

/// Confirmation echoed after the default config has been written.
pub fn wizard_echo(agent: &str, path: &Path) -> String {
    format!("agent = \"{agent}\"\ntermassist: config written to {}", path.display())
}

/// Interactive first-run wizard: when there is no config file and stdin /
/// stdout are terminals, ask for the agent command and exit — the wizard
/// never enters the TUI. A valid command writes the default config, echoes
/// it, and exits 0; empty input / EOF / write failure print a hint and exit
/// with a non-zero status (all still before any raw mode / TUI setup).
/// Does nothing when the wizard does not apply (config exists, or
/// non-interactive) — the caller then loads the config the usual way.
pub fn first_run_wizard(path: &Path) {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !should_run_wizard(path.exists(), interactive) {
        return;
    }

    let agent = match prompt_agent_command(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        path,
    ) {
        Some(agent) => agent,
        None => {
            println!("termassist: no agent command entered — agent is not configured.");
            println!("  To configure it later, edit: {}", path.display());
            std::process::exit(1);
        }
    };

    match write_default_config(path, &agent) {
        Ok(()) => {
            println!("{}", wizard_echo(&agent, path));
            println!("termassist: run `termassist` again to start.");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("termassist: cannot write config to {}: {e:#}", path.display());
            eprintln!("  Fix the path or permissions, or retry with --config <path>.");
            std::process::exit(1);
        }
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

    #[test]
    fn prompt_agent_command_takes_input_verbatim() {
        let mut input = std::io::Cursor::new("claude --verbose\n");
        let mut output = Vec::new();
        let agent =
            prompt_agent_command(&mut input, &mut output, Path::new("/tmp/x/config.toml"));
        assert_eq!(agent, Some("claude --verbose".to_string()));
        let shown = String::from_utf8(output).unwrap();
        assert!(shown.contains("first run"));
    }

    #[test]
    fn prompt_agent_command_none_on_empty_blank_or_eof() {
        for input_text in ["\n", "  \t \n", ""] {
            let mut input = std::io::Cursor::new(input_text);
            let mut output = Vec::new();
            let agent =
                prompt_agent_command(&mut input, &mut output, Path::new("/tmp/x/config.toml"));
            assert_eq!(agent, None, "input {input_text:?} should mean 'not configured'");
        }
    }

    #[test]
    fn wizard_echo_shows_agent_and_path() {
        let echo = wizard_echo("claude --verbose", Path::new("/home/u/.config/termassist/config.toml"));
        assert!(echo.contains("agent = \"claude --verbose\""), "{echo}");
        assert!(echo.contains("/home/u/.config/termassist/config.toml"), "{echo}");
    }

    #[test]
    fn default_config_toml_roundtrips_with_chosen_agent() {
        let text = default_config_toml("claude --verbose");
        let c: Config = toml::from_str(&text).unwrap();
        assert_eq!(c.agent, "claude --verbose");
        assert!((c.ratio - 0.5).abs() < f32::EPSILON);
        assert_eq!(c.layout, Layout::Horizontal);
        assert_eq!(c.scrollback_lines, 10_000);
        assert_eq!(
            c.keybindings.toggle_agent,
            KeyBind::new(KeyModifiers::CONTROL, KeyCode::Char('n'))
        );
    }

    #[test]
    fn write_default_config_creates_parent_dirs() {
        let base = std::env::temp_dir().join(format!("termassist-cfg-{}", std::process::id()));
        let path = base.join("nested").join("config.toml");
        let _ = std::fs::remove_dir_all(&base);
        write_default_config(&path, "codex").unwrap();
        let c: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(c.agent, "codex");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn wizard_decision_truth_table() {
        assert!(should_run_wizard(false, true));
        assert!(!should_run_wizard(true, true), "existing config: stay silent");
        assert!(!should_run_wizard(false, false), "non-interactive: skip");
        assert!(!should_run_wizard(true, false));
    }
}
