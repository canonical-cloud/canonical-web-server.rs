const allowedReadOnlyReusableWorkflows = new Set([
  "canonical-cloud/canonical.cloud/.github/workflows/agents-hierarchy.yml@adffdd4fe89aebdff1494195389b16a3cebc308c",
  "canonical-cloud/.github/.github/workflows/reusable-policy.yml@0ea46201f6a0055aa5d28c465488394d3c2c56c0",
]);

const publisherSignals = [
  ["write-all permissions", /\bpermissions\s*:\s*["']?write-all["']?/i],
  [
    "contents write permission",
    /(?:^\s*|[{,]\s*)["']?contents["']?\s*:\s*["']?write["']?/im,
  ],
  [
    "package write permission",
    /(?:^\s*|[{,]\s*)["']?packages["']?\s*:\s*["']?write["']?/im,
  ],
  [
    "attestation write permission",
    /(?:^\s*|[{,]\s*)["']?attestations["']?\s*:\s*["']?write["']?/im,
  ],
  [
    "OIDC write permission",
    /(?:^\s*|[{,]\s*)["']?id-token["']?\s*:\s*["']?write["']?/im,
  ],
  ["secret-backed credential", /\$\{\{\s*secrets(?:\.|\[)/i],
  [
    "inherited reusable-workflow secrets",
    /^\s*secrets\s*:\s*["']?inherit["']?\s*$/im,
  ],
  ["Docker registry login action", /\buses\s*:\s*["']?docker\/login-action@/i],
  [
    "Docker publishing action",
    /\buses\s*:\s*["']?docker\/build-push-action@/i,
  ],
  [
    "registry attestation upload",
    /^\s*push-to-registry\s*:\s*["']?true["']?\s*(?:#.*)?$/im,
  ],
  [
    "publishing action input",
    /^\s*push\s*:\s*["']?true["']?\s*(?:#.*)?$/im,
  ],
  ["Docker push command", /\bdocker\b[^\r\n]*\b(?:image\s+)?push\b/i],
  ["Docker registry login command", /\bdocker\b[^\r\n]*\blogin\b/i],
  ["Docker build push flag", /\bdocker\b[^\r\n]*\s--push(?:[=\s]|$)/i],
  ["OCI publishing tool", /\b(?:oras|crane|skopeo)\b/i],
  [
    "alternate container push command",
    /\b(?:podman|buildah|nerdctl)\b[^\r\n]*\bpush\b/i,
  ],
  ["language package publishing command", /\b(?:npm|cargo)\s+publish\b/i],
  ["Python package upload command", /\btwine\s+upload\b/i],
  ["inbound reusable workflow", /^\s*["']?workflow_call["']?\s*:/im],
];

function executableWorkflowText(workflow) {
  // Backslash-newline belongs to the same shell command. Normalize it
  // before scanning to reject publishing verbs split across physical lines.
  return workflow
    .replace(/^\s*#.*$/gm, "")
    .replace(/\\\r?\n[ \t]*/g, " ");
}

function hasReadOnlyTopLevelPermissions(workflow) {
  const lines = workflow.split(/\r?\n/);
  const permissionLines = lines
    .map((line, index) => [line, index])
    .filter(([line]) => /^permissions:\s*(?:#.*)?$/.test(line));
  if (permissionLines.length !== 1) return false;

  const [, start] = permissionLines[0];
  const entries = [];
  for (let index = start + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim() === "" || /^\s*#/.test(line)) continue;
    if (!/^\s/.test(line)) break;
    entries.push(line.trim().replace(/\s+#.*$/, ""));
  }
  return entries.length === 1 && entries[0] === "contents: read";
}

function outboundReusableWorkflowViolations(workflow) {
  const pattern =
    /^\s*uses\s*:\s*["']?([^\s"'#]+\.github\/workflows\/[^\s"'#]+@[^\s"'#]+)["']?\s*(?:#.*)?$/gim;
  return [...workflow.matchAll(pattern)]
    .map((match) => match[1])
    .filter((target) => !allowedReadOnlyReusableWorkflows.has(target))
    .map(() => "outbound reusable workflow");
}

export function workflowViolations(workflow) {
  const executable = executableWorkflowText(workflow);
  const violations = publisherSignals
    .filter(([, pattern]) => pattern.test(executable))
    .map(([description]) => description);
  violations.push(...outboundReusableWorkflowViolations(executable));
  if (!hasReadOnlyTopLevelPermissions(workflow)) {
    violations.push("top-level permissions are not exactly contents: read");
  }
  return violations;
}

