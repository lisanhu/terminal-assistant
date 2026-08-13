---
status: current
updated: 2026-08-14
source: changes/borderless-view-selection
---
# Mouse interaction

termassist enables terminal mouse capture when the TUI starts. While capture
is enabled, a left click focuses a pane, dragging the divider resizes the
split, and the wheel scrolls the pane under the pointer.

Native text selection belongs to the outer terminal rather than to an
internal termassist clipboard model. `Shift` + left-drag bypasses mouse
capture in terminal emulators that support the conventional override; copying
then uses the outer terminal's normal copy command. termassist does not add a
separate mouse-capture key binding.

The existing `F9 v` fullscreen view is borderless and gives the focused PTY
the entire terminal area. This prevents pane borders or titles from entering
multi-line native selections. Toggling the view off restores the split panes
and their borders.
