#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${CANONICAL_LIB_READ_TOKEN:-}" ]]; then
  echo 'CANONICAL_LIB_READ_TOKEN is required for read-only private Canonical dependencies.' >&2
  exit 1
fi

# Keep the credential scoped to dependency acquisition. Callers should run all
# build/test commands afterward with CARGO_NET_OFFLINE=true so build scripts and
# tests never inherit this token or need network access to private repositories.
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export GIT_CONFIG_COUNT=1
export GIT_CONFIG_KEY_0="url.https://x-access-token:${CANONICAL_LIB_READ_TOKEN}@github.com/.insteadOf"
export GIT_CONFIG_VALUE_0="https://github.com/"

cargo fetch --locked
