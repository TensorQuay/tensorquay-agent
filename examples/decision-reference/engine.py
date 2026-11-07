#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""A reference decision engine: routine steps go to a cheaper route, every other step to the task's own route.

This is a teaching example of the `decision/v1` protocol (ADR-0013). Copy it and change `choose_route`. It uses only
the Python standard library. It works at the lowest exposure, `none`: it reads the names and statuses of the previous
step's calls, never their arguments or the conversation.

The gateway posts JSON to two paths:
  POST /v1/before_step     -> answer one decision, here always {"decision": "call_model", ...}
  POST /v1/after_response  -> answer {"decision": "select", "call_ids": [...]}: which of the model's calls run

Run it:   python3 engine.py --port 8700 --cheap-route cheap
Check it: tools/ext-check decision http://127.0.0.1:8700
"""

import argparse
import hmac
import http.server
import json
import os
import sys
import threading
import unittest
import urllib.error
import urllib.request

# The largest request the gateway sends, and the largest answer it accepts, in bytes.
MAX_BYTES = 64 * 1024

# The product's tools that only read. A step after nothing but these (all successful) is "routine".
READ_ONLY_TOOLS = {"read_file", "glob", "grep", "read_notes"}


def choose_route(request, cheap_route, read_only):
    """The policy: the cheap route after a step whose calls all only read and all succeeded; otherwise the task's own.

    `request["routes"]` lists the routes this step may use, the task's own first. A route outside it is refused by
    the agent, so the cheap route is used only when it is listed.
    """
    own = request["routes"][0]["name"]
    allowed = {route["name"] for route in request["routes"]}
    previous = request["previous_calls"]
    routine = bool(previous) and all(
        call["name"] in read_only and call.get("status") == "ok" for call in previous
    )
    return cheap_route if routine and cheap_route in allowed else own


def decide(path, request, cheap_route, read_only):
    """The answer for one consultation. Every tool stays available: this engine only chooses the route."""
    if path == "/v1/before_step":
        tools = [tool["name"] for tool in request["tools"]]
        return {"decision": "call_model", "route": choose_route(request, cheap_route, read_only), "tools": tools}
    # After a response, keep every call the model made: the same as having no engine.
    return {"decision": "select", "call_ids": [call["id"] for call in request.get("response_calls", [])]}


def handler(cheap_route, read_only, key):
    """An HTTP handler bound to the engine's settings."""

    class Engine(http.server.BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802 (the standard library's name)
            if self.path not in ("/v1/before_step", "/v1/after_response"):
                return self.answer(404, {"error": "unknown path"})
            # With a key, the gateway sends it as a bearer token; compare it in constant time.
            if key and not hmac.compare_digest(self.headers.get("Authorization", ""), f"Bearer {key}"):
                return self.answer(401, {"error": "missing or wrong key"})
            length = int(self.headers.get("Content-Length") or 0)
            if length > MAX_BYTES:
                return self.answer(413, {"error": "request over 64 KiB"})
            try:
                request = json.loads(self.rfile.read(length))
                decision = decide(self.path, request, cheap_route, read_only)
            except (ValueError, KeyError, TypeError, IndexError):
                return self.answer(400, {"error": "not a decision/v1 request"})
            # One line per decision, without any content from the task.
            print(f"step {request.get('step')}: {decision['decision']} {decision.get('route', '')}", flush=True)
            return self.answer(200, decision)

        def answer(self, status, body):
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *_):  # the decision line above is the log
            pass

    return Engine


def serve(host, port, cheap_route, read_only, key):
    """A threaded server; each consultation is answered on its own, with no warm-up."""
    return http.server.ThreadingHTTPServer((host, port), handler(cheap_route, read_only, key))


class ReferenceEngineTests(unittest.TestCase):
    """The policy's rules, and the HTTP edges a gateway meets."""

    def request(self, previous, routes=("local", "cheap")):
        return {
            "point": "before_step", "run_id": "r-1", "step": 2, "attempt": 1,
            "routes": [{"name": name, "context_window": None, "max_output": None, "native_tools": True}
                       for name in routes],
            "tools": [{"name": "read_file", "description": "", "parameters": {}},
                      {"name": "shell", "description": "", "parameters": {}}],
            "budget": {"steps_used": 1, "max_steps": 30, "output_tokens_used": 0, "max_output_tokens": 1000},
            "previous_calls": previous,
        }

    def route(self, previous, **kwargs):
        return decide("/v1/before_step", self.request(previous, **kwargs), "cheap", READ_ONLY_TOOLS)["route"]

    def test_routine_steps_go_to_the_cheap_route_and_others_stay(self):
        ok = [{"name": "read_file", "status": "ok"}, {"name": "grep", "status": "ok"}]
        self.assertEqual(self.route(ok), "cheap")
        self.assertEqual(self.route([]), "local", "the first step")
        self.assertEqual(self.route([{"name": "read_file", "status": "error"}]), "local", "a failed call")
        self.assertEqual(self.route(ok + [{"name": "shell", "status": "ok"}]), "local", "a command")
        self.assertEqual(self.route([{"name": "read_file"}]), "local", "no status")
        self.assertEqual(self.route(ok, routes=("local",)), "local", "the cheap route isn't allowed")

    def test_every_tool_stays_and_every_call_runs(self):
        answer = decide("/v1/before_step", self.request([]), "cheap", READ_ONLY_TOOLS)
        self.assertEqual(answer["tools"], ["read_file", "shell"])
        selected = decide("/v1/after_response", {"response_calls": [{"id": "c1"}, {"id": "c2"}]}, "", set())
        self.assertEqual(selected, {"decision": "select", "call_ids": ["c1", "c2"]})
        self.assertEqual(decide("/v1/after_response", {}, "", set())["call_ids"], [])

    def test_http_edges(self):
        server = serve("127.0.0.1", 0, "cheap", READ_ONLY_TOOLS, "synthetic-key")
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{server.server_address[1]}"

        def post(path, body, key="synthetic-key"):
            request = urllib.request.Request(base + path, data=body, method="POST",
                                             headers={"Authorization": f"Bearer {key}"})
            try:
                with urllib.request.urlopen(request, timeout=5) as response:
                    return response.status, json.loads(response.read())
            except urllib.error.HTTPError as error:
                return error.code, None

        try:
            status, answer = post("/v1/before_step", json.dumps(self.request([])).encode())
            self.assertEqual((status, answer["route"]), (200, "local"))
            self.assertEqual(post("/v1/before_step", b"{}", key="wrong")[0], 401)
            self.assertEqual(post("/v1/before_step", b"not json")[0], 400)
            self.assertEqual(post("/v1/before_step", b"x" * (MAX_BYTES + 1))[0], 413)
            self.assertEqual(post("/v1/other", b"{}")[0], 404)
        finally:
            server.shutdown()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8700)
    parser.add_argument("--cheap-route", default="cheap", help="the declared route for routine steps")
    parser.add_argument("--read-only", action="append", default=[], metavar="TOOL",
                        help="another tool that only reads, such as an MCP search tool; repeatable")
    parser.add_argument("--bearer-from-env", metavar="VAR", help="require this environment variable's value as the key")
    parser.add_argument("--self-test", action="store_true", help="run the engine's own tests")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(ReferenceEngineTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    key = os.environ.get(args.bearer_from_env, "") if args.bearer_from_env else ""
    if args.bearer_from_env and not key:
        print(f"engine: {args.bearer_from_env} is unset or empty", file=sys.stderr)
        return 2
    server = serve(args.host, args.port, args.cheap_route, READ_ONLY_TOOLS | set(args.read_only), key)
    print(f"engine: listening on {args.host}:{server.server_address[1]}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
