# Cross-surface delivery

Verified **2026-08-06**.

## Surfaces

- Rust web/BFF and compliance-quote application: `canonical-cloud/canonical-web-server.rs`
- Flutter Android/iOS, Flutter Web/mobile web, and Flutter desktop: `canonical-cloud/canonical-agent-flutter` — proposed candidate
- Rust desktop/local evidence agent: `canonical-cloud/canonical-agent-desktop.rs` — proposed candidate
- Shared contracts: Canonical interfaces, generated clients, quote/intake/framework/context/evidence schemas, route types, compliance fixtures, and conformance tests

The native pair is a candidate allocation. Repository names must not be described as published until their remotes and builds are verified.

## Judgment-based propagation

Evaluate Flutter mobile, Flutter Web, Flutter desktop, Rust desktop/local agent, and shared contracts for every user-visible or contract-changing web change. Public marketing, SEO, server-rendered quote presentation, and browser-only account administration may remain web-specific. Local filesystem inventory, evidence collection, policy checks, browser/device attestations, watched folders, secure upload queues, signing/update behavior, and offline collection may be native-specific. Quote/intake semantics, framework selection, context records, evidence status, authentication, permissions, errors, notifications, and navigation normally propagate or require an explicit rationale and parity issue.

Mobile does not need privileged local-evidence collection merely for feature parity. A good decision may keep collection in the signed desktop agent while mobile provides status, approval, notification, and deep-link workflows. Each issue and pull request records affected surfaces, omitted surfaces and rationale, accepted parity gaps, follow-up work, and separate validation/release status.

## Deep links

Canonical:

```text
https://<verified-canonical-owned-host>/open/<route>?<bounded-query>
```

The exact HTTPS host must be proven before publication. A custom-scheme fallback requires a reviewed ADR and must not be guessed. All surfaces share versioned route types and golden fixtures and support cold start, already-running delivery, authentication resume, replay/expiry rejection, browser fallback, and explicit confirmation before quote submission, evidence import/upload, connector changes, attestations, approvals, or destructive operations.

Never put quote answers, compliance evidence, private documents, filesystem inventories, absolute local paths, browser/device attestations, credentials, Supabase sessions, service tokens, model prompts/context, personally identifiable information, bearer/refresh tokens, or signing material in URLs. Use bounded identifiers or short-lived, single-use, audience-bound codes and validate route version, user/quote/framework/context/evidence IDs, action, authorization, assurance level, limits, and user intent.

## Review checklist

- [ ] Flutter Android/iOS impact evaluated.
- [ ] Flutter Web/mobile-web impact evaluated.
- [ ] Flutter desktop impact evaluated.
- [ ] Rust desktop/local-agent impact evaluated.
- [ ] Shared quote/evidence/client/route/fixture impact evaluated.
- [ ] Deep-link, auth-resume, and browser-fallback compatibility tested where relevant.
- [ ] Privileged local-evidence features omitted from mobile have a documented security/UX rationale.
- [ ] Omitted surfaces have a follow-up when needed.

## Routing

- GitHub Project: [`canonical-cloud-project` — Project 1](https://github.com/orgs/canonical-cloud/projects/1)
- Linear project: [`github.com/canonical-cloud`](https://linear.app/denman/project/githubcomcanonical-cloud-1659c8ea1adf)
- Central policy: [`cross-surface-delivery.md`](https://github.com/ORESoftware/project-registry/blob/main/docs/cross-surface-delivery.md)
- Desktop registry: [`desktop-applications.json`](https://github.com/ORESoftware/project-registry/blob/main/registry/desktop-applications.json)
