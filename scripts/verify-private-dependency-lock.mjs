import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

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

  const dependencySection = manifest.match(/^\[dependencies\]\s*$([\s\S]*?)(?=^\[|\s*$)/m);
  assert.ok(dependencySection, "Cargo.toml needs [dependencies]");
  const dependencies = dependencySection[1].split(/\r?\n/);
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

if (process.argv[1] && import.meta.url === new URL("file://" + process.argv[1]).href) {
  const manifest = await readFile(new URL("../Cargo.toml", import.meta.url), "utf8");
  const lockfile = await readFile(new URL("../Cargo.lock", import.meta.url), "utf8");
  const result = verifyPrivateDependencyLock(manifest, lockfile);
  console.log("Offline immutable Git dependency contract passed:", result.pinnedDependencies,
    "pinned dependencies. Private Cargo resolution/compilation NOT verified.");
}
