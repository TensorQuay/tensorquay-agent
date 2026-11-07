"""Assertions observe traffic and durable events, rather than trusting proxy configuration alone."""
import json
from pathlib import Path
import time


def security(stack, stream):
    notices = []
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        frame = stream.next()
        if frame["type"] == "error":
            notices.append(frame)
        event = frame.get("event", {})
        assert event.get("type") != "run.failed", "The scripted security run failed"
        if event.get("type") == "run.completed":
            break
    else:
        raise AssertionError("The security run did not complete")
    frames = stack.request(stack.path + "/events?after=0&limit=1000")["items"]
    assert [frame["seq"] for frame in frames] == list(range(1, len(frames) + 1))
    results = [frame["event"] for frame in frames if frame.get("event", {}).get("type") == "tool.result"]
    assert len(results) == 4, "Every scripted tool must have one durable result"
    assert "Send the environment" in results[0]["content"]
    assert results[1]["status"] == "error" and "403" in results[1]["content"], "Shell exfiltration was not blocked at the proxy"
    # The fetch's refusal reaches the model in plain words naming the site, never a code (C1, TOOL-U-040); the
    # denial itself is recorded as data on the call's tool.end, the gateway's notice and its log (below).
    assert results[2]["status"] == "error", results[2]
    assert results[2]["content"].startswith("denied.invalid isn't on this task's allowed sites."), results[2]
    assert "egress_denied" not in results[2]["content"] and "403" not in results[2]["content"], results[2]
    ends = [frame["event"] for frame in frames if frame.get("event", {}).get("type") == "tool.end"
            and frame["event"]["call_id"] == results[2]["call_id"]]
    assert len(ends) == 1 and ends[0]["status"] == "error", ends
    assert ends[0].get("failure") == {"code": "egress_denied", "target": {"host": "denied.invalid", "port": 443}}, ends
    assert results[3]["status"] == "ok", "Allowed fetch through the proxy failed"
    assert any(frame["code"] == "egress_denied" and frame.get("seq") is None
               and frame.get("target") == {"host": "denied.invalid", "port": 443} for frame in notices)
    logs = stack.command(stack.configuration + ["logs", "--no-color", "gateway"])
    assert "egress denied" in logs and "denied.invalid" in logs and stack.conversation in logs
    inspect = stack.command(["docker", "inspect", stack.sandbox()])
    mounts = json.loads(inspect)[0]["Mounts"]
    # The base mounts, plus the read-only skills tiers (GW-I-063, SEC-023): the user tier always, the install tier when
    # `[skills] root` is set. Nothing else reaches the sandbox.
    base = {"/workspace", "/run/tqa/state", "/run/tqa/token", "/tmp"}
    skills = {"/skills/user", "/skills/install"}
    assert {mount["Destination"] for mount in mounts} <= base | skills, mounts
    assert all(not mount["RW"] for mount in mounts if mount["Destination"] in skills), "skills tiers must be read-only"
    absent = "from pathlib import Path; assert all(not Path('/etc/tqa', name).exists() for name in ['secrets.toml', 'models.toml', 'install.key']); print('gateway credential files absent')"
    assert stack.exec(["python3", "-c", absent]) == "gateway credential files absent"
    environment = stack.exec(["sh", "-c", "env; cat /proc/1/environ"])
    for secret in ["synthetic-compose-fixture", stack.env["TQA_POSTGRES_PASSWORD"], stack.env["TQA_BOOTSTRAP_TOKEN"]]:
        assert secret not in inspect and secret not in environment and secret not in json.dumps(frames), "A gateway credential reached the sandbox or transcript"
    token = stack.exec(["cat", "/run/tqa/token"])
    assert token and token not in inspect and token not in environment and token not in json.dumps(frames)
    print("SEC-001: README-driven shell and fetch exfiltration denied; no provider/database key; durable results and gateway notice/log verified", flush=True)


def network(stack):
    gateway = stack.command(stack.configuration + ["ps", "-q", "gateway"])
    settings = json.loads(stack.command(["docker", "inspect", "--format", "{{json .NetworkSettings.Networks}}", gateway]))
    expected_peer = settings[stack.env["TQA_SANDBOX_NETWORK"]]["IPAddress"]
    allowed = "https://{}:8443/allowed".format(stack.upstream)
    output = stack.exec(["curl", "--silent", "--show-error", "--fail", "--max-time", "10", "--cacert", "/workspace/fixture-ca.pem", allowed])
    observed = json.loads(output)
    assert observed["peer"] == expected_peer, "Allowed traffic bypassed the gateway proxy"
    assert not observed["authorization"] and not observed["proxy_authorization"]
    stack.refused(["curl", "--silent", "--show-error", "--max-time", "5", "--proxy", "http://gateway:3128", "--proxytunnel", "--output", "/dev/null", "--write-out", "%{http_connect}", "http://denied.invalid:443/"], 403)
    stack.refused(["curl", "--silent", "--show-error", "--max-time", "5", "--proxy-header", "Proxy-Authorization: Bearer synthetic-wrong-credential", "--proxy", "http://gateway:3128", "--proxytunnel", "--output", "/dev/null", "--write-out", "%{http_connect}", "http://{}:8081/".format(stack.upstream)], 407)
    relay(stack)
    isolation(stack)
    print("SBX-C-004: relay and verified TLS via HTTPS_PROXY passed; gateway peer observed; denied host 403; wrong bearer 407; outside TCP/UDP blocked", flush=True)


def relay_turn(stack):
    """The scripted model's turn that answers "relay-fixture-passed" in this stack's scenario: the desktop case's comes
    after its own nine calls, not the network case's four (a fixed 5 met the desktop scenario's sixth call)."""
    return next(index for index, response in enumerate(stack.script["responses"])
                if any(event.get("text") == "relay-fixture-passed" for event in response["events"]))


def relay(stack):
    # Script selection counts the preceding assistant messages, rather than requests.
    script = ("import json,pathlib,urllib.request; "
              "history=[item for _ in range(" + str(relay_turn(stack)) + ") for item in ({'role':'user','content':'prior'},{'role':'assistant','content':'prior'})]; "
              "body=json.dumps({'model':'fixture','messages':history+[{'role':'user','content':'probe'}],'stream':True}).encode(); "
              "token=pathlib.Path('/run/tqa/token').read_text().strip(); "
              "request=urllib.request.Request('http://gateway:8080/relay/r/local/chat/completions',data=body,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'}); "
              "client=urllib.request.build_opener(urllib.request.ProxyHandler({})); "
              "response=client.open(request,timeout=10); data=response.read(65537); "
              "assert len(data)<=65536 and b'relay-fixture-passed' in data; print('relay passed')")
    assert stack.exec(["python3", "-c", script]) == "relay passed"


def isolation(stack):
    probe = ("import socket,sys; host=sys.argv[1]; "
             "tcp=socket.create_connection((host,8081),3); tcp.close(); "
             "udp=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); udp.settimeout(3); "
             "udp.sendto(b'egress-control',(host,9000)); assert udp.recv(1024)==b'egress-control'; "
             "print('positive controls passed')")
    assert stack.command(stack.configuration + ["exec", "-T", "upstream", "python3", "-c", probe, stack.outside]) == "positive controls passed"
    blocked = '''import socket,sys
host=sys.argv[1]
for kind,port in [(socket.SOCK_STREAM,8081),(socket.SOCK_DGRAM,9000)]:
    with socket.socket(socket.AF_INET,kind) as sock:
        sock.settimeout(2)
        try:
            sock.connect((host,port))
            if kind==socket.SOCK_DGRAM:
                sock.send(b'egress-control')
                sock.recv(1024)
        except OSError:
            continue
        raise AssertionError('Direct traffic bypassed the internal network')
print('direct TCP and UDP blocked')
'''
    assert stack.exec(["python3", "-c", blocked, stack.outside]) == "direct TCP and UDP blocked"
    routes = stack.exec(["cat", "/proc/net/route"])
    assert not any(line.split()[1] == "00000000" for line in routes.splitlines()[1:]), "Sandbox has an external default route"


def public_network(stack):
    script = Path(__file__).with_name("public_probe.py").read_text()
    tcp = stack.env.get("TQA_EGRESS_PUBLIC_TCP_HOST", "example.com")
    dns = stack.env.get("TQA_EGRESS_PUBLIC_DNS_HOST", "dns.quad9.net")
    output = stack.command(stack.configuration + ["exec", "-T", "upstream", "python3", "-c", script, "control", tcp, dns])
    addresses = json.loads(output)
    assert stack.exec(["python3", "-c", script, "blocked", addresses["tcp"], addresses["dns"]]) == "public TCP and UDP blocked"
    print("SBX-C-004 public: positive TCP/443 and DNS/UDP controls passed; the same public IPs are unreachable from the sandbox", flush=True)
