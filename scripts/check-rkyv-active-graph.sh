#!/usr/bin/env bash
# Do not mistake missing private Git authentication for a safe Cargo graph.
set -euo pipefail
lock_file="Cargo.lock"
if (( $# > 1 )); then
  echo "usage: check-rkyv-active-graph.sh [lockfile]" >&2
  exit 2
fi
if (( $# == 1 )); then lock_file="$1"; fi
if [[ ! -f "$lock_file" ]]; then
  echo "ERROR: missing lockfile: $lock_file" >&2
  exit 2
fi
# Fast path requires recognizable Cargo lock structure, never an empty file.
if ! grep -Eq '^[[:space:]]*version[[:space:]]*=[[:space:]]*[0-9]+[[:space:]]*$' "$lock_file" ||
   ! grep -Eq '^\[\[package\]\]$' "$lock_file"; then
  echo "ERROR: invalid/incomplete Cargo.lock; dependency graph NOT verified" >&2
  exit 2
fi
if ! grep -Eq "^[[:space:]]*name[[:space:]]*=[[:space:]]*['\"]rkyv['\"][[:space:]]*$" "$lock_file"; then
  echo "verified: rkyv absent from Cargo.lock"
  exit 0
fi
# Preserve Cargo errors and reject unresolved private Git dependencies.
if graph="$(cargo tree --workspace --locked \
  --target x86_64-unknown-linux-gnu \
  --invert rkyv@0.7.46 \
  --edges normal,build,dev \
  --prefix none 2>&1)"; then
  :
else
  status=$?
  printf '%s\n' "$graph" >&2
  echo "ERROR: cargo tree failed ($status); dependency graph NOT verified" >&2
  exit "$status"
fi
if grep -Eq '^[[:space:]]*rkyv v0\.7\.46([[:space:]]|$)' <<<"$graph"; then
  printf '%s\n' "$graph" >&2
  echo "ERROR: vulnerable rkyv 0.7.46 is reachable" >&2
  exit 1
fi
# Never accept an empty response or mixed warnings as absence evidence.
if [[ "$graph" == "nothing to print" ]]; then
  echo "verified: rkyv 0.7.46 absent from the active Linux graph"
  exit 0
fi
printf '%s\n' "$graph" >&2
echo "ERROR: unrecognized cargo tree output; cannot establish absence" >&2
exit 1
