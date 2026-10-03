# Independent customer readiness pages

The customer web process serves a session-protected readiness chooser at `/app/readiness`, with separate worksheets at `/app/readiness/<framework>`. The API-only process does not expose these pages. All worksheet and asset handlers use the existing `SessionAuthenticated` extractor; identifiers entered into a worksheet are assessment labels, not authentication credentials.

## Coverage and source

Fifteen independently completed questionnaires are available: SOC 2; NIST CSF 2.0; ISO/IEC 27001; GDPR; HIPAA; PCI DSS; CIS Controls; CSA CCM; ISO/IEC 27701; ISO/IEC 42001; NIST SP 800-53; NIST SP 800-171; NIS2; DORA; and the explicitly selected FedRAMP Rev5 path. Each has ten original broad intake questions, suggested evidence, an examine/interview/test method, edition and authoritative references. This is a pre-audit intake foundation, not an exhaustive normative-control assessment, legal opinion, certification, attestation or authorization.

The immutable `vendor/canonical-auditor-readiness` submodule pins the catalog and browser contract from `canonical-cloud/canonical-company-auditor.rs`. The Rust server embeds the reviewed HTML, CSS, JavaScript and JSON at compile time. It does not fetch source code or a mutable catalog from another server at runtime. Updating the pin requires review of the upstream diff, catalog revision, shared tests and customer migration implications. Do not silently carry old answers into changed questions.

Initialize the source before building, including direct Cargo and Docker builds:

```sh
git submodule update --init --recursive
```

Existing CI checkouts initialize the submodules. No new Cargo or npm dependencies are required. The web process exposes exactly four embedded asset names under `/app/readiness/assets/`; it does not expose the upstream repository, probe scripts or arbitrary filesystem paths.

## Customer workflow and data boundary

Select a framework, supply the customer and assessment identifiers, describe the system/organizational scope and define the evidence period and as-of date. Start the worksheet and record each answer independently. Questions collect a declaration, owner, explanation, opaque evidence reference, evidence collection date, self-reported reviewer and remediation target date. Export a JSON draft to resume later, export Markdown for the pre-audit packet, or print the filled checklist.

**Answers stay only in the current tab's memory.** No answers, evidence or uploads are sent to the server, and no drafts are persisted in browser storage or the customer's account. Export before leaving. Exported files are unencrypted and belong in the customer's approved protected storage. The page warns about unsaved changes, but a crash or browser termination can still lose work. Do not put secrets, PHI, personal records or raw evidence in notes; use opaque evidence IDs referring to approved evidence storage.

JSON imports must match the exact framework, catalog revision, customer, assessment, scope and dates. They reject duplicate or unknown fields/questions, invalid dates, malformed encoding and oversized input. Answers cannot be copied automatically between frameworks. Reusing a relevant evidence artifact requires a separate explicit answer and applicability review in every framework.

A completed declaration is not verified implementation. Missing evidence is reported as unknown, not a proved control failure. Declared gaps, unanswered questions, incomplete metadata, pending reviewers and overdue actions remain separate. A typed reviewer name is not an authenticated approval or signature. There is no blended compliance score, certificate or automated readiness opinion.

## Security and deployment

The pages use a route-specific CSP without inline script/style or eval permissions, plus `Cache-Control: no-store`, `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`. Customer text is rendered through text/value properties, not dynamic HTML. Markdown export escapes customer markup. The forms do not trigger live network probes, import modules or execute customer code.

This change adds no durable-draft API, database tables or evidence-upload endpoint. A future persistence feature must derive ownership from authenticated identity and test server-side authorization/RLS, access auditing, retention/deletion and encryption before launch. The application's existing release boundary remains: canonical-monorepo is the sole publisher; merging this source PR is not a production deployment.

## Tests and engagement guidance

```sh
node --test vendor/canonical-auditor-readiness/readiness/readiness.test.mjs
cargo test --locked -p canonical-web-server
npm run test:browser --prefix tests/browser
```

The existing browser harness drives a real server with a temporary SQLite database and debug-only test authentication. Added Playwright cases cover unauthenticated routes/assets, authenticated framework coverage, strict CSP, draft export/import, cross-framework/customer rejection, independent tabs, escaped customer text, Markdown, mobile width, print values and the absence of answer-upload requests. Release builds continue to forbid the test-auth feature.

Consult `vendor/canonical-auditor-readiness/readiness/README.md` for CLI usage and `vendor/canonical-auditor-readiness/readiness/methodology.md` for the source-backed pre-audit procedure: scope and authorization, independent assessment plans, evidence provenance, sampling, framework-specific applicability, findings, remediation, retesting and qualified-assessor handoff. No customer audit has been performed merely by installing these pages.
