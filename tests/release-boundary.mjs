import assert from "node:assert/strict";
import { access, readdir, readFile } from "node:fs/promises";
import { workflowViolations } from "./release-boundary-policy.mjs";

const root = new URL("../", import.meta.url);
const workflowsDirectory = new URL(".github/workflows/", root);

const workflowNames = (await readdir(workflowsDirectory))
  .filter((name) => /\.ya?ml$/i.test(name))
  .sort();
assert.ok(workflowNames.length > 0, "expected GitHub Actions workflows");

const violations = [];
for (const name of workflowNames) {
  const workflow = await readFile(new URL(name, workflowsDirectory), "utf8");
  for (const violation of workflowViolations(workflow)) {
    violations.push(`${name}: ${violation}`);
  }
}

await assert.rejects(
  access(new URL("release/current-containers.env", root)),
  /ENOENT/,
  "application repositories must not declare deployable releases",
);

assert.deepEqual(
  violations,
  [],
  `canonical-monorepo is the sole release publisher:\n${violations.join("\n")}`,
);
