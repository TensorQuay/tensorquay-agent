import { SLOW_RUN, SLOW_RUN_FILES } from "./slow-run.mjs";

// The task's workspace for the fake front (task 068b): an in-memory tree per conversation behind the same four routes
// and limits as the gateway's files module, so the panel and composer run end to end without a sandbox.
const MiB = 1024 * 1024;
const LIMITS = { upload: 25 * MiB, preview: 512 * 1024, page: 200 };
const encoder = new TextEncoder();
const now = () => new Date().toISOString();

function seed() {
  const files = new Map();
  const put = (path, value) =>
    files.set(path, {
      bytes: typeof value === "string" ? encoder.encode(value) : value,
      modified_at: now(),
    });
  put(
    "README.md",
    "# Example project\n\nA small workspace for the test front.\n",
  );
  put(
    "src/app.py",
    'def main():\n    print("hello")\n\n\nif __name__ == "__main__":\n    main()\n',
  );
  put(
    "data/sample.bin",
    Uint8Array.from({ length: 256 }, (_, index) => index),
  );
  put(".tqa/state.json", "{}");
  put("reports/report.html", REPORT);
  put("reports/brief.pdf", BRIEF_PDF);
  put("reports/chinese.pdf", CHINESE_PDF);
  put("reports/symbol.pdf", SYMBOL_PDF);
  put("reports/broken.pdf", "%PDF-1.7\nthis is not a PDF body\n%%EOF\n");
  // Its bytes are only a stand-in: the fake front answers the sheet itself (SALES below); the gateway parses real files.
  put("reports/sales.xlsx", Uint8Array.from([0x50, 0x4b, 0x03, 0x04, 0, 0]));
  return files;
}
const workspace = (conversation) => (conversation.files ??= seed());

/** The gateway's policy for /files/view (files/view.rs), sent exactly, so the page runs as it would. */
const VIEW_POLICY =
  "sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; " +
  "img-src data:; font-src data:; media-src data:; connect-src 'none'; form-action 'none'; frame-src 'none'; " +
  "child-src 'none'; worker-src 'none'; base-uri 'none'; frame-ancestors 'self'";
const VIEW_BYTES = 2 * MiB;
const SHEET_BYTES = 10 * MiB;

/**
 * A report as an agent writes one: inline style, and a script that draws its chart and records what it could reach
 * (the app's document must be out of reach, WEB-U-087).
 */
const REPORT = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Q3 sales</title>
<style>body{font:14px/1.4 system-ui,sans-serif;margin:16px;color:#1f2328}h1{font-size:18px;margin:0 0 8px}
.bar{fill:#3b6ea8}.label{font-size:11px;fill:#57606a}#ran,#reach{font-size:12px;color:#57606a}</style></head>
<body><h1>Q3 sales by region</h1><svg id="chart" width="300" height="140" role="img" aria-label="Sales chart"></svg>
<p id="ran">Scripts did not run.</p><p id="reach"></p>
<script>
const data=[["North",12],["South",30],["East",21],["West",17]];
const svg=document.getElementById("chart");const ns="http://www.w3.org/2000/svg";
data.forEach(([name,value],i)=>{const bar=document.createElementNS(ns,"rect");bar.setAttribute("class","bar");
bar.setAttribute("x",String(20+i*70));bar.setAttribute("y",String(120-value*3));bar.setAttribute("width","40");
bar.setAttribute("height",String(value*3));svg.appendChild(bar);const label=document.createElementNS(ns,"text");
label.setAttribute("class","label");label.setAttribute("x",String(20+i*70));label.setAttribute("y","135");
label.textContent=name;svg.appendChild(label);});
document.getElementById("ran").textContent="Scripts ran.";
let reach;try{reach=window.parent.document?"The app's page was reachable.":"";}catch{reach="The app's page is out of reach.";}
document.getElementById("reach").textContent=reach;
</script></body></html>
`;

/** A PDF of `objects` (1 is the catalogue) with correct cross-reference offsets. */
function pdfOf(objects) {
  let body = "%PDF-1.7\n";
  const offsets = objects.map((object, index) => {
    const at = body.length;
    body += `${index + 1} 0 obj\n${object}\nendobj\n`;
    return at;
  });
  const xref = body.length;
  body += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const at of offsets)
    body += `${String(at).padStart(10, "0")} 00000 n \n`;
  body += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return body;
}

/**
 * A real one-page PDF, drawn by pdf.js in the browser spec (WEB-U-091). Its Helvetica is one of the 14 standard fonts,
 * not embedded: pdf.js draws it from the standard font data we serve (WEB-E2E-030). It opens with a JavaScript action
 * that would raise an alert: the preview must never run it.
 */
function pdf() {
  const content = "BT /F1 24 Tf 30 120 Td (Quarterly brief) Tj ET";
  return pdfOf([
    "<< /Type /Catalog /Pages 2 0 R /OpenAction 5 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 6 0 R >> >> >>",
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
    "<< /S /JavaScript /JS (app.alert\\('the PDF script ran'\\);) >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
  ]);
}
const BRIEF_PDF = pdf();

/**
 * A one-page PDF in Chinese, its font not embedded: STSong-Light with the UniGB-UCS2-H CMap (Adobe-GB1), which pdf.js
 * needs from the CMaps we serve to read the text (WEB-E2E-030). The text is 中文报告 ("Chinese report"), as UCS-2.
 */
function chinesePdf() {
  const content = "BT /F1 28 Tf 30 110 Td <4E2D65875831544A> Tj ET";
  return pdfOf([
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
    "<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light /Encoding /UniGB-UCS2-H /DescendantFonts [6 0 R] >>",
    "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /STSong-Light /CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 4 >> /FontDescriptor 7 0 R /DW 1000 >>",
    "<< /Type /FontDescriptor /FontName /STSong-Light /Flags 6 /FontBBox [-25 -254 1000 880] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 880 /StemV 93 >>",
  ]);
}
const CHINESE_PDF = chinesePdf();

/**
 * A one-page PDF in the Symbol font, one of the 14 standard fonts and not embedded: pdf.js draws it from the standard
 * font data we serve (FoxitSymbol, WEB-E2E-030), as it has no system font to use for it. The text is αβγ.
 */
function symbolPdf() {
  const content = "BT /F1 36 Tf 30 110 Td (abg) Tj ET";
  return pdfOf([
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>",
  ]);
}
const SYMBOL_PDF = symbolPdf();

/** The shown table of the seeded spreadsheet, as /files/sheet answers it (two sheets). */
const SALES = {
  Totals: [
    ["Region", "Q1", "Q2", "Q3", "Total"],
    ["North", "10", "11", "12", "33"],
    ["South", "22", "26", "30", "78"],
    ["East", "18", "19", "21", "58"],
    ["West", "15", "16", "17", "48"],
    ["", "", "", "", ""],
    [
      "Source",
      "https://example.invalid/reports/2026/q3/sales-by-region-final.xlsx",
      "",
      "",
      "",
    ],
  ],
  Notes: [["Figures in thousands."], ["Q3 is provisional."]],
};

/** Mirrors the gateway's path rule: relative, no empty, "." or ".." segment, no NUL; ".tqa" is hidden. */
function parse(raw) {
  if (raw === null)
    return { error: [400, "bad_request", "A path is required."] };
  const path = raw === "." ? "" : raw;
  if (
    path.length > 1024 ||
    path.startsWith("/") ||
    path.includes("\0") ||
    (path &&
      path.split("/").some((part) => !part || part === "." || part === ".."))
  )
    return { error: [400, "path_invalid", "The path is not allowed."] };
  if (path === ".tqa" || path.startsWith(".tqa/"))
    return { error: [404, "not_found", "No such file."] };
  return { path };
}

function entries(files, folder) {
  const prefix = folder ? `${folder}/` : "";
  const found = new Map();
  for (const [path, file] of files) {
    if (!path.startsWith(prefix) || (!folder && path.startsWith(".tqa/")))
      continue;
    const rest = path.slice(prefix.length);
    const name = rest.split("/")[0];
    if (rest.includes("/"))
      found.set(name, {
        name,
        kind: "dir",
        size: 0,
        modified_at: file.modified_at,
      });
    else
      found.set(name, {
        name,
        kind: "file",
        size: file.bytes.length,
        modified_at: file.modified_at,
      });
  }
  return [...found.values()].sort(
    (a, b) =>
      (a.kind === "dir" ? 0 : 1) - (b.kind === "dir" ? 0 : 1) ||
      a.name.localeCompare(b.name),
  );
}

function text(bytes) {
  if (bytes.includes(0)) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** The gateway's sanitising rule for upload names. */
export function sanitise(raw) {
  const last = raw.split(/[/\\]/).pop() ?? "";
  let name = last.replace(/[^A-Za-z0-9._-]/g, "_").replace(/^\.+/, "");
  if (!name) name = "file";
  return name.slice(0, 128);
}
function unique(files, name) {
  if (!files.has(`uploads/${name}`)) return name;
  const dot = name.lastIndexOf(".");
  const [stem, extension] =
    dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ""];
  for (let index = 2; ; index++) {
    const candidate = `${stem}-${index}${extension}`;
    if (!files.has(`uploads/${candidate}`)) return candidate;
  }
}

/** Answers the files routes; returns false for any other path. */
export async function filesRoute(
  request,
  response,
  conversation,
  suffix,
  url,
  reply,
) {
  const files = workspace(conversation);
  const method = request.method;
  if (suffix === "/uploads" && method === "POST") {
    const declared = Number(request.headers["content-length"] ?? 0);
    if (declared > LIMITS.upload) {
      request.resume();
      reply.error(
        413,
        "upload_too_large",
        "The file is larger than the upload limit.",
      );
      return true;
    }
    const chunks = [];
    let size = 0;
    for await (const chunk of request) {
      size += chunk.length;
      if (size > LIMITS.upload) {
        reply.error(
          413,
          "upload_too_large",
          "The file is larger than the upload limit.",
        );
        return true;
      }
      chunks.push(chunk);
    }
    const name = unique(files, sanitise(url.searchParams.get("name") ?? ""));
    files.set(`uploads/${name}`, {
      bytes: new Uint8Array(Buffer.concat(chunks)),
      modified_at: now(),
    });
    reply.json(201, { path: `uploads/${name}`, name, size });
    return true;
  }
  if (
    method !== "GET" ||
    ![
      "/files",
      "/files/content",
      "/files/download",
      "/files/view",
      "/files/sheet",
    ].includes(suffix)
  )
    return false;
  const parsed = parse(url.searchParams.get("path"));
  if (parsed.error) {
    reply.error(...parsed.error);
    return true;
  }
  const { path } = parsed;
  if (suffix === "/files") {
    const all = entries(files, path);
    if (path && all.length === 0 && !files.has(path)) {
      reply.error(404, "not_found", "No such folder.");
      return true;
    }
    const after = url.searchParams.get("after");
    const limit = Math.min(
      500,
      Number(url.searchParams.get("limit") ?? LIMITS.page) || LIMITS.page,
    );
    const rest = after
      ? all.slice(all.findIndex((entry) => entry.name === after) + 1)
      : all;
    const items = rest.slice(0, limit);
    reply.json(200, {
      path,
      items,
      next_after: rest.length > limit ? (items.at(-1)?.name ?? null) : null,
    });
    return true;
  }
  const file = files.get(path);
  if (!file) {
    reply.error(404, "not_found", "No such file.");
    return true;
  }
  if (suffix === "/files/view") {
    if (!/[^/]\.html?$/i.test(path)) {
      reply.error(415, "preview_unsupported", "This file type has no preview.");
      return true;
    }
    if (file.bytes.length > VIEW_BYTES) {
      reply.error(413, "body_too_large", "The file is too large to preview.");
      return true;
    }
    response.writeHead(200, {
      "content-type": "text/html; charset=utf-8",
      "content-security-policy": VIEW_POLICY,
      "x-content-type-options": "nosniff",
      "referrer-policy": "no-referrer",
      "cache-control": "no-store",
      "x-dns-prefetch-control": "off",
      "cross-origin-resource-policy": "same-origin",
    });
    response.end(Buffer.from(file.bytes));
    return true;
  }
  if (suffix === "/files/sheet") {
    if (!/[^/]\.(xlsx|xlsm|xlsb|ods)$/i.test(path)) {
      reply.error(415, "preview_unsupported", "This file type has no preview.");
      return true;
    }
    if (file.bytes.length > SHEET_BYTES) {
      reply.error(413, "body_too_large", "The file is too large to preview.");
      return true;
    }
    if (path !== "reports/sales.xlsx") {
      reply.error(422, "preview_damaged", "This spreadsheet is damaged.");
      return true;
    }
    const names = Object.keys(SALES);
    const sheet = url.searchParams.get("sheet") ?? names[0];
    const rows = SALES[sheet];
    if (!rows) {
      reply.error(404, "not_found", "No such sheet.");
      return true;
    }
    reply.json(200, {
      sheets: names,
      sheet,
      rows,
      total_rows: rows.length,
      more_rows: false,
      more_columns: false,
      truncated_cells: 0,
    });
    return true;
  }
  if (suffix === "/files/download") {
    response.writeHead(200, {
      "content-type": "application/octet-stream",
      "content-disposition": `attachment; filename="${path.split("/").pop()}"`,
      "x-content-type-options": "nosniff",
    });
    response.end(Buffer.from(file.bytes));
    return true;
  }
  const preview = file.bytes.subarray(0, LIMITS.preview);
  const value = text(preview);
  reply.json(200, {
    path,
    size: file.bytes.length,
    modified_at: file.modified_at,
    etag: `"${file.bytes.length}-${Date.parse(file.modified_at)}"`,
    binary: value === null,
    truncated: value !== null && file.bytes.length > LIMITS.preview,
    text: value,
  });
  return true;
}

/** A message may name only uploaded files of its task's workspace (contract item 4). */
export function missingAttachment(conversation, attachments = []) {
  const files = workspace(conversation);
  return attachments.some(
    (path) =>
      !/^uploads\/[A-Za-z0-9_-][A-Za-z0-9._-]{0,127}$/.test(path) ||
      !files.has(path),
  );
}

const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGN4FKOtEXWCQSPqxKMYbQAmBgVnvp73dAAAAABJRU5ErkJggg==";

/** The fake agent: a message with attachments writes a summary; later messages update or remove it. */
export function agentWork(conversation, text, attachments) {
  const files = workspace(conversation);
  const put = (value) =>
    files.set("results/summary.md", {
      bytes: encoder.encode(value),
      modified_at: now(),
    });
  if (attachments.length)
    put(
      `# Summary\n\nRead:\n${attachments.map((path) => `- ${path}`).join("\n")}\n`,
    );
  else if (text.startsWith("Update the summary"))
    put(
      "# Summary, revised\n\nRead:\n- every upload\n\nChecked twice, including a deliberately long line that is wider than the files panel so the diff has to wrap it.\n",
    );
  else if (text.startsWith("Remove the summary"))
    files.delete("results/summary.md");
  // MAKE_REPORT in stream.mjs: a research and data run leaves a brief, a table, a chart and
  // a logo, so the files panel shows each as what it is.
  else if (text === "Make a chart and a brief") {
    const put = (path, bytes) => files.set(path, { bytes, modified_at: now() });
    put(
      "brief.md",
      encoder.encode(
        "# Findings\n\nSales grew in **North**; South and East were flat.\n\n- North: 1,204\n- South: 98\n- East: 7\n",
      ),
    );
    put(
      "data/clean.csv",
      encoder.encode(
        'region,total,note\nNorth,1204,"big, early"\nSouth,98,\nEast,7,"two\nlines"\n',
      ),
    );
    // A 2 × 2 PNG, and an SVG whose script must never run.
    put("out/chart.png", Buffer.from(PNG, "base64"));
    put(
      "out/logo.svg",
      encoder.encode(
        '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#e25c2b"/><script>window.svgRan = true</script></svg>',
      ),
    );
  }
  // ADD_TEST in stream.mjs: the run writes a test file, as its write_file call says.
  else if (text === "Add a test for the parser")
    files.set("tests/test_parser.py", {
      bytes: encoder.encode("def test_parse(): ...\n"),
      modified_at: now(),
    });
  // EDIT_TEST in stream.mjs: the run edits that test file, as its edit_file call says.
  else if (text === "Change the parser test")
    files.set("tests/test_parser.py", {
      bytes: encoder.encode("def test_parse(): assert parse('1') == 1\n"),
      modified_at: now(),
    });
  // The slow scenario (slow-run.mjs): the demo its calls write.
  else if (text === SLOW_RUN)
    for (const [path, content] of Object.entries(SLOW_RUN_FILES))
      files.set(path, { bytes: encoder.encode(content), modified_at: now() });
}

/** The workspace as a run found it (task 068c): uploads and private state are left out, as by the gateway. */
export function snapshot(conversation, run) {
  (conversation.snapshots ??= []).push({ run, files: tracked(conversation) });
}

// As the gateway: uploads, private state and generated caches are not changes.
const CACHES =
  /(^|\/)(__pycache__|\.pytest_cache|\.mypy_cache|\.ruff_cache|\.tox|\.venv|\.git|node_modules)\//;
function tracked(conversation) {
  return new Map(
    [...workspace(conversation)].filter(
      ([path]) =>
        !path.startsWith("uploads/") &&
        !path.startsWith(".tqa/") &&
        !CACHES.test(path),
    ),
  );
}

/** Line operations of a longest-common-subsequence diff (fixture files are small). */
function operations(before, after) {
  const a = before ? before.replace(/\n$/, "").split("\n") : [];
  const b = after ? after.replace(/\n$/, "").split("\n") : [];
  const table = Array.from({ length: a.length + 1 }, () =>
    new Array(b.length + 1).fill(0),
  );
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--)
      table[i][j] =
        a[i] === b[j]
          ? table[i + 1][j + 1] + 1
          : Math.max(table[i + 1][j], table[i][j + 1]);
  const lines = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      lines.push(` ${a[i++]}`);
      j++;
    } else if (
      i < a.length &&
      (j === b.length || table[i + 1][j] >= table[i][j + 1])
    )
      // Removed lines first, as a unified diff lists them.
      lines.push(`-${a[i++]}`);
    else lines.push(`+${b[j++]}`);
  }
  return { lines, old: a.length, new: b.length };
}

function compared(conversation, run) {
  const snapshots = conversation.snapshots ?? [];
  const index = snapshots.findIndex((value) => value.run === run);
  if (index < 0) return null;
  const next = snapshots[index + 1];
  const after = next ? next.files : tracked(conversation);
  const before = snapshots[index].files;
  const paths = [...new Set([...before.keys(), ...after.keys()])].sort();
  const files = [];
  for (const path of paths) {
    const old = before.get(path);
    const current = after.get(path);
    if (
      old &&
      current &&
      Buffer.compare(Buffer.from(old.bytes), Buffer.from(current.bytes)) === 0
    )
      continue;
    const oldText = old ? text(old.bytes) : "";
    const newText = current ? text(current.bytes) : "";
    const binary = oldText === null || newText === null;
    const ops = binary ? { lines: [] } : operations(oldText, newText);
    files.push({
      path,
      status: !old ? "added" : !current ? "deleted" : "modified",
      added: ops.lines.filter((line) => line.startsWith("+")).length,
      removed: ops.lines.filter((line) => line.startsWith("-")).length,
      binary,
      too_large: false,
      ops,
    });
  }
  return { live: !next, files };
}

/** GET /runs/{run}/changes and /changes/diff; returns false for any other path. */
export function changesRoute(conversation, suffix, url, reply) {
  const route = /^\/runs\/([A-Za-z0-9_-]{1,64})\/changes(\/diff)?$/.exec(
    suffix,
  );
  if (!route) return false;
  const result = compared(conversation, route[1]);
  if (!result) {
    reply.error(
      404,
      "changes_unavailable",
      "This run's changes were not recorded.",
    );
    return true;
  }
  const running =
    result.live && conversation.running && conversation.run === route[1];
  if (!route[2]) {
    reply.json(200, {
      run_id: route[1],
      live: result.live,
      running,
      truncated: false,
      files: result.files.map((file) => ({ ...file, ops: undefined })),
    });
    return true;
  }
  const path = url.searchParams.get("path");
  const file = result.files.find((value) => value.path === path);
  if (!file) {
    reply.error(404, "not_found", "This file did not change.");
    return true;
  }
  const { ops } = file;
  const unified = file.binary
    ? null
    : `--- a/${path}\n+++ b/${path}\n@@ -1,${ops.old} +1,${ops.new} @@\n${ops.lines.join("\n")}\n`;
  // FileDiff has exactly these fields (openapi.json); the list's line counts aren't part of it.
  reply.json(200, {
    path: file.path,
    status: file.status,
    unified,
    binary: file.binary,
    too_large: file.too_large,
    truncated: false,
  });
  return true;
}
