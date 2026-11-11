#!/usr/bin/env python3
"""The image builds' one way to download a file: python3 fetch.py URL DEST [URL DEST ...] (REL-U-012).

Each file is tried again after a network error: a refused or reset connection, a TLS connection dropped mid-body, a
try that stalls or crawls, a body cut short, or a 5xx or 429 answer. A few tries with a short pause between them, then
the build stops naming the URL, the tries and the last error. Any other answer (403, 404) is no network error and
stops at once. A file is written whole or not at all; the caller checks its SHA-256 after this, so a wrong file fails
without a new try.

When a try ends: a link that keeps moving finishes however long the file takes (a 150 MB archive at 200 KB/s runs
for over 12 minutes). A try ends only when no byte comes for STALL seconds (30), or, once it has run a whole WINDOW
(60 s), when the bytes of the last WINDOW came at less than MIN_RATE (50 KB/s) on average.
Standard library only: it runs in the build stages' Debian python3."""
import argparse
import collections
import http.client
import os
import socket
import ssl
import sys
import time
import urllib.error
import urllib.request

TRIES = 4
PAUSE = 5.0  # seconds before the second try; doubled before each later one
STALL = 30.0  # seconds a try waits for a byte (or for the answer's head) before it ends
WINDOW = 60.0  # seconds the rate is measured over
MIN_RATE = 50 * 1024  # bytes a second over the last WINDOW; a try that has run a whole WINDOW below it ends
CHUNK = 1 << 16


class Retry(Exception):
    """A network error: worth another try."""


def fetch_once(url, dest, stall, window, min_rate):
    """Downloads `url` to `dest`, or raises Retry (network) or another error (no retry); the module says when a try
    ends."""
    part = dest + ".part"
    try:
        with urllib.request.urlopen(url, timeout=stall) as answer, open(part, "wb") as out:
            expected = answer.headers.get("Content-Length")
            received = 0
            started = time.monotonic()
            # (time, bytes by then); the first entry is the last one at least a window old.
            seen = collections.deque([(started, 0)])
            while True:
                chunk = answer.read1(CHUNK)  # what has arrived, so the rate is checked as bytes trickle in
                if not chunk:
                    break
                out.write(chunk)
                received += len(chunk)
                now = time.monotonic()
                seen.append((now, received))
                while len(seen) > 1 and seen[1][0] <= now - window:
                    seen.popleft()
                if now - started >= window:
                    since, before = seen[0]
                    rate = (received - before) / (now - since)
                    if rate < min_rate:
                        raise Retry("too slow: {:.0f} B/s over the last {:g} s, under {:g} B/s".format(
                            rate, window, min_rate))
            if expected is not None and received != int(expected):
                raise Retry("the body ended after {} of {} bytes".format(received, expected))
        os.replace(part, dest)
    except urllib.error.HTTPError as error:
        if error.code >= 500 or error.code == 429:
            raise Retry("HTTP {} {}".format(error.code, error.reason)) from None
        raise RuntimeError("HTTP {} {}".format(error.code, error.reason)) from None
    except (urllib.error.URLError, http.client.HTTPException, socket.timeout, ConnectionError, TimeoutError,
            ssl.SSLError) as error:
        reason = getattr(error, "reason", None) or error
        raise Retry("{}: {}".format(type(reason).__name__, reason) if str(reason) else type(reason).__name__) from None
    finally:
        if os.path.exists(part):
            os.remove(part)


def fetch(url, dest, tries, pause, stall=STALL, window=WINDOW, min_rate=MIN_RATE):
    """Downloads with retries; returns None, or the message to stop the build with."""
    for attempt in range(1, tries + 1):
        try:
            fetch_once(url, dest, stall, window, min_rate)
            return None
        except Retry as error:
            if attempt == tries:
                return "download failed after {} tries: {}: {}".format(tries, url, error)
            wait = pause * 2 ** (attempt - 1)
            print("fetch: try {} of {} for {} failed ({}); trying again in {:g} s".format(
                attempt, tries, url, error, wait), file=sys.stderr, flush=True)
            time.sleep(wait)
        except (RuntimeError, OSError) as error:
            return "download failed: {}: {}".format(url, error)
    return None


def main(argv=None):
    parser = argparse.ArgumentParser(description="Download files, trying again after a network error.")
    parser.add_argument("--tries", type=int, default=TRIES)
    parser.add_argument("--pause", type=float, default=PAUSE)
    parser.add_argument("--stall", type=float, default=STALL, help="seconds a try waits for a byte")
    parser.add_argument("--window", type=float, default=WINDOW, help="seconds the rate is measured over")
    parser.add_argument("--min-rate", type=float, default=MIN_RATE, help="bytes a second a try must keep up")
    parser.add_argument("pairs", nargs="+", metavar="URL DEST")
    args = parser.parse_args(argv)
    if len(args.pairs) % 2 or args.tries < 1:
        parser.error("give each URL its destination, and at least one try")
    for url, dest in zip(args.pairs[::2], args.pairs[1::2]):
        problem = fetch(url, dest, args.tries, args.pause, args.stall, args.window, args.min_rate)
        if problem:
            print(problem, file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
