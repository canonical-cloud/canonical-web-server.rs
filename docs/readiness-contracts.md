# Readiness contract admission in the web runtime

The web server consumes readiness assets from the immutable `vendor/canonical-auditor-readiness` gitlink. That upstream repository owns two independent wire-shape authorities: TypeSpec and JSON Schema Draft 2020-12. Neither authority is generated from the other.

This repository does not trust an upstream green badge transitively. `.github/workflows/readiness-contracts.yml` recomputes parity from the exact pinned submodule with `ORESoftware/typespec-json-schema-validator` (TJSV) at a reviewed immutable commit. It then verifies the freshly produced Contract IR against the exact TypeSpec, authored JSON Schema, generated comparison witness, parity receipt, and complete declaration inventory. Finally it runs TJSV's altered-evidence and incomplete-scope consumer-admission regressions.

Generated JSON Schema, parity receipts, Contract IR, and verification receipts are evidence only and stay under the ignored `.typespec-json-schema-validator/` directory. They never overwrite either authority. A TJSV failure blocks the consumer PR; do not repair it by generating one authority from the other, removing declarations from the expected scope, disabling differential instances, or accepting a stale receipt.

The browser implementation still applies runtime semantics that are not merely JSON shape: exact framework question membership, unique question IDs, customer/assessment/scope/date binding, evidence-date rules, and independent review handling. SOC 2, NIST, ISO, GDPR, HIPAA, and every other framework remain independently completed even when an evidence artifact is relevant to more than one.

TJSV dependency-security findings are evaluated separately from contract correctness. A passing parity/consumer-admission run is not a declaration that the validator's dependency graph is vulnerability-free. Current upstream dependency-triage work is tracked in `ORESoftware/typespec-json-schema-validator#62`.
