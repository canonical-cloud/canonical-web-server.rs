Two parallel implementations of the same task (consume canonical-interfaces through Zed + Cargo with a
provenance lock, lifecycle hooks and a CI contract): main's single commit 8cae042 and this 8-commit branch.
Kept main's implementation (contracts/interfaces-source.lock.json, scripts/verify-interface-dependency.py,
.zpkg lifecycle tables, workflow) and folded in everything this PR had that main lacked:
- src/routes/health.rs: main's handlers + the PR's two wire-shape tests (health/info serialize to the
  generated interface shape).
- scripts/verify-interface-dependency.py: ported the PR's Cargo.lock check (the lockfile must resolve the
  crate at exactly the reviewed revision, not just declare it).
- .zpkg/pre-publish: the PR's `cargo test --workspace --all-targets --locked` gate before the drift check.
- .github/workflows/interface-contract.yml: the PR's concurrency guard (cancel superseded runs).
- .zpkg.toml: main's [scripts]/[lifecycle]; kept the PR's package `role`/`family` metadata; removed the
  auto-merged duplicate prebuild/prepublish keys and the reference to the PR's now-removed verifier.
Dropped as duplicates of main's files: scripts/verify_interface_contract.py, interfaces-source.lock.json.
