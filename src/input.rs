//! Keyboard/mouse/paste event routing and key-to-ANSI-bytes encoding.
//! Platform-independent.

use crate::app::{App, Focus, Mode};
use crate::config::{KeyBindings, Layout};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

pub fn handle_key(app: &mut App, kb: &KeyBindings, ev: KeyEvent) {
    if ev.kind == KeyEventKind::Release {
        return;
    }

    // TUI chrome actions live behind the prefix key and only fire on fresh
    // presses (ignore key repeat so toggles don't flicker).
    if ev.kind == KeyEventKind::Press {
        if app.prefix_pending {
            app.prefix_pending = false;
            if kb.prefix.matches(&ev) {
                // prefix+prefix: send the prefix key itself to the pane.
                write_key(app, kb.prefix.code, kb.prefix.mods);
            } else {
                chrome_action(app, kb, ev);
            }
            return;
        }
        if kb.prefix.matches(&ev) {
            app.prefix_pending = true;
            return;
        }
    }

    match app.mode {
        Mode::Scroll => scroll_key(app, ev),
        Mode::Normal => write_key(app, ev.code, ev.modifiers),
    }
}

/// Run the chrome action bound to `ev`, if any; returns true when matched.
fn chrome_action(app: &mut App, kb: &KeyBindings, ev: KeyEvent) -> bool {
    if kb.quit.matches(&ev) {
        app.should_quit = true;
    } else if kb.focus_toggle.matches(&ev) {
        app.focus = app.focus.other();
        if app.zoomed {
            app.relayout();
        }
    } else if kb.layout_toggle.matches(&ev) {
        app.layout = app.layout.flipped();
        app.relayout();
    } else if kb.ratio_increase.matches(&ev) {
        app.ratio = (app.ratio + 0.03).min(0.9);
        app.relayout();
    } else if kb.ratio_decrease.matches(&ev) {
        app.ratio = (app.ratio - 0.03).max(0.1);
        app.relayout();
    } else if app.mode == Mode::Normal && kb.scroll_mode.matches(&ev) {
        app.mode = Mode::Scroll;
    } else if kb.toggle_agent.matches(&ev) {
        // The main loop performs the respawn (it must also update the
        // IPC server state).
        app.toggle_agent_requested = true;
    } else if kb.zoom.matches(&ev) {
        app.toggle_zoom();
    } else {
        return false;
    }
    true
}

/// Encode a key as terminal bytes and forward them to the focused pane.
fn write_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    let app_cursor = app
        .focused()
        .and_then(|p| p.term.lock().ok())
        .map(|t| t.parser().screen().application_cursor())
        .unwrap_or(false);
    if let Some(bytes) = key_to_bytes(KeyEvent::new(code, mods), app_cursor) {
        if let Some(pane) = app.focused_mut() {
            pane.scroll = 0;
            pane.write_input(&bytes);
        }
    }
}

/// Forward a paste to the focused pane: wrapped in bracketed-paste markers
/// when the child has enabled mode 2004, raw text otherwise (as if typed).
pub fn handle_paste(app: &mut App, text: &str) {
    let Some(pane) = app.focused_mut() else {
        return;
    };
    let bracketed = pane
        .term
        .lock()
        .map(|t| t.parser().screen().bracketed_paste())
        .unwrap_or(false);
    let bytes = paste_to_bytes(text, bracketed);
    pane.scroll = 0;
    pane.write_input(&bytes);
}

/// Encode pasted text as the bytes a terminal would send: wrapped in
/// `\x1b[200~` / `\x1b[201~` when the child enabled bracketed paste.
pub fn paste_to_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(text.as_bytes());
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

fn scroll_key(app: &mut App, ev: KeyEvent) {
    if !ev.modifiers.is_empty() {
        return;
    }
    match ev.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if let Some(pane) = app.focused_mut() {
                pane.scroll = 0;
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') => app.scroll_focused(1),
        KeyCode::Down | KeyCode::Char('j') => app.scroll_focused(-1),
        KeyCode::PageUp => app.scroll_page_focused(1),
        KeyCode::PageDown => app.scroll_page_focused(-1),
        KeyCode::Home | KeyCode::Char('g') => {
            let limit = app.scrollback_limit;
            if let Some(pane) = app.focused_mut() {
                pane.scroll = limit;
            }
        }
        KeyCode::End | KeyCode::Char('G') => {
            if let Some(pane) = app.focused_mut() {
                pane.scroll = 0;
            }
            app.mode = Mode::Normal;
        }
        _ => {}
    }
}

pub fn handle_mouse(app: &mut App, ev: MouseEvent) {
    app.prefix_pending = false;
    let (la, ra) = app.rects();
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if on_divider(app, ev.column, ev.row) {
                app.dragging = true;
            } else if la.is_some_and(|r| r.contains((ev.column, ev.row).into())) {
                app.focus = Focus::Left;
            } else if ra.is_some_and(|r| r.contains((ev.column, ev.row).into())) {
                app.focus = Focus::Right;
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.dragging {
                let (w, h) = app.term_size;
                let ratio = match app.layout {
                    Layout::Horizontal => f32::from(ev.column) / f32::from(w.max(1)),
                    Layout::Vertical => f32::from(ev.row) / f32::from(h.max(1)),
                };
                app.ratio = ratio.clamp(0.1, 0.9);
                app.relayout();
            }
        }
        MouseEventKind::Up(MouseButton::Left) => app.dragging = false,
        MouseEventKind::ScrollUp => wheel_scroll(app, ev.column, ev.row, 3),
        MouseEventKind::ScrollDown => wheel_scroll(app, ev.column, ev.row, -3),
        _ => {}
    }
}

/// The divider is the two adjacent border columns/rows between the panes
/// (only meaningful when both panes exist).
fn on_divider(app: &App, x: u16, y: u16) -> bool {
    let (Some(la), Some(_)) = app.rects() else {
        return false;
    };
    match app.layout {
        Layout::Horizontal => x == la.right().saturating_sub(1) || x == la.right(),
        Layout::Vertical => y == la.bottom().saturating_sub(1) || y == la.bottom(),
    }
}

fn wheel_scroll(app: &mut App, x: u16, y: u16, delta: isize) {
    let (la, ra) = app.rects();
    let pos = (x, y).into();
    let pane = if la.is_some_and(|r| r.contains(pos)) {
        app.left.as_mut()
    } else if ra.is_some_and(|r| r.contains(pos)) {
        app.right.as_mut()
    } else {
        None
    };
    if let Some(pane) = pane {
        let limit = app.scrollback_limit as isize;
        pane.scroll = (pane.scroll as isize + delta).clamp(0, limit) as usize;
    }
}

/// Encode a key event as the bytes a terminal would send for it.
/// `app_cursor` selects application cursor key mode (SS3) for arrows/Home/End.
pub fn key_to_bytes(ev: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let mods = ev.modifiers;
    match ev.code {
        KeyCode::Char(c) => {
            let mut bytes = Vec::new();
            if mods.contains(KeyModifiers::ALT) {
                bytes.push(0x1b);
            }
            if mods.contains(KeyModifiers::CONTROL) {
                let b = match c.to_ascii_lowercase() {
                    'a'..='z' => c.to_ascii_lowercase() as u8 - b'a' + 1,
                    ' ' | '@' | '2' => 0x00,
                    '[' | '3' => 0x1b,
                    '\\' | '4' => 0x1c,
                    ']' | '5' => 0x1d,
                    '^' | '6' => 0x1e,
                    '_' | '/' | '7' => 0x1f,
                    '?' | '8' => 0x7f,
                    _ => return None,
                };
                bytes.push(b);
            } else {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            Some(bytes)
        }
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(csi_key(b'A', mods, app_cursor)),
        KeyCode::Down => Some(csi_key(b'B', mods, app_cursor)),
        KeyCode::Right => Some(csi_key(b'C', mods, app_cursor)),
        KeyCode::Left => Some(csi_key(b'D', mods, app_cursor)),
        KeyCode::Home => Some(csi_key(b'H', mods, app_cursor)),
        KeyCode::End => Some(csi_key(b'F', mods, app_cursor)),
        KeyCode::PageUp => Some(csi_tilde(5, mods)),
        KeyCode::PageDown => Some(csi_tilde(6, mods)),
        KeyCode::Delete => Some(csi_tilde(3, mods)),
        KeyCode::Insert => Some(csi_tilde(2, mods)),
        KeyCode::F(n) => Some(match n {
            1..=4 if mods.is_empty() => vec![0x1b, b'O', b'P' + n - 1],
            1..=4 => csi_tilde_mod1(b'P' + n - 1, mods),
            5 => csi_tilde(15, mods),
            6 => csi_tilde(17, mods),
            7 => csi_tilde(18, mods),
            8 => csi_tilde(19, mods),
            9 => csi_tilde(20, mods),
            10 => csi_tilde(21, mods),
            11 => csi_tilde(23, mods),
            12 => csi_tilde(24, mods),
            _ => return None,
        }),
        _ => None,
    }
}

/// xterm modifier parameter: 1 + (shift?1) + (alt?2) + (ctrl?4).
fn mod_param(mods: KeyModifiers) -> u8 {
    1 + u8::from(mods.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(mods.contains(KeyModifiers::ALT))
        + 4 * u8::from(mods.contains(KeyModifiers::CONTROL))
}

fn csi_key(letter: u8, mods: KeyModifiers, app_cursor: bool) -> Vec<u8> {
    if mods.is_empty() {
        if app_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    } else {
        format!("\x1b[1;{}{}", mod_param(mods), letter as char).into_bytes()
    }
}

fn csi_tilde(n: u8, mods: KeyModifiers) -> Vec<u8> {
    if mods.is_empty() {
        format!("\x1b[{n}~").into_bytes()
    } else {
        format!("\x1b[{n};{}~", mod_param(mods)).into_bytes()
    }
}

/// F1-F4 with modifiers: CSI 1;<mod>P/Q/R/S.
fn csi_tilde_mod1(letter: u8, mods: KeyModifiers) -> Vec<u8> {
    format!("\x1b[1;{}{}", mod_param(mods), letter as char).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AgentSpec;
    use crate::pane::Pane;
    use crate::term::Term;
    use std::sync::{Arc, Mutex};

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn dummy_app() -> App {
        let pane = || Pane::dummy(Arc::new(Mutex::new(Term::new(5, 20, 10))));
        App::new(
            Some(pane()),
            Some(pane()),
            Layout::Horizontal,
            0.5,
            10,
            AgentSpec {
                program: "x".to_string(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
        )
    }

    #[test]
    fn prefix_arms_and_action_key_fires() {
        let kb = KeyBindings::default();
        let mut app = dummy_app();
        assert_eq!(app.focus, Focus::Left);
        handle_key(&mut app, &kb, ev(kb.prefix.code, kb.prefix.mods));
        assert!(app.prefix_pending);
        assert_eq!(app.focus, Focus::Left, "prefix alone does nothing");
        handle_key(&mut app, &kb, ev(KeyCode::Char('g'), KeyModifiers::NONE));
        assert!(!app.prefix_pending);
        assert_eq!(app.focus, Focus::Right);
    }

    #[test]
    fn action_key_without_prefix_goes_to_pane() {
        let kb = KeyBindings::default();
        let mut app = dummy_app();
        // Plain `g` is forwarded, not a focus toggle.
        handle_key(&mut app, &kb, ev(KeyCode::Char('g'), KeyModifiers::NONE));
        assert!(!app.prefix_pending);
        assert_eq!(app.focus, Focus::Left);
        // A modifier+key combo never fires without the prefix either.
        handle_key(&mut app, &kb, ev(KeyCode::Char('g'), KeyModifiers::CONTROL));
        assert!(!app.prefix_pending);
        assert_eq!(app.focus, Focus::Left);
    }

    #[test]
    fn unknown_key_after_prefix_is_swallowed() {
        let kb = KeyBindings::default();
        let mut app = dummy_app();
        handle_key(&mut app, &kb, ev(kb.prefix.code, kb.prefix.mods));
        handle_key(&mut app, &kb, ev(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.prefix_pending);
        assert_eq!(app.focus, Focus::Left);
        assert!(!app.should_quit);
    }

    #[test]
    fn prefix_prefix_sends_prefix_through_without_firing() {
        let kb = KeyBindings::default();
        let mut app = dummy_app();
        handle_key(&mut app, &kb, ev(kb.prefix.code, kb.prefix.mods));
        handle_key(&mut app, &kb, ev(kb.prefix.code, kb.prefix.mods));
        assert!(!app.prefix_pending);
        assert_eq!(app.focus, Focus::Left, "no chrome action fired");
    }

    #[test]
    fn key_release_neither_arms_nor_fires() {
        let kb = KeyBindings::default();
        let mut app = dummy_app();
        let rel = KeyEvent::new_with_kind(kb.prefix.code, kb.prefix.mods, KeyEventKind::Release);
        handle_key(&mut app, &kb, rel);
        assert!(!app.prefix_pending);
    }

    #[test]
    fn quit_via_prefix() {
        let kb = KeyBindings::default();
        let mut app = dummy_app();
        handle_key(&mut app, &kb, ev(kb.prefix.code, kb.prefix.mods));
        handle_key(&mut app, &kb, ev(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn zoom_via_prefix() {
        let kb = KeyBindings::default();
        let mut app = dummy_app();
        handle_key(&mut app, &kb, ev(kb.prefix.code, kb.prefix.mods));
        handle_key(&mut app, &kb, ev(KeyCode::Char('v'), KeyModifiers::NONE));
        assert!(app.zoomed);
    }

    #[test]
    fn plain_chars() {
        assert_eq!(
            key_to_bytes(ev(KeyCode::Char('a'), KeyModifiers::NONE), false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::Char('A'), KeyModifiers::SHIFT), false),
            Some(b"A".to_vec())
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::Char('é'), KeyModifiers::NONE), false),
            Some("é".as_bytes().to_vec())
        );
    }

    #[test]
    fn ctrl_and_alt_chars() {
        assert_eq!(
            key_to_bytes(ev(KeyCode::Char('c'), KeyModifiers::CONTROL), false),
            Some(vec![0x03])
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::Char('x'), KeyModifiers::ALT), false),
            Some(b"\x1bx".to_vec())
        );
    }

    #[test]
    fn paste_raw_when_child_has_no_bracketed_paste() {
        assert_eq!(paste_to_bytes("a\nb", false), b"a\nb".to_vec());
    }

    #[test]
    fn paste_wrapped_when_child_enabled_bracketed_paste() {
        assert_eq!(
            paste_to_bytes("a\nb", true),
            b"\x1b[200~a\nb\x1b[201~".to_vec()
        );
    }

    /// The encoding decision reads the child's mode-2004 state from the
    /// vendored vt100 screen — pin that dependency.
    #[test]
    fn bracketed_paste_flag_comes_from_child_output() {
        let mut term = crate::term::Term::new(5, 20, 10);
        assert!(!term.parser().screen().bracketed_paste());
        term.feed(b"\x1b[?2004h");
        assert!(term.parser().screen().bracketed_paste());
        term.feed(b"\x1b[?2004l");
        assert!(!term.parser().screen().bracketed_paste());
    }

    #[test]
    fn special_keys() {
        assert_eq!(
            key_to_bytes(ev(KeyCode::Enter, KeyModifiers::NONE), false),
            Some(vec![b'\r'])
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::Backspace, KeyModifiers::NONE), false),
            Some(vec![0x7f])
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::Up, KeyModifiers::NONE), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::Up, KeyModifiers::NONE), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::Up, KeyModifiers::CONTROL), false),
            Some(b"\x1b[1;5A".to_vec())
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::PageDown, KeyModifiers::NONE), false),
            Some(b"\x1b[6~".to_vec())
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::F(1), KeyModifiers::NONE), false),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_to_bytes(ev(KeyCode::BackTab, KeyModifiers::SHIFT), false),
            Some(b"\x1b[Z".to_vec())
        );
    }
}
