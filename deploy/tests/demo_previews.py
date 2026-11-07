"""WEB-E2E-032's driver side (the file previews design): the agent writes an HTML report, PDFs and spreadsheets on the
real stack, and the browser checks each preview and each failure. The files are built here, byte for byte, and the
scripted model's one shell call writes them into the workspace with `base64 -d` (which even the minimal image has), so
the suite runs on the Docker and the microVM backends alike.

The report page is hostile on purpose: its script tries the network (fetch, an image beacon, a WebSocket, a beacon, a
form, a popup, the top window) against a witness server run here. The browser can reach the witness (the spec's
control request proves it), so a witness that sees nothing else proves nothing left the sandboxed page."""

import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
from pathlib import Path
import importlib.util
import shutil
import subprocess
import threading
import zipfile

import compose

# The prompt the scenario answers, and the folder its files land in.
PROMPT = "Make the preview files"
FOLDER = "previews"
# The witness's own paths: the spec's control request, and the log it reads.
CONTROL, LOG = "/control", "/log"
# A shell argument must stay under the kernel's 128 KiB single-argument limit.
COMMAND_LIMIT = 100 * 1024
# Files over each preview limit, written in the sandbox (not embedded): HTML 2 MiB, PDF 20 MiB, sheets 10 MiB.
LARGE = {"big.html": 3 * 1024 * 1024, "big.pdf": 21 * 1024 * 1024, "big.xlsx": 11 * 1024 * 1024}

MAIN = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
RELATIONSHIPS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PACKAGE = "http://schemas.openxmlformats.org/package/2006/relationships"


def report(witness):
    """The hostile report: a chart drawn by its script, then every way out it can try, each result in its own DOM."""
    socket_origin = witness.replace("http://", "ws://", 1)
    return """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Q3 report</title>
<style>body{{font:14px/1.4 system-ui,sans-serif;margin:16px;color:#1f2328;background:#fff}}h1{{font-size:18px}}
.bar{{fill:#3b6ea8}}li{{font-size:12px;color:#57606a}}</style></head>
<body><h1>Q3 sales by region</h1>
<svg id="chart" width="300" height="140" role="img" aria-label="Sales chart"></svg>
<ul id="probes"></ul>
<form id="form" action="{witness}/form" method="post"><input type="hidden" name="field" value="1"></form>
<script>
const probes = document.getElementById("probes");
const note = (id, text) => {{ const item = document.createElement("li"); item.id = id; item.textContent = text; probes.append(item); }};
const svg = document.getElementById("chart"), ns = "http://www.w3.org/2000/svg";
[["North", 12], ["South", 30], ["East", 21], ["West", 17]].forEach(([name, value], index) => {{
  const bar = document.createElementNS(ns, "rect");
  bar.setAttribute("class", "bar"); bar.setAttribute("x", String(20 + index * 70)); bar.setAttribute("y", String(120 - value * 3));
  bar.setAttribute("width", "40"); bar.setAttribute("height", String(value * 3)); svg.appendChild(bar);
}});
note("ran", "Scripts ran.");
try {{ note("parent", window.parent !== window && window.parent.document ? "The app's page was reachable." : "No app page around."); }}
catch {{ note("parent", "The app's page is out of reach."); }}
try {{ note("cookie", "Cookies: " + (document.cookie || "none")); }} catch {{ note("cookie", "Cookies: none"); }}
try {{ localStorage.setItem("probe", "1"); note("storage", "Storage worked."); }} catch {{ note("storage", "Storage refused."); }}
fetch("{witness}/fetch").then(() => note("fetch", "The request went out."), () => note("fetch", "The request was blocked."));
fetch("/api/v1/auth/me").then((answer) => note("api", "The API answered " + answer.status + "."), () => note("api", "The API was out of reach."));
new Image().src = "{witness}/beacon";
try {{ new WebSocket("{socket_origin}/socket").onerror = () => undefined; }} catch {{ }}
try {{ navigator.sendBeacon("{witness}/send-beacon"); }} catch {{ }}
try {{ window.open("{witness}/popup"); }} catch {{ }}
try {{ document.getElementById("form").submit(); }} catch {{ }}
// Only when framed: opened on its own, the top window is this page, and moving it would be its own navigation.
if (window.top !== window.self) {{
  try {{ window.top.location.href = "{witness}/top"; note("top", "The app was moved."); }}
  catch {{ note("top", "Moving the app was refused."); }}
}}
note("done", "Probes done.");
</script></body></html>
""".format(witness=witness, socket_origin=socket_origin)


def pdf(pages, script=False):
    """A PDF of `pages` pages, each titled with its number, with correct cross-reference offsets; with `script`, an
    OpenAction that would raise an alert (a preview must never run it)."""
    objects = {1: "<< /Type /Catalog /Pages 2 0 R{} >>".format(" /OpenAction 3 0 R" if script else ""),
               3: "<< /S /JavaScript /JS (app.alert\\('the PDF script ran'\\);) >>",
               4: "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"}
    kids = []
    for page in range(pages):
        number = 5 + page * 2
        content = "BT /F1 24 Tf 30 120 Td (Page {} of the brief) Tj ET".format(page + 1)
        objects[number] = ("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents {} 0 R "
                           "/Resources << /Font << /F1 4 0 R >> >> >>").format(number + 1)
        objects[number + 1] = "<< /Length {} >>\nstream\n{}\nendstream".format(len(content), content)
        kids.append("{} 0 R".format(number))
    objects[2] = "<< /Type /Pages /Kids [{}] /Count {} >>".format(" ".join(kids), pages)
    body, offsets = b"%PDF-1.7\n", {}
    for number in sorted(objects):
        offsets[number] = len(body)
        body += "{} 0 obj\n{}\nendobj\n".format(number, objects[number]).encode()
    xref, size = len(body), max(objects) + 1
    body += "xref\n0 {}\n0000000000 65535 f \n".format(size).encode()
    for number in range(1, size):
        body += ("{:010d} 00000 n \n".format(offsets[number]) if number in offsets else "0000000000 65535 f \n").encode()
    body += "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n".format(size, xref).encode()
    return body


def zipped(parts, stored_first=False):
    """A zip of (name, bytes) parts, deflated; with `stored_first`, the first part stored (ODS's mimetype)."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        for index, (name, content) in enumerate(parts):
            method = zipfile.ZIP_STORED if stored_first and index == 0 else zipfile.ZIP_DEFLATED
            archive.writestr(name, content, compress_type=method)
    return buffer.getvalue()


def xlsx():
    """Two sheets: Totals (a heading row, numbers, a formula with its cached value, a URL cell) and Notes."""
    def cell(ref, value):
        if isinstance(value, str):
            return '<c r="{}" t="inlineStr"><is><t>{}</t></is></c>'.format(ref, value)
        if isinstance(value, tuple):
            return '<c r="{}"><f>{}</f><v>{}</v></c>'.format(ref, value[0], value[1])
        return '<c r="{}"><v>{}</v></c>'.format(ref, value)

    def sheet(rows):
        body = "".join('<row r="{}">{}</row>'.format(number, "".join(
            cell("{}{}".format("ABCDE"[column], number), value) for column, value in enumerate(row)))
            for number, row in enumerate(rows, start=1))
        return '<worksheet xmlns="{}"><sheetData>{}</sheetData></worksheet>'.format(MAIN, body)

    totals = [["Region", "Q1", "Q2", "Q3", "Total"], ["North", 10, 11, 12, ("SUM(B2:D2)", 33)],
              ["South", 22, 26, 30, ("SUM(B3:D3)", 78)],
              ["Source", "https://example.invalid/reports/2026/q3/sales-by-region-final.xlsx"]]
    notes = [["Figures in thousands."], ["Q3 is provisional."]]
    return zipped([
        ("_rels/.rels", '<Relationships xmlns="{}"><Relationship Id="r" Type="{}/officeDocument" '
                        'Target="xl/workbook.xml"/></Relationships>'.format(PACKAGE, RELATIONSHIPS)),
        ("xl/workbook.xml", '<workbook xmlns="{}" xmlns:r="{}"><sheets><sheet name="Totals" sheetId="1" r:id="s1"/>'
                            '<sheet name="Notes" sheetId="2" r:id="s2"/></sheets></workbook>'.format(MAIN, RELATIONSHIPS)),
        ("xl/_rels/workbook.xml.rels", '<Relationships xmlns="{0}"><Relationship Id="s1" Type="{1}/worksheet" '
                                       'Target="worksheets/1.xml"/><Relationship Id="s2" Type="{1}/worksheet" '
                                       'Target="worksheets/2.xml"/></Relationships>'.format(PACKAGE, RELATIONSHIPS)),
        ("xl/worksheets/1.xml", sheet(totals)),
        ("xl/worksheets/2.xml", sheet(notes)),
    ])


def ods():
    """One sheet, with a row repeated 1,000,000 times across 16,384 columns: counted, never expanded."""
    content = ('<?xml version="1.0" encoding="UTF-8"?><office:document-content '
               'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
               'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
               'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet>'
               '<table:table table:name="Plan"><table:table-row><table:table-cell><text:p>Month</text:p>'
               '</table:table-cell><table:table-cell><text:p>Target</text:p></table:table-cell></table:table-row>'
               '<table:table-row table:number-rows-repeated="1000000"><table:table-cell '
               'table:number-columns-repeated="16384"><text:p>7</text:p></table:table-cell></table:table-row>'
               '</table:table></office:spreadsheet></office:body></office:document-content>')
    manifest = ('<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0">'
                '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>'
                '</manifest:manifest>')
    return zipped([("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
                   ("META-INF/manifest.xml", manifest), ("content.xml", content)], stored_first=True)


def bomb():
    """A tiny .xlsx whose one part claims to inflate to 200 MiB: refused before anything is inflated."""
    data = bytearray(zipped([("xl/worksheets/1.xml", b"<worksheet/>")]))
    claimed = (200 * 1024 * 1024).to_bytes(4, "little")
    for signature, offset in ((b"PK\x03\x04", 22), (b"PK\x01\x02", 24)):
        at = data.find(signature)
        while at != -1:
            data[at + offset:at + offset + 4] = claimed
            at = data.find(signature, at + 4)
    return bytes(data)


def fixtures(witness):
    """Every embedded file, by its name in the previews folder."""
    return {
        "report.html": report(witness).encode(),
        "brief.pdf": pdf(1, script=True),
        "long.pdf": pdf(25),
        "broken.pdf": b"%PDF-1.7\nthis is not a PDF body\n%%EOF\n",
        "sales.xlsx": xlsx(),
        "plan.ods": ods(),
        "bomb.xlsx": bomb(),
        "damaged.xlsx": b"PK\x03\x04 a spreadsheet cut short",
    }


def commands(witness):
    """The shell commands that write every file: the embedded ones decoded, the large ones generated in place."""
    lines = ["mkdir -p " + FOLDER]
    for name, content in fixtures(witness).items():
        lines.append("printf %s '{}' | base64 -d > {}/{}".format(base64.b64encode(content).decode(), FOLDER, name))
    for name, size in LARGE.items():
        lines.append("head -c {} /dev/zero | tr '\\0' 'x' > {}/{}".format(size, FOLDER, name))
    command = " && ".join(lines)
    if len(command.encode()) > COMMAND_LIMIT:
        raise ValueError("the previews command exceeds the single-argument limit")
    return [command]


def scenario(witness):
    """The scripted model: one shell call writing the files, then a short answer."""
    calls = [{"type": "tool_call", "name": "shell", "arguments": {"command": command}, "split_into": 3}
             for command in commands(witness)]
    return {"model": "fixture", "select": "prompt", "responses": [
        {"when": PROMPT, "events": calls, "usage": {"prompt_tokens": 20, "completion_tokens": 20}},
        {"when": PROMPT, "events": [{"type": "text", "text": "I wrote the report, the PDFs and the spreadsheets in "
                                     + FOLDER + "/."}], "usage": {"prompt_tokens": 40, "completion_tokens": 20}},
    ]}


class Witness:
    """An HTTP listener that records every request's path; the spec asks it for the log at LOG."""

    def __init__(self, host):
        self.requests, self.lock = [], threading.Lock()
        witness = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):  # noqa: N802 (the standard library's name)
                witness.answer(self)

            def do_POST(self):  # noqa: N802
                witness.answer(self)

            def log_message(self, *arguments):
                return

        self.server = ThreadingHTTPServer((host, 0), Handler)
        self.origin = "http://{}:{}".format(host, self.server.server_address[1])
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def answer(self, request):
        path = request.path.split("?", 1)[0]
        if path == LOG:
            body = json.dumps(self.seen()).encode()
        else:
            with self.lock:
                self.requests.append(path)
            body = b"witnessed"
        request.send_response(200)
        request.send_header("content-type", "application/json" if path == LOG else "text/plain")
        request.send_header("content-length", str(len(body)))
        request.send_header("access-control-allow-origin", "*")
        request.end_headers()
        request.wfile.write(body)

    def seen(self):
        with self.lock:
            return list(self.requests)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


def firecracker_stack():
    """The microVM stack's own helpers (deploy/firecracker/stack.py), loaded only for a microVM run."""
    source = Path(__file__).resolve().parents[1] / "firecracker" / "stack.py"
    spec = importlib.util.spec_from_file_location("firecracker_stack", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def microvm_overlay(configuration, env):
    """The Compose files for the microVM backend, with a free sandbox network drawn for its overlay."""
    microvm = firecracker_stack()
    microvm.claim_network(env)
    return configuration[:len(configuration) - 2] + microvm.OVERLAY + configuration[-2:]


def to_microvm(directory, env, runtime):
    """The installation on the microVM backend (as the soak's): the VM host, the kernel and one template, Minimal,
    whose rootfs is built from the runtime image (the suite needs only a shell)."""
    microvm = firecracker_stack()
    for name in ("TQA_VMM_IMAGE", "TQA_FC_KERNEL_DIR"):
        if not env.get(name):
            raise SystemExit("--backend firecracker needs " + name)
    vmm_image = microvm.image_id(env["TQA_VMM_IMAGE"], env)
    # The sandboxes run the VM host image: it is under test, as for the microVM stacks (TK-U-044, candidate 14b).
    microvm.vm_host_under_test(env, vmm_image)
    directory = Path(directory)
    config = directory / "gateway.toml"
    compose.ownership(directory, env)
    content = microvm.firecracker_config(config.read_text(), vmm_image, env["TQA_FC_GATEWAY_ADDRESS"])
    content += ('\n[[templates]]\nname="minimal"\ndisplay_name="Minimal"\nimage={}\ncapabilities=["shell"]\n'
                'default=true\n').format(json.dumps(runtime))
    compose.write(config, content)
    microvm.artefacts(directory / "storage", env, runtime, vmm_image)
    compose.ownership(directory, env, "1000:1000")


def configure(directory, env, witness):
    """The scenario, written for this run's witness, and its read-only mount for the scripted model. The installation
    belongs to the containers' user (1000:1000) by now, and the file is private, so it is written as the host user and
    handed back to them, as the environments' configuration does."""
    scenario_file = Path(directory) / "previews.json"
    compose.ownership(directory, env)
    compose.write(scenario_file, json.dumps(scenario(witness.origin)))
    compose.ownership(directory, env, "1000:1000")
    env.update(TQA_PREVIEWS_SCENARIO=str(scenario_file.resolve()), TQA_WITNESS_URL=witness.origin)


def verify_overlay(env, configuration):
    """The scripted model may read only this run's previews scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/previews.json", "Unexpected scenario target"
    assert mount.get("source") == env["TQA_PREVIEWS_SCENARIO"], "Unexpected scenario source"


def workspace_files(workspaces, identity):
    """The names in the workspace's previews folder: a directory on the Docker backend, an ext4 image on the microVM
    backend (listed read-only with debugfs, which needs no root)."""
    directory = workspaces / identity
    if directory.is_dir():
        folder = directory / FOLDER
        return {path.name for path in folder.iterdir() if path.is_file()} if folder.is_dir() else set()
    image = workspaces / (identity + ".ext4")
    assert image.is_file(), "No workspace directory or image for " + identity
    debugfs = shutil.which("debugfs") or "/usr/sbin/debugfs"
    listing = subprocess.run([debugfs, "-R", "ls -p /" + FOLDER, str(image)], check=True, capture_output=True,
                             text=True, timeout=60).stdout
    # ls -p: /inode/mode/uid/gid/name/size/ per entry; a regular file's mode starts 100.
    entries = [line.strip().split("/") for line in listing.splitlines() if line.startswith("/")]
    return {entry[5] for entry in entries if len(entry) > 6 and entry[2].startswith("100")}


def verify_result(env, witness):
    """Every file in the workspace, and nothing reached the witness but the spec's control requests."""
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 8192, "Previews result exceeded its bound"
    record = json.loads(result.read_text())
    workspaces = Path(env["TQA_STORAGE_ROOT"]).resolve() / "workspaces"
    names = workspace_files(workspaces, record["workspaceId"])
    for name in list(fixtures(witness.origin)) + list(LARGE):
        assert name in names, "Missing " + name
    seen = witness.seen()
    assert seen and set(seen) == {CONTROL}, "The witness saw more than the control: {}".format(seen)
    print("Previews: every view and failure checked; the witness saw only the control ({}x)".format(len(seen)),
          flush=True)
