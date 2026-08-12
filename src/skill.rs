//! The agent-facing skill: embedded SKILL.md, installation, and the
//! pre-TUI install check. Platform-independent.

use anyhow::{Context, Result};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

pub const SKILL_MD: &str = include_str!("../skills/termassist/SKILL.md");

/// Install scope for `install-skill`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Scope {
    /// `~/.agents/skills/termassist/SKILL.md`
    User,
    /// `./.agents/skills/termassist/SKILL.md`
    Project,
}

pub fn user_skill_path() -> PathBuf {
    let home = directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".agents")
        .join("skills")
        .join("termassist")
        .join("SKILL.md")
}

pub fn project_skill_path() -> PathBuf {
    PathBuf::from(".agents")
        .join("skills")
        .join("termassist")
        .join("SKILL.md")
}

pub fn is_installed() -> bool {
    user_skill_path().is_file() || project_skill_path().is_file()
}

pub fn install(scope: Scope) -> Result<PathBuf> {
    let path = match scope {
        Scope::User => user_skill_path(),
        Scope::Project => project_skill_path(),
    };
    write_to(&path)
}

/// Install into a custom directory (`<dir>/SKILL.md`).
pub fn install_to(dir: &Path) -> Result<PathBuf> {
    write_to(&dir.join("SKILL.md"))
}

fn write_to(path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, SKILL_MD).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path.to_path_buf())
}

/// Called before entering the TUI (still in cooked mode): if the skill is
/// not installed in either common location, ask the user whether to install
/// it. Skips silently when stdin is not a terminal.
pub fn pre_tui_check() {
    if is_installed() {
        return;
    }
    if !std::io::stdin().is_terminal() {
        return;
    }
    eprintln!("termassist: the agent skill is not installed yet.");
    eprintln!("  It teaches the nested agent how to use `termassist read-pane`");
    eprintln!("  to see your shell pane.");
    eprint!("Install now? [u]ser / [p]roject / [s]kip (default: skip): ");
    let _ = std::io::stderr().flush();

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        eprintln!();
        return;
    }
    let scope = match answer.trim().to_ascii_lowercase().as_str() {
        "u" | "user" => Scope::User,
        "p" | "project" => Scope::Project,
        _ => {
            eprintln!("  skipped (run `termassist install-skill` later to install).");
            return;
        }
    };
    match install(scope) {
        Ok(path) => eprintln!("  installed to {}", path.display()),
        Err(e) => eprintln!("  install failed: {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_has_frontmatter() {
        assert!(SKILL_MD.starts_with("---"));
        assert!(SKILL_MD.contains("name: termassist"));
        assert!(SKILL_MD.contains("description:"));
        assert!(SKILL_MD.contains("termassist read-pane"));
    }

    #[test]
    fn install_to_custom_dir() {
        let dir =
            std::env::temp_dir().join(format!("termassist-skill-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = install_to(&dir).unwrap();
        assert!(path.is_file());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SKILL_MD);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
