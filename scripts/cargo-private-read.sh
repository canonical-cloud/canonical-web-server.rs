#!/usr/bin/env sh
set -eu

if [ -z "${CANONICAL_LIB_READ_TOKEN:-}" ]; then
  echo 'CANONICAL_LIB_READ_TOKEN is required for read-only private Canonical Cargo dependencies.' >&2
  exit 1
fi

# Keep the credential process-local. Do not write it to ~/.gitconfig, Cargo
# config, the repository, or an image layer. Cargo delegates git fetches to the
# CLI and Git rewrites only github.com URLs for this process tree.
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export GIT_CONFIG_COUNT=1
export GIT_CONFIG_KEY_0="url.https://x-access-token:${CANONICAL_LIB_READ_TOKEN}@github.com/.insteadOf"
export GIT_CONFIG_VALUE_0="https://github.com/"

exec cargo "$@"
