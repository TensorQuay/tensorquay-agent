// Every browser spec loads: Playwright lists the whole suite, stack specs included, and each spec file under e2e/ is in
// the list. A spec whose imports can't load (an app module reaching the generated validators, say) makes Playwright
// list nothing at all, so without this check a full run fails before its first test and nothing else notices.
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";

const specs = readdirSync("e2e", { recursive: true })
  .map(String)
  .filter((file) => file.endsWith(".spec.ts"));
const run = spawnSync("pnpm", ["-s", "exec", "playwright", "test", "--list"], {
  encoding: "utf8",
  env: { ...process.env, TQA_E2E_LIST_ALL: "1" },
});
const output = `${run.stdout}${run.stderr}`;
const total = /Total: (\d+) tests? in (\d+) files?/.exec(output);
const files = total ? Number(total[2]) : 0;
const problems = [];
if (run.status !== 0) problems.push(`playwright --list exited ${run.status}`);
if (/^Error:/m.test(output))
  problems.push(output.split("\n").find((line) => line.startsWith("Error:")));
if (files !== specs.length)
  problems.push(`listed ${files} spec files of the ${specs.length} under e2e/`);
if (problems.length) {
  console.error(
    `e2e:list: the browser suite doesn't load:\n  ${problems.join("\n  ")}`,
  );
  process.exit(1);
}
console.log(`e2e:list: ${total[1]} tests in all ${files} spec files load.`);
