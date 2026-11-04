#!/usr/bin/env bash
# Operator-invoked only, after reviewed private Cargo dependency admission.
# Never execute with credentials in untrusted pull_request jobs.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ -n "$(git status --porcelain -- Cargo.lock)" ]]; then
  echo 'ERROR: Cargo.lock has local changes; refuse to overwrite review evidence' >&2
  exit 1
fi
for command in git cargo node cargo-audit; do
  if ! command -v "$command" >/dev/null; then
    echo "ERROR: $command must be installed before authenticated remediation" >&2
    exit 1
  fi
done
# Avoid 1.54.0: RUSTSEC-2026-0384 identifies a timer use-after-free.
# Each update is Cargo-resolved. Never invent checksums or manually alter source entries.
cargo +1.95.0 update -p ctutils@0.4.2 --precise 0.4.4
cargo +1.95.0 update -p quinn-proto@0.11.16 --precise 0.11.18
cargo +1.95.0 update -p rustls@0.23.45 --precise 0.23.46
cargo +1.95.0 update -p rustls-webpki@0.103.15 --precise 0.103.16
cargo +1.95.0 update -p webpki-root-certs@1.0.9 --precise 1.0.10
cargo +1.95.0 update -p webpki-roots@1.0.9 --precise 1.0.10
cargo +1.95.0 update -p tokio@1.53.1 --precise 1.53.3
node scripts/rustsec-lock-floor.mjs
git diff --check -- Cargo.lock
cargo +1.95.0 metadata --format-version 1 --locked > /dev/null
cargo audit --deny unmaintained --deny unsound --deny yanked \
  --ignore RUSTSEC-2026-0173 --ignore RUSTSEC-2026-0235
printf '\nRemediation resolved and audited. Run actual Rust tests, private graph checks and integration tests before reviewing this diff.\n'
git diff --stat -- Cargo.lock
