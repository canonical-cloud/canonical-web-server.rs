# canonical-cloud/canonical-web-server.rs#87 — DEN-2843: import lib-core and orm-core through Zed

head: DEN-2843/zed-core-imports  base: main  author: ORESoftware  updated: 2026-08-29T19:20:48Z
dir: /Users/maca5/codes/.claude-fleet/scratch/merge/canonical-cloud_canonical-web-server.rs__87

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

## head (DEN-2843/zed-core-imports) last 8 commits
dddbea2 DEN-2843: import lib-core and orm-core with Zed
9f2e890 build(deps): bump uuid from 1.24.1 to 1.25.0 (#84)
1ae6562 feat(env): adopt fleet-wide sops+age encrypted env files (#77)
f994ea2 docs: record canonical web/API data paths (#75)
f3248e5 fix(quote): reconcile optimistic web flow with API v1 contract (#74)
cb22de3 ci: add shared source policy lint (#73)
09664fc feat(quote): consume the canonical v1 fixture contract (#57)
dfd4609 build(deps): update Swatinem/rust-cache requirement to f0d9c3887740aee45f6153b24b3a6b815192ec16 (#67)

## merge-base: 9f2e890f791e42ff49224f172c0a08d258e2e7e9

## PR diff stat (merge-base..head)
 .zpkg.toml | 4 +++-
 1 file changed, 3 insertions(+), 1 deletion(-)

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
Automatic merge failed; fix conflicts and then commit the result.
