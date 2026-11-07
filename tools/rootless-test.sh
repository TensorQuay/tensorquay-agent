#!/usr/bin/env bash
# Run a Docker test command inside a rootless daemon's user namespace. There the invoking user is uid 0 and may
# assign files to container uids, as the containerised gateway does for the sandbox user (uid 1000). A rootful or
# unidentified daemon is refused: nothing is relaxed, and those hosts run the suites as documented.
set -euo pipefail

if [[ $# -eq 0 ]]; then
    printf 'Usage: %s <command> [arguments...]\n' "$0" >&2
    exit 2
fi

fail() {
    printf 'rootless-test: %s\n' "$1" >&2
    exit 1
}

# Rootless Docker's own default when the session variable is absent.
runtime_dir=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
socket="unix://$runtime_dir/docker.sock"
# The namespace is taken from this user's daemon; the Docker client must address the same daemon.
[[ ${DOCKER_HOST:-$socket} == "$socket" ]] || fail "DOCKER_HOST does not name this user's rootless daemon ($socket)"
options=$(DOCKER_HOST=$socket docker info --format '{{json .SecurityOptions}}') || fail "the Docker daemon is unreachable"
[[ $options == *'"name=rootless"'* ]] || fail "the Docker daemon is not rootless"
pid=$(<"$runtime_dir/docker.pid") || fail "the rootless daemon's pid file is unreadable"
[[ $pid =~ ^[1-9][0-9]*$ ]] || fail "the rootless daemon's pid file is malformed"

export DOCKER_HOST=$socket
# Opt-in, for tests that serve a container from the test process (the tqa-runtime Docker suites' fake gateway): a rootless
# daemon runs its containers in a detached network namespace with --disable-host-loopback, so `host-gateway` there never
# reaches a listener in the host's namespace. TQA_ROOTLESS_NETNS=daemon also joins the containers' namespace. Off by
# default: a test that needs the host's loopback (the database on 127.0.0.1) must not use it.
case ${TQA_ROOTLESS_NETNS:-} in
    "") exec nsenter --user --preserve-credentials --target "$pid" -- "$@" ;;
    daemon)
        # The namespace file under the state directory is a mount only rootlesskit's mount namespace sees; its child
        # process lives in the containers' network namespace, so join through the child.
        child_file=$runtime_dir/dockerd-rootless/child_pid
        [[ -r $child_file ]] || fail "the rootless daemon has no detached network namespace (no $child_file)"
        child=$(<"$child_file")
        [[ $child =~ ^[1-9][0-9]*$ && -e /proc/$child/ns/net ]] || fail "the rootless daemon has no detached network namespace (child $child)"
        exec nsenter --user --preserve-credentials --target "$pid" --net="/proc/$child/ns/net" -- "$@"
        ;;
    *) fail "TQA_ROOTLESS_NETNS must be unset or 'daemon'" ;;
esac
