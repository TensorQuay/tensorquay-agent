"""Clone with the runtime initializer inside a real proxy-admitted sandbox."""
import json
from pathlib import Path

from .stack import Stack, compose


class CloneStack(Stack):
    def overlay(self):
        super().overlay()
        compose.ownership(self.directory, self.env)
        source = Path(__file__).with_name("git_server.py")
        compose.write(self.directory / "upstream.py", source.read_text())
        path = self.directory.parent / "egress.json"
        overlay = json.loads(path.read_text())
        overlay["services"]["upstream"]["tmpfs"] = ["/tmp:rw,noexec,nosuid,size=64m,mode=1777"]
        overlay["services"]["upstream"]["mem_limit"] = "128m"
        compose.write(path, json.dumps(overlay))
        compose.ownership(self.directory, self.env, "1000:1000")

    def scenario(self):
        response = {"events": [{"type": "text", "text": "Fixture sandbox admitted."}],
                    "usage": {"prompt_tokens": 10, "completion_tokens": 10}}
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": [response]}))

    def probe(self):
        image = self.env["TQA_CLONE_PROBE_IMAGE"]
        if not (image.startswith("sha256:") or "@sha256:" in image):
            raise ValueError("TQA_CLONE_PROBE_IMAGE must identify the built test-only image by digest")
        print("Clone probe image: " + image, flush=True)
        compose.ownership(self.directory, self.env)
        root = Path(self.env["TQA_STORAGE_ROOT"]) / "workspaces" / self.conversation / "clone-fixture"
        root.mkdir()
        compose.write(root / "input.json", json.dumps({"url": "https://{}:8443/repo.git".format(self.upstream),
                                                      "proxy": "http://gateway:3128"}))
        compose.write(root / "fixture-ca.pem", (self.directory / "fixture-ca.pem").read_text())
        container = self.command(["docker", "create", image])
        try:
            self.command(["docker", "cp", container + ":/usr/local/bin/workspace-clone-probe", str(root / "probe")])
        finally:
            self.command(["docker", "rm", container])
        (root / "probe").chmod(0o755)
        compose.ownership(self.directory, self.env, "1000:1000")
        self.exec(["env", "TQA_CLONE_FIXTURE=/workspace/clone-fixture", "/workspace/clone-fixture/probe",
                   "--ignored", "--exact", "workspace_clone_probe", "--nocapture"])
        return json.loads(self.exec(["cat", "/workspace/clone-fixture/results.json"]))

    def witness(self):
        program = ("import urllib.request; "
                   "print(urllib.request.urlopen('http://127.0.0.1:8081/witness',timeout=5).read(65537).decode())")
        return json.loads(self.command(self.configuration + ["exec", "-T", "upstream", "python3", "-c", program]))


def check(stack):
    stream = stack.start()
    stream.until("run.completed")
    results = stack.probe()
    receipts = stack.witness()
    assert receipts, "The real Git origin received no checkout request"
    gateway = stack.command(stack.configuration + ["ps", "-q", "gateway"])
    networks = json.loads(stack.command(["docker", "inspect", "--format", "{{json .NetworkSettings.Networks}}", gateway]))
    expected = networks[stack.env["TQA_SANDBOX_NETWORK"]]["IPAddress"]
    assert all(item["peer"] == expected for item in receipts), "Git clone bypassed the gateway proxy"
    assert all(not item["authorization"] and not item["proxy_authorization"] for item in receipts)
    assert all(item["path"].startswith("/repo.git/") for item in receipts)
    assert sum(item["path"].startswith("/repo.git/info/refs") for item in receipts) == 3
    assert results == {"main": "completed", "branch": "completed", "tag": "completed",
                       "denied": "egress_denied", "preserved": "skipped"}, "Unexpected clone outcomes"
    print("RT-I-015 clone: full checkout, branch, tag, denied host, preserved workspace; "
          "TLS verified and every Git request observed from the gateway proxy", flush=True)
