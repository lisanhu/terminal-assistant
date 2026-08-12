//! Optional debug logging of the raw byte streams, for diagnosing
//! hard-to-reproduce terminal issues. Enabled by setting
//! `TERM_ASSIST_DEBUG_LOG` to a file path; completely off otherwise.
//! Platform-independent.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

fn path() -> Option<&'static PathBuf> {
    PATH.get_or_init(|| std::env::var_os("TERM_ASSIST_DEBUG_LOG").map(PathBuf::from))
        .as_ref()
}

/// Append one timestamped line to the debug log (no-op when disabled).
pub fn log(msg: &str) {
    let Some(p) = path() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let _ = writeln!(f, "[{ts:.3}] {msg}");
    }
}

/// Render raw bytes readably: printable ASCII as-is, everything else as
/// `\xNN` (e.g. `\x1b[<35;12;4M` for an SGR mouse report).
pub fn fmt_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if (0x20..0x7f).contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

/// Log a chunk of a byte stream (one line, escaped).
pub fn log_bytes(tag: &str, bytes: &[u8]) {
    if path().is_some() {
        log(&format!("{tag} {}", fmt_bytes(bytes)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_bytes_escapes_control_and_keeps_printable() {
        assert_eq!(fmt_bytes(b"abc"), "abc");
        assert_eq!(fmt_bytes(b"\x1b[<35;1;2M"), "\\x1b[<35;1;2M");
        assert_eq!(fmt_bytes("你".as_bytes()), "\\xe4\\xbd\\xa0");
        assert_eq!(fmt_bytes(b""), "");
    }
}
