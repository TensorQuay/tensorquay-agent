#!/bin/sh
# RT-I-059: what a Screen viewer's keys type on the Desktop, through the x11vnc the image's own init starts (its exact
# options, read from /usr/local/bin/tqa-desktop-init). RFB key events as noVNC sends them, read back by xev:
#   - Caps Lock on a Mac viewer is one press and release (a toggle), then each letter's own keysym: 'A', not 'a';
#   - Ctrl with that 'A' must still be ctrl+A; ':' and ';' as a non-US layout sends them, without Shift.
# The owner's check on 15 Nov 2026 (a MacBook, Take over): with Caps Lock on, letters came out lower case, because
# x11vnc added Shift for 'A' while the desktop's own Caps Lock was on. Run in the Desktop image:
#   docker run --rm --network none --entrypoint sh <desktop image> /path/desktop-keys-check.sh
# Prints one line per case and exits 1 if any typed text differs.
set -u
Xvfb :0 -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
i=0; while [ $i -lt 50 ] && [ ! -S /tmp/.X11-unix/X0 ]; do sleep 0.1; i=$((i+1)); done
export DISPLAY=:0
args=$(sed -n 's/^keep vnc x11vnc //p' /usr/local/bin/tqa-desktop-init)
[ -n "$args" ] || { echo "FAIL: no x11vnc line in the init"; exit 1; }
# shellcheck disable=SC2086 # the init's own words, as it passes them
x11vnc $args >/dev/null 2>&1 &
xev -geometry 200x100+0+0 > /tmp/xev.log 2>&1 &
sleep 1.5
w=$(/usr/libexec/tqa/xdotool search --name "Event Tester" | head -1)
/usr/libexec/tqa/xdotool windowactivate --sync "$w" 2>/dev/null || /usr/libexec/tqa/xdotool windowfocus "$w"
sleep 0.5
python3 - <<'PY'
import socket, struct, time
def exact(s, n):
    d = b""
    while len(d) < n:
        c = s.recv(n - len(d)); assert c; d += c
    return d
s = socket.create_connection(("127.0.0.1", 5900), timeout=5)
exact(s, 12); s.sendall(b"RFB 003.008\n"); n = exact(s, 1)[0]; exact(s, n); s.sendall(b"\x01"); exact(s, 4); s.sendall(b"\x01")
init = exact(s, 24); exact(s, struct.unpack(">I", init[20:24])[0])
def key(sym, down): s.sendall(struct.pack(">BBHI", 4, 1 if down else 0, 0, sym)); time.sleep(0.08)
def tap(sym): key(sym, 1); key(sym, 0)
CTRL, SHIFT, CAPS = 0xffe3, 0xffe1, 0xffe5
time.sleep(0.5)
tap(0x61)                                       # 'a'
key(SHIFT, 1); tap(0x41); key(SHIFT, 0)         # Shift + 'A'
tap(CAPS); tap(0x41); tap(0x42)                 # Caps Lock on, then 'A' and 'B'
key(CTRL, 1); tap(0x41); key(CTRL, 0)           # ctrl + 'A' with Caps Lock on
tap(CAPS); tap(0x61)                            # Caps Lock off, then 'a'
tap(0x3a); tap(0x3b)                            # ':' and ';' without Shift
time.sleep(0.5)
s.close()
PY
sleep 1
# The text each key press typed, in order (modifier and lock keys type nothing).
typed=$(awk '/^KeyPress/{p=1} p&&/XLookupString gives/{print; p=0}' /tmp/xev.log | sed -nE 's/.*gives ([0-9]+) bytes: (\(([0-9a-f]+)\))?.*/\3/p' | grep -v '^$' | tr '\n' ' ')
want="61 41 41 42 01 61 3a 3b "
printf 'typed:    %s\nexpected: %s\n' "$typed" "$want"
if [ "$typed" = "$want" ]; then echo "PASS: a, Shift+A, Caps Lock A B, Ctrl+A under Caps Lock, a, ':' and ';'"; else echo "FAIL"; exit 1; fi
