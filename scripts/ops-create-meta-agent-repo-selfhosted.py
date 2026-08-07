#!/usr/bin/env python3
"""Create and verify the exact Meta Agents repository after owner device auth."""

from __future__ import annotations

import base64
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any

API = "https://api.github.com"
OAUTH_CLIENT_ID = "178c6fc778ccc68e1d6a"
EXPECTED_LOGIN = "ORESoftware"
TARGET = "meta-agents-demo/meta-agent-control-plane.rs"
EXPECTED_MAIN = "4d6ec3ad0ec7b688f0e777129eee7e0f0d999df1"
FEATURE_REF = "agent/den-1057-meta-agent-control-plane"
EXPECTED_FEATURE = "789d48039da232faed985d4f8de176959f117e08"
BUNDLE_SHA256 = "1ddaa03743b864348162149b7d2d2e2dce7eab585cf092ea14547c647fcec031"
PUBLISHER_SHA256 = "e2fe6eaa622db02a54f83e27a822f64ad4b54971c883f97bbda4ac0a4db5d278"


def request_json(
    method: str,
    url: str,
    *,
    token: str | None = None,
    form: dict[str, str] | None = None,
    payload: dict[str, Any] | None = None,
) -> tuple[int, Any]:
    data = None
    headers = {
        "Accept": "application/json",
        "User-Agent": "meta-agent-selfhosted-publisher",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
        headers["X-GitHub-Api-Version"] = "2022-11-28"
    if form is not None:
        data = urllib.parse.urlencode(form).encode()
        headers["Content-Type"] = "application/x-www-form-urlencoded"
    elif payload is not None:
        data = json.dumps(payload).encode()
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
            return response.status, json.loads(raw.decode()) if raw else None
    except urllib.error.HTTPError as exc:
        body = exc.read(4096).decode(errors="replace")
        raise RuntimeError(f"GitHub request failed with HTTP {exc.code}: {body}") from exc


def api_get(path: str, token: str) -> Any:
    status, payload = request_json("GET", API + path, token=token)
    if status != 200:
        raise RuntimeError(f"GitHub GET {path} returned {status}")
    return payload


def add_comment(body: str, comment_token: str) -> None:
    repository = os.environ["GITHUB_REPOSITORY"]
    issue = os.environ["TRACKING_ISSUE"]
    status, _ = request_json(
        "POST",
        f"{API}/repos/{repository}/issues/{issue}/comments",
        token=comment_token,
        payload={"body": body},
    )
    if status != 201:
        raise RuntimeError(f"comment creation returned {status}")


def authorize(comment_token: str) -> str:
    status, device = request_json(
        "POST",
        "https://github.com/login/device/code",
        form={"client_id": OAUTH_CLIENT_ID, "scope": "repo read:org"},
    )
    if status != 200 or not isinstance(device, dict):
        raise RuntimeError("GitHub device-code request failed")

    device_code = device.get("device_code")
    user_code = device.get("user_code")
    verification_uri = device.get("verification_uri")
    expires_in = int(device.get("expires_in", 0))
    interval = int(device.get("interval", 5))
    values = (device_code, user_code, verification_uri)
    if not all(isinstance(value, str) and value for value in values):
        raise RuntimeError("GitHub device-code response is incomplete")
    if expires_in <= 0 or interval <= 0:
        raise RuntimeError("GitHub device-code timing is invalid")

    run_url = (
        f"https://github.com/{os.environ['GITHUB_REPOSITORY']}/actions/runs/"
        f"{os.environ['GITHUB_RUN_ID']}"
    )
    add_comment(
        "**Authorize exact Meta Agents repository creation now:** "
        f"open {verification_uri} and enter **`{user_code}`**. Ignore all older codes. "
        f"This self-hosted run accepts only GitHub account `{EXPECTED_LOGIN}` with active "
        f"admin access to `meta-agents-demo`. Run: {run_url}",
        comment_token,
    )
    print(
        f"::notice title=GitHub owner authorization::Open {verification_uri} "
        f"and enter {user_code}",
        flush=True,
    )

    deadline = time.monotonic() + expires_in
    while time.monotonic() < deadline:
        time.sleep(interval)
        status, response = request_json(
            "POST",
            "https://github.com/login/oauth/access_token",
            form={
                "client_id": OAUTH_CLIENT_ID,
                "device_code": str(device_code),
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            },
        )
        if status != 200 or not isinstance(response, dict):
            raise RuntimeError("GitHub device-token request failed")
        error = response.get("error")
        if not error:
            token = response.get("access_token")
            if isinstance(token, str) and token:
                print(f"::add-mask::{token}", flush=True)
                return token
            raise RuntimeError("GitHub device-token response lacks access_token")
        if error == "authorization_pending":
            continue
        if error == "slow_down":
            interval += 5
            continue
        raise RuntimeError(f"GitHub device authorization failed: {error}")
    raise RuntimeError("GitHub device authorization expired")


def verify_owner(token: str) -> None:
    user = api_get("/user", token)
    if not isinstance(user, dict) or user.get("login") != EXPECTED_LOGIN:
        raise RuntimeError("unexpected GitHub owner identity")
    membership = api_get("/user/memberships/orgs/meta-agents-demo", token)
    if not isinstance(membership, dict):
        raise RuntimeError("organization membership response is invalid")
    if (membership.get("role"), membership.get("state")) != ("admin", "active"):
        raise RuntimeError("ORESoftware is not an active meta-agents-demo owner")


def run(command: list[str], *, env: dict[str, str] | None = None) -> str:
    completed = subprocess.run(
        command,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(command)}\n"
            f"stdout:\n{completed.stdout[-3000:]}\n"
            f"stderr:\n{completed.stderr[-3000:]}"
        )
    return completed.stdout.strip()


def reconstruct_and_publish(token: str) -> None:
    source = pathlib.Path(os.environ["SOURCE_ROOT"]).resolve()
    parts = sorted((source / "scripts/critical-org-fleet/assets").glob("meta.part*"))
    if not parts:
        raise RuntimeError("recovered bundle parts are missing")
    encoded = b"".join(part.read_bytes() for part in parts)
    bundle_bytes = base64.b64decode(encoded, validate=True)
    if hashlib.sha256(bundle_bytes).hexdigest() != BUNDLE_SHA256:
        raise RuntimeError("recovered bundle digest mismatch")

    publisher = source / "scripts/critical-org-fleet/publish_meta_control_plane.py"
    if hashlib.sha256(publisher.read_bytes()).hexdigest() != PUBLISHER_SHA256:
        raise RuntimeError("publisher digest mismatch")

    with tempfile.TemporaryDirectory(prefix="meta-agent-selfhosted-") as directory:
        bundle = pathlib.Path(directory) / "meta-agent-control-plane.bundle"
        bundle.write_bytes(bundle_bytes)
        heads = run(["git", "bundle", "list-heads", str(bundle)])
        expected_lines = {
            f"{EXPECTED_MAIN} refs/heads/main",
            f"{EXPECTED_FEATURE} refs/heads/{FEATURE_REF}",
        }
        if set(heads.splitlines()) != expected_lines:
            raise RuntimeError("recovered bundle refs changed")
        run(["python3", "-m", "py_compile", str(publisher)])
        child_env = os.environ.copy()
        child_env["GITHUB_REPOSITORY_ADMIN_TOKEN"] = token
        child_env["GIT_TERMINAL_PROMPT"] = "0"
        run(["python3", str(publisher), str(bundle)], env=child_env)


def verify_target(token: str) -> None:
    metadata = api_get(f"/repos/{TARGET}", token)
    if not isinstance(metadata, dict):
        raise RuntimeError("target repository response is invalid")
    if metadata.get("visibility") != "public" or metadata.get("default_branch") != "main":
        raise RuntimeError("target repository metadata mismatch")
    expected = {"main": EXPECTED_MAIN, FEATURE_REF: EXPECTED_FEATURE}
    for branch, sha in expected.items():
        ref = api_get(f"/repos/{TARGET}/git/ref/heads/{branch}", token)
        observed = ((ref or {}).get("object") or {}).get("sha")
        if observed != sha:
            raise RuntimeError(f"{branch} ref mismatch: {observed} != {sha}")


def main() -> int:
    comment_token = os.environ["COMMENT_TOKEN"]
    try:
        owner_token = authorize(comment_token)
        verify_owner(owner_token)
        reconstruct_and_publish(owner_token)
        verify_target(owner_token)
        run_url = (
            f"https://github.com/{os.environ['GITHUB_REPOSITORY']}/actions/runs/"
            f"{os.environ['GITHUB_RUN_ID']}"
        )
        add_comment(
            f"Created and verified `{TARGET}`: `main` `{EXPECTED_MAIN}`; "
            f"`{FEATURE_REF}` `{EXPECTED_FEATURE}`. Run: {run_url}",
            comment_token,
        )
        return 0
    except Exception as exc:
        run_url = (
            f"https://github.com/{os.environ['GITHUB_REPOSITORY']}/actions/runs/"
            f"{os.environ['GITHUB_RUN_ID']}"
        )
        try:
            add_comment(
                "Exact Meta Agents repository creation failed before live ref verification. "
                f"Inspect: {run_url}",
                comment_token,
            )
        except Exception:
            pass
        raise SystemExit(str(exc)) from exc


if __name__ == "__main__":
    raise SystemExit(main())
