# termassist — agent guide

## Project overview

`termassist` is a split-terminal assistant written in Rust: the user's normal
shell runs in one pane and a nested CLI agent (`kimi`, `claude`, `codex`, …)
runs in the other, inside the terminal the user already has. No tmux, no
custom terminal emulator. On the first run (no config file) a plain-text
wizard asks for the agent command (arguments allowed; empty input or EOF
aborts with a non-zero exit and no config written), writes the default
config, and exits 0 without entering the TUI — the next start finds the
config and goes straight to the TUI; non-interactive runs skip it. An
existing but invalid config file is reported and repaired through the same
wizard (interactive) or aborts non-zero (non-interactive) — never silently
ignored. `--config <path>` overrides the config file location everywhere
(wizard, loading, messages); `./dev-config.toml` is git-ignored for local
development.

Architecture (single process, ratatui event loop):

- Each pane is a real PTY child process whose output feeds a `vt100` terminal
  emulator; the renderer maps the vt100 screen 1:1 onto a ratatui buffer
  (crossterm backend). Colors, full-screen TUIs and CJK/emoji work.
- The TUI also runs a local IPC server (Unix domain socket / Windows named
  pipe). `termassist read-pane` (used by the agent to see the user's pane)
  and a nested `termassist`/`ta` invocation are thin clients over that
  socket. Protocol: one JSON request, one JSON response, connection closes.
- The socket path is injected into both panes as `TERM_ASSIST_SOCK`.
- The bundled agent skill (`skills/termassist/SKILL.md`, embedded into the
  binary via `include_str!`) teaches the nested agent to run
  `termassist read-pane`. The `install-skill` subcommand writes it to
  `~/.agents/skills/termassist/` (`--scope user`), `./.agents/skills/termassist/`
  (`--scope project`), or an arbitrary `--path`; on startup, if the skill is
  missing from both common locations, termassist asks (before entering the
  TUI) whether to install it.

The user-facing documentation (features, key bindings, config reference) is
in `README.md` (English) and `README.zh-CN.md` (Chinese) — keep both in sync
when changing behavior.

## Tech stack

- Rust 2021 edition. Crate type: lib (`src/lib.rs` holds almost everything)
  + thin bin (`src/main.rs` calls `termassist::main_entry()`).
- Key dependencies: `ratatui` + `crossterm` (TUI), `portable-pty` (PTY
  spawning, ConPTY/openpty), `interprocess` (local socket / named pipe),
  `clap` (CLI), `serde`/`serde_json` (IPC protocol), `toml` + `directories`
  (config), `anyhow` (errors).
- `vt100` is **vendored** at `vendor/vt100` (0.15.2 + backported panic fixes
  for deep-scrollback windowing and a `col_wrap` underflow — see
  `vendor/vt100/src/grid.rs`). Do not switch to the crates.io version; the
  upstream crate panics on scrollback offsets larger than the screen height.

## Module map (`src/`)

- `lib.rs` — CLI definition (clap: default TUI, `read-pane`, `install-skill`
  subcommands), `run()`, the TUI bootstrap, the main event loop, `TermGuard`
  (RAII terminal restore), nested-invocation delegation.
- `main.rs` — thin binary entry point; calls `termassist::main_entry()`.
- `app.rs` — `App` state: two optional panes, focus, layout direction, split
  ratio, scroll mode, borderless view/zoom (focused pane fullscreen and the
  PTY resized to the whole terminal), agent hide/show/respawn state machine,
  pane lifecycle (`close_dead_panes`: a pane whose child has exited is removed,
  the survivor goes fullscreen). Platform-independent.
- `pane.rs` — `Pane`: a PTY child + `Arc<Mutex<Term>>` + scroll offset; owns
  the feeder thread that pumps PTY output into the vt100 parser (panics in
  the parser are caught so the mutex is never poisoned).
- `pty.rs` — thin wrapper over `portable-pty`: `SpawnSpec`, spawn, resize,
  write, exit polling, `default_shell()`. **All platform `cfg` for process
  spawning lives here.**
- `term.rs` — platform-independent wrapper around `vt100::Parser`; notably
  `capture()`, which reconstructs full scrollback+screen text by walking
  one-screen-height windows (vt100 only exposes one window at a time).
- `ipc.rs` — local-socket server/client (`Request`/`Response` JSON), socket
  auto-discovery (`TERM_ASSIST_SOCK` first, then newest `termassist-*.sock`
  in temp dir; temp-dir discovery not supported on Windows). **All platform
  `cfg` for IPC lives here.**
- `input.rs` — key/mouse/paste routing and key-to-ANSI-bytes encoding
  (`key_to_bytes`, honoring application-cursor mode). TUI chrome actions sit
  behind a configurable prefix key (default `F9`; `prefix_pending` in
  `App`); prefix+prefix forwards the prefix key itself. `handle_paste` wraps
  the paste in bracketed-paste markers when the focused pane's child has
  enabled mode 2004 (read from the vt100 screen), raw text otherwise.
- `skill.rs` — the agent-facing skill: embedded `SKILL.md`, `install-skill`
  command implementation, and the pre-TUI "install the skill?" prompt.
  Platform-independent.
- `dbglog.rs` — opt-in debug logging of raw byte streams (host events, both
  panes' input/output), enabled by `TERM_ASSIST_DEBUG_LOG=<path>`; off
  otherwise. For diagnosing hard-to-reproduce terminal issues.
- `ui.rs` — ratatui rendering: pane borders and focus styling in split view,
  no chrome in fullscreen view, cell-by-cell vt100 → buffer mapping (wide
  chars occupy two columns).
- `config.rs` — TOML config (`~/.config/termassist/config.toml` on Linux).
  `agent` is required in the file; other fields default individually
  (`shell`, `layout`, `ratio`, `scrollback_lines`, `[keybindings]`).
  `read_config` + `config_action` decide between using the file, running the
  plain-text wizard (missing config, or invalid config interactively —
  rewrites the file and exits 0), or a hard non-zero exit (invalid config
  non-interactively). `KeyBind` parsing (`"Ctrl+g"`, `"Alt+x"`, `"F5"`, …).
  Entry point: `resolve_config_or_wizard`.

Cross-platform rule: platform-specific code is confined to `src/pty.rs` and
`src/ipc.rs`; everything else must stay platform-independent. Built and
tested on Linux; macOS/Windows compile but are untested.

## Build and test commands

```sh
cargo build            # debug build
cargo build --release  # binary at target/release/termassist
cargo test             # unit tests, all in-module (58 tests as of writing)
cargo clippy           # lint
cargo fmt              # format
```

Demo assets are reproducible: build the release binary, run
`vhs assets/demo.tape` to create `assets/demo.mp4`, then run
`python3 assets/overlay.py` to add shortcut badges and create
`assets/demo_overlay.mp4` plus `assets/demo.gif`. VHS cannot emit function
keys, so the tape uses `Alt+q` as its recording-only prefix while the badges
show the real default (`F9`). When tape pacing changes, resample the MP4 and
update `EVENTS` in `assets/overlay.py` before regenerating the GIF. The tape
uses the git-ignored `./dev-config.toml` and clears `TERM_ASSIST_SOCK` so it is
reproducible even when launched from inside termassist.

There is no CI configuration and no separate integration-test harness; tests
are `#[cfg(test)]` modules inside each source file. Some tests spawn real
processes (`/bin/cat`, `/bin/pwd`) or bind real Unix sockets in the temp dir
and are gated with `#[cfg(unix)]`.

## Code style guidelines

- Comments and documentation are in **English**; write new code comments and
  commit artifacts in English too (README has a Chinese mirror, source does
  not).
- Every module starts with a `//!` doc comment stating its responsibility and
  whether it is platform-independent.
- Error handling: `anyhow::Result` with `.context()` at boundaries; no
  `unwrap()` outside tests. Prefer graceful degradation (e.g. spawn failure
  marks a pane dead instead of crashing).
- Threads share pane state through `Arc<Mutex<Term>>`; lock failures are
  handled (`if let Ok(...)`), not unwrapped, and `Term::feed` is wrapped in
  `catch_unwind` in the feeder thread.
- Keep changes minimal and match the surrounding style: compact doc comments
  on public items, plain structs, no speculative abstraction.

## Testing instructions

- Run `cargo test` before considering work done; all tests must pass.
- When changing behavior in a module, extend that module's `#[cfg(test)]`
  tests (see `app.rs` for state-machine tests, `term.rs` for scrollback
  capture tests, `ui.rs` for rendering tests using
  `ratatui::backend::TestBackend`).
- Unix-only behavior (PTY spawning, sockets) needs `#[cfg(unix)]` gates.
- Manual smoke test: `cargo run` inside a real terminal, verify both panes
  spawn, `F9 n` toggles the agent, `F9 v` gives a borderless fullscreen view,
  `Shift` + left-drag performs native terminal selection, and
  `termassist read-pane` works from within a pane.

## Security considerations

- The IPC socket gives full read access to both panes' screen contents to any
  local process that can reach it. Sockets live in the temp dir as
  `termassist-<pid>.sock`; rely on OS file permissions, do not add
  authentication without a design discussion.
- `TERM_ASSIST_SOCK` is exported to child processes in both panes; treat
  anything read from the socket as untrusted local input.
- The `agent` config string is split on whitespace only (no shell quoting);
  it is never passed through a shell — keep it that way.
- Do not read or write files outside the user's config/skill locations
  (`~/.config/termassist/`, `~/.agents/skills/`, `./.agents/skills/`) without
  an explicit user request.
