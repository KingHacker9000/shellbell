FROM node:24-bookworm-slim AS pwa-build
WORKDIR /build/pwa
COPY pwa/package.json pwa/package-lock.json ./
RUN npm ci
COPY pwa/ ./
RUN npm run build && npm run validate:manifest

FROM rust:1.97-bookworm AS relay-build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
RUN cargo build --locked --release --package shellbell-relay

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --home /data shellbell && mkdir -p /app/pwa /data && chown shellbell:shellbell /data
COPY --from=relay-build /build/target/release/shellbell-relay /usr/local/bin/shellbell-relay
COPY --from=pwa-build /build/pwa/dist/ /app/pwa/
USER shellbell
ENV SHELLBELL_DATA_DIR=/data SHELLBELL_STATIC_DIR=/app/pwa SHELLBELL_LISTEN=0.0.0.0:8080
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=5 CMD curl --fail --silent http://127.0.0.1:8080/health || exit 1
ENTRYPOINT ["shellbell-relay"]
