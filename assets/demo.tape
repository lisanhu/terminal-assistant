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
Hide
Type "export PATH=/home/lisanhu/mine/workspace/AI/terminal-assistant/target/release:$PATH"
Enter
Type `mkdir -p ~/.config/termassist && printf 'agent = "kimi"\n' > ~/.config/termassist/config.toml`
Enter
Type "clear"
Enter
Show

# --- visible setup: the ta alias ---
Type "alias ta=termassist"
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

# --- Ctrl+G: toggle focus ---
Ctrl+G
Sleep 3s
Ctrl+G
Sleep 2.5s

# --- Ctrl+T: toggle split direction ---
Ctrl+T
Sleep 3s
Ctrl+T
Sleep 2.5s

# --- Ctrl+Left / Ctrl+Right: move the divider ---
Ctrl+Left
Sleep 1s
Ctrl+Left
Sleep 2.5s
Ctrl+Right
Sleep 1s
Ctrl+Right
Sleep 2.5s

# --- Ctrl+S: scroll mode ---
Ctrl+S
Sleep 2s
Up 5
Sleep 1.5s
PageUp
Sleep 2s
Escape
Sleep 2s

# --- Ctrl+N: hide / show agent panel ---
Ctrl+N
Sleep 3s
Ctrl+N
Sleep 3s

# --- Ctrl+Q: quit ---
Ctrl+Q
Sleep 2s
Type "echo demo finished"
Enter
Sleep 2s
