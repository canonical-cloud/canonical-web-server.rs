import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = fileURLToPath(new URL("../scripts/check-rkyv-active-graph.sh", import.meta.url));
function runFixture(mode, lockIncludesRkyv = true) {
  const dir = mkdtempSync(join(tmpdir(), "rkyv-graph-"));
  try {
    const lock = join(dir, "Cargo.lock");
    writeFileSync(lock, lockIncludesRkyv
      ? 'version = 4\n[[package]]\nname = "rkyv"\nversion = "0.7.46"\n'
      : 'version = 4\n[[package]]\nname = "safe-crate"\nversion = "1.0.0"\n');
    const fakeCargo = join(dir, "cargo");
    writeFileSync(fakeCargo, [
      "#!/usr/bin/env bash",
      'case "$FAKE_CARGO_MODE" in',
      '  absent) exit 0 ;;',
      '  nothing) echo "nothing to print"; exit 0 ;;',
      '  reachable) echo "rkyv v0.7.46"; exit 0 ;;',
      '  auth-error) echo "failed to authenticate when downloading repository" >&2; exit 101 ;;',
      '  unexplained) echo "warning: could not prove absence"; exit 0 ;;',
      '  *) echo "unrecognized fixture" >&2; exit 17 ;;',
      "esac",
    ].join("\n") + "\n");
    chmodSync(fakeCargo, 0o755);
    return spawnSync("bash", [script, lock], {
      cwd: dir,
      env: { ...process.env, PATH: dir + ":" + process.env.PATH, FAKE_CARGO_MODE: mode },
      encoding: "utf8",
    });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("accepts dependency absent from Cargo.lock", () => {
  const r = runFixture("auth-error", false);
  assert.equal(r.status, 0, r.stderr);
});
test("accepts successfully verified empty dependency graph", () => {
  const r = runFixture("absent");
  assert.equal(r.status, 0, r.stderr);
});
test("accepts Cargo's explicit nothing-to-print result", () => {
  const r = runFixture("nothing");
  assert.equal(r.status, 0, r.stderr);
});
test("rejects reachable vulnerable rkyv", () => {
  const r = runFixture("reachable");
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /vulnerable rkyv/);
});
test("rejects failed Cargo fetch (private dependency auth)", () => {
  const r = runFixture("auth-error");
  assert.equal(r.status, 101);
  assert.match(r.stderr, /NOT verified/);
});
test("rejects unrecognized successful Cargo output", () => {
  const r = runFixture("unexplained");
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /cannot establish absence/);
});
