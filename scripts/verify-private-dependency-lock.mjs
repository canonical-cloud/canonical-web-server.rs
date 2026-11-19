import assert from "node:assert/strict";
import { lstat, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const allowedGitDependencies = new Map([
  ["canonical-interfaces", "https://github.com/canonical-cloud/canonical-interfaces"],
  ["canonical-lib", "https://github.com/canonical-cloud/canonical-lib-core"],
  ["canonical-orm-core", "https://github.com/canonical-cloud/canonical-orm-core"],
]);
const sha40 = /^[a-f0-9]{40}$/;

function quotedAttribute(attributes, key) {
  const matches = [...attributes.matchAll(new RegExp("(?:^|,)\\s*" + key + "\\s*=\\s*\"([^\"]*)\"", "g"))];
  assert.equal(matches.length, 1, "expected exactly one quoted " + key + " attribute");
  return matches[0][1];
}

export function verifyPrivateDependencyLock(manifest, lockfile) {
  assert.match(lockfile, /^version\s*=\s*\d+\s*$/m, "Cargo.lock version is required");
  assert.match(lockfile, /^\[\[package\]\]\s*$/m, "Cargo.lock package entries required");

  const heading = /^\[dependencies\]\s*$/m.exec(manifest);
  assert.ok(heading, "Cargo.toml needs [dependencies]");
  const remainder = manifest.slice(heading.index + heading[0].length);
  const nextSection = /^\[[^\r\n]+\]\s*$/m.exec(remainder);
  const section = nextSection ? remainder.slice(0, nextSection.index) : remainder;
  // No build/dev/target/patch Git dependencies may silently escape the
  // direct-dependency review. Any addition needs an explicit policy update.
  const outsideDependencies = manifest.slice(0, heading.index) +
    "\n" + (nextSection ? remainder.slice(nextSection.index) : "");
  assert.doesNotMatch(outsideDependencies, /^[^#\r\n]*\bgit\s*=/gm,
    "unreviewed Git reference outside [dependencies]");
  const dependencies = section.split(/\r?\n/);
  const manifestGit = new Map();
  for (const line of dependencies) {
    if (!/\bgit\s*=/.test(line)) continue;
    const assignment = line.match(/^\s*([a-zA-Z0-9_-]+)\s*=\s*\{([^}\n]+)\}\s*(?:#.*)?$/);
    assert.ok(assignment, "Git dependencies must use reviewable single-line inline tables");
    const [, packageName, attributes] = assignment;
    assert.ok(allowedGitDependencies.has(packageName), "unapproved Git dependency: " + packageName);
    assert.ok(!manifestGit.has(packageName), "duplicate Git dependency: " + packageName);
    const url = quotedAttribute(attributes, "git");
    const rev = quotedAttribute(attributes, "rev");
    assert.equal(url, allowedGitDependencies.get(packageName), "noncanonical Git URL: " + packageName);
    assert.match(rev, sha40, "dependency revision must be a full immutable SHA: " + packageName);
    assert.doesNotMatch(attributes, /(?:^|,)\s*(?:branch|tag)\s*=/, "branch or tag pin is not immutable");

    assert.match(attributes,
      /^\s*(?:git\s*=\s*"[^"]+"\s*,\s*rev\s*=\s*"[^"]+"|rev\s*=\s*"[^"]+"\s*,\s*git\s*=\s*"[^"]+")\s*$/,
      "Git dependency attributes must contain only approved git and rev keys");
    manifestGit.set(packageName, { url, rev });
  }
  assert.equal(manifestGit.size, allowedGitDependencies.size, "exact expected Git dependency set required");
  for (const name of allowedGitDependencies.keys()) {
    assert.ok(manifestGit.has(name), "missing Git dependency: " + name);
  }

  const packages = lockfile.split(/^\[\[package\]\]\s*$/m).slice(1);
  const lockedGit = new Map();
  for (const pkg of packages) {
    const source = pkg.match(/^source\s*=\s*"([^"]+)"\s*$/m)?.[1];
    const packageName = pkg.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
    if (allowedGitDependencies.has(packageName)) {
      assert.ok(source?.startsWith("git+"),
        "approved private dependency must use its pinned Git source: " + packageName);
    }
    if (!source?.startsWith("git+")) continue;
    const name = pkg.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
    assert.ok(name && allowedGitDependencies.has(name), "unexpected Git source in Cargo.lock");
    assert.ok(!lockedGit.has(name), "duplicate locked Git dependency: " + name);
    lockedGit.set(name, source);
  }
  assert.equal(lockedGit.size, allowedGitDependencies.size, "lockfile Git source count differs from manifest");
  for (const [name, { url, rev }] of manifestGit.entries()) {
    assert.equal(lockedGit.get(name), "git+" + url + "?rev=" + rev + "#" + rev,
      "locked Git source differs from immutable manifest pin: " + name);
  }
  return { pinnedDependencies: manifestGit.size };
}


const approvedWorkspaceMembers = new Set([
  ".",
  "crates/canonical-auth",
  "crates/canonical-config",
  "crates/canonical-session",
  "crates/canonical-store",
  "services/canonical-session-revoker",
]);

export function workspaceMembers(rootManifest) {
  const heading = /^\[workspace\]\s*$/m.exec(rootManifest);
  assert.ok(heading, "root Cargo.toml must declare [workspace]");
  const remainder = rootManifest.slice(heading.index + heading[0].length);
  const next = /^\[[^\r\n]+\]\s*$/m.exec(remainder);
  const section = next ? remainder.slice(0, next.index) : remainder;
  const declaration = /^\s*members\s*=\s*\[([\s\S]*?)\]\s*(?:#.*)?$/m.exec(section);
  assert.ok(declaration, "workspace members must be an explicit reviewed array");
  const raw = declaration[1].replace(/#[^\r\n]*/g, "");
  const members = [...raw.matchAll(/"([^"]+)"\s*,?/g)].map((match) => match[1]);
  assert.equal(raw.replace(/"[^"]+"\s*,?/g, "").trim(), "",
    "workspace membership must use plain literal paths");
  assert.ok(members.length > 0, "workspace member list must not be empty");
  assert.equal(new Set(members).size, members.length, "duplicate workspace member");
  assert.ok(members.includes("."), "root package must remain a workspace member");
  for (const member of members) {
    assert.match(member, /^(?:\.|[A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*)$/,
      "workspace member is not a safe literal relative path: " + member);
    assert.ok(approvedWorkspaceMembers.has(member),
      "new workspace member needs explicit private-dependency policy review: " + member);
  }
  assert.equal(members.length, approvedWorkspaceMembers.size,
    "reviewed workspace member set changed");
  return members;
}

export function verifyWorkspaceMemberGitSources(rootManifest, memberManifests) {
  const members = workspaceMembers(rootManifest);
  assert.equal(memberManifests.size, members.length - 1,
    "all non-root workspace members must be inspected");
  for (const member of members) {
    if (member === ".") continue;
    assert.ok(memberManifests.has(member), "missing workspace Cargo.toml: " + member);
    const content = memberManifests.get(member);
    assert.equal(typeof content, "string", "invalid workspace Cargo.toml: " + member);
    assert.match(content, /^\[package\]\s*$/m,
      "workspace member must declare a normal package: " + member);
    // A child may not introduce a Git source in build, dev, target, patch,
    // workspace-inherited or direct dependencies. Comments are not executable.
    const active = content.replace(/^\s*#.*$/gm, "");
    assert.doesNotMatch(active, /\bgit\s*=/,
      "unapproved Git source in workspace member: " + member);
  }
  return { checkedMembers: members.length - 1 };
}

async function readCheckedWorkspaceMembers(rootManifest) {
  const manifests = new Map();
  for (const member of workspaceMembers(rootManifest)) {
    if (member === ".") continue;
    const url = new URL("../" + member + "/Cargo.toml", import.meta.url);
    const metadata = await lstat(fileURLToPath(url));
    assert.ok(metadata.isFile(), "workspace manifest must be a regular file: " + member);
    manifests.set(member, await readFile(url, "utf8"));
  }
  return verifyWorkspaceMemberGitSources(rootManifest, manifests);
}

if (process.argv[1] && import.meta.url === new URL("file://" + process.argv[1]).href) {
  const manifest = await readFile(new URL("../Cargo.toml", import.meta.url), "utf8");
  const lockfile = await readFile(new URL("../Cargo.lock", import.meta.url), "utf8");
  const result = verifyPrivateDependencyLock(manifest, lockfile);
  const checked = await readCheckedWorkspaceMembers(manifest);
  console.log("Offline immutable Git dependency contract passed:", result.pinnedDependencies,
    "pinned dependencies and", checked.checkedMembers,
    "workspace member manifests. Private Cargo resolution/compilation NOT verified.");
}
