"""GW-I-314 on a real backend: a sandbox's idle keep-alive connection to the gateway, cut off by the soak's own link_cut
primitive (its networks disconnected) and then killed, so no FIN or RST ever reaches the gateway, is freed by the
gateway within the bound: its socket leaves the gateway's table and the gateway's descriptors, less its database
connections, are back to their level without the sandbox. Before TCP keepalive on accepted sockets (GW-U-205) the
gateway held such a connection for good (the 16x soak's lasting step). This module holds the scenario, the parsing and
the checks; robustness.py runs them on the Docker stack.
"""
import ipaddress
import json

# The run only has to bring the sandbox up and finish; the case then works on the idle sandbox.
SCENARIO = {"model": "fixture", "responses": [
    {"events": [{"type": "text", "text": "Ready."}], "usage": {"prompt_tokens": 10, "completion_tokens": 5}}]}
SETTINGS = ""

# The gateway's keepalive on accepted sockets: the first probe after 60 s idle, then every 10 s, closed after 3.
KEEPALIVE_SECONDS = 60 + 3 * 10
# The held connection's own bound from the cut: its last packet came before the cut, plus a loaded host's margin.
HELD_SECONDS = KEEPALIVE_SECONDS + 30
# Everything the sandbox held, from the cut: the runtime link closes on its own liveness (a ping every 30 s, closed
# 90 s after the last pong), so up to 120 s, plus the margin.
ALL_SECONDS = 30 + 90 + 30
POLL_SECONDS = 5
GATEWAY_PORT = 8080

# Run in the sandbox: one HTTP/1.1 keep-alive request to the gateway, answered, then the connection left idle, as a
# runtime's pooled connection is. Its local port is written to the workspace so the case finds its socket by name.
PORT_FILE = "/workspace/.halfopen-port"
CLIENT = ("import socket, time\n"
          "s = socket.create_connection(('gateway', {port}))\n"
          "s.sendall(b'GET /healthz HTTP/1.1\\r\\nHost: gateway\\r\\n\\r\\n')\n"
          "assert s.recv(65536).startswith(b'HTTP/1.1 200')\n"
          "open('{file}', 'w').write(str(s.getsockname()[1]))\n"
          "time.sleep(3600)\n").format(port=GATEWAY_PORT, file=PORT_FILE)

# Run by the soak's observer in the gateway's PID namespace: its open descriptors and its network namespace's TCP
# tables, as one JSON line.
OBSERVER = ("import json, os; from pathlib import Path; "
            "print(json.dumps({'fds': len(os.listdir('/proc/1/fd')), "
            "'tcp': Path('/proc/1/net/tcp').read_text(), 'tcp6': Path('/proc/1/net/tcp6').read_text()}))")

LISTEN = "0A"


def address(field):
    """A /proc/net/tcp{,6} `address:port` field as (address text, port); an IPv4-mapped IPv6 address as IPv4."""
    host, port = field.split(":")
    raw = bytes.fromhex(host)
    if len(raw) == 4:
        parsed = ipaddress.IPv4Address(raw[::-1])
    else:
        # Four 32-bit words, each in host (little-endian) order.
        parsed = ipaddress.IPv6Address(b"".join(raw[i:i + 4][::-1] for i in range(0, 16, 4)))
        parsed = parsed.ipv4_mapped or parsed
    return str(parsed), int(port, 16)


def sockets(table):
    """The rows of a /proc/net/tcp{,6} text as dicts: local and remote (address, port), state, and inode."""
    rows = []
    for line in table.splitlines()[1:]:
        fields = line.split()
        if len(fields) >= 10:
            rows.append({"local": address(fields[1]), "remote": address(fields[2]), "state": fields[3],
                         "inode": fields[9]})
    return rows


def sample(observed, gateway_connections):
    """The observer's JSON line and the database's count of the gateway's connections, as one sample."""
    data = json.loads(observed)
    assert isinstance(data.get("fds"), int) and data["fds"] > 0, "empty process measurement"
    return {"fds": data["fds"], "database": gateway_connections,
            "sockets": sockets(data["tcp"]) + sockets(data["tcp6"])}


def held(snapshot, peer):
    """The gateway's sockets on its port whose remote end is `peer` (address, port)."""
    return [row for row in snapshot["sockets"]
            if row["state"] != LISTEN and row["local"][1] == GATEWAY_PORT and row["remote"] == peer]


def from_sandbox(snapshot, addresses):
    """Every gateway socket whose remote end is one of the sandbox's addresses, on any port."""
    return [row for row in snapshot["sockets"] if row["state"] != LISTEN and row["remote"][0] in addresses]


def level(snapshot, addresses):
    """The gateway's descriptors less its database connections and the sandbox's sockets: what it holds without them."""
    return snapshot["fds"] - snapshot["database"] - len(from_sandbox(snapshot, addresses))


def check_before(snapshot, peer, addresses):
    """Before the cut: the idle connection is there, established, and the level without the sandbox is known."""
    rows = held(snapshot, peer)
    assert len(rows) == 1 and rows[0]["state"] == "01", \
        "the sandbox's idle keep-alive connection isn't established at the gateway: {}".format(rows)
    return level(snapshot, addresses)


def freed(snapshot, peer, addresses, baseline):
    """Whether the held connection is gone, and whether everything the sandbox held is gone and the level is back."""
    return not held(snapshot, peer), not from_sandbox(snapshot, addresses) and \
        snapshot["fds"] - snapshot["database"] <= baseline


def watch(measure, peer, addresses, baseline, clock, sleep):
    """Samples from the cut until the sandbox's sockets are all gone and the level is back, or the bound passes.
    Returns the seconds to the held connection's end and to everything's end (None where it never came) and the last
    sample."""
    started, connection, everything = clock(), None, None
    while True:
        snapshot = measure()
        gone, all_gone = freed(snapshot, peer, addresses, baseline)
        seconds = clock() - started
        if gone and connection is None:
            connection = seconds
        if all_gone and gone:
            everything = seconds
            return connection, everything, snapshot
        if seconds >= ALL_SECONDS:
            return connection, everything, snapshot
        sleep(POLL_SECONDS)


# What a sandbox's exit code points at (the lead's reading, 20 Oct 15:49).
EXIT_MEANINGS = {"0": "a deliberate exit", "1": "a crash", "137": "killed: SIGKILL, the case's kill or memory"}


def container_id(item):
    """A container event's id: its actor's (Docker 28 dropped the deprecated top-level "id"), else the old field."""
    return (item.get("Actor") or {}).get("ID") or item.get("id")


def exits(lines, tails):
    """Each of the task's sandboxes from its `docker events --format '{{json .}}'` start and die lines, in order of
    first sight: how long it lived, its exit code and what that points at, and its last log lines (`tails`, by
    container id) where they were read before the gateway removed it."""
    seen = {}
    for line in lines:
        try:
            item = json.loads(line)
        except ValueError:
            continue
        ident = container_id(item)
        if item.get("Type") != "container" or item.get("Action") not in ("start", "die") or not ident:
            continue
        record = seen.setdefault(ident, {})
        record[item["Action"]] = item["timeNano"] / 1e9
        if item["Action"] == "die":
            record["code"] = item["Actor"]["Attributes"].get("exitCode", "?")
    out = []
    for ident, record in seen.items():
        if "die" not in record:
            life = "started, no die seen"
        else:
            lived = "started, died {:.1f} s later".format(record["die"] - record["start"]) if "start" in record else \
                "start not seen, died"
            life = "{}, exit {} ({})".format(lived, record["code"], EXIT_MEANINGS.get(record["code"], "see its log"))
        tail = " | ".join(tails.get(ident) or []) or "none read"
        out.append("sandbox {}: {}; last log lines: {}".format(ident[:12], life, tail))
    return out or ["no sandbox start or die events seen for the task"]


def report(connection, everything, snapshot, peer, addresses, baseline):
    """What the watch saw, as one line printed before any later check, so a run stopped after the watch still shows its
    figures."""
    ended = "ended {:.0f} s after the cut".format(connection) if connection is not None else \
        "not ended within {} s".format(ALL_SECONDS)
    gone = "all gone by {:.0f} s (bound {} s)".format(everything, ALL_SECONDS) if everything is not None else \
        "not all gone by {0} s (bound {0} s): {1} left".format(ALL_SECONDS, len(from_sandbox(snapshot, addresses)))
    return "GW-I-314 watch: the idle connection {} (bound {} s); the sandbox's sockets {}; descriptors less the " \
           "database {} (level without the sandbox {})".format(
               ended, HELD_SECONDS, gone, snapshot["fds"] - snapshot["database"], baseline)


def check(connection, everything, snapshot, peer, addresses, baseline):
    """GW-I-314: the held connection ends within its bound, and everything the sandbox held within the link's."""
    assert connection is not None, \
        "the gateway still holds the sandbox's idle connection {} s after the cut: {}".format(
            ALL_SECONDS, held(snapshot, peer))
    assert connection <= HELD_SECONDS, \
        "the held connection ended {:.0f} s after the cut (bound {} s)".format(connection, HELD_SECONDS)
    assert everything is not None, \
        "{} s after the cut: sockets from the sandbox {}, descriptors less the database {} (level without the " \
        "sandbox {})".format(ALL_SECONDS, from_sandbox(snapshot, addresses), snapshot["fds"] - snapshot["database"],
                             baseline)
    return "GW-I-314: the cut-off, killed sandbox's idle keep-alive connection was freed {:.0f} s after the cut " \
           "(bound {} s), and everything it held by {:.0f} s (bound {} s); the gateway's descriptors less the " \
           "database are back to {}".format(connection, HELD_SECONDS, everything, ALL_SECONDS, baseline)


# GW-I-318, the second half-open case: the link comes back. A sandbox's idle keep-alive connection, its networks cut
# (the soak's link_cut) and reconnected with the same address after RECONNECT_AFTER_SECONDS, before the gateway's first
# keepalive probe is due. The watch runs past the probes' whole span, so they come after the reconnect, are answered,
# and nothing may close the connection: it stays established to KEPT_SECONDS after the cut, then answers a second
# request on the same socket. Red if keepalive, or a later idle timeout, closes a live peer.
RECONNECT_AFTER_SECONDS = 30
KEPT_SECONDS = 150
# The sandbox outlives the case whatever the default idle window: the gateway mustn't retire it mid-watch.
RECONNECT_SETTINGS = "idle_seconds=900\n"
ASK_FILE = "/workspace/.halfopen-ask"
ANSWER_FILE = "/workspace/.halfopen-answer"

# As CLIENT, then: when ASK_FILE appears, a second request on the same socket, its status line (or the error) written to
# ANSWER_FILE.
RECONNECT_CLIENT = ("import os, socket, time\n"
                    "s = socket.create_connection(('gateway', {port}))\n"
                    "s.sendall(b'GET /healthz HTTP/1.1\\r\\nHost: gateway\\r\\n\\r\\n')\n"
                    "assert s.recv(65536).startswith(b'HTTP/1.1 200')\n"
                    "open('{port_file}', 'w').write(str(s.getsockname()[1]))\n"
                    "while not os.path.exists('{ask}'):\n"
                    "    time.sleep(1)\n"
                    "try:\n"
                    "    s.settimeout(15)\n"
                    "    s.sendall(b'GET /healthz HTTP/1.1\\r\\nHost: gateway\\r\\n\\r\\n')\n"
                    "    answer = s.recv(65536).split(b'\\r\\n', 1)[0].decode('latin-1')\n"
                    "except OSError as error:\n"
                    "    answer = 'error: ' + str(error)\n"
                    "open('{answer}', 'w').write(answer or 'error: closed')\n"
                    "time.sleep(3600)\n").format(port=GATEWAY_PORT, port_file=PORT_FILE, ask=ASK_FILE,
                                                 answer=ANSWER_FILE)

ESTABLISHED = "01"


def check_restored(before, after):
    """The reconnected sandbox has its addresses back (network name to address); else the case tests nothing, since a
    connection to a changed address can't survive, whatever the gateway does."""
    assert after and all(after.get(name) == address for name, address in before.items()), \
        "the reconnect tests nothing: the sandbox's address changed ({} -> {})".format(before, after)


def watch_kept(measure, peer, clock, sleep, reconnect=None):
    """Samples from the cut to KEPT_SECONDS, calling `reconnect` once RECONNECT_AFTER_SECONDS have passed. Stops at the
    first sample where the connection isn't established. Returns what it saw: when it was lost (None if never), its
    last state, the samples taken and the seconds watched."""
    started, reconnected, count, state = clock(), reconnect is None, 0, ESTABLISHED
    while True:
        seconds = clock() - started
        if not reconnected and seconds >= RECONNECT_AFTER_SECONDS:
            reconnect()
            reconnected = True
            seconds = clock() - started
        rows = held(measure(), peer)
        count += 1
        state = rows[0]["state"] if rows else None
        if state != ESTABLISHED:
            return {"lost_at": seconds, "last_state": state, "samples": count, "watched": seconds}
        if seconds >= KEPT_SECONDS:
            return {"lost_at": None, "last_state": state, "samples": count, "watched": seconds}
        sleep(POLL_SECONDS)


def answered(answer):
    return str(answer or "").startswith("HTTP/1.1 200")


def report_kept(seen, answer):
    """What the watch and the second request saw, as one line printed before the check."""
    if seen["lost_at"] is None:
        watch = "stayed established in all {} samples to {:.0f} s after the cut".format(
            seen["samples"], seen["watched"])
    else:
        watch = "was gone {:.0f} s after the cut (state {})".format(seen["lost_at"], seen["last_state"] or "none left")
    return "GW-I-318 watch: the idle connection, its link reconnected after {} s, {}; the second request: {}".format(
        RECONNECT_AFTER_SECONDS, watch, answer or "no answer")


def check_kept(seen, answer):
    """GW-I-318: the connection outlives the cut and keepalive's probes, and still answers on the same socket."""
    assert seen["lost_at"] is None, \
        "the gateway closed a live peer's connection {:.0f} s after the cut (its link back after {} s; " \
        "state {})".format(seen["lost_at"], RECONNECT_AFTER_SECONDS, seen["last_state"] or "none left")
    assert answered(answer), "the second request on the same socket got {}".format(answer or "no answer")
    return "GW-I-318: the sandbox's idle keep-alive connection, its link cut and reconnected after {} s, stayed " \
           "established for {} s after the cut (past keepalive's {} s) and answered a second request on the same " \
           "socket".format(RECONNECT_AFTER_SECONDS, KEPT_SECONDS, KEEPALIVE_SECONDS)
