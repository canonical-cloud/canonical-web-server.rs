import assert from "node:assert/strict";
import { test } from "node:test";
import { workflowViolations } from "./release-boundary-policy.mjs";

const safePreamble = "permissions:\n  contents: read\n";

test("accepts approved read-only workflows", () => {
  const safeValidationWorkflow = `${safePreamble}jobs:\n  validate:\n    uses: canonical-cloud/canonical.cloud/.github/workflows/agents-hierarchy.yml@adffdd4fe89aebdff1494195389b16a3cebc308c`;
  assert.deepEqual(
    workflowViolations(safeValidationWorkflow),
    [],
    "the immutable read-only hierarchy validator is not a release publisher",
  );
  
  const safeOrganizationPolicyWorkflow = `${safePreamble}jobs:\n  policy:\n    uses: canonical-cloud/.github/.github/workflows/reusable-policy.yml@0ea46201f6a0055aa5d28c465488394d3c2c56c0`;
  assert.deepEqual(
    workflowViolations(safeOrganizationPolicyWorkflow),
    [],
    "the immutable read-only organization policy is not a release publisher",
  );
  assert.deepEqual(workflowViolations(`${safePreamble}jobs:\n  ok:\n    steps:\n      - run: >\n          echo safe\n          echo harmless`), []);
});

const adversarialFixtures = [
  ["write-all permissions", "permissions: write-all"],
  [
    "contents write permission",
    "jobs:\n  release:\n    permissions: { contents: write }",
  ],
  [
    "package write permission",
    "jobs:\n  release:\n    permissions:\n      packages: write",
  ],
  [
    "attestation write permission",
    "jobs:\n  release:\n    permissions: { attestations: write }",
  ],
  [
    "OIDC write permission",
    "jobs:\n  release:\n    permissions:\n      id-token: write",
  ],
  ["secret-backed credential", "env:\n  TOKEN: ${{ secrets.REGISTRY_TOKEN }}"],
  [
    "inherited reusable-workflow secrets",
    "jobs:\n  release:\n    secrets: inherit",
  ],
  [
    "Docker registry login action",
    "steps:\n  - uses: docker/login-action@deadbeef",
  ],
  [
    "Docker publishing action",
    "steps:\n  - uses: docker/build-push-action@deadbeef",
  ],
  ["registry attestation upload", "with:\n  push-to-registry: true"],
  ["publishing action input", "with:\n  push: true"],
  [
    "Docker push command",
    "run: |\n  docker image \\\n    push ghcr.io/x/app:sha",
  ],
  [
    "Docker registry login command",
    "run: |\n  docker \\\n    login ghcr.io",
  ],
  [
    "Docker build push flag",
    "run: |\n  docker buildx build \\\n    --push -t ghcr.io/x/app:sha .",
  ],
  [
    "language package publishing command",
    "run: |\n  npm \\\n    publish",
  ],
  [
    "Docker push command",
    "run: >\n  docker image\n  push ghcr.io/x/app:sha",
  ],
  [
    "Docker build push flag",
    "run: >-\n  docker buildx build\n  --push -t ghcr.io/x/app:sha .",
  ],
  [
    "language package publishing command",
    "run: >\n  npm\n  publish",
  ],
  [
    "Docker push command",
    "run: docker --config /tmp/config image push ghcr.io/x/app:sha",
  ],
  [
    "Docker registry login command",
    "run: docker --config /tmp/config login ghcr.io",
  ],
  [
    "Docker build push flag",
    "run: docker buildx build --push -t ghcr.io/x/app:sha .",
  ],
  ["OCI publishing tool", "run: crane append --base source --new_layer layer"],
  [
    "alternate container push command",
    "run: buildah push app ghcr.io/x/app:sha",
  ],
  [
    "outbound reusable workflow",
    "jobs:\n  release:\n    uses: example/publisher/.github/workflows/release.yml@main",
  ],
  ["inbound reusable workflow", "on:\n  workflow_call:"],
];

for (const [expected, fixture] of adversarialFixtures) {
  test(`rejects ${expected}: ${fixture.slice(0, 42).replace(/\n/g, " ")}`, () => {
    const workflow = fixture.startsWith("permissions:")
      ? fixture
      : `${safePreamble}${fixture}`;
    assert.ok(
      workflowViolations(workflow).includes(expected),
      `expected ${expected} for:\n${fixture}`,
    );
  });
}
