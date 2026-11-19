import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { verifyPrivateDependencyLock, workspaceMembers, verifyWorkspaceMemberGitSources, rejectRepositoryCargoOverrides } from "../scripts/verify-private-dependency-lock.mjs";

const pins = [
  ["canonical-interfaces", "canonical-interfaces", "0cab33c2b2a494d2368ef1da0ebe5d614b3a96ef"],
  ["canonical-lib", "canonical-lib-core", "54ce24670694b1f920e4ab69043378db24fd7296"],
  ["canonical-orm-core", "canonical-orm-core", "954fd9811b9c6db1acf15d3a282e65395e347b5a"],
];
const toml = [
  "[package]", 'name = "sample"', "[dependencies]",
  ...pins.map(([name, repo, rev]) =>
    name + ' = { git = "https://github.com/canonical-cloud/' + repo + '", rev = "' + rev + '" }'),
  '[dev-dependencies]', 'example = "1.0"',
].join("\n") + "\n";
const packages = pins.map(([name, repo, rev]) =>
  '[[package]]\nname = "' + name + '"\nversion = "0.1.0"\nsource = "git+https://github.com/canonical-cloud/' +
  repo + '?rev=' + rev + '#' + rev + '"\n');
const lock = 'version = 4\n\n' + packages.join("\n");

test("actual repository manifest and lock agree on immutable Git sources", async () => {
  const manifest = await readFile(new URL("../Cargo.toml", import.meta.url), "utf8");
  const lockfile = await readFile(new URL("../Cargo.lock", import.meta.url), "utf8");
  assert.equal(verifyPrivateDependencyLock(manifest, lockfile).pinnedDependencies, 3);
});
test("accept exactly three matching immutable source references", () => {
  assert.equal(verifyPrivateDependencyLock(toml, lock).pinnedDependencies, 3);
});
test("reject short Git revision", () => {
  assert.throws(() => verifyPrivateDependencyLock(toml.replace(pins[0][2], "main"), lock), /immutable SHA/);
});
test("reject credential-bearing Git URL", () => {
  assert.throws(() => verifyPrivateDependencyLock(toml.replace("https://github.com/", "https://user:password@github.com/"), lock), /noncanonical Git URL/);
});
test("reject Git branches even alongside an immutable rev", () => {
  const bad = toml.replace('rev = "' + pins[0][2] + '" }', 'rev = "' + pins[0][2] + '", branch = "main" }');
  assert.throws(() => verifyPrivateDependencyLock(bad, lock), /branch or tag/);
});
test("reject manifest and lock SHA mismatch", () => {
  const bad = lock.replace("#" + pins[0][2], "#" + "a".repeat(40));
  assert.throws(() => verifyPrivateDependencyLock(toml, bad), /locked Git source differs/);
});
test("reject missing locked Git package", () => {
  assert.throws(() => verifyPrivateDependencyLock(toml, 'version = 4\n\n' + packages.slice(1).join("\n")), /count differs/);
});
test("reject duplicate locked Git package", () => {
  assert.throws(() => verifyPrivateDependencyLock(toml, lock + "\n" + packages[0]), /duplicate locked/);
});
test("reject an unexpected additional Git source", () => {
  const bad = lock + '\n[[package]]\nname = "attacker"\nsource = "git+https://example.com/a?branch=main#123"\n';
  assert.throws(() => verifyPrivateDependencyLock(toml, bad), /unexpected Git source/);
});
test("reject non-reviewed Git manifest dependencies", () => {
  const bad = toml.replace("[dev-dependencies]", 'new-lib = { git = "https://example.com/new", rev = "' + "f".repeat(40) + '" }\n[dev-dependencies]');
  assert.throws(() => verifyPrivateDependencyLock(bad, lock), /unapproved Git dependency/);
});
test("reject duplicate direct Git manifest entry", () => {
  const line = 'canonical-lib = { git = "https://github.com/canonical-cloud/canonical-lib-core", rev = "' + pins[1][2] + '" }';
  assert.throws(() => verifyPrivateDependencyLock(toml.replace("[dev-dependencies]", line + "\n[dev-dependencies]"), lock), /duplicate Git dependency/);
});
test("reject malformed or incomplete manifest syntax", () => {
  assert.throws(() => verifyPrivateDependencyLock(toml.replace("git = ", "git: "), lock), /exact expected Git dependency set/);
});
test("reject malformed or incomplete Cargo.lock", () => {
  for (const bad of ["", "version = 4\n", "[[package]]\nname = \"other\"\n"]) {
    assert.throws(() => verifyPrivateDependencyLock(toml, bad));
  }
});
test("reject new Git dependencies in dev/build/target sections", () => {
  for (const heading of ["[dev-dependencies]", "[build-dependencies]",
    "[target.'cfg(unix)'.dependencies]"]) {
    const bad = toml + "\n" + heading + "\n" +
      'external = { git = "https://example.com/other", rev = "' + "a".repeat(40) + '" }';
    assert.throws(() => verifyPrivateDependencyLock(bad, lock), /outside \[dependencies\]/);
  }
});
test("permit ordinary registry dependencies in other sections", () => {
  assert.equal(verifyPrivateDependencyLock(toml, lock).pinnedDependencies, 3);
});


const safeWorkspaceMember = '[package]\nname = "fixture"\nversion = "0.1.0"\n[dependencies]\nserde = "1"\n';
const workspaceManifest = '[workspace]\nmembers = [".", "crates/canonical-auth", "crates/canonical-config", "crates/canonical-session", "crates/canonical-store", "services/canonical-session-revoker"]\nresolver = "2"\n[package]\nname = "root"\n';
const fixtureMembers = () => new Map(workspaceMembers(workspaceManifest)
  .filter((member) => member !== ".").map((member) => [member, safeWorkspaceMember]));

test("audit the checked-in root and every reviewed workspace member", async () => {
  const root = await readFile(new URL("../Cargo.toml", import.meta.url), "utf8");
  const manifests = new Map();
  for (const member of workspaceMembers(root)) {
    if (member === ".") continue;
    manifests.set(member, await readFile(new URL("../" + member + "/Cargo.toml", import.meta.url), "utf8"));
  }
  assert.equal(verifyWorkspaceMemberGitSources(root, manifests).checkedMembers, 5);
});

test("allow only the reviewed workspace member set without Git sources", () => {
  assert.equal(verifyWorkspaceMemberGitSources(workspaceManifest, fixtureMembers()).checkedMembers, 5);
});

test("reject hidden Git dependencies in every workspace member section", () => {
  for (const section of ["[dependencies]", "[build-dependencies]", "[dev-dependencies]",
    "[target.'cfg(unix)'.dependencies]", "[patch.crates-io]", "[workspace.dependencies]"]) {
    const manifests = fixtureMembers();
    manifests.set("crates/canonical-auth",
      safeWorkspaceMember + "\n" + section + '\nevil = { git = "https://example.org/attacker", rev = "' +
      "a".repeat(40) + '" }\n');
    assert.throws(() => verifyWorkspaceMemberGitSources(workspaceManifest, manifests),
      /unapproved Git source/);
  }
});

test("reject missing, extra, or nonpackage workspace manifests", () => {
  const missing = fixtureMembers();
  missing.delete("crates/canonical-auth");
  assert.throws(() => verifyWorkspaceMemberGitSources(workspaceManifest, missing),
    /all non-root workspace members/);
  const extra = fixtureMembers();
  extra.set("crates/unreviewed", safeWorkspaceMember);
  assert.throws(() => verifyWorkspaceMemberGitSources(workspaceManifest, extra),
    /all non-root workspace members/);
  const malformed = fixtureMembers();
  malformed.set("crates/canonical-auth", "[dependencies]\nserde = '1'\n");
  assert.throws(() => verifyWorkspaceMemberGitSources(workspaceManifest, malformed),
    /normal package/);
});

test("reject additional, duplicate, or traversing workspace member paths", () => {
  const renamed = workspaceManifest.replace('"crates/canonical-auth"', '"crates/attacker"');
  assert.throws(() => verifyWorkspaceMemberGitSources(renamed, fixtureMembers()),
    /policy review/);
  const duplicate = workspaceManifest.replace('"crates/canonical-auth"', '"crates/canonical-config"');
  assert.throws(() => verifyWorkspaceMemberGitSources(duplicate, fixtureMembers()),
    /duplicate workspace member/);
  const traversal = workspaceManifest.replace('"crates/canonical-auth"', '"../outside"');
  assert.throws(() => verifyWorkspaceMemberGitSources(traversal, fixtureMembers()),
    /safe literal relative path/);
  const wildcard = workspaceManifest.replace('"crates/canonical-auth"', '"crates/*"');
  assert.throws(() => verifyWorkspaceMemberGitSources(wildcard, fixtureMembers()),
    /safe literal relative path/);
});

test("reject unsupported Git dependency alias, path, and feature attributes", () => {
  for (const extra of ['package = "unexpected"', 'path = "../elsewhere"', 'features = ["unsafe"]']) {
    const bad = toml.replace('rev = "' + pins[0][2] + '" }',
      'rev = "' + pins[0][2] + '", ' + extra + ' }');
    assert.throws(() => verifyPrivateDependencyLock(bad, lock),
      /only approved git and rev keys/);
  }
});

test("reject a registry or local shadow of an approved locked Git package", () => {
  const shadow = '[[package]]\nname = "canonical-lib"\nversion = "2.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n';
  assert.throws(() => verifyPrivateDependencyLock(toml, lock + "\n" + shadow),
    /must use its pinned Git source/);
  const localShadow = '[[package]]\nname = "canonical-lib"\nversion = "2.0.0"\n';
  assert.throws(() => verifyPrivateDependencyLock(toml, lock + "\n" + localShadow),
    /must use its pinned Git source/);
});


test("allow reviewed local Cargo path edges inside the workspace", () => {
  const members = fixtureMembers();
  const root = workspaceManifest.replace('[package]\nname = "root"',
    '[package]\nname = "root"\n[dependencies]\nauth = { path = "crates/canonical-auth" }');
  members.set("crates/canonical-session", safeWorkspaceMember +
    'store = { path = "../canonical-store" }\n');
  assert.equal(verifyWorkspaceMemberGitSources(root, members).checkedMembers, 5);
});

test("reject local dependency edges to external or undeclared packages", () => {
  for (const target of ["../external", "crates/hidden", "/tmp/private", "crates\\evil"]) {
    const members = fixtureMembers();
    const root = workspaceManifest.replace('[package]\nname = "root"',
      '[package]\nname = "root"\n[dependencies]\nevil = { path = "' + target.replaceAll("\\", "\\\\") + '" }');
    assert.throws(() => verifyWorkspaceMemberGitSources(root, members),
      /local Cargo (dependency leaves reviewed workspace|path must be literal|dependency path forbidden)|absolute Cargo dependency/);
  }
});

test("reject local dependency edges introduced from child manifests", () => {
  const members = fixtureMembers();
  members.set("crates/canonical-session", safeWorkspaceMember +
    'evil = { path = "../../../unreviewed", package = "secret" }\n');
  assert.throws(() => verifyWorkspaceMemberGitSources(workspaceManifest, members),
    /leaves reviewed workspace/);
});


test("reject repository-local Cargo configuration that could redirect pinned Git sources", () => {
  rejectRepositoryCargoOverrides(["Cargo.toml", "crates/canonical-auth/Cargo.toml"]);
  for (const path of [".cargo", ".cargo/config", ".cargo/config.toml",
    "crates/canonical-auth/.cargo/config.toml",
    "services/canonical-session-revoker/.cargo/config"]) {
    assert.throws(() => rejectRepositoryCargoOverrides([path]),
      /independent admission/);
  }
});

test("offline pin workflow watches Cargo configuration paths", async () => {
  const workflow = await readFile(new URL("../.github/workflows/private-persistence-lock.yml", import.meta.url), "utf8");
  assert.equal((workflow.match(/"\\.cargo\\/\\*\\*"/g) ?? []).length, 2,
    "both push and pull_request must trigger checks on root Cargo configuration changes");
});
