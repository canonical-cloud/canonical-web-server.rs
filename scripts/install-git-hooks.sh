#!/usr/bin/env sh
set -eu
root="$(git rev-parse --show-toplevel)"
cd "$root"
existing="$(git config --path --get core.hooksPath 2>/dev/null || true)"
if [ -n "$existing" ] && [ "$existing" != ".githooks" ]; then
  printf '%s\n' "refusing to replace existing core.hooksPath=$existing" >&2
  exit 1
fi
chmod +x .githooks/pre-commit .githooks/pre-push
git config core.hooksPath .githooks
printf '%s\n' 'installed git hooks from .githooks'
