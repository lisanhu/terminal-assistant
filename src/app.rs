//! Application state: up to two panes, layout direction, split ratio, focus
//! and input mode. A pane whose child process has exited is removed; the
//! surviving pane takes the full screen. Platform-independent.

use crate::config::Layout;
use crate::pane::Pane;
use crate::pty::SpawnSpec;
use crate::term::Term;
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// How to (re)spawn the agent pane.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The original startup cwd of the TUI (used for every respawn).
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Left,
    Right,
}

impl Focus {
    pub fn other(self) -> Focus {
        match self {
            Focus::Left => Focus::Right,
            Focus::Right => Focus::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Input is forwarded to the focused pane.
    Normal,
    /// Arrow/page keys scroll the focused pane's history.
    Scroll,
}

/// Compute the two pane rects for a terminal of `size`, splitting in
/// `layout` direction at `ratio` (fraction for the left/top pane).
pub fn pane_areas(term_size: (u16, u16), layout: Layout, ratio: f32) -> (Rect, Rect) {
    let (w, h) = term_size;
    match layout {
        Layout::Horizontal => {
            let lw = ((w as f32) * ratio).round() as i32;
            let lw = lw.clamp(2, (w as i32 - 2).max(2)) as u16;
            (
                Rect::new(0, 0, lw, h),
                Rect::new(lw, 0, w.saturating_sub(lw), h),
            )
        }
        Layout::Vertical => {
            let th = ((h as f32) * ratio).round() as i32;
            let th = th.clamp(2, (h as i32 - 2).max(2)) as u16;
            (
                Rect::new(0, 0, w, th),
                Rect::new(0, th, w, h.saturating_sub(th)),
            )
        }
    }
}

pub struct App {
    pub left: Option<Pane>,
    pub right: Option<Pane>,
    pub focus: Focus,
    pub layout: Layout,
    pub ratio: f32,
    pub mode: Mode,
    pub term_size: (u16, u16),
    /// True while the user drags the divider with the mouse.
    pub dragging: bool,
    pub should_quit: bool,
    pub scrollback_limit: usize,
    /// The agent pane is hidden (suspended, not killed): its child process
    /// and terminal state keep running in the background.
    pub agent_hidden: bool,
    /// Set by the toggle-agent key binding; the main loop performs the
    /// actual spawn/hide (it also needs to update the IPC server state).
    pub toggle_agent_requested: bool,
    /// True after the prefix key was pressed, until the next key press.
    pub prefix_pending: bool,
    /// Zoomed view: the focused pane takes the full screen while the other
    /// keeps running in the background without a rect.
    pub zoomed: bool,
    agent: AgentSpec,
}

impl App {
    pub fn new(
        left: Option<Pane>,
        right: Option<Pane>,
        layout: Layout,
        ratio: f32,
        scrollback_limit: usize,
        agent: AgentSpec,
    ) -> App {
        let focus = if left.is_some() {
            Focus::Left
        } else {
            Focus::Right
        };
        App {
            left,
            right,
            focus,
            layout,
            ratio: ratio.clamp(0.1, 0.9),
            mode: Mode::Normal,
            term_size: (80, 24),
            dragging: false,
            should_quit: false,
            scrollback_limit,
            agent_hidden: false,
            toggle_agent_requested: false,
            prefix_pending: false,
            zoomed: false,
            agent,
        }
    }

    /// The agent pane is visible only when it exists and is not hidden.
    fn agent_visible(&self) -> bool {
        self.right.is_some() && !self.agent_hidden
    }

    /// Rects of the panes to draw/resize. A lone (or the only visible) pane
    /// gets the full screen; a hidden agent pane gets no rect and is left
    /// untouched. When zoomed, the focused pane takes the full screen and
    /// the other keeps running in the background without a rect.
    pub fn rects(&self) -> (Option<Rect>, Option<Rect>) {
        let full = Rect::new(0, 0, self.term_size.0, self.term_size.1);
        match (self.left.is_some(), self.agent_visible()) {
            (true, true) => {
                if self.zoomed {
                    return match self.focus {
                        Focus::Left => (Some(full), None),
                        Focus::Right => (None, Some(full)),
                    };
                }
                let (l, r) = pane_areas(self.term_size, self.layout, self.ratio);
                (Some(l), Some(r))
            }
            (true, false) => (Some(full), None),
            (false, true) => (None, Some(full)),
            (false, false) => (None, None),
        }
    }

    /// Inner (content) size of a pane rect, as (rows, cols).
    pub fn inner_size(area: Rect) -> (u16, u16) {
        (
            area.height.saturating_sub(2).max(1),
            area.width.saturating_sub(2).max(1),
        )
    }

    pub fn set_term_size(&mut self, w: u16, h: u16) {
        self.term_size = (w, h);
    }

    /// Recompute pane sizes and resize the PTYs and terminals.
    pub fn relayout(&mut self) {
        let (la, ra) = self.rects();
        if let (Some(area), Some(pane)) = (la, self.left.as_mut()) {
            let (rows, cols) = Self::inner_size(area);
            pane.resize(rows, cols);
        }
        if let (Some(area), Some(pane)) = (ra, self.right.as_mut()) {
            let (rows, cols) = Self::inner_size(area);
            pane.resize(rows, cols);
        }
    }

    pub fn focused(&self) -> Option<&Pane> {
        match self.focus {
            Focus::Left => self.left.as_ref(),
            Focus::Right => self.right.as_ref(),
        }
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        match self.focus {
            Focus::Left => self.left.as_mut(),
            Focus::Right => self.right.as_mut(),
        }
    }

    /// Scroll the focused pane by `delta` lines (positive = backwards).
    pub fn scroll_focused(&mut self, delta: isize) {
        let limit = self.scrollback_limit as isize;
        if let Some(pane) = self.focused_mut() {
            pane.scroll = (pane.scroll as isize + delta).clamp(0, limit) as usize;
            if pane.scroll == 0 {
                self.mode = Mode::Normal;
            }
        }
    }

    /// Scroll the focused pane by one page (sign selects direction).
    pub fn scroll_page_focused(&mut self, sign: isize) {
        let (la, ra) = self.rects();
        let area = match self.focus {
            Focus::Left => la,
            Focus::Right => ra,
        };
        if let Some(area) = area {
            let page = Self::inner_size(area).0 as isize;
            self.scroll_focused(sign * page.max(1));
        }
    }

    /// Key-binding action: zoom the focused pane to the full screen
    /// (toggle). A no-op unless both panes are visible.
    pub fn toggle_zoom(&mut self) {
        if self.left.is_some() && self.agent_visible() {
            self.zoomed = !self.zoomed;
            self.relayout();
        }
    }

    /// Poll children for exit.
    pub fn poll_alive(&mut self) {
        if let Some(p) = self.left.as_mut() {
            p.poll_alive();
        }
        if let Some(p) = self.right.as_mut() {
            p.poll_alive();
        }
    }

    /// Remove panes whose child has exited; the survivor keeps the focus and
    /// gets relayouted to the full screen. A hidden agent pane that died is
    /// really closed (and unhidden). If the shell pane dies while the agent
    /// is hidden, the agent becomes visible again. Returns the indices of
    /// the closed panes (0 = left, 1 = right) so the caller can update the
    /// IPC server.
    pub fn close_dead_panes(&mut self) -> Vec<usize> {
        let mut closed = Vec::new();
        if self.left.as_ref().is_some_and(|p| !p.alive) {
            self.left = None;
            closed.push(0);
        }
        if self.right.as_ref().is_some_and(|p| !p.alive) {
            self.right = None;
            self.agent_hidden = false;
            closed.push(1);
        }
        if !closed.is_empty() {
            self.zoomed = false;
            if self.left.is_none() && self.right.is_some() {
                // Nothing left to hide behind.
                self.agent_hidden = false;
            }
            if self.focus == Focus::Left && self.left.is_none() {
                self.focus = Focus::Right;
            }
            if self.focus == Focus::Right
                && (!self.agent_visible() || self.right.is_none())
                && self.left.is_some()
            {
                self.focus = Focus::Left;
            }
            self.mode = Mode::Normal;
            self.relayout();
        }
        closed
    }

    pub fn all_closed(&self) -> bool {
        self.left.is_none() && self.right.is_none()
    }

    pub fn right_term(&self) -> Option<Arc<Mutex<Term>>> {
        self.right.as_ref().map(|p| Arc::clone(&p.term))
    }

    /// Key-binding action: toggle the agent panel. Hidden is a suspend —
    /// the agent process and its terminal state stay alive in the
    /// background. A pane whose process exited is respawned instead.
    pub fn toggle_agent(&mut self) -> String {
        if self.right.as_ref().is_some_and(|p| p.alive) {
            if self.agent_hidden {
                self.show_agent()
            } else if self.left.is_some() {
                self.agent_hidden = true;
                self.focus = Focus::Left;
                self.relayout();
                "agent pane hidden".to_string()
            } else {
                // The agent is the only pane; hiding it would leave a blank
                // screen, so just keep it focused.
                self.focus = Focus::Right;
                "agent pane focused".to_string()
            }
        } else {
            self.spawn_agent()
        }
    }

    /// IPC action (`ta` with no subcommand): make sure the agent pane is
    /// open and visible, then focus it.
    pub fn ensure_agent_open(&mut self) -> String {
        if self.right.as_ref().is_some_and(|p| p.alive) {
            if self.agent_hidden {
                self.show_agent()
            } else {
                self.focus = Focus::Right;
                "agent pane focused".to_string()
            }
        } else {
            self.spawn_agent()
        }
    }

    fn show_agent(&mut self) -> String {
        self.agent_hidden = false;
        self.focus = Focus::Right;
        self.relayout();
        "agent pane shown".to_string()
    }

    /// Respawn the agent process in a fresh pane, restoring the split.
    fn spawn_agent(&mut self) -> String {
        self.right = None;
        self.agent_hidden = false;
        let (_, ra) = pane_areas(self.term_size, self.layout, self.ratio);
        let (rows, cols) = Self::inner_size(ra);
        let term = Arc::new(Mutex::new(Term::new(rows, cols, self.scrollback_limit)));
        let spec = SpawnSpec {
            program: self.agent.program.clone(),
            args: self.agent.args.clone(),
            env: self.agent.env.clone(),
            cwd: self.agent.cwd.clone(),
            rows,
            cols,
        };
        let cwd_display = self
            .agent
            .cwd
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "?".to_string());
        let pane = Pane::spawn(
            format!("agent: {} — {}", self.agent.program, cwd_display),
            &spec,
            term,
        );
        if pane.alive {
            self.right = Some(pane);
            self.focus = Focus::Right;
            self.relayout();
            "agent pane opened".to_string()
        } else {
            let err = pane
                .spawn_error
                .unwrap_or_else(|| "unknown error".to_string());
            format!("failed to open agent pane: {err}")
        }
    }
}

#[cfg(all(test, unix))]
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

    fn live_pane() -> Pane {
        Pane::spawn(
            "live",
            &spec("/bin/cat"),
            Arc::new(Mutex::new(Term::new(5, 20, 10))),
        )
    }

    fn dead_pane() -> Pane {
        Pane::spawn(
            "dead",
            &spec("/definitely/not/a/program"),
            Arc::new(Mutex::new(Term::new(5, 20, 10))),
        )
    }

    fn agent() -> AgentSpec {
        AgentSpec {
            program: "/bin/cat".to_string(),
            args: vec![],
            env: vec![],
            cwd: None,
        }
    }

    #[test]
    fn dead_pane_closes_and_survivor_goes_fullscreen() {
        let mut app = App::new(
            Some(dead_pane()),
            Some(live_pane()),
            Layout::Horizontal,
            0.5,
            10,
            agent(),
        );
        app.set_term_size(80, 24);
        assert!(app.focus == Focus::Left);
        let closed = app.close_dead_panes();
        assert_eq!(closed, vec![0]);
        assert!(app.left.is_none());
        assert!(app.right.is_some());
        assert_eq!(app.focus, Focus::Right, "survivor gets focus");
        let (la, ra) = app.rects();
        assert!(la.is_none());
        assert_eq!(ra, Some(Rect::new(0, 0, 80, 24)), "survivor is fullscreen");
        assert!(!app.all_closed());
    }

    #[test]
    fn all_closed_when_both_dead() {
        let mut app = App::new(
            Some(dead_pane()),
            Some(dead_pane()),
            Layout::Horizontal,
            0.5,
            10,
            agent(),
        );
        let closed = app.close_dead_panes();
        assert_eq!(closed, vec![0, 1]);
        assert!(app.all_closed());
        assert_eq!(app.rects(), (None, None));
    }

    #[test]
    fn ensure_agent_open_respawns_then_focuses() {
        let mut app = App::new(None, None, Layout::Horizontal, 0.5, 10, agent());
        app.set_term_size(80, 24);
        // No live panes at all: respawn still works (left would normally
        // keep the TUI alive in practice).
        assert_eq!(app.ensure_agent_open(), "agent pane opened");
        assert!(app.right.is_some());
        assert_eq!(app.focus, Focus::Right);
        // Second call: the pane is alive, so it just focuses.
        assert_eq!(app.ensure_agent_open(), "agent pane focused");
    }

    #[test]
    fn ensure_agent_open_reports_spawn_failure() {
        let mut app = App::new(
            Some(live_pane()),
            None,
            Layout::Horizontal,
            0.5,
            10,
            AgentSpec {
                program: "/definitely/not/a/program".to_string(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
        );
        app.set_term_size(80, 24);
        let msg = app.ensure_agent_open();
        assert!(msg.starts_with("failed to open agent pane"), "{msg}");
        assert!(app.right.is_none(), "failed respawn leaves pane closed");
    }

    #[test]
    fn toggle_agent_state_machine() {
        let mut app = App::new(
            Some(live_pane()),
            None,
            Layout::Horizontal,
            0.5,
            10,
            agent(),
        );
        app.set_term_size(80, 24);

        // Never started -> spawn and show.
        assert_eq!(app.toggle_agent(), "agent pane opened");
        assert!(app.right.is_some());
        assert!(!app.agent_hidden);
        assert_eq!(app.focus, Focus::Right);
        assert!(app.rects().1.is_some(), "split layout restored");

        // Running and visible -> hide (suspend): process stays alive, term
        // handle stays readable, left goes fullscreen.
        assert_eq!(app.toggle_agent(), "agent pane hidden");
        assert!(app.agent_hidden);
        assert!(app.right.as_ref().is_some_and(|p| p.alive));
        assert!(
            app.right_term().is_some(),
            "hidden pane keeps its term handle"
        );
        assert_eq!(app.focus, Focus::Left);
        let (la, ra) = app.rects();
        assert_eq!(
            la,
            Some(Rect::new(0, 0, 80, 24)),
            "shell fullscreen while hidden"
        );
        assert!(ra.is_none(), "hidden agent gets no rect");

        // Hidden -> shown again.
        assert_eq!(app.toggle_agent(), "agent pane shown");
        assert!(!app.agent_hidden);
        assert_eq!(app.focus, Focus::Right);
        assert!(app.rects().1.is_some());

        // IPC variant: visible -> focused, hidden -> shown.
        assert_eq!(app.ensure_agent_open(), "agent pane focused");
        app.agent_hidden = true;
        assert_eq!(app.ensure_agent_open(), "agent pane shown");
        assert!(!app.agent_hidden);
    }

    #[test]
    fn zoom_gives_focused_pane_fullscreen() {
        let mut app = App::new(
            Some(live_pane()),
            Some(live_pane()),
            Layout::Horizontal,
            0.5,
            10,
            agent(),
        );
        app.set_term_size(80, 24);
        let full = Rect::new(0, 0, 80, 24);

        // Zoom the focused (left) pane: it takes the screen, the right pane
        // keeps running without a rect.
        app.toggle_zoom();
        assert!(app.zoomed);
        assert_eq!(app.rects(), (Some(full), None));

        // Focus switch while zoomed: the fullscreen follows the focus.
        app.focus = Focus::Right;
        assert_eq!(app.rects(), (None, Some(full)));

        // Unzoom restores the split.
        app.toggle_zoom();
        assert!(!app.zoomed);
        let (la, ra) = app.rects();
        assert!(la.is_some() && ra.is_some());
        assert_ne!(la, Some(full));
    }

    #[test]
    fn zoom_is_cleared_when_a_pane_closes() {
        let mut app = App::new(
            Some(dead_pane()),
            Some(live_pane()),
            Layout::Horizontal,
            0.5,
            10,
            agent(),
        );
        app.set_term_size(80, 24);
        app.toggle_zoom();
        assert!(app.zoomed);
        app.close_dead_panes();
        assert!(!app.zoomed);
    }

    #[test]
    fn dead_agent_is_respawned_by_toggle() {
        let mut app = App::new(
            Some(live_pane()),
            Some(dead_pane()),
            Layout::Horizontal,
            0.5,
            10,
            agent(),
        );
        app.set_term_size(80, 24);
        // The dead pane is cleaned up first (as the main loop would).
        let closed = app.close_dead_panes();
        assert_eq!(closed, vec![1]);
        assert!(app.right.is_none());
        assert_eq!(app.toggle_agent(), "agent pane opened");
        assert!(app.right.as_ref().is_some_and(|p| p.alive));
    }

    #[test]
    fn hidden_agent_unhides_when_shell_dies() {
        let mut app = App::new(
            Some(dead_pane()),
            Some(live_pane()),
            Layout::Horizontal,
            0.5,
            10,
            agent(),
        );
        app.set_term_size(80, 24);
        app.agent_hidden = true;
        let closed = app.close_dead_panes();
        assert_eq!(closed, vec![0]);
        assert!(!app.agent_hidden, "agent unhidden when the shell dies");
        assert_eq!(app.focus, Focus::Right);
        assert_eq!(app.rects().1, Some(Rect::new(0, 0, 80, 24)));
    }
}
