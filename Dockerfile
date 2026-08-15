FROM rust:1.96-bookworm AS builder
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim
ARG VERSION=0.1.0
ARG REVISION=unknown
LABEL org.opencontainers.image.title="ring-intercom-bridge" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$REVISION" \
      org.opencontainers.image.licenses="Apache-2.0"
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 bridge \
    && useradd --uid 10001 --gid bridge --no-create-home --shell /usr/sbin/nologin bridge
COPY --from=builder /source/target/release/ring-intercom-bridge /usr/local/bin/ring-intercom-bridge
USER 10001:10001
EXPOSE 8775
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/usr/local/bin/ring-intercom-bridge", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/ring-intercom-bridge"]
