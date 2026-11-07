#!/bin/sh
# ADR-0039 slice 1: the spike image's init. It starts the virtual display before the runtime, as the sandbox's own
# user, and hands the runtime DISPLAY in its own environment only (the image declares only PATH and HOME). The
# Desktop template's init (slice 2) replaces this one and adds the window manager, the session bus and VNC.
set -eu
Xvfb :0 -screen 0 1280x800x24 -nolisten tcp -nolisten unix-abstract >/dev/null 2>&1 &
waited=0
while [ ! -S /tmp/.X11-unix/X0 ]; do
    if [ "$waited" -ge 100 ]; then
        echo "tqa-desktop-init: the display didn't start" >&2
        exit 1
    fi
    sleep 0.1
    waited=$((waited + 1))
done
export DISPLAY=:0
# The probe image runs RT-I-046's probe on the display instead of the runtime.
if [ "${1:-}" = "--exec" ]; then
    shift
    exec "$@"
fi
exec /usr/local/bin/tqa-runtime "$@"
