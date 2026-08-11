# termassist

**English** | [中文](README.zh-CN.md)

A split-terminal assistant: your normal shell on one side, a nested CLI agent
(default [`kimi`](https://github.com/MoonshotAI/kimi-cli)) on the other — in
the terminal you already use. No tmux, no new terminal emulator.

```
┌ shell: ~/project ────────────┬ agent: kimi — ~/project ────────┐
│ you@host:~/project$ ls       │                                 │
│ ...                          │   Kimi Code                     │
│ you@host:~/project$          │   > _                           │
└──────────────────────────────┴─────────────────────────────────┘
```

Both sides are fully interactive programs sharing one physical terminal.
The agent can see your side: the TUI exposes a local socket, and the bundled
skill teaches the agent to run `termassist read-pane` to read your pane's
screen + scrollback — so you can ask "why did that command fail?" without
copy-pasting anything.

## Features

- **Two real terminals, one window** — left: your `$SHELL`; right: any CLI
  agent (`kimi` by default, configurable). Each side is a real PTY rendered
  through a full vt100 terminal emulator, so colors, full-screen TUIs and
  CJK/emoji all render correctly.
- **Agent sees your terminal** — via `termassist read-pane` over a local
  socket, packaged as a portable [skill](skills/termassist/SKILL.md) that
  works with skill-based agents (kimi, Pi, Claude Code, …); no MCP required.
- **Toggleable agent panel** — `Ctrl+N` spawns / hides / shows the agent.
  Hiding is a *suspend, not a kill*: the agent keeps running in the
  background while your shell goes fullscreen.
- **Sane pane lifecycle** — when a pane's process exits, the pane closes and
  the survivor takes the full screen. Both panes start in the directory you
  launched `termassist` from.
- **Nested invocation just works** — running `termassist` (or `ta`) inside a
  pane talks to the running instance instead of nesting a new TUI: it opens
  or focuses the agent panel and exits.
- **Keyboard & mouse** — focus switching, split direction, divider dragging,
  per-pane scrollback; every key binding is configurable.
- **Cross-platform by design** — platform-specific code is confined to
  `src/pty.rs` (portable-pty: Unix PTY / Windows ConPTY) and `src/ipc.rs`
  (interprocess: Unix socket / Windows named pipe). Built and tested on
  Linux; macOS/Windows compile but are untested.

## Install

```sh
cargo build --release
# the binary is target/release/termassist
```

For the short `ta` alias, add to your shell rc (`~/.bashrc` / `~/.zshrc`):

```sh
alias ta=termassist
```

## Usage

```sh
termassist                 # start the split TUI (left: $SHELL, right: kimi)
termassist read-pane       # print the user pane's screen + scrollback
termassist read-pane --lines 50
termassist read-pane --pane right   # read the agent pane instead
termassist install-skill --scope user
```

- `read-pane` connects to a running TUI over a local socket (Unix domain
  socket / Windows named pipe). The socket is auto-discovered via
  `$TERM_ASSIST_SOCK` (injected into both panes), then the newest socket in
  the temp dir; `--socket <path>` overrides. Reading a pane whose process
  has exited fails cleanly with `pane closed`.
- `install-skill` writes the bundled `SKILL.md` (`--scope user` →
  `~/.agents/skills/termassist/`, `--scope project` →
  `./.agents/skills/termassist/`, `--path <dir>` → `<dir>/SKILL.md`). On
  startup, if the skill is missing from both common locations, termassist
  asks (before entering the TUI) whether to install it.

## Key bindings

Everything that is not a TUI binding is forwarded to the focused pane. All
bindings are configurable (see below).

| Key | Action |
| --- | --- |
| `Ctrl+G` | Toggle focus between panes |
| `Ctrl+T` | Toggle split direction (left/right ↔ top/bottom) |
| `Ctrl+S` | Enter scroll mode for the focused pane |
| `Ctrl+←` / `Ctrl+→` | Move the divider (resize split ratio) |
| `Ctrl+N` | Toggle the agent panel (spawn / hide / show) |
| `Ctrl+Q` | Quit |

**Scroll mode:** `↑`/`k`, `↓`/`j`, `PageUp`/`PageDown`, `Home`/`g` (top),
`End`/`G` (bottom), `Esc`/`q` (exit). Reaching the bottom also exits.

**Mouse:** click a pane to focus it, drag the divider to resize,
wheel-scroll a pane to browse its history. The focused pane has a
highlighted border. Resizing the terminal re-layouts the panes and resizes
both PTYs.

### Agent panel states

| Agent state | `Ctrl+N` |
| --- | --- |
| Never started / process exited | Spawn it (in the original startup cwd) and show |
| Running and visible | Hide it; the shell goes fullscreen |
| Running but hidden | Show it again |

## Configuration

TOML at the per-OS config dir (`~/.config/termassist/config.toml` on Linux).
All fields are optional; defaults shown:

```toml
agent = "kimi"            # command for the agent pane (args allowed)
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
```

Key syntax: `Ctrl+`/`Alt+`/`Shift+` modifiers plus a key name (`a`–`z`,
`Enter`, `Esc`, `Tab`, `Backspace`, `Space`, arrows, `Home`, `End`,
`PageUp`, `PageDown`, `Delete`, `Insert`, `F1`–`F12`). An invalid config
falls back to defaults with a warning on stderr.

## How it works

```
┌─ termassist (single process, ratatui event loop) ───────────┐
│  ┌─ Pane A ────────────┐   ┌─ Pane B ────────────┐          │
│  │ PTY → vt100 → screen│   │ PTY → vt100 → screen│          │
│  │  $SHELL             │   │  agent CLI          │          │
│  └──────────┬──────────┘   └──────────┬──────────┘          │
│             └──► ratatui renderer ◄───┘                     │
│  IPC server (local socket) ◄── reads pane screens           │
└─────────────────────────────────────────────────────────────┘
        ▲
        └── termassist read-pane (client) / nested `termassist`
```

- Each pane is a PTY child ([portable-pty](https://crates.io/crates/portable-pty))
  feeding a [vt100](https://crates.io/crates/vt100) parser; the renderer maps
  the vt100 screen 1:1 onto a [ratatui](https://crates.io/crates/ratatui)
  buffer (crossterm backend).
- `read-pane` and nested `termassist` are thin clients over a local socket
  ([interprocess](https://crates.io/crates/interprocess)).
- Vendored `vendor/vt100` is vt100 0.15.2 with two backported panic fixes
  (deep-scrollback windowing, a `col_wrap` underflow) — upstream panics on
  scrollback offsets larger than the screen height.

## Limitations

- MVP scope: no tabs, no session persistence, no plugins.
- The `agent` command line is split on whitespace; quoting/escaping is not
  supported.
- macOS/Windows are untested (architecture keeps platform code isolated in
  `src/pty.rs` / `src/ipc.rs`); on Windows, socket auto-discovery is not
  supported — use `TERM_ASSIST_SOCK` or `--socket`.

## License

MIT
