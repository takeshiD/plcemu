#!/bin/sh
# Records the real TUI driven by key presses (tmux send-keys) with asciinema, then renders a GIF with agg.
# Requires: tmux, asciinema, agg. Output: docs/demo.gif
set -eu
sh scripts/cargo.sh build -q
rm -f /tmp/plcemu-demo.cast
S=plcemu-demo
tmux kill-session -t $S 2>/dev/null || true
tmux new-session -d -s $S -x 80 -y 24 "env -u NO_COLOR asciinema rec -q --overwrite --cols 80 --rows 24 -c './target/debug/plcemu tui examples/motor.plc.json --scan-ms 100' /tmp/plcemu-demo.cast"
k() { sleep "$1"; shift; tmux send-keys -t $S "$@"; }
k 1.5 Enter          # X0 ON (start)
k 1.2 s              # 1 scan -> Y0 ON (self hold)
k 1.2 Enter          # X0 OFF
k 1.0 Space          # RUN -> timer T0 counts to 2s, Y1 ON
k 3.0 Down
k 0.5 Down           # select X2
k 0.6 Enter          # X2 ON -> counter C0 +1
k 0.8 Enter
k 0.8 '?'            # help
k 2.0 Escape
k 0.8 Space          # STOP
k 1.0 q
sleep 1
agg --font-size 14 /tmp/plcemu-demo.cast docs/demo.gif
echo "wrote docs/demo.gif"
