//! A pane = a PTY child process + a vt100 terminal + view scroll state.

use crate::pty::{Pty, SpawnSpec};
use crate::term::Term;
use std::io::Read;
use std::sync::{Arc, Mutex};

pub struct Pane {
    pub name: String,
    pub term: Arc<Mutex<Term>>,
    pty: Option<Pty>,
    /// View scroll offset into the scrollback (0 = bottom / live view).
    pub scroll: usize,
    pub alive: bool,
    /// Set when the initial spawn failed.
    pub spawn_error: Option<String>,
}

impl Pane {
    /// Spawn the child process described by `spec`, feeding its output into
    /// `term`. Spawn failures are not fatal: the pane shows the error text
    /// and is marked dead.
    pub fn spawn(name: impl Into<String>, spec: &SpawnSpec, term: Arc<Mutex<Term>>) -> Pane {
        let name = name.into();
        match Pty::spawn(spec) {
            Ok(pty) => {
                if let Ok(mut reader) = pty.reader() {
                    let term = Arc::clone(&term);
                    std::thread::spawn(move || {
                        let mut buf = [0u8; 16 * 1024];
                        loop {
                            match reader.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    if let Ok(mut t) = term.lock() {
                                        // Insurance against panics in the
                                        // terminal emulator on unusual byte
                                        // streams: don't poison the mutex.
                                        let _ = std::panic::catch_unwind(
                                            std::panic::AssertUnwindSafe(|| t.feed(&buf[..n])),
                                        );
                                    }
                                }
                            }
                        }
                    });
                }
                Pane { name, term, pty: Some(pty), scroll: 0, alive: true, spawn_error: None }
            }
            Err(e) => {
                let msg = format!("failed to spawn `{}`: {e:#}", spec.program);
                if let Ok(mut t) = term.lock() {
                    t.feed(format!("[{msg}]\r\n").as_bytes());
                }
                Pane { name, term, pty: None, scroll: 0, alive: false, spawn_error: Some(msg) }
            }
        }
    }

    /// Forward input bytes to the child.
    pub fn write_input(&mut self, bytes: &[u8]) {
        if let Some(pty) = self.pty.as_mut() {
            let _ = pty.write(bytes);
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if let Ok(mut t) = self.term.lock() {
            t.resize(rows, cols);
        }
        if let Some(pty) = self.pty.as_ref() {
            let _ = pty.resize(rows, cols);
        }
    }

    /// Refresh the `alive` flag by polling the child.
    pub fn poll_alive(&mut self) {
        if self.alive {
            if let Some(pty) = self.pty.as_mut() {
                if pty.has_exited() {
                    self.alive = false;
                }
            }
        }
    }

    /// A pane with no child process, for rendering tests.
    #[cfg(test)]
    pub fn dummy(term: Arc<Mutex<Term>>) -> Pane {
        Pane {
            name: "test".to_string(),
            term,
            pty: None,
            scroll: 0,
            alive: false,
            spawn_error: None,
        }
    }
}
