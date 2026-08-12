//! Platform-specific local IPC: the TUI runs a server on a Unix domain
//! socket (Linux/macOS) or a Windows named pipe; `termassist read-pane` and
//! a nested `termassist`/`ta` (no subcommand) are clients. Protocol: one
//! JSON request (`Request`), one JSON response (`Response`), then the
//! server closes the connection.

use crate::term::Term;
use anyhow::{anyhow, Context, Result};
use interprocess::local_socket::traits::{Listener as _, Stream as _};
#[cfg(not(windows))]
use interprocess::local_socket::{GenericFilePath, ToFsName};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use interprocess::local_socket::{ListenerOptions, Stream};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// Read a pane's text. `pane`: 0 = left/top (user shell),
    /// 1 = right/bottom (agent).
    ReadPane {
        pane: usize,
        #[serde(default)]
        lines: Option<usize>,
    },
    /// Ensure the agent pane exists: respawn it if closed, focus it if open.
    OpenAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub type PaneHandle = Arc<Mutex<Term>>;

/// Shared, mutable view of which panes exist (main loop updates it when
/// panes close or respawn).
pub struct ServerState {
    pub panes: [Option<PaneHandle>; 2],
}

/// Handle the main loop uses to talk to the IPC server.
pub struct Server {
    pub path: String,
    state: Arc<Mutex<ServerState>>,
    /// OpenAgent requests from clients; the payload is the reply channel.
    pub cmd_rx: mpsc::Receiver<mpsc::Sender<String>>,
}

impl Server {
    /// Start the IPC server on the default socket for this process.
    pub fn start(panes: [Option<PaneHandle>; 2]) -> Result<Server> {
        let path = default_socket_name(std::process::id());
        Self::start_on(&path, panes)
    }

    pub fn start_on(path: &str, panes: [Option<PaneHandle>; 2]) -> Result<Server> {
        #[cfg(not(windows))]
        {
            let _ = std::fs::remove_file(path);
        }
        let listener = ListenerOptions::new()
            .name(socket_name(path)?)
            .create_sync()
            .with_context(|| format!("failed to bind ipc socket {path}"))?;
        let state = Arc::new(Mutex::new(ServerState { panes }));
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let thread_state = Arc::clone(&state);
        std::thread::spawn(move || {
            while let Ok(stream) = listener.accept() {
                let state = Arc::clone(&thread_state);
                let cmd_tx = cmd_tx.clone();
                std::thread::spawn(move || {
                    let _ = handle_conn(stream, state, cmd_tx);
                });
            }
        });
        Ok(Server {
            path: path.to_string(),
            state,
            cmd_rx,
        })
    }

    /// Update (or clear) a pane handle after a respawn or close.
    pub fn set_pane(&self, idx: usize, pane: Option<PaneHandle>) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(slot) = state.panes.get_mut(idx) {
                *slot = pane;
            }
        }
    }
}

fn handle_conn(
    mut stream: Stream,
    state: Arc<Mutex<ServerState>>,
    cmd_tx: mpsc::Sender<mpsc::Sender<String>>,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    let n = stream.read(&mut buf)?;
    let req: Request = serde_json::from_slice(&buf[..n]).context("invalid request")?;
    let resp = match req {
        Request::ReadPane { pane, lines } => {
            let handle = state
                .lock()
                .map_err(|_| anyhow!("state lock poisoned"))?
                .panes
                .get(pane)
                .cloned();
            match handle {
                None => Response {
                    ok: false,
                    text: String::new(),
                    error: Some(format!("no pane {pane}")),
                },
                Some(None) => Response {
                    ok: false,
                    text: String::new(),
                    error: Some("pane closed".to_string()),
                },
                Some(Some(term)) => {
                    let text = term
                        .lock()
                        .map_err(|_| anyhow!("pane lock poisoned"))?
                        .capture(lines);
                    Response {
                        ok: true,
                        text,
                        error: None,
                    }
                }
            }
        }
        Request::OpenAgent => {
            let (reply_tx, reply_rx) = mpsc::channel();
            cmd_tx
                .send(reply_tx)
                .map_err(|_| anyhow!("tui is not listening"))?;
            match reply_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(msg) => Response {
                    ok: true,
                    text: msg,
                    error: None,
                },
                Err(_) => Response {
                    ok: false,
                    text: String::new(),
                    error: Some("tui did not respond".to_string()),
                },
            }
        }
    };
    let out = serde_json::to_vec(&resp)?;
    stream.write_all(&out)?;
    stream.flush()?;
    Ok(())
}

fn request(socket: &str, req: &Request) -> Result<String> {
    let mut stream = Stream::connect(socket_name(socket)?)
        .with_context(|| format!("cannot connect to {socket}"))?;
    let payload = serde_json::to_vec(req)?;
    stream.write_all(&payload)?;
    stream.flush()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;
    let resp: Response = serde_json::from_slice(&buf).context("invalid response")?;
    if resp.ok {
        Ok(resp.text)
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "unknown error".to_string())))
    }
}

/// Client side: ask a running TUI instance for a pane's text.
pub fn read_pane(socket: &str, pane: usize, lines: Option<usize>) -> Result<String> {
    request(socket, &Request::ReadPane { pane, lines })
}

/// Client side: ask a running TUI instance to open (or focus) the agent
/// pane. Returns the instance's result message.
pub fn open_agent(socket: &str) -> Result<String> {
    request(socket, &Request::OpenAgent)
}

/// Socket path (Unix) or pipe name (Windows) for this TUI instance.
pub fn default_socket_name(pid: u32) -> String {
    #[cfg(windows)]
    {
        format!("termassist-{pid}")
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir()
            .join(format!("termassist-{pid}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(not(windows))]
fn socket_name(s: &str) -> std::io::Result<interprocess::local_socket::Name<'static>> {
    std::path::PathBuf::from(s).to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn socket_name(s: &str) -> std::io::Result<interprocess::local_socket::Name<'static>> {
    s.to_owned().to_ns_name::<GenericNamespaced>()
}

/// Best-effort discovery of a running instance's socket (used when neither
/// `--socket` nor `TERM_ASSIST_SOCK` is given). Not supported on Windows.
#[cfg(not(windows))]
pub fn discover_socket() -> Option<String> {
    let dir = std::env::temp_dir();
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("termassist-") && name.ends_with(".sock") {
            if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    if best.as_ref().is_none_or(|(bm, _)| mtime > *bm) {
                        best = Some((mtime, entry.path().to_string_lossy().into_owned()));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(windows)]
pub fn discover_socket() -> Option<String> {
    None
}

/// Remove the socket file (no-op for named pipes).
pub fn remove_socket(path: &str) {
    #[cfg(not(windows))]
    {
        let _ = std::fs::remove_file(path);
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_socket_path(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!("termassist-test-{tag}-{}.sock", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn request_response_serde_roundtrip() {
        let req = Request::ReadPane {
            pane: 1,
            lines: Some(42),
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        assert_eq!(serde_json::from_slice::<Request>(&bytes).unwrap(), req);
        assert_eq!(
            String::from_utf8(serde_json::to_vec(&Request::OpenAgent).unwrap()).unwrap(),
            r#"{"cmd":"open_agent"}"#
        );
        let req2: Request = serde_json::from_str(r#"{"cmd":"read_pane","pane":0}"#).unwrap();
        assert_eq!(
            req2,
            Request::ReadPane {
                pane: 0,
                lines: None
            }
        );

        let resp = Response {
            ok: true,
            text: "hi".to_string(),
            error: None,
        };
        let bytes = serde_json::to_vec(&resp).unwrap();
        assert_eq!(serde_json::from_slice::<Response>(&bytes).unwrap(), resp);

        let resp2: Response = serde_json::from_str(r#"{"ok":false,"error":"boom"}"#).unwrap();
        assert!(!resp2.ok);
        assert_eq!(resp2.error.as_deref(), Some("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn server_client_roundtrip() {
        let left: PaneHandle = Arc::new(Mutex::new(Term::new(24, 80, 100)));
        left.lock().unwrap().feed(b"hello from left\r\nline2\r\n");

        let path = test_socket_path("rw");
        let server = Server::start_on(&path, [Some(left), None]).unwrap();

        let text = read_pane(&path, 0, None).unwrap();
        assert!(text.contains("hello from left"));
        assert!(text.contains("line2"));

        let text = read_pane(&path, 0, Some(1)).unwrap();
        assert_eq!(text, "line2");

        // Closed pane: clear error, no panic/hang.
        let err = read_pane(&path, 1, None).unwrap_err();
        assert!(err.to_string().contains("pane closed"), "{err}");

        // Invalid pane index.
        assert!(read_pane(&path, 5, None).is_err());

        drop(server);
        remove_socket(&path);
    }

    #[cfg(unix)]
    #[test]
    fn open_agent_roundtrip() {
        let path = test_socket_path("agent");
        let server = Server::start_on(&path, [None, None]).unwrap();

        // Fake main loop: answer the first OpenAgent command.
        let cmd_rx = server.cmd_rx;
        std::thread::spawn(move || {
            if let Ok(reply_tx) = cmd_rx.recv() {
                let _ = reply_tx.send("agent pane opened".to_string());
            }
        });

        let msg = open_agent(&path).unwrap();
        assert_eq!(msg, "agent pane opened");

        remove_socket(&path);
    }

    #[cfg(unix)]
    #[test]
    fn open_agent_times_out_cleanly_without_main_loop() {
        // Nobody answers cmd_rx; the client must get an error, not hang
        // forever. (The server waits up to 5s; keep the test shorter by
        // just checking it eventually errors.)
        let path = test_socket_path("timeout");
        let _server = Server::start_on(&path, [None, None]).unwrap();
        let err = open_agent(&path).unwrap_err();
        assert!(err.to_string().contains("did not respond"), "{err}");
        remove_socket(&path);
    }
}
