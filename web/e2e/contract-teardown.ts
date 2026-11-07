import { readFile } from "node:fs/promises";

import { driftLog } from "./front/contract.mjs";

/**
 * Fails the run when the fake front answered outside the gateway's contract (openapi.json), even if every page coped:
 * the fake front must not drift from the gateway (GW-I-129).
 */
export default async function contractTeardown() {
  const drift = await readFile(driftLog(3100), "utf8").catch(() => "");
  if (drift.trim())
    throw new Error(
      `The fake front drifted from openapi.json:\n${[...new Set(drift.trim().split("\n"))].join("\n")}`,
    );
}
