"""E2E-PROFILE-007's fixtures and checks, without a stack: the connector's MCP server, the gateway configuration, the
profile, the fake model's replies, and what the journal must show.

The install enables two MCP sources by default: the catalogue server `words` (E2E-MCP-006's) and the connector `tracker`.
The profile allows one built-in tool, `read_file`, and one connector, `tracker`, and no catalogue server. The fake model
first calls `shell`, which the profile leaves out, then the connector's `lookup`, then answers.
"""

import ipaddress
import json

from marker import marked

PROFILE_NAME = "restricted"
ISSUE = "T-1"
ANSWER = "T-1: open, assigned to the release team"
# The connector's answer as the model gets it: marked as untrusted content (SEC-040).
MARKED_ANSWER = marked("tracker", "lookup", ANSWER)
# Everything the run may call: the built-in the profile allows and the connector's one tool; sorted as the registry lists.
KNOWN = "mcp.tracker.lookup, read_file"
REFUSED_SHELL = "unsupported: unknown tool; known tools: " + KNOWN
CONNECTOR_PORT = 8080

# A streamable-HTTP MCP server on the standard library: initialize, tools/list and tools/call as JSON replies; a
# notification is accepted with 202; GET (no event stream) is 405. It prints `listening` once it accepts connections.
CONNECTOR = r'''
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
TOOLS = [{"name": "lookup", "description": "Look up an issue by its id",
          "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}}]
ISSUES = {"T-1": "T-1: open, assigned to the release team"}
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *arguments):
        pass
    def answer(self, status, body=None):
        data = b"" if body is None else json.dumps(body).encode()
        self.send_response(status)
        if body is not None:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_GET(self):
        self.answer(405)
    def do_DELETE(self):
        self.answer(204)
    def do_POST(self):
        if self.path != "/mcp":
            return self.answer(404)
        length = int(self.headers.get("Content-Length", "0"))
        message = json.loads(self.rfile.read(length) or b"null")
        if not isinstance(message, dict) or "id" not in message:
            return self.answer(202)
        method = message.get("method")
        if method == "initialize":
            result = {"protocolVersion": "2025-11-25", "capabilities": {"tools": {}},
                      "serverInfo": {"name": "tracker", "version": "1"}}
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "tools/call":
            arguments = (message.get("params") or {}).get("arguments") or {}
            result = {"content": [{"type": "text", "text": ISSUES.get(arguments.get("id"), "no such issue")}]}
        else:
            result = {}
        self.answer(200, {"jsonrpc": "2.0", "id": message["id"], "result": result})
server = HTTPServer(("0.0.0.0", int(sys.argv[1])), Handler)
print("listening", flush=True)
server.serve_forever()
'''


def gateway_config(connector_ip, words_server):
    """The catalogue server and the connector, both enabled by default; the connector named by its address, since the
    proxy refuses a name that resolves to a private address (SEC-024) but accepts an address the file names itself."""
    address = ipaddress.ip_address(connector_ip)
    if address.version != 4:
        raise ValueError("the connector's address must be IPv4")
    return ("\n[[mcp.servers]]\nname='words'\ncommand='/usr/bin/python3'\nargs=" + json.dumps(["-c", words_server])
            + "\nenabled_by_default=true\ncall_timeout_seconds=30\n"
            + "\n[[connectors]]\nname='tracker'\nurl='http://" + str(address) + ":" + str(CONNECTOR_PORT) + "/mcp'\n"
            + "insecure_http=true\nenabled_by_default=true\n")


def profile():
    """A personal profile: one built-in tool, one connector, no catalogue server, and its own instructions."""
    return {"name": PROFILE_NAME, "display_name": "Restricted", "model_route": "local",
            "instructions": "Answer from the tracker only.", "tools": ["read_file"], "skills": "none",
            "mcp_servers": [], "connectors": ["tracker"]}


def replies():
    """The fake model: a tool outside the profile, then the connector's tool, then the answer."""
    usage = {"prompt_tokens": 10, "completion_tokens": 10}
    return {"model": "fixture", "responses": [
        {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": "ls"}}],
         "usage": usage},
        {"events": [{"type": "tool_call", "name": "mcp__tracker__lookup", "split_into": 1,
                     "arguments": {"id": ISSUE}}],
         "usage": usage},
        {"events": [{"type": "text", "text": "T-1 is open and assigned to the release team."}], "usage": usage},
    ]}


def verify(conversation, stored):
    """The conversation took the profile's sets, and the run completed using only the tools the profile allows."""
    assert conversation.get("profile", {}).get("name") == PROFILE_NAME, conversation
    assert (conversation["mcp_servers"], conversation["connectors"]) == ([], ["tracker"]), conversation
    events = [frame["event"] for frame in stored if frame.get("type") == "event"]
    kinds = [event["type"] for event in events]
    servers = [event for event in events if event["type"] == "mcp.server"]
    assert servers == [{"type": "mcp.server", "name": "tracker", "state": "ready", "tools": 1}], servers
    calls = [event["name"] for event in events if event["type"] == "tool.call"]
    assert calls == ["shell", "mcp.tracker.lookup"], calls
    results = [(event["status"], event["content"].strip()) for event in events if event["type"] == "tool.result"]
    assert results == [("error", REFUSED_SHELL), ("ok", MARKED_ANSWER)], results
    assert kinds and kinds[-1] == "run.completed", kinds[-5:]
    return {"calls": calls, "results": [status for status, _ in results], "servers": [server["name"] for server in servers]}
