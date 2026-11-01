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
test("reject unreviewed attributes in private Git inline dependency tables", () => {
  for (const attribute of ['features = ["unsafe"]', 'optional = true', 'default-features = false']) {
    const bad = toml.replace('rev = "' + pins[0][2] + '" }',
      'rev = "' + pins[0][2] + '", ' + attribute + ' }');
    assert.throws(() => verifyPrivateDependencyLock(bad, lock), /attributes must be exactly/);
  }
});

test("pull-request offline pin workflow cannot read private credentials", async () => {
  const workflow = await readFile(new URL("../.github/workflows/private-persistence-lock.yml", import.meta.url), "utf8");
  assert.match(workflow, /^  pull_request:/m);
  assert.match(workflow, /^permissions:\s*\n  contents: read$/m);
  assert.match(workflow, /persist-credentials: false/);
  assert.match(workflow, /node --test tests\/private-dependency-lock\.test\.mjs/);
  assert.doesNotMatch(workflow, /\$\{\{\s*secrets[.\[]/);
  assert.doesNotMatch(workflow, /(?:PRIVATE_.*TOKEN|CANONICAL_LIB_READ_TOKEN|GIT_CONFIG_KEY_\d+|x-access-token)/);
  assert.doesNotMatch(workflow, /^\s+(?:contents|packages|actions|id-token): write\s*$/m);
});

test("permit ordinary registry dependencies in other sections", () => {
  assert.equal(verifyPrivateDependencyLock(toml, lock).pinnedDependencies, 3);
});


test("reject repeated manifest git/rev attributes in an inline table", () => {
  const base='rev = "' + pins[0][2] + '"';
  const duplicateRev=toml.replace(base,base+', '+base);
  assert.throws(() => verifyPrivateDependencyLock(duplicateRev,lock), /exactly one quoted rev/);
  const duplicateGit=toml.replace(
    'git = "https://github.com/canonical-cloud/canonical-interfaces"',
    'git = "https://github.com/canonical-cloud/canonical-interfaces", git = "https://github.com/canonical-cloud/canonical-interfaces"');
  assert.throws(() => verifyPrivateDependencyLock(duplicateGit,lock), /exactly one quoted git/);
});

test("reject unapproved Git declarations in workspace and patch sections", () => {
  for(const heading of ["[workspace.dependencies]", "[patch.crates-io]", "[target.'cfg(windows)'.build-dependencies]"]){
    const bad=toml+"\n"+heading+"\n"+
      'thirdparty = { git = "https://example.invalid/repo", rev = "'+"a".repeat(40)+'" }\n';
    assert.throws(() => verifyPrivateDependencyLock(bad,lock), /outside \[dependencies\]/, heading);
  }
});

test("reject Git URL aliases that bypass exact canonical source identity", () => {
  for(const alias of [
    "https://github.com/canonical-cloud/canonical-interfaces.git",
    "http://github.com/canonical-cloud/canonical-interfaces",
    "https://github.com/canonical-cloud/canonical-interfaces?ref=main",
    "https://github.com/Canonical-Cloud/canonical-interfaces",
    "https://github.com/canonical-cloud%2Fcanonical-interfaces",
  ]){
    const bad=toml.replace("https://github.com/canonical-cloud/canonical-interfaces",alias);
    assert.throws(() => verifyPrivateDependencyLock(bad,lock), /noncanonical Git URL/, alias);
  }
});


// defense-in-depth exact Cargo source tests; never resolve private Git in PR CI.
test("reject source substitutions that retain correct package names and revision count",()=>{
  const badUrl=lock.replace(
    "git+https://github.com/canonical-cloud/canonical-lib-core",
    "git+https://github.com/untrusted/canonical-lib-core");
  assert.throws(()=>verifyPrivateDependencyLock(toml,badUrl),/locked Git source differs/);
  const badRevision=lock.replace("?rev="+pins[1][2],"?rev="+"b".repeat(40));
  assert.throws(()=>verifyPrivateDependencyLock(toml,badRevision),/locked Git source differs/);
});

test("reject a lockfile using a moving tag or branch even with the pinned fragment",()=>{
  for(const selector of ["?branch=main#","?tag=v1#","?rev=main#"]){
    const original="?rev="+pins[0][2]+"#";
    const changed=lock.replace(original,selector);
    assert.throws(()=>verifyPrivateDependencyLock(toml,changed),/locked Git source differs/);
  }
});

test("reject unexpected private dependency aliases in Cargo.lock",()=>{
  const altered=lock.replace('name = "canonical-lib"','name = "canonical-lib-alias"');
  assert.throws(()=>verifyPrivateDependencyLock(toml,altered),/unexpected Git source/);
});

test("reject alternative manifest Git dependency packages and SSH schemes",()=>{
  const original="https://github.com/canonical-cloud/canonical-orm-core";
  for(const changed of [
    "ssh://git@github.com/canonical-cloud/canonical-orm-core",
    "git@github.com:canonical-cloud/canonical-orm-core",
    "https://user:token@github.com/canonical-cloud/canonical-orm-core",
    "https://github.com/canonical-cloud/canonical-orm-core/",
  ]){
    assert.throws(()=>verifyPrivateDependencyLock(toml.replace(original,changed),lock),/noncanonical Git URL/);
  }
});

test("reject unreviewed extra git dependencies in target-specific manifests",()=>{
  for(const section of [
    "[target.'cfg(target_os = \"linux\")'.dependencies]",
    "[target.'cfg(target_os = \"macos\")'.build-dependencies]",
    "[target.'cfg(target_arch = \"aarch64\")'.dev-dependencies]",
  ]){
    const altered=toml+"\n"+section+"\n"+
      'hostile = { git = "https://example.invalid/hostile", rev = "'+"f".repeat(40)+'" }\n';
    assert.throws(()=>verifyPrivateDependencyLock(altered,lock),/outside \[dependencies\]/,section);
  }
});

test("accepts unrelated ordinary registry lockfile packages without broadening Git allowlist",()=>{
  const registryEntry='\n[[package]]\nname = "harmless-registry-dependency"\nversion = "0.1.0"\n'+
    'source = "registry+https://github.com/rust-lang/crates.io-index"\n'+
    'checksum = "'+"a".repeat(64)+'"\n';
  assert.equal(verifyPrivateDependencyLock(toml,lock+registryEntry).pinnedDependencies,3);
});
