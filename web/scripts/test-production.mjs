import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import net from "node:net";
import { setTimeout as delay } from "node:timers/promises";

// WEB-U-007: exercise the default production artefact, independently of the visual test build.
const reservation = net.createServer();
reservation.listen(0, "127.0.0.1");
await once(reservation, "listening");
const address = reservation.address();
assert(address && typeof address !== "string");
await new Promise((resolve) => reservation.close(resolve));
const server = spawn(process.execPath, [".next/standalone/server.js"], {
  env: { ...process.env, HOSTNAME: "127.0.0.1", PORT: String(address.port) },
  stdio: ["ignore", "ignore", "pipe"],
});
let stderr = "";
server.stderr.on("data", (data) => {
  stderr = (stderr + String(data)).slice(-8192);
});
const closed = once(server, "close");
const origin = `http://127.0.0.1:${address.port}`;
try {
  const deadline = Date.now() + 30_000;
  let ready = false;
  while (Date.now() < deadline && server.exitCode === null) {
    try {
      ready = (
        await fetch(`${origin}/login`, { signal: AbortSignal.timeout(1000) })
      ).ok;
    } catch {
      /* The owned server may still be starting. */
    }
    if (ready) break;
    await delay(100);
  }
  assert(ready, `Production server did not become ready: ${stderr}`);
  const design = await fetch(`${origin}/design`, {
    signal: AbortSignal.timeout(5000),
  });
  assert.equal(
    design.status,
    404,
    "The production design route must be absent",
  );
  const home = await fetch(origin, { signal: AbortSignal.timeout(5000) });
  assert.equal(home.status, 200);
  assert(
    !(await home.text()).includes('href="/design"'),
    "The product navigation must omit the design reference",
  );
  console.log(
    "Production route check passed: /design is 404; navigation omits the reference.",
  );
} finally {
  server.kill("SIGTERM");
  const force = setTimeout(() => server.kill("SIGKILL"), 5000);
  try {
    await closed;
  } finally {
    clearTimeout(force);
  }
}
