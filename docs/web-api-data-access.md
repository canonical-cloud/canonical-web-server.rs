# Web/API data-access decision

This repository adopts the
[portfolio four-path ADR](https://github.com/ORESoftware/k8s-cluster/blob/main/docs/architecture/web-api-data-access.md)
for [ORESoftware/k8s-cluster#1399](https://github.com/ORESoftware/k8s-cluster/issues/1399)
and [DEN-3960](https://linear.app/denman/issue/DEN-3960/document-4-web-server-to-api-server-data-access-patterns-across-10).
The choice is made per operation; a deployment does not receive every path by
default. `src/data_plane.rs` and the API's `src/web_data_plane.rs` expose the
same `canonical-cloud/web-api/v1` envelope, `canonical-plus-api` audience,
bounds, framing, and durable-status rules. Selecting one mode never authorizes
fallback to another.

## Current boundary

The Cargo workspace produces both `canonical-web-server` and the separately
deployable `canonical-api-server`. Code reuse does not merge their credentials
or authority. The current combined web process is an API owner for its local
session and sync routes; those local writes are not path 1. The quote surface is
already split: browser-facing handlers call the dedicated quote API through
`QuoteApiClient`.

| Operation | Path | Contract |
| --- | --- | --- |
| Read or mutate web-owned session state | Local web authority | `canonical_web_server` is a non-owner, non-`BYPASSRLS` role; this is outside the four cross-tier paths. |
| Read or mutate sync records while the combined router is deployed | Local API authority | The same process owns the authenticated API route and forced-RLS transaction; this is not P1. |
| Submit, get, or list dedicated quote records | P2: stateless HTTP | `QuoteApiClient` calls the private API origin with a service credential and verified subject. |
| Curated product-domain views | P1: direct read-only DB | Supported only through the exact `canonical_cloud__quote__web_ro` role and a separately configured pool; no write operation validates for this mode. |
| Long-lived high-volume API stream | P3: stateful mTLS/TCP | Supported through TLS 1.3, mutual certificate references, exact 4-byte big-endian framing, and bounded frames/timeouts. |
| Durable asynchronous commands | P4: NATS JetStream | Supported through distinct request/status subjects plus durable outbox, inbox/dedupe, and status records. |
| Browser invalidation WebSocket | Browser/API transport | This is not a web-server-to-API P3 connection; REST pull remains authoritative. |
| PostgreSQL `LISTEN`/`NOTIFY` invalidation | Database wake-up hint | This is disposable and non-authoritative, not durable P4 messaging. |
| Durable logout reconciliation | Database-backed worker queue | The revoker has a separate role and no ingress; this is not NATS/MQ P4. |

## Path 1: constrained direct reads

P1 can never become a second product-domain writer. The shared contract rejects
every non-read operation and requires the exact
`canonical_cloud__quote__web_ro` login, a read-only transaction, forced RLS, a
1,000-row ceiling, a statement timeout no greater than 5 seconds, and a lock
timeout no greater than 1 second. Activating it also requires all of the
following:

- a distinct read-only database role with no DML, DDL, ownership, membership,
  or `BYPASSRLS` capability;
- forced-RLS or an equally reviewed owner predicate derived from verified
  identity, with cross-owner negative tests;
- an allow-list of stable views/queries rather than arbitrary table access;
- a short query timeout, bounded pool, cancellation on request loss, and no
  fallback to a writer credential.

Read-after-write may be stale when P1 targets a replica. A caller that requires
authoritative command status must use P2. Existing session writes and combined
API route writes are local ownership, not precedent for widening P1.

## Path 2: stateless HTTP

P2 is the default for a physical web/API split and is the current quote path.
`CANONICAL_API_URL` must be a private Kubernetes origin or HTTPS origin;
`CANONICAL_INTERNAL_AUTH_TOKEN` and the verified `x-canonical-subject` are
server-controlled. The browser never receives either value.

The client has a 2-second connect timeout, 10-second total timeout, no redirects,
a 64 KiB request limit, and a 256 KiB response limit enforced during streaming.
The internal service credential and verified user subject remain separate
headers and neither is accepted from browser input. Mutation retries must reuse the same stable
`Idempotency-Key`; GET retries use capped exponential backoff and a total
deadline. Never retry authentication failures or unboundedly retry overload.
Propagate `traceparent` and the request ID when the API contract supports them,
and record route template, status class, latency, timeout, retry count, and
idempotency outcome without credentials or customer payloads.

## Path 3: bounded stateful API connection

P3 is an available contract but is not the active quote transport. It requires
TLS 1.3, a pinned server name, CA/client-certificate/client-private-key
references, mutual authentication, a two-second maximum connect deadline, a
ten-second maximum I/O deadline, and a 256 KiB maximum frame. Each frame is a
four-byte big-endian length followed by exactly that many payload bytes;
truncated, zero-length, oversized, or trailing-byte frames are rejected.
Operationally it also keeps a small per-pod connection budget, an authenticated
handshake, connect and idle deadlines, a heartbeat, bounded inbound and outbound
buffers, reconnect jitter, and graceful drain.
Browser WebSockets and PostgreSQL `LISTEN`/`NOTIFY` are different boundaries
and must not be cited as P3. Overflow fails closed and an operator may require
an authoritative P2 resync; code never silently falls back to P1.

## Path 4: asynchronous NATS or message queue

P4 is an available contract but is not the active quote transport. Its policy
requires a versioned request envelope, tenant and user subject, trace context,
stable dedupe key, a 64 KiB message limit, durable consumer, explicit ack only
after commit, an explicit ack deadline and retry ceiling, a dead-letter policy,
distinct request/status subjects, and named database-backed outbox,
inbox/dedupe, and status records. Queue-age and redelivery metrics are
required. Status transitions are monotonic:
`pending -> published -> processing -> succeeded|failed`; a failed operation
may be republished within the retry budget or moved to `dead_letter`.
`pg_notify` remains a disposable wake-up and is not P4. The authoritative
result stays in API-owned storage; request handlers do not wait indefinitely
for a reply subject.

## Consistency, backpressure, and failure behavior

- API/domain writes remain API-owned; browser-session state remains web-owned.
- P1 returns only the database snapshot it read. P2 returns the API's accepted
  or committed result. P3 frames and invalidations are hints until a P2 read
  confirms state. P4 acceptance is not completion.
- Saturated HTTP semaphores, pools, stream buffers, and worker leases reject or
  shed bounded work. No path creates an unbounded in-memory queue.
- A database or API outage produces an explicit unavailable response. It never
  broadens credentials, bypasses owner scope, or changes transport implicitly.
- Shutdown stops admission, drains bounded in-flight work, closes stateful
  connections, and leaves unacknowledged asynchronous work eligible for retry.

## Schema and migrations

`deploy/postgres/schema.sql` is the declarative desired state consumed by dpm;
`crates/canonical-store/src/migration.rs` is the executable SeaORM migration.
CI proves they converge. Production DDL runs only through the one-shot migration
identity. Long-lived web, API, and revoker processes must pass their exact
runtime-role verification and never receive the migration credential.
