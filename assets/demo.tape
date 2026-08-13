# termassist demo: all key bindings, slow pace, mp4 master for overlay pass
# Render from repo root: vhs assets/demo.tape
# Then add the key-label badges and produce the GIF:
#   python3 assets/overlay.py   (requires Pillow + ffmpeg)

Output assets/demo.mp4

Set Shell bash
Set FontSize 16
Set Width 1600
Set Height 900
Set TypingSpeed 100ms
Set CursorBlink false

# --- hidden prep ---
# NOTE: vhs cannot emit function keys, so the recording pins
# prefix = "Alt+q" below and presses Alt+Q as a stand-in for the real
# default prefix (F9); the overlay badges show the true F9 bindings.
Hide
Type "export PATH=$PWD/target/release:$PATH"
Enter
Type "unset TERM_ASSIST_SOCK"
Enter
Type `printf 'agent = "kimi"\n\n[keybindings]\nprefix = "Alt+q"\n' > ./dev-config.toml`
Enter
Type "clear"
Enter
Show

# --- visible setup: the ta alias ---
Type "alias ta='termassist --config ./dev-config.toml'"
Enter
Sleep 1s

# --- launch ---
Type "ta"
Enter
Wait+Screen /Welcome to Kimi/
Sleep 3s

# give the shell pane some scrollback
Type "seq 1 60"
Enter
Sleep 2s

# --- F9 g: toggle focus ---
Alt+Q
Type "g"
Sleep 3s
Alt+Q
Type "g"
Sleep 2.5s

# --- F9 t: toggle split direction ---
Alt+Q
Type "t"
Sleep 3s
Alt+Q
Type "t"
Sleep 2.5s

# --- F9 Left / F9 Right: move the divider ---
Alt+Q
Left
Sleep 1s
Alt+Q
Left
Sleep 2.5s
Alt+Q
Right
Sleep 1s
Alt+Q
Right
Sleep 2.5s

# --- F9 s: scroll mode ---
Alt+Q
Type "s"
Sleep 2s
Up 5
Sleep 1.5s
PageUp
Sleep 2s
Escape
Sleep 2s

# --- F9 n: hide / show agent panel ---
Alt+Q
Type "n"
Sleep 3s
Alt+Q
Type "n"
Sleep 3s

# --- Shift + drag: native terminal selection (documented by overlay) ---
Sleep 3s

# --- F9 v: borderless fullscreen view / restore ---
Alt+Q
Type "v"
Sleep 4s
Alt+Q
Type "v"
Sleep 2.5s

# --- F9 q: quit ---
Alt+Q
Type "q"
Sleep 2s
Type "echo demo finished"
Enter
Sleep 2s
