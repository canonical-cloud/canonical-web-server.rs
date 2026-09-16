# Ores browser/native correlation

`src/server.rs` applies the native `ores-otel-web` wrapper to the fully assembled
web router before binding it to `axum::serve`. The existing OTLP trace/metric
providers, shutdown guard, database role checks, session/CSRF/Origin policy,
WebSocket admission and backplane remain unchanged. The isolated session
revoker does not acquire this dependency or an HTTP listener.

The adapter pins the corrected source
`74bc1c421f5a56be28cade1a47f5d45de51f3b4b` from merged ores-otel PR #56. It accepts
one bounded valid version-00 traceparent, creates a child span ID and preserves
the trace ID and sampling flags. Malformed or duplicate headers start a new
unsampled root. These headers convey correlation, never identity or privileges.
Only method/status and validated correlation enter its request-completion log;
raw URL/query/body/cookies/baggage do not.

## Lifecycle and delivery limits

This is a canonical log-correlation adapter, not another OTLP provider. It
covers handler execution through response creation, not subsequent streaming
body polling, WebSocket message processing or detached task execution. Those
operations require explicit context propagation. The application's existing
exporters own delivery, flush and shutdown. Browser consent, redaction, queues,
retries, custom-client integration and page-lifecycle handling are not provided
by this server change.

## Acceptance

Cargo generated the dependency lock in commit
`723df16feea63e80350a4d771bb5f9d490ee1a9f`; that follow-up changes only Cargo.lock
and formatting in src/server.rs. No hand-written package checksums or source
identities were substituted. The exact wrapper has a regression test preserving
an authorization denial and the parent sampling decision.

Final acceptance still requires this repository's existing protected test,
audit, PostgreSQL/RLS, declarative-schema, CockroachDB/RLS, browser-e2e and
container-smoke checks. Branch maintenance or the merged SDK's tests do not
waive any of them. The independent ores-otel-test PR #4 proves real Chrome WASM
execution and real Leptos/Dioxus browser/SSR compilation, not production UI or
collector delivery. Test-auth must remain excluded from production profiles.
