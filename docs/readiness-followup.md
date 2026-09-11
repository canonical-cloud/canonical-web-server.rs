# Readiness draft safety and assessor-workpaper integration

This change consumes the published Canonical auditor follow-up rather than maintaining a second browser implementation. The upstream catalog, response-v1 schema, independent framework state and non-assurance report semantics remain unchanged.

New worksheet behavior includes complete answer-field replacement confirmation, serialized file imports, native validity checks before export, non-stale error summaries, question views and an **Export assessor workpaper** button. The workpaper is a planning template with explicit permission, evidence request, population/sampling, observation/provenance, finding and retest fields; all assessor outcomes are **not assessed**, and independent review is **pending**. It never treats a customer declaration or typed reviewer name as an approval.

Question views are display-only. Every JSON/Markdown export, workpaper and printed packet still contains every question in the selected framework. Input-time errors no longer move keyboard focus away from the field being edited. Invalid files, cancelled replacements and overlapping reads must preserve the current answers. Read failures must not display arbitrary File error content.

All seven answer-field-only edits, cancellation/acceptance, overlapping asynchronous reads, read failures, oversized files, malformed UTF-8, duplicate JSON fields, unknown fields, filters, workpapers, native date/reference validity, focus and print restoration are covered by new cases in the existing real-server Playwright suite. Existing authentication, wrong-framework/customer, independent-tab, mobile, no-upload, CSP and anti-XSS cases remain intact. No test-only authentication is enabled in release builds.

The source pin must only land after the upstream exact-head checks and this repository's complete applicable CI gates pass. Existing CI and release/publisher policy are not weakened. See the upstream docs/readiness-followup.md and readiness/methodology.md for assessment-planning references and limitations.

Answers remain tab-memory only and must be exported to protected storage. Browser downloads do not inherit the upstream Rust CLI's Unix file-mode guarantees; their permissions are controlled by the browser/OS. Neither this integration nor the upstream change adds evidence uploads, durable customer storage, authenticated assessor approvals, an exhaustive normative program, live probes or a production deployment.
