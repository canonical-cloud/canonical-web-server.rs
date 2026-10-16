import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { verifyPrivateDependencyLock } from "../scripts/verify-private-dependency-lock.mjs";

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
