# canonical-cloud/canonical-web-server.rs#97 — feat(interfaces): use canonical wire types through Zed and Cargo

head: feat/zed-interface-contract  base: main  author: ORESoftware  updated: 2026-09-02T16:35:03Z
dir: /Users/maca5/codes/.claude-fleet/scratch/merge/canonical-cloud_canonical-web-server.rs__97

## conflicted files
- .github/workflows/interface-contract.yml
- .zpkg/pre-build
- .zpkg/pre-publish
- src/routes/health.rs

## base (main) last 8 commits
8cae042 Consume canonical interface contracts through Zed and Rust
0f417fe fix(quote): prevent expired-session POST replay and HTMX swap (#95)
a84bb01 refactor(auth): model login rate limiting as a pure transition (#92)
a1ce32e feat(client): serialize optimistic quote delivery with RxJS (#91)
9f2e890 build(deps): bump uuid from 1.24.1 to 1.25.0 (#84)
1ae6562 feat(env): adopt fleet-wide sops+age encrypted env files (#77)
f994ea2 docs: record canonical web/API data paths (#75)
f3248e5 fix(quote): reconcile optimistic web flow with API v1 contract (#74)

## head (feat/zed-interface-contract) last 8 commits
2ff3358 build(zed): make interface lifecycle gates executable
0542abb ci(interfaces): compile and test the shared contract edge
64e5959 build(zed): block publish on interface and test drift
00ab179 build(zed): gate builds on interface parity
bc4c275 test(interfaces): verify Zed Cargo and source parity
710f016 build(interfaces): pin cross-manager interface provenance
354205c refactor(health): use canonical-interfaces wire types
cfb6ac4 build(zed): declare the canonical interface role edge

## merge-base: 0f417fe0c4b31ae1ad3942abc5d3806949dc2aaa

## PR diff stat (merge-base..head)
 .github/workflows/interface-contract.yml |  53 +++++++++++++++
 .zpkg.toml                               |   6 ++
 .zpkg/pre-build                          |   4 ++
 .zpkg/pre-publish                        |   7 ++
 interfaces-source.lock.json              |  12 ++++
 scripts/verify_interface_contract.py     | 108 +++++++++++++++++++++++++++++++
 src/routes/health.rs                     |  64 +++++++++++-------
 7 files changed, 232 insertions(+), 22 deletions(-)

## base diff stat (merge-base..base)
 .github/workflows/interface-contract.yml |  37 ++++++++++
 .zpkg.lock                               |   1 -
 .zpkg.toml                               |  12 ++++
 .zpkg/pre-build                          |   4 ++
 .zpkg/pre-publish                        |   6 ++
 contracts/interfaces-source.lock.json    |  12 ++++
 scripts/verify-interface-dependency.py   | 114 +++++++++++++++++++++++++++++++
 src/routes/health.rs                     |  39 ++++-------
 8 files changed, 199 insertions(+), 26 deletions(-)

## merge output
Auto-merging .github/workflows/interface-contract.yml
CONFLICT (add/add): Merge conflict in .github/workflows/interface-contract.yml
Auto-merging .zpkg.toml
Auto-merging .zpkg/pre-build
CONFLICT (add/add): Merge conflict in .zpkg/pre-build
Auto-merging .zpkg/pre-publish
CONFLICT (add/add): Merge conflict in .zpkg/pre-publish
Auto-merging src/routes/health.rs
CONFLICT (content): Merge conflict in src/routes/health.rs
Automatic merge failed; fix conflicts and then commit the result.
