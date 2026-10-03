.zpkg.toml: [dependencies] union — main's canonical-interfaces + canonical-lib, this PR's canonical-lib-core,
oresoftware/next-loggers and shared-auth/shared-auth-clients (the four-path auth work it adds). [scripts]:
kept main's separated test/lint/format plus its new interface_contract/prebuild/prepublish hooks; the PR's
combined `test` (fmt+test+clippy) is covered by those three scripts and the .zpkg/pre-build gate.
The PR's Rust changes (src/auth/shared_auth.rs, src/data_plane.rs, …) merged without conflict; `cargo check` gates.
