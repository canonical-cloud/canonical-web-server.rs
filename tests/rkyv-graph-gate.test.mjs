import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const gate = fileURLToPath(new URL("../scripts/check-rkyv-active-graph.sh", import.meta.url));
const lockWithRkyv = 'version = 4\n[[package]]\nname = "rkyv"\nversion = "0.7.46"\n';
const lockWithoutRkyv = 'version = 4\n[[package]]\nname = "safe-crate"\nversion = "1.0.0"\n';

function probe(mode, lockContents = lockWithRkyv) {
  const dir = mkdtempSync(join(tmpdir(), "rkyv-gate-"));
  try {
    const lock = join(dir, "Cargo.lock");
    if (lockContents !== null) writeFileSync(lock, lockContents);
    const cargo = join(dir, "cargo");
    writeFileSync(cargo, [
      "#!/usr/bin/env bash",
      '[[ "$*" == *"--locked"* && "$*" == *"x86_64-unknown-linux-gnu"* ]] || exit 27',
      'case "$FAKE_CARGO_MODE" in',
      '  empty) exit 0 ;;',
      '  nothing) echo "nothing to print"; exit 0 ;;',
      '  mixed) printf "nothing to print\\nwarning: graph unresolved\\n"; exit 0 ;;',
      '  mismatch) echo "did not match any packages"; exit 0 ;;',
      '  reachable) echo "rkyv v0.7.46"; exit 0 ;;',
      '  indented-reachable) echo "  rkyv v0.7.46"; exit 0 ;;',
      '  auth-error) echo "failed to authenticate when downloading private repository" >&2; exit 101 ;;',
      '  empty-error) exit 17 ;;',
      '  unexplained) echo "warning: cannot prove absence"; exit 0 ;;',
      '  *) exit 29 ;;',
      "esac",
    ].join("\n") + "\n");
    chmodSync(cargo, 0o755);
    return spawnSync("bash", [gate, lock], {
      cwd: dir,
      env: { ...process.env, PATH: dir + ":" + process.env.PATH, FAKE_CARGO_MODE: mode },
      encoding: "utf8",
    });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("absent package from a recognizable lockfile is admissible", () => {
  const r = probe("auth-error", lockWithoutRkyv);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /absent from Cargo.lock/);
});
test("missing lockfile is never accepted", () => {
  const r = probe("nothing", null);
  assert.equal(r.status, 2);
  assert.match(r.stderr, /missing lockfile/);
});
test("empty or malformed lockfile is never accepted", () => {
  for (const raw of ["", "garbage", "version = 4\n"]) {
    const r = probe("nothing", raw);
    assert.equal(r.status, 2, r.stderr);
  }
});
test("single-quoted valid rkyv lock entry cannot bypass graph resolution", () => {
  const r = probe("auth-error", "version = 4\n[[package]]\nname = 'rkyv'\nversion = '0.7.46'\n");
  assert.equal(r.status, 101);
});
test("exact successful absence evidence is accepted", () => {
  const r = probe("nothing");
  assert.equal(r.status, 0, r.stderr);
});
test("empty successful Cargo output is not absence evidence", () => {
  const r = probe("empty");
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /cannot establish absence/);
});
test("mixed absence text and warning is rejected", () => {
  const r = probe("mixed");
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /cannot establish absence/);
});
test("a package mismatch message on exit zero is rejected", () => {
  const r = probe("mismatch");
  assert.notEqual(r.status, 0);
});
test("reachable rkyv 0.7.46 is rejected, including indented output", () => {
  for (const mode of ["reachable", "indented-reachable"]) {
    const r = probe(mode);
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /vulnerable rkyv/);
  }
});
test("private dependency authentication failure preserves Cargo exit code", () => {
  const r = probe("auth-error");
  assert.equal(r.status, 101);
  assert.match(r.stderr, /NOT verified/);
});
test("silent nonzero Cargo failure is rejected", () => {
  const r = probe("empty-error");
  assert.equal(r.status, 17);
  assert.match(r.stderr, /NOT verified/);
});
test("successful but unrecognized Cargo output is rejected", () => {
  const r = probe("unexplained");
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /cannot establish absence/);
});
