//! Platform-specific PTY layer: a thin wrapper over `portable-pty`
//! (ConPTY on Windows, openpty on Unix). All platform `cfg` for process
//! spawning lives here.

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, SlavePty};
use std::io::{Read, Write};

/// Parameters for spawning a child process in a new PTY.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Working directory for the child. The TUI passes its own cwd so both
    /// panes start where the user invoked termassist.
    pub cwd: Option<std::path::PathBuf>,
    pub rows: u16,
    pub cols: u16,
}

/// Build the command for `spec` (separated from `spawn` for testability).
fn command_for(spec: &SpawnSpec) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(dir) = &spec.cwd {
        cmd.cwd(dir);
    }
    cmd
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    // Keep the slave alive: on Windows (ConPTY) dropping it kills the child.
    _slave: Box<dyn SlavePty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    pub fn spawn(spec: &SpawnSpec) -> Result<Pty> {
        let system = portable_pty::native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows: spec.rows.max(1),
                cols: spec.cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open a pty pair")?;
        let child = pair
            .slave
            .spawn_command(command_for(spec))
            .with_context(|| format!("failed to spawn `{}`", spec.program))?;
        let writer = pair.master.take_writer().context("failed to get pty writer")?;
        Ok(Pty {
            master: pair.master,
            _slave: pair.slave,
            writer,
            child,
        })
    }

    /// A second handle for reading the child's output (used by the feeder
    /// thread).
    pub fn reader(&self) -> Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader().context("failed to clone pty reader")
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to resize pty")
    }

    /// True once the child has exited.
    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

/// Default shell for the user pane.
pub fn default_shell() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(program: &str) -> SpawnSpec {
        SpawnSpec {
            program: program.to_string(),
            args: vec![],
            env: vec![],
            cwd: None,
            rows: 5,
            cols: 20,
        }
    }

    #[test]
    fn command_builder_gets_cwd() {
        let mut s = spec("sh");
        assert_eq!(command_for(&s).get_cwd(), None);
        s.cwd = Some(std::path::PathBuf::from("/tmp"));
        assert_eq!(
            command_for(&s).get_cwd(),
            Some(std::ffi::OsStr::new("/tmp").to_os_string()).as_ref()
        );
    }

    #[cfg(unix)]
    #[test]
    fn child_runs_in_given_cwd() {
        let mut s = spec("/bin/pwd");
        s.cwd = Some(std::env::temp_dir());
        let pty = Pty::spawn(&s).unwrap();
        let mut reader = pty.reader().unwrap();
        let mut output = String::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let mut buf = [0u8; 1024];
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => output.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(_) => break,
            }
            if output.contains('/') && output.trim().len() > 1 {
                break;
            }
        }
        // /tmp may resolve to a symlink target (e.g. /private/tmp on macOS);
        // pwd prints the physical directory, so compare against canonical.
        let want = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(
            output.trim_end().ends_with(want.to_str().unwrap()),
            "pwd output {output:?} does not end with {want:?}"
        );
    }
}
