#!/bin/bash
# ---------------------------------------------------------------------------
# Owner-side check for the panel's taskbar/Alt-Tab behaviour.
#
# Run this on YOUR machine (xfce4-panel with the tasklist plugin, which this
# agent host does not have). It needs wmctrl, which only you have.
#
# What it does, in order:
#   1. shows which WM you are on
#   2. opens a plain NORMAL-type test window
#   3. asks you to LOOK at the taskbar and Alt+Tab, and press Enter
#   4. adds SKIP_TASKBAR + SKIP_PAGER with wmctrl
#   5. asks you to LOOK again
#   6. repeats for a UTILITY-type window, for comparison
#   7. restores the state and prints what it observed in X terms
#
# Usage:  bash panel-owner-check.sh
# ---------------------------------------------------------------------------
set -u

echo "=============================================================="
echo " Panel taskbar / Alt-Tab check for the owner"
echo "=============================================================="
echo "WM:     $(xfwm4 --version 2>&1 | head -1 || echo '(unknown)')"
echo "DE:     XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-?}"
echo "SESSION: XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-?}  DISPLAY=${DISPLAY:-?}"
echo "compositor: $(xprop -root _NET_WM_COMPOSITING 2>&1 | head -1)"
echo
command -v wmctrl >/dev/null || { echo "wmctrl NOT FOUND - install it, this check needs it."; exit 1; }
echo "wmctrl: $(wmctrl --version 2>&1 | head -1)"
echo

say() { echo; echo ">>> $*"; echo; }

show() { # win label
  echo "  _NET_WM_STATE        = $(xprop -id "$1" _NET_WM_STATE 2>/dev/null | sed 's/.*= //')"
  echo "  _NET_WM_WINDOW_TYPE  = $(xprop -id "$1" _NET_WM_WINDOW_TYPE 2>/dev/null | sed 's/.*= //')"
  local h; h=$(printf '%x' "$1")
  echo "  in _NET_CLIENT_LIST   = $(xprop -root _NET_CLIENT_LIST 2>/dev/null | grep -qi "$h" && echo YES || echo no)"
  echo "  windows managed by WM = $(wmctrl -l 2>/dev/null | wc -l)"
}

launch() { # title
  # A plain X window with a recognisable name; no probe binaries needed.
  xterm -T "$1" -e "sleep 600" >/dev/null 2>&1 &
  sleep 2
  local w
  w=$(xdotool search --name "^$1$" 2>/dev/null | tail -1)
  echo "$w"
}

say "STEP 1 - a plain NORMAL window."
W1=$(launch "PANELTEST-NORMAL")
echo "window id = ${W1:-none}"
show "${W1:-0}"
cat <<'EOF'

  LOOK NOW:
    * is there a taskbar button for this window?
    * does Alt+Tab include it?

  Press Enter when you have looked.
EOF
read -r

say "STEP 2 - same window, wmctrl adds SKIP_TASKBAR + SKIP_PAGER."
wmctrl -i -r "$W1" -b add,_NET_WM_STATE_SKIP_TASKBAR
wmctrl -i -r "$W1" -b add,_NET_WM_STATE_SKIP_PAGER
sleep 2
show "$W1"
cat <<'EOF'

  LOOK NOW:
    * did the taskbar button disappear?
    * did it disappear from Alt+Tab?

  Press Enter when you have looked.
EOF
read -r

say "STEP 3 - restore, and confirm it comes back (the remove path)."
wmctrl -i -r "$W1" -b remove,_NET_WM_STATE_SKIP_TASKBAR
wmctrl -i -r "$W1" -b remove,_NET_WM_STATE_SKIP_PAGER
sleep 2
show "$W1"
cat <<'EOF'

  LOOK NOW: did the taskbar button come back?

  Press Enter when you have looked.
EOF
read -r
kill %1 2>/dev/null

say "STEP 4 - a UTILITY-type window, for comparison."
echo "Note: xterm cannot change its _NET_WM_WINDOW_TYPE, so this step is"
echo "informational only - it shows what xfwm4 does with a Utility window"
echo "when one appears. The probe that sets the type is:"
echo "    cd <lapsphererepo>"
echo "    cargo run --release --example panel_h_probe -- --x11-type utility \\"
echo "      --seconds 300 --title 'PANELTEST-UTILITY'"
echo
cat <<'EOF'
  Run the command above in another terminal, then come back and press Enter.
EOF
read -r
W2=$(xdotool search --name "^PANELTEST-UTILITY$" 2>/dev/null | tail -1)
if [ -n "$W2" ]; then
  show "$W2"
  cat <<'EOF'

  LOOK NOW: does the UTILITY window appear in the taskbar / Alt+Tab?
  Press Enter when you have looked.
EOF
  read -r
  kill %1 2>/dev/null
else
  echo "  (probe window not found - skipped)"
fi

say "DONE"
echo "Please report, for each step: button visible / not visible, and"
echo "Alt+Tab yes / no. Also report whether your DE is GNOME or KDE, since"
echo "those WMs are NOT TESTED and may differ from xfwm4."