# canonical-cloud/canonical-web-server.rs#78 — Harden Shared Auth and four web API data paths

head: codex/canonical-auth-four-path-20260825  base: main  author: ORESoftware  updated: 2026-08-28T20:42:13Z
dir: /Users/maca5/codes/.claude-fleet/scratch/merge/canonical-cloud_canonical-web-server.rs__78

## conflicted files
- .zpkg.toml

## base (main) last 8 commits
8cae042 Consume canonical interface contracts through Zed and Rust
0f417fe fix(quote): prevent expired-session POST replay and HTMX swap (#95)
a84bb01 refactor(auth): model login rate limiting as a pure transition (#92)
a1ce32e feat(client): serialize optimistic quote delivery with RxJS (#91)
9f2e890 build(deps): bump uuid from 1.24.1 to 1.25.0 (#84)
1ae6562 feat(env): adopt fleet-wide sops+age encrypted env files (#77)
f994ea2 docs: record canonical web/API data paths (#75)
f3248e5 fix(quote): reconcile optimistic web flow with API v1 contract (#74)

## head (codex/canonical-auth-four-path-20260825) last 8 commits
ce47755 Merge main into codex/canonical-auth-four-path-20260825 to refresh the PR against current main.
9f2e890 build(deps): bump uuid from 1.24.1 to 1.25.0 (#84)
0ae2144 feat: harden auth and web API data paths
1ae6562 feat(env): adopt fleet-wide sops+age encrypted env files (#77)
f994ea2 docs: record canonical web/API data paths (#75)
f3248e5 fix(quote): reconcile optimistic web flow with API v1 contract (#74)
cb22de3 ci: add shared source policy lint (#73)
09664fc feat(quote): consume the canonical v1 fixture contract (#57)

## merge-base: 9f2e890f791e42ff49224f172c0a08d258e2e7e9

## PR diff stat (merge-base..head)
 .env.example                        |   9 +-
 .gitignore                          |   3 +
 .zpkg.lock                          |   1 -
 .zpkg.toml                          |   9 +-
 Cargo.lock                          | 149 +++++++---
 Cargo.toml                          |   6 +-
 README.md                           |  54 +++-
 crates/canonical-auth/Cargo.toml    |   2 +-
 crates/canonical-config/Cargo.toml  |   2 +-
 crates/canonical-session/Cargo.toml |   2 +-
 crates/canonical-store/Cargo.toml   |   2 +-
 docs/web-api-data-access.md         |  56 ++--
 src/auth/extractor.rs               |  28 +-
 src/auth/shared_auth.rs             | 555 ++++++++++++++++++++++++++----------
 src/data_plane.rs                   | 542 +++++++++++++++++++++++++++++++++++
 src/lib.rs                          |   1 +
 src/quote_api.rs                    |  29 +-
 src/telemetry.rs                    |  52 +++-
 18 files changed, 1265 insertions(+), 237 deletions(-)

## base diff stat (merge-base..base)
 .github/workflows/interface-contract.yml |  37 +++++
 .zpkg.lock                               |   1 -
 .zpkg.toml                               |  12 ++
 .zpkg/pre-build                          |   4 +
 .zpkg/pre-publish                        |   6 +
 Cargo.lock                               |   4 +-
 client/package-lock.json                 |  18 ++-
 client/package.json                      |   3 +-
 client/src/quote-delivery-scheduler.ts   | 147 +++++++++++++++++
 client/src/quote-optimistic.ts           |  38 ++---
 client/test/quote-optimistic.test.ts     |  75 ++++++++-
 contracts/interfaces-source.lock.json    |  12 ++
 docs/QUOTE-SESSION-EXPIRY.md             |  38 +++++
 scripts/verify-interface-dependency.py   | 114 ++++++++++++++
 src/auth/rate_limit.rs                   | 260 +++++++++++++++++++++++++------
 src/routes/health.rs                     |  39 ++---
 src/routes/quote.rs                      | 150 ++++++++++++++++--
 17 files changed, 846 insertions(+), 112 deletions(-)

## merge output
Auto-merging .zpkg.toml
CONFLICT (content): Merge conflict in .zpkg.toml
Auto-merging Cargo.lock
Automatic merge failed; fix conflicts and then commit the result.
