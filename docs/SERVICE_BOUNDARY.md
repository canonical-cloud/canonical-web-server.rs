# Web/API service boundary

The Canonical web server is the browser/session/HTMX tier. It preserves the
same discipline for any separate JSON API or internal domain service: typed
contracts, a verified subject, and the API transaction remain the authoritative
customer-data path.

| Connection | Allowed use | Boundary |
| --- | --- | --- |
| Direct database read | isolated encrypted web-session storage | never customer-domain rows, admin capability data, or sync mutations |
| Stateless HTTP/JSON | default for authenticated data and every write | API re-derives the subject and runs user-context transactions/CAS |
| Stateful TCP | authenticated WebSocket wakeups and bounded live status | stream data does not advance a durable sync cursor or authorize a mutation |
| NATS/MQ | post-commit notifications, telemetry, and slow work | never revocation, ledger, or user-write request/response authority |

Static marketing fallback remains last. `/api`, `/auth`, `/app`, and reserved
admin paths must never become a direct database or static-file bypass.
