#!/usr/bin/env python3
"""Fail closed when Zed, Cargo, or source disagree on canonical-interfaces."""
from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
LOCK_PATH = ROOT / "contracts/interfaces-source.lock.json"


def fail(message: str) -> None:
    print(f"interface-contract: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        fail(f"cannot parse {path.relative_to(ROOT)}: {exc}")
    if not isinstance(value, dict):
        fail(f"{path.relative_to(ROOT)} must contain a TOML table")
    return value


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"cannot parse {path.relative_to(ROOT)}: {exc}")
    if not isinstance(value, dict):
        fail(f"{path.relative_to(ROOT)} must contain a JSON object")
    return value


def main() -> None:
    lock = load_json(LOCK_PATH)
    zpkg = load_toml(ROOT / ".zpkg.toml")
    cargo = load_toml(ROOT / "Cargo.toml")

    coordinate = lock.get("coordinate")
    version = lock.get("version_requirement")
    repository = lock.get("repository")
    crate = lock.get("rust_crate")
    revision = lock.get("rust_revision")
    consumers = lock.get("consumers")
    if not all(isinstance(value, str) and value for value in (coordinate, version, repository, crate, revision)):
        fail("interfaces-source lock has incomplete string fields")
    if not isinstance(consumers, list) or not consumers:
        fail("interfaces-source lock must declare at least one source consumer")
    if re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        fail("rust_revision must be a full lowercase Git SHA")

    if zpkg.get("dependencies", {}).get(coordinate) != version:
        fail(".zpkg.toml does not declare the locked interface coordinate and version")

    cargo_dep = cargo.get("dependencies", {}).get(crate)
    if not isinstance(cargo_dep, dict):
        fail(f"Cargo.toml must directly depend on {crate}")
    if cargo_dep.get("git") != repository or cargo_dep.get("rev") != revision:
        fail("Cargo interface source does not match the reviewed interface lock")

    lifecycle = zpkg.get("lifecycle", {})
    for phase in ("pre-build", "pre-publish"):
        config = lifecycle.get(phase)
        expected = f"sh ./.zpkg/{phase}"
        if not isinstance(config, dict) or config.get("mode") not in {"replace", "override"}:
            fail(f"lifecycle.{phase} must explicitly replace convention discovery")
        if config.get("command") != expected:
            fail(f"lifecycle.{phase} must execute {expected}")
        hook = ROOT / ".zpkg" / phase
        if hook.is_symlink() or not hook.is_file():
            fail(f"missing regular lifecycle hook: {hook.relative_to(ROOT)}")

    required_markers = {
        "use canonical_interfaces::{HealthStatus, HealthStatusStatus, ServiceInfo};",
        "Json<HealthStatus>",
        "Json<ServiceInfo>",
        "HealthStatusStatus::Ok",
    }
    for relative in consumers:
        if not isinstance(relative, str) or not relative:
            fail("consumer paths must be non-empty strings")
        path = (ROOT / relative).resolve()
        if not path.is_relative_to(ROOT) or not path.is_file():
            fail(f"missing interface consumer: {relative}")
        source = path.read_text(encoding="utf-8")
        missing = sorted(required_markers - set(source.splitlines()))
        if missing:
            fail(f"{relative} does not import and construct generated health/info types")
        if "struct HealthResponse" in source or "struct InfoResponse" in source:
            fail(f"{relative} redefines health/info contracts instead of importing them")

    zed_lock = ROOT / ".zpkg.lock"
    if zed_lock.is_file() and zed_lock.read_text(encoding="utf-8").strip() == "version = 1":
        fail("placeholder .zpkg.lock is forbidden; generate a real lock with zed install")

    print(
        "interface-contract: canonical-interfaces is aligned across Zed, Cargo, "
        "the source lock, and generated health/info consumers"
    )


if __name__ == "__main__":
    main()
