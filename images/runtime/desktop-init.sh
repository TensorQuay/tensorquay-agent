#!/bin/sh
# ADR-0039 slice 2: the Desktop template's init, replacing the spike's. As the sandbox's own user, before the runtime,
# it starts the display stack and keeps it up: Xvfb at 1280x800, a session bus with the accessibility (AT-SPI) bus
# enabled, the window manager, and a VNC server on loopback only with no password (the runtime's screen bridge is the
# only way in, ADR-0039 section 5). A part that exits is started again. It never starts a browser: the runtime owns the
# browser's lifecycle (section 6). The runtime gets DISPLAY and DBUS_SESSION_BUS_ADDRESS in its own environment only.
set -eu

RUN_DIR=/tmp/tqa-desktop
mkdir -p "$RUN_DIR"
chmod 0700 "$RUN_DIR"
export XDG_RUNTIME_DIR="$RUN_DIR"
export DISPLAY=:0
export DBUS_SESSION_BUS_ADDRESS="unix:path=$RUN_DIR/bus"

# keep <name> <command...>: runs it in the background and starts it again a second after it exits, for the sandbox's
# life. A crash shows in the sandbox's log; the computer tool answers display_unavailable meanwhile.
keep() {
    name=$1
    shift
    (
        while :; do
            "$@" >/dev/null 2>&1 || true
            echo "tqa-desktop-init: $name exited; starting it again" >&2
            sleep 1
        done
    ) &
}

# wait_for <path> <what>: up to 10 s for a socket.
wait_for() {
    waited=0
    while [ ! -S "$1" ]; do
        if [ "$waited" -ge 100 ]; then
            echo "tqa-desktop-init: $2 didn't start" >&2
            exit 1
        fi
        sleep 0.1
        waited=$((waited + 1))
    done
}

# The framebuffer as a file in the sandbox's /run/tqa-display (Xvfb_screen0, XWD), which the runtime reads to see
# whether the screen changed and for screenshots, with no other tool in the image. Its root window is white (-wr, not
# Xvfb's black): a Take over's reset closes the browser and starts it again, and the bare root between the two was a
# black frame in the person's live picture (SBX-C-024); white is the blank page the browser comes back with.
keep display Xvfb :0 -screen 0 1280x800x24 -wr -fbdir /run/tqa-display -nolisten tcp -nolisten unix-abstract
wait_for /tmp/.X11-unix/X0 "the display"
keep bus dbus-daemon --session --nofork --nopidfile --address="$DBUS_SESSION_BUS_ADDRESS"
wait_for "$RUN_DIR/bus" "the session bus"
# The accessibility bus, and its flag set on the session bus, so toolkits and Chromium build their trees at once. One
# launcher only: the Set below D-Bus-activates it (at-spi2-core's org.a11y.Bus service), and the session bus starts it
# again on the next call if it ever exits. A launcher of our own under `keep` raced that activation: whichever lost the
# name exited, and ours was started again every second for the sandbox's life (c16, the Desktop's restart loop).
waited=0
until dbus-send --session --print-reply --dest=org.a11y.Bus /org/a11y/bus \
        org.freedesktop.DBus.Properties.Set string:org.a11y.Status string:IsEnabled variant:boolean:true \
        >/dev/null 2>&1; do
    if [ "$waited" -ge 100 ]; then
        echo "tqa-desktop-init: the accessibility bus didn't start" >&2
        exit 1
    fi
    sleep 0.1
    waited=$((waited + 1))
done
keep window-manager openbox
# Loopback only, no password; one connection per viewer (-shared -forever: a second viewer never drops the first).
# Nothing is ever copied out: no cut text is sent to a viewer (-noclipboard -noprimary, and -seldir recv: selections
# only received). A paste in is accepted, and the gateway passes one only from the person holding Take over (section 7).
keep vnc x11vnc -display :0 -localhost -rfbport 5900 -nopw -forever -shared -noprimary -noclipboard -seldir recv -quiet

# x11vnc 0.9.17 makes the window it owns the X selections with only about 10 s into a viewer's connection (no flag
# makes it sooner), and a paste it gets before then reaches no selection: ctrl+v in the browser got nothing (c16,
# 12 Oct 2026). So a throwaway viewer warms it up once, before the runtime starts and so before any person can view:
# loopback only, the handshake and nothing else (no input, no paste, no update requested, so no frames), gone as soon
# as that window appears, at most 20 s. It is told by its shape: x11vnc makes it nameless, 1x1, at +3+2, where the
# window manager's own nameless windows sit at +0+0 (any new window, as first written, also took those for it). Its failure never stops the sandbox. An x11vnc the loop restarts later isn't
# warmed again: a paste in its first seconds is lost, as one before this was (ADR-0039 section 6).
python3 - <<'WARM' || true
import socket, struct, subprocess, sys, time

def say(text):
    print("tqa-desktop-init: " + text, file=sys.stderr)

def exact(sock, count):
    data = b""
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise EOFError("the VNC server closed")
        data += chunk
    return data

def windows():
    """The nameless 1x1 windows at +3+2 under the root: x11vnc's selection windows."""
    listed = subprocess.run(["xwininfo", "-root", "-children"], capture_output=True, text=True, timeout=5).stdout
    return {line.split()[0] for line in listed.splitlines()
            if line.strip().startswith("0x") and "(has no name): ()" in line and " 1x1+3+2 " in line}

sock, deadline = None, time.monotonic() + 10
while sock is None and time.monotonic() < deadline:
    try:
        sock = socket.create_connection(("127.0.0.1", 5900), timeout=2)
    except OSError:
        time.sleep(0.2)
if sock is None:
    say("the VNC server isn't listening; it wasn't warmed up")
    sys.exit(0)
try:
    before = windows()
    exact(sock, 12)
    sock.sendall(b"RFB 003.008\n")
    exact(sock, exact(sock, 1)[0])
    sock.sendall(b"\x01")
    exact(sock, 4)
    sock.sendall(b"\x01")
    init = exact(sock, 24)
    exact(sock, struct.unpack(">I", init[20:24])[0])
    end = time.monotonic() + 20
    while not windows() - before:
        if time.monotonic() > end:
            say("the VNC server's selection window didn't appear in 20 s")
            break
        time.sleep(0.25)
except (OSError, EOFError, subprocess.SubprocessError) as error:
    say("the VNC server's warm-up failed ({})".format(type(error).__name__))
finally:
    sock.close()
WARM

# The probe image runs RT-I-046's probe on this display instead of the runtime.
if [ "${1:-}" = "--exec" ]; then
    shift
    exec "$@"
fi

# The runtime runs as this script's child, never in its place: it is a child-subreaper that kills every direct child
# it didn't start (process_cleanup), and with `exec` the loops above would be its children, so the first reap after a
# command would take the whole display down. As siblings they are never the runtime's, and anything of theirs left
# behind goes to tini. TERM and INT go on to the runtime (its own handlers stop it); when it ends, this script ends
# with its exact status and tini ends the sandbox, without waiting for the loops.
/usr/local/bin/tqa-runtime "$@" &
runtime=$!
forwarded=
trap 'forwarded=1; kill -TERM "$runtime" 2>/dev/null' TERM
trap 'forwarded=1; kill -INT "$runtime" 2>/dev/null' INT
# A signal cuts `wait` short (128 + the signal) while the runtime still runs, so it is waited for again. A wait that
# answers 127 after one was cut short means the runtime's status was already given; the status before it stands.
code=127
while :; do
    forwarded=
    status=0
    wait "$runtime" || status=$?
    if [ "$status" = 127 ] && [ "$code" != 127 ]; then
        break
    fi
    code=$status
    [ -n "$forwarded" ] || break
done
exit "$code"
