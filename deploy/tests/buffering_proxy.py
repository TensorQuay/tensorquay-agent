"""A proxy that buffers the event stream's tail (WEB-E2E-121): what a reverse proxy that buffers responses does to a
task's live updates. It stands between the browser and the gateway, passes every request and answer through, and for a
live event stream (`stream=1`, `text/event-stream`) forwards only the first bytes, then holds the rest for a while
before letting it go. Everything else, the paged read of the same route included, passes whole and at once. Standard
library only, so it runs in the runtime's own image; no upgrade (WebSocket) is proxied: 501."""
import collections
import http.client
import http.server
import os
import socketserver
import sys
import time
import urllib.parse

HOP_BY_HOP = {"connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer",
              "transfer-encoding", "upgrade"}
Settings = collections.namedtuple("Settings", "upstream pass_bytes hold_seconds port")


def settings(env):
    return Settings(env.get("TQA_PROXY_UPSTREAM") or "gateway:8080", int(env.get("TQA_PROXY_PASS_BYTES") or 2048),
                    int(env.get("TQA_PROXY_HOLD_SECONDS") or 40), int(env.get("TQA_PROXY_PORT") or 8080))


def hold_plan(path, headers, pass_bytes, hold_seconds):
    """(bytes to pass, seconds to hold) for a live event stream; None for an answer that passes whole."""
    kind = next((value for name, value in headers.items() if name.lower() == "content-type"), "")
    query = urllib.parse.parse_qs(urllib.parse.urlsplit(path).query)
    if kind.lower().startswith("text/event-stream") and query.get("stream") == ["1"]:
        return (pass_bytes, hold_seconds)
    return None


def held_line(path, plan, passed):
    """The one log line a hold leaves, which the suite's check (demo.verify_catch_up) looks for."""
    return "buffering proxy: held {} for {} s after {} bytes".format(path, plan[1], passed)


def forwardable(headers):
    return {name: value for name, value in headers.items() if name.lower() not in HOP_BY_HOP}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    config = settings(os.environ)

    def log_message(self, fmt, *args):  # quiet: the stack's logs are the gateway's
        pass

    def handle_one(self):
        if "upgrade" in self.headers.get("Connection", "").lower():
            self.send_error(501, "no upgrade through this proxy")
            return
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else None
        host, _, port = self.config.upstream.partition(":")
        upstream = http.client.HTTPConnection(host, int(port or 80), timeout=600)
        request_headers = forwardable(dict(self.headers.items()))
        request_headers["Connection"] = "close"
        upstream.request(self.command, self.path, body=body, headers=request_headers)
        answer = upstream.getresponse()
        self.send_response(answer.status, answer.reason)
        response_headers = forwardable(dict(answer.getheaders()))
        streamed = "Content-Length" not in {k.title() for k in response_headers}
        for name, value in response_headers.items():
            self.send_header(name, value)
        if streamed:
            self.send_header("Transfer-Encoding", "chunked")
        self.send_header("Connection", "close")
        self.end_headers()
        plan = hold_plan(self.path, response_headers, self.config.pass_bytes, self.config.hold_seconds)
        passed, held = 0, False
        while True:
            chunk = answer.read1(4096) if hasattr(answer, "read1") else answer.read(4096)
            if not chunk:
                break
            if plan and not held and passed + len(chunk) > plan[0]:
                head, chunk = chunk[:max(plan[0] - passed, 0)], chunk[max(plan[0] - passed, 0):]
                if head:
                    self.write_chunk(head, streamed)
                print(held_line(self.path, plan, passed + len(head)), file=sys.stderr, flush=True)
                time.sleep(plan[1])  # the tail is held: the browser hears nothing meanwhile
                held = True
            self.write_chunk(chunk, streamed)
            passed += len(chunk)
        if streamed:
            self.wfile.write(b"0\r\n\r\n")
        self.wfile.flush()
        upstream.close()

    def write_chunk(self, data, streamed):
        if streamed:
            self.wfile.write(("%x\r\n" % len(data)).encode() + data + b"\r\n")
        else:
            self.wfile.write(data)
        self.wfile.flush()

    def do_GET(self):  # noqa: N802
        self.handle_one()

    do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = do_OPTIONS = do_GET


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main():
    config = settings(os.environ)
    Handler.config = config
    print("buffering proxy: {} -> {}, passes {} bytes of a live stream then holds {} s".format(
        config.port, config.upstream, config.pass_bytes, config.hold_seconds), file=sys.stderr, flush=True)
    Server(("0.0.0.0", config.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
