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

# --- Alt+Q g: toggle focus ---
Alt+Q
Type "g"
Sleep 3s
Alt+Q
Type "g"
Sleep 2.5s

# --- Alt+Q t: toggle split direction ---
Alt+Q
Type "t"
Sleep 3s
Alt+Q
Type "t"
Sleep 2.5s

# --- Alt+Q Left / Alt+Q Right: move the divider ---
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

# --- Alt+Q s: scroll mode ---
Alt+Q
Type "s"
Sleep 2s
Up 5
Sleep 1.5s
PageUp
Sleep 2s
Escape
Sleep 2s

# --- Alt+Q n: hide / show agent panel ---
Alt+Q
Type "n"
Sleep 3s
Alt+Q
Type "n"
Sleep 3s

# --- Alt+Q v: zoom the focused pane fullscreen / restore ---
Alt+Q
Type "v"
Sleep 3s
Alt+Q
Type "v"
Sleep 2.5s

# --- Alt+Q q: quit ---
Alt+Q
Type "q"
Sleep 2s
Type "echo demo finished"
Enter
Sleep 2s
