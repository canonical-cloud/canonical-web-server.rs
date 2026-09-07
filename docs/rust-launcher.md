# Shared Rust launcher in all three service images

The `web`, `api`, and `revoker` targets each use the same canonical `ores-launcher`
from ores-otel/ores.otel.log. `docker/ores-launcher.rev` is the single immutable
source pin, installed with Cargo's locked dependency resolution. No launcher
implementation is copied. Existing application/base-image pins, binaries, ports,
static assets, role separation, and target names are unchanged.

The app stays in exec-form ENTRYPOINT after `/ores-launcher`, with an empty CMD,
so arguments supplied after the image name retain their previous meaning. The
launcher records the command through ores-otel, flushes stderr locally and uses
Unix exec. It does not fork/wait, decrypt secrets, add an ingress endpoint, or
substitute for the app's shutdown/child-process handling. The revoker still has
no browser dependency or application ingress.

Use `--entrypoint /ores-launcher IMAGE /absolute/program [arguments...]` only when
intentionally replacing the executable. Secret injection remains host/platform
side. Never put credentials in argv: bounded redaction cannot identify every
positional secret. Existing telemetry exporters remain in the application.

CI builds and checks each of the three targets on native amd64 and arm64. The
host-side image smoke validates exact metadata, nonroot, exits 64/127, canonical
stderr records, literal argv logging and absent sh/bash using read-only,
network-none, cap-drop-ALL and no-new-privileges containers. This is launcher/image
certification, not a replacement for the existing test, audit, RLS, declarative
schema, browser and application container-smoke gates. No production deployment
or database/secret mutation is part of this rollout.
