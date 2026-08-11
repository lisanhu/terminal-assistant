//! Platform-independent wrapper around the `vt100` terminal emulator: feed
//! raw PTY bytes in, get screen state and scrollback text out.

pub struct Term {
    parser: vt100::Parser,
    scrollback_limit: usize,
}

impl Term {
    pub fn new(rows: u16, cols: u16, scrollback_limit: usize) -> Term {
        Term {
            parser: vt100::Parser::new(rows.max(1), cols.max(1), scrollback_limit),
            scrollback_limit,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(rows.max(1), cols.max(1));
    }

    pub fn scrollback_limit(&self) -> usize {
        self.scrollback_limit
    }

    pub fn parser(&self) -> &vt100::Parser {
        &self.parser
    }

    pub fn parser_mut(&mut self) -> &mut vt100::Parser {
        &mut self.parser
    }

    /// Full text of the pane: scrollback followed by the current screen, with
    /// trailing blank lines removed. When `lines` is `Some(n)`, only the last
    /// `n` lines are returned.
    ///
    /// `vt100` only exposes one screen-height window at a time (via
    /// `Parser::set_scrollback`), so this walks the scrollback from the
    /// oldest window to the live one, dropping the overlapping lines between
    /// adjacent windows.
    pub fn capture(&mut self, lines: Option<usize>) -> String {
        let height = self.parser.screen().size().0.max(1) as usize;
        self.parser.set_scrollback(usize::MAX);
        let max = self.parser.screen().scrollback();

        let mut text = String::new();
        let mut offset = max;
        let mut prev_offset: Option<usize> = None;
        loop {
            self.parser.set_scrollback(offset);
            let chunk = self.parser.screen().contents();
            // Lines shared with the previous (older) window.
            let skip = match prev_offset {
                Some(p) => height - (p - offset),
                None => 0,
            };
            for (i, line) in chunk.lines().enumerate() {
                if i < skip {
                    continue;
                }
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(line);
            }
            if offset == 0 {
                break;
            }
            prev_offset = Some(offset);
            offset = offset.saturating_sub(height);
        }
        self.parser.set_scrollback(0);

        let mut rows: Vec<&str> = text.lines().collect();
        while matches!(rows.last(), Some(l) if l.trim().is_empty()) {
            rows.pop();
        }
        let start = match lines {
            Some(n) => rows.len().saturating_sub(n),
            None => 0,
        };
        rows[start..]
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_and_capture() {
        let mut t = Term::new(24, 80, 100);
        t.feed(b"hello world\r\nsecond line\r\n");
        let text = t.capture(None);
        assert!(text.contains("hello world"));
        assert!(text.contains("second line"));
    }

    #[test]
    fn capture_respects_line_limit() {
        let mut t = Term::new(24, 80, 100);
        t.feed(b"one\r\ntwo\r\nthree\r\n");
        let text = t.capture(Some(2));
        assert_eq!(text, "two\nthree");
    }

    #[test]
    fn scrollback_is_captured() {
        let mut t = Term::new(3, 80, 100);
        for i in 0..10 {
            t.feed(format!("line {i}\r\n").as_bytes());
        }
        let text = t.capture(None);
        assert!(text.contains("line 0"), "scrollback missing: {text:?}");
        assert!(text.contains("line 9"));
    }

    #[test]
    fn scrollback_limit_is_enforced_by_vt100() {
        let mut t = Term::new(3, 80, 5);
        for i in 0..20 {
            t.feed(format!("line {i}\r\n").as_bytes());
        }
        let text = t.capture(None);
        assert!(!text.contains("line 0"));
        assert!(text.contains("line 19"));
    }

    #[test]
    fn resize_keeps_working() {
        let mut t = Term::new(24, 80, 100);
        t.feed(b"before resize\r\n");
        t.resize(10, 40);
        t.feed(b"after");
        let text = t.capture(None);
        assert!(text.contains("before resize"));
        assert!(text.contains("after"));
    }

    #[test]
    fn set_scrollback_is_clamped() {
        // The UI relies on vt100 clamping out-of-range scroll offsets.
        let mut t = Term::new(5, 80, 10);
        for i in 0..12 {
            t.feed(format!("line {i}\r\n").as_bytes());
        }
        t.parser_mut().set_scrollback(usize::MAX);
        let text = t.parser().screen().contents();
        assert!(text.contains("line 0"), "not clamped to oldest: {text:?}");
        t.parser_mut().set_scrollback(0);
        assert!(t.parser().screen().contents().contains("line 11"));
    }
}
