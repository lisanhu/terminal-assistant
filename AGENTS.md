# termassist — agent guide

## Project overview

`termassist` is a split-terminal assistant written in Rust: the user's normal
shell runs in one pane and a nested CLI agent (default `kimi`) runs in the
other, inside the terminal the user already has. No tmux, no custom terminal
emulator.

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
  `termassist read-pane`.

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

- `lib.rs` — CLI definition (clap), `run()`, the TUI bootstrap, the main
  event loop, `TermGuard` (RAII terminal restore), nested-invocation
  delegation.
- `app.rs` — `App` state: two optional panes, focus, layout direction, split
  ratio, scroll mode, agent hide/show/respawn state machine, pane lifecycle
  (`close_dead_panes`). Platform-independent.
- `pane.rs` — `Pane`: a PTY child + `Arc<Mutex<Term>>` + scroll offset; owns
  the feeder thread that pumps PTY output into the vt100 parser (panics in
  the parser are caught so the mutex is never poisoned).
- `pty.rs` — thin wrapper over `portable-pty`: `SpawnSpec`, spawn, resize,
  write, exit polling, `default_shell()`. **All platform `cfg` for process
  spawning lives here.**
- `term.rs` — platform-independent wrapper around `vt100::Parser`; notably
  `capture()`, which reconstructs full scrollback+screen text by walking
  one-screen-height windows (vt100 only exposes one window at a time).
- `ipc.rs` — local-socket server/client (`Request`/`Response` JSON),
  socket auto-discovery (newest `termassist-*.sock` in temp dir; not
  supported on Windows). **All platform `cfg` for IPC lives here.**
- `input.rs` — key/mouse routing and key-to-ANSI-bytes encoding
  (`key_to_bytes`, honoring application-cursor mode).
- `ui.rs` — ratatui rendering: borders, focus styling, cell-by-cell vt100 →
  buffer mapping (wide chars occupy two columns).
- `config.rs` — TOML config (`~/.config/termassist/config.toml` on Linux),
  defaults, `KeyBind` parsing (`"Ctrl+g"`, `"Alt+x"`, `"F5"`, …). Invalid
  config falls back to defaults with a stderr warning.

Cross-platform rule: platform-specific code is confined to `src/pty.rs` and
`src/ipc.rs`; everything else must stay platform-independent. Built and
tested on Linux; macOS/Windows compile but are untested.

## Build and test commands

```sh
cargo build            # debug build
cargo build --release  # binary at target/release/termassist
cargo test             # unit tests, all in-module (33 tests as of writing)
cargo clippy           # lint
cargo fmt              # format
```

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
  capture tests, `ui.rs` for rendering tests using `ratatui::backend::TestBackend`).
- Unix-only behavior (PTY spawning, sockets) needs `#[cfg(unix)]` gates.
- Manual smoke test: `cargo run` inside a real terminal, verify both panes
  spawn, `Ctrl+N` toggles the agent, and `termassist read-pane` works from
  within a pane.

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
