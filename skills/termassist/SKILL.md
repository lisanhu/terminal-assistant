---
name: termassist
description: Use when running inside a termassist split-terminal session and you need to see the user's shell pane. Read its current screen plus scrollback with `termassist read-pane` — e.g. to inspect command output, error messages, or what the user is looking at.
---

# termassist pane reader

You are running in one pane of a `termassist` split terminal. The user's
interactive shell runs in the other pane. To see what the user sees, run:

```sh
termassist read-pane             # full scrollback + current screen of the user pane
termassist read-pane --lines 50  # last 50 lines only
```

Notes:

- Output is plain text on stdout; safe to run as often as needed.
- Use it when the user says "look at my terminal", when you need the output
  of a command the user ran, or to check error messages on their side before
  suggesting fixes.
- The connection is discovered automatically via the `TERM_ASSIST_SOCK`
  environment variable (set in both panes); `--socket <path>` overrides it.
- `--pane right` reads the agent pane instead of the user pane (rarely useful).
