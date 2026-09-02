#!/usr/bin/env python3
"""Verify the canonical interface dependency across Zed, Cargo, and source."""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCK_PATH = ROOT / "interfaces-source.lock.json"
ZPKG_PATH = ROOT / ".zpkg.toml"
CARGO_PATH = ROOT / "Cargo.toml"
CARGO_LOCK_PATH = ROOT / "Cargo.lock"


def fail(message: str) -> None:
    print(f"interface-contract: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(path: Path) -> dict:
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot parse {path.relative_to(ROOT)}: {error}")
    if not isinstance(value, dict):
        fail(f"{path.relative_to(ROOT)} must contain a TOML document")
    return value


def main() -> None:
    try:
        lock = json.loads(LOCK_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot parse interfaces-source.lock.json: {error}")
    if not isinstance(lock, dict) or lock.get("schema_version") != 1:
        fail("interfaces-source.lock.json must use schema_version 1")

    coordinate = lock.get("coordinate")
    requirement = lock.get("version_requirement")
    package_name = lock.get("cargo_package")
    git_url = lock.get("git")
    revision = lock.get("revision")
    if coordinate != "canonical-cloud/canonical-interfaces":
        fail("unexpected Zed interface coordinate")
    if requirement != "^0.1.0":
        fail("unexpected Zed interface version requirement")
    if package_name != "canonical-interfaces":
        fail("unexpected Cargo interface package name")
    if git_url != "https://github.com/canonical-cloud/canonical-interfaces":
        fail("unexpected interface Git source")
    if not isinstance(revision, str) or re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        fail("interface revision must be a full lowercase Git SHA")

    zpkg = load_toml(ZPKG_PATH)
    package = zpkg.get("package")
    if not isinstance(package, dict):
        fail(".zpkg.toml is missing [package]")
    if package.get("role") != "server" or package.get("family") != "canonical":
        fail(".zpkg.toml must identify the canonical server role")
    dependencies = zpkg.get("dependencies")
    if not isinstance(dependencies, dict) or dependencies.get(coordinate) != requirement:
        fail(".zpkg.toml interface dependency differs from the provenance lock")

    cargo = load_toml(CARGO_PATH)
    cargo_dependencies = cargo.get("dependencies")
    if not isinstance(cargo_dependencies, dict):
        fail("Cargo.toml is missing [dependencies]")
    native = cargo_dependencies.get(package_name)
    if not isinstance(native, dict):
        fail("Cargo.toml must directly depend on canonical-interfaces")
    if native.get("git") != git_url or native.get("rev") != revision:
        fail("Cargo interface source differs from the provenance lock")

    cargo_lock = CARGO_LOCK_PATH.read_text(encoding="utf-8")
    if f'name = "{package_name}"' not in cargo_lock:
        fail("Cargo.lock does not contain canonical-interfaces")
    if revision not in cargo_lock:
        fail("Cargo.lock does not contain the exact interface revision")

    consumers = lock.get("consumers")
    if not isinstance(consumers, list) or not consumers:
        fail("provenance lock must identify at least one source consumer")
    for relative in consumers:
        if not isinstance(relative, str):
            fail("consumer paths must be strings")
        source_path = ROOT / relative
        if not source_path.is_file():
            fail(f"missing interface consumer: {relative}")
        source = source_path.read_text(encoding="utf-8")
        if "use canonical_interfaces::{" not in source:
            fail(f"{relative} does not import canonical_interfaces")
        for duplicate in ("struct HealthResponse", "struct InfoResponse"):
            if duplicate in source:
                fail(f"{relative} still declares duplicate wire type {duplicate}")

    print(
        "interface-contract: Zed, Cargo, Cargo.lock, provenance, and source "
        f"agree on {coordinate}@{revision}"
    )


if __name__ == "__main__":
    main()
