"""SEC-013 on the microVM (crates/tqa-sandbox/tests/egress/connector_isolation.py with --backend firecracker): the parts that differ from the Docker
cases, which search and probe the sandbox with `docker exec`. A guest has none, so a scripted shell turn after the
call runs guest_search.py (given hashes only) and, for SEC-013, asks for the connector outside the task's set
with the runtime token, read inside the guest. SEC-013's traffic capture sits in the VM host's network namespace, where
the guest's tap is; the host never learns the runtime token, so the capture's positive control is the stream carrying
the call with its authorization header.
"""
import guest_search

OUTSIDE_MARK = "outside-the-set: "


def outside_probe(other, gateway_url):
    """Shell that asks for the connector `other` with the runtime token and prints its status and answer, marked. The
    gateway's URL is given: the shell tool clears the environment, so the runtime's TQA_GATEWAY_URL is not there."""
    body = ('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25",'
            '"capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}')
    # --noproxy: straight to the gateway's link port, as the runtime reaches it; the tool's environment may carry
    # HTTP_PROXY without NO_PROXY, and the egress proxy would answer instead of the gateway.
    return ('status=$(curl -s --noproxy "*" -o /tmp/outside-answer -w "%{http_code}" -X POST '
            '-H "Authorization: Bearer $(cat /run/tqa/token)" -H "Content-Type: application/json" '
            '-H "Accept: application/json, text/event-stream" '
            "--data '" + body + "' \"" + gateway_url.rstrip("/") + "/mcp/" + other + "\")\n"
            'printf "' + OUTSIDE_MARK + '%s %s\\n" "$status" "$(head -c 600 /tmp/outside-answer)"\n'
            "rm -f /tmp/outside-answer\n")


def turn(event):
    return {"events": [event], "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


def connector_scenario(tool, arguments, labelled, other, gateway_url):
    """The connector call, then the in-guest search (with its planted control) and the refusal probe, then a
    closing answer. `labelled`: the (label, form) pairs to search for, given to the guest as hashes only."""
    command = guest_search.shell(labelled) + outside_probe(other, gateway_url)
    return [turn({"type": "tool_call", "name": tool, "arguments": arguments, "split_into": 1}),
            turn({"type": "tool_call", "name": "shell", "arguments": {"command": command}, "split_into": 1}),
            turn({"type": "text", "text": "Created issue #7 in the tracker."})]


def outside_answer(output):
    """(status, answer) from the probe's marked line."""
    for line in output.splitlines():
        if line.startswith(OUTSIDE_MARK):
            status, _, answer = line[len(OUTSIDE_MARK):].partition(" ")
            return status, answer
    raise AssertionError("the refusal probe printed nothing: " + output[-300:])


def saw_the_call(flows, connector):
    """Whether one reassembled stream carries the call to the connector with its authorization header."""
    path = b"/mcp/" + connector.encode()
    return any(path in data and b"tools/call" in data and b"authorization: bearer " in data.lower()
               for data in flows.values())
