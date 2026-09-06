# syntax=docker/dockerfile:1

FROM node:26-bookworm-slim@sha256:cd565714d4da3e84bfd341e31448f81d47c6362198f152345297c9c1154e6341 AS client-build
WORKDIR /build
COPY vendor/opto-sync-clients/ ./vendor/opto-sync-clients/
WORKDIR /build/client
COPY client/package.json client/package-lock.json ./
RUN npm ci
COPY client/ ./
RUN npm run typecheck && npm test && npm run build

FROM rust:1.98-slim-bookworm@sha256:1469a27c125cb5a3aebfa4f4e4665d935b02fb72cc093b2c974b3d740e43f157 AS rust-base
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update \
    && apt-get install --yes --no-install-recommends build-essential cmake
WORKDIR /build/canonical-web-server.rs

# The no-ingress worker build intentionally has no dependency on the browser
# bundle or the customer HTTP binary.
FROM rust-base AS revoker-build
COPY . .
RUN cargo build --locked --release -p canonical-session-revoker \
    && strip target/release/canonical-session-revoker

# The API image is intentionally independent of the browser bundle. It serves
# only the REST and WebSocket route family used by api.canonical.plus.
FROM rust-base AS api-build
COPY . .
RUN cargo build --locked --release -p canonical-web-server --bin canonical-api-server \
    && strip target/release/canonical-api-server

FROM rust-base AS web-build
COPY . .
COPY --from=client-build /build/client/dist ./client/dist
RUN cargo build --locked --release -p canonical-web-server --bin canonical-web-server \
    && strip target/release/canonical-web-server

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:adcd20c7b4c988b73cbfbddb26d2eee574571e6d7c9ffea29b3821e0690efb77 AS revoker
COPY --from=revoker-build --chown=65532:65532 \
    /build/canonical-web-server.rs/target/release/canonical-session-revoker \
    /usr/local/bin/canonical-session-revoker
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/canonical-session-revoker"]

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:adcd20c7b4c988b73cbfbddb26d2eee574571e6d7c9ffea29b3821e0690efb77 AS api
COPY --from=api-build --chown=65532:65532 \
    /build/canonical-web-server.rs/target/release/canonical-api-server \
    /usr/local/bin/canonical-api-server
EXPOSE 8081
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/canonical-api-server"]

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:adcd20c7b4c988b73cbfbddb26d2eee574571e6d7c9ffea29b3821e0690efb77 AS web
COPY --from=web-build --chown=65532:65532 \
    /build/canonical-web-server.rs/target/release/canonical-web-server \
    /usr/local/bin/canonical-web-server
COPY --from=client-build --chown=65532:65532 /build/client/dist /app/client
ENV APP_ASSET_DIR=/app/client
ENV STATIC_DIR=/app/static
EXPOSE 8081
USER 65532:65532
# ores-otel: in-process OTLP to the cluster collector. The *-sidecar.rs image is a separate loopback helper on 127.0.0.1:9090 — do not EXPOSE 4317/4318 or 9090.
ENV OTEL_SERVICE_NAME=canonical-web-server \
    OTEL_EXPORTER_OTLP_ENDPOINT=http://dd-otel-collector.observability.svc.cluster.local:4318 \
    RUST_LOG=info
# ores-sops: distroless has no shell — decrypt host-side (just env-docker-run / k8s Secret from env/enc). Do not bake plaintext or age keys into this image.
ENTRYPOINT ["/usr/local/bin/canonical-web-server"]
