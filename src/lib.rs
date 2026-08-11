//! termassist — split-terminal assistant: your shell on one side, a CLI
//! agent on the other.

pub mod app;
pub mod config;
pub mod input;
pub mod ipc;
pub mod pane;
pub mod pty;
pub mod skill;
pub mod term;
pub mod ui;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn main_entry() {
    if let Err(e) = run() {
        eprintln!("termassist: {e:#}");
        std::process::exit(1);
    }
}

#[derive(Parser)]
#[command(
    name = "termassist",
    version,
    about = "Split-terminal assistant: shell on one side, CLI agent on the other"
)]
struct Cli {
    /// Use this config file instead of the default per-OS path
    /// (affects the first-run wizard and config loading).
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the text (screen + scrollback) of a pane in a running termassist.
    ReadPane {
        /// Only print the last N lines.
        #[arg(long)]
        lines: Option<usize>,
        /// Which pane to read.
        #[arg(long, value_enum, default_value_t = PaneArg::Left)]
        pane: PaneArg,
        /// Socket path/name (default: $TERM_ASSIST_SOCK, else auto-discover).
        #[arg(long)]
        socket: Option<String>,
    },
    /// Install the built-in SKILL.md that teaches the agent `read-pane`.
    InstallSkill {
        /// Install scope: user (~/.agents/skills) or project (./.agents/skills).
        #[arg(long, value_enum, conflicts_with = "path")]
        scope: Option<skill::Scope>,
        /// Custom directory; SKILL.md is written inside it.
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum PaneArg {
    /// The user's shell pane.
    Left,
    /// The agent pane.
    Right,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::ReadPane { lines, pane, socket }) => {
            let socket = socket
                .or_else(|| std::env::var("TERM_ASSIST_SOCK").ok())
                .or_else(ipc::discover_socket)
                .ok_or_else(|| {
                    anyhow!("no running termassist found (use --socket or TERM_ASSIST_SOCK)")
                })?;
            let text = ipc::read_pane(&socket, pane as usize, lines)?;
            println!("{text}");
        }
        Some(Cmd::InstallSkill { scope, path }) => {
            let installed = match path {
                Some(dir) => skill::install_to(&dir)?,
                None => skill::install(scope.unwrap_or(skill::Scope::User))?,
            };
            println!("installed SKILL.md to {}", installed.display());
        }
        None => {
            // Nested invocation inside a termassist pane: delegate to the
            // running instance instead of starting a nested TUI. Fall back
            // to a normal TUI when there's no instance to talk to.
            let delegated = nested_target(std::env::var("TERM_ASSIST_SOCK").ok())
                .and_then(|sock| ipc::open_agent(&sock).ok());
            match delegated {
                Some(msg) => println!("{msg}"),
                None => run_tui(config_path_or_default(cli.config)),
            }
        }
    }
    Ok(())
}

/// The effective config file path: `--config <path>` if given, else the
/// default per-OS location.
pub(crate) fn config_path_or_default(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(config::Config::default_path)
}

/// Decide whether a no-subcommand invocation should delegate to a running
/// instance: yes iff `TERM_ASSIST_SOCK` is set and non-empty.
pub(crate) fn nested_target(var: Option<String>) -> Option<String> {
    var.filter(|s| !s.is_empty())
}

/// RAII guard restoring the terminal on exit.
struct TermGuard;

impl TermGuard {
    fn enter() -> Result<TermGuard> {
        crossterm::terminal::enable_raw_mode().context("enable raw mode")?;
        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )
        .context("enter alternate screen")?;
        Ok(TermGuard)
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen
        );
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Split a command string like `"kimi --verbose"` into program + args.
fn split_command(cmd: &str) -> (String, Vec<String>) {
    let mut parts = cmd.split_whitespace();
    let program = parts.next().unwrap_or("").to_string();
    (program, parts.map(str::to_string).collect())
}

fn run_tui(config_path: PathBuf) {
    if let Err(e) = tui_inner(config_path) {
        eprintln!("termassist: {e:#}");
        std::process::exit(1);
    }
}

fn tui_inner(config_path: PathBuf) -> Result<()> {
    // First-run wizard (no config file + interactive): asks for the agent
    // command, writes the config and exits without entering the TUI. Runs
    // before the skill prompt; a no-op when a config file exists.
    config::first_run_wizard(&config_path);
    let cfg = config::Config::load_from(&config_path);
    skill::pre_tui_check();

    // Panes (spawned in cooked mode so spawn errors can still be reported
    // cleanly, though they are also shown inside the pane itself).
    // Fall back to 80x24 when the size can't be detected (e.g. no tty).
    let size = crossterm::terminal::size()
        .ok()
        .filter(|&(w, h)| w >= 4 && h >= 4)
        .unwrap_or((80, 24));
    let (la, ra) = app::pane_areas(size, cfg.layout, cfg.clamped_ratio());
    let (lr, lc) = app::App::inner_size(la);
    let (rr, rc) = app::App::inner_size(ra);

    let left_term = Arc::new(Mutex::new(term::Term::new(lr, lc, cfg.scrollback_lines)));
    let right_term = Arc::new(Mutex::new(term::Term::new(rr, rc, cfg.scrollback_lines)));

    let server = ipc::Server::start([Some(Arc::clone(&left_term)), Some(Arc::clone(&right_term))])?;
    // Exported to both panes' children: `termassist read-pane` and nested
    // `ta`/`termassist` use it to reach this instance.
    let env = vec![("TERM_ASSIST_SOCK".to_string(), server.path.clone())];

    let shell = cfg.resolved_shell();
    let (agent_prog, agent_args) = split_command(&cfg.agent);
    // Both panes start in the directory termassist was invoked from; the
    // same cwd is used when the agent pane is respawned later.
    let cwd = std::env::current_dir().ok();
    let cwd_display = cwd
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "?".to_string());

    let left = pane::Pane::spawn(
        format!("shell: {cwd_display}"),
        &pty::SpawnSpec {
            program: shell,
            args: vec![],
            env: env.clone(),
            cwd: cwd.clone(),
            rows: lr,
            cols: lc,
        },
        left_term,
    );
    let right = pane::Pane::spawn(
        format!("agent: {agent_prog} — {cwd_display}"),
        &pty::SpawnSpec {
            program: agent_prog.clone(),
            args: agent_args.clone(),
            env: env.clone(),
            cwd: cwd.clone(),
            rows: rr,
            cols: rc,
        },
        right_term,
    );

    // A pane that never started is treated as already closed.
    let left = keep_alive_pane(left, "shell", 0, &server);
    let right = keep_alive_pane(right, "agent", 1, &server);

    let mut app = app::App::new(
        left,
        right,
        cfg.layout,
        cfg.clamped_ratio(),
        cfg.scrollback_lines,
        app::AgentSpec { program: agent_prog, args: agent_args, env, cwd },
    );
    app.set_term_size(size.0, size.1);

    let _guard = TermGuard::enter()?;
    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend).context("create terminal")?;

    let result = event_loop(&mut terminal, &mut app, &cfg, &server);

    drop(terminal);
    drop(_guard);
    ipc::remove_socket(&server.path);
    result
}

/// Keep the pane only if its process started; otherwise warn (still in
/// cooked mode) and clear the pane in the IPC server.
fn keep_alive_pane(
    pane: pane::Pane,
    role: &str,
    idx: usize,
    server: &ipc::Server,
) -> Option<pane::Pane> {
    if pane.alive {
        Some(pane)
    } else {
        if let Some(err) = &pane.spawn_error {
            eprintln!("termassist: cannot start {role} pane: {err}");
        }
        server.set_pane(idx, None);
        None
    }
}

fn event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut app::App,
    cfg: &config::Config,
    server: &ipc::Server,
) -> Result<()> {
    while !app.should_quit {
        // Commands from IPC clients (`ta` with no subcommand).
        while let Ok(reply_tx) = server.cmd_rx.try_recv() {
            let msg = app.ensure_agent_open();
            server.set_pane(1, app.right_term());
            let _ = reply_tx.send(msg);
        }
        // Toggle-agent key binding (hide/show/respawn).
        if app.toggle_agent_requested {
            app.toggle_agent_requested = false;
            app.toggle_agent();
            server.set_pane(1, app.right_term());
        }

        terminal.draw(|f| ui::draw(f, app)).context("draw")?;

        if crossterm::event::poll(Duration::from_millis(16)).context("poll event")? {
            match crossterm::event::read().context("read event")? {
                crossterm::event::Event::Key(k) => input::handle_key(app, &cfg.keybindings, k),
                crossterm::event::Event::Mouse(m) => input::handle_mouse(app, m),
                crossterm::event::Event::Resize(w, h) => {
                    app.set_term_size(w, h);
                    app.relayout();
                }
                _ => {}
            }
        }

        app.poll_alive();
        for idx in app.close_dead_panes() {
            server.set_pane(idx, None);
        }
        if app.all_closed() {
            break;
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    #[test]
    fn nested_target_requires_non_empty_socket() {
        assert_eq!(super::nested_target(None), None);
        assert_eq!(super::nested_target(Some(String::new())), None);
        assert_eq!(
            super::nested_target(Some("/tmp/termassist-1.sock".to_string())),
            Some("/tmp/termassist-1.sock".to_string())
        );
    }

    #[test]
    fn split_command_works() {
        assert_eq!(super::split_command("kimi"), ("kimi".to_string(), vec![]));
        assert_eq!(
            super::split_command("kimi --verbose  --x"),
            ("kimi".to_string(), vec!["--verbose".to_string(), "--x".to_string()])
        );
    }

    #[test]
    fn config_path_prefers_explicit_value() {
        assert_eq!(
            super::config_path_or_default(Some(std::path::PathBuf::from("./dev-config.toml"))),
            std::path::PathBuf::from("./dev-config.toml")
        );
        assert_eq!(
            super::config_path_or_default(None),
            crate::config::Config::default_path()
        );
    }
}
