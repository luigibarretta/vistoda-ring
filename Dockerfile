FROM rust:1.96-bookworm@sha256:a339861ae23e9abb272cea45dfafde21760d2ce6577a70f8a926153677902663 AS builder
RUN apt-get update \
    && apt-get install --yes --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY vendor ./vendor
COPY src ./src
RUN cargo build --locked --release

COPY packaging/collect-licenses.sh /usr/local/bin/collect-licenses
RUN sh /usr/local/bin/collect-licenses /licenses

FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241
ARG VERSION=0.13.1
ARG REVISION=unknown
LABEL org.opencontainers.image.title="Vistoda Ring" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$REVISION" \
      org.opencontainers.image.source="https://git.luigibarretta.com/luigibarretta/vistoda-ring" \
      org.opencontainers.image.licenses="Apache-2.0"
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 bridge \
    && useradd --uid 10001 --gid bridge --no-create-home --shell /usr/sbin/nologin bridge
COPY --from=builder /licenses /usr/share/doc/vistoda/dependencies
COPY LICENSE NOTICE /usr/share/doc/vistoda/
COPY SOURCE_AVAILABILITY.md /usr/share/doc/vistoda/
COPY vendor/fcm-push-listener-ring/LICENSE /usr/share/doc/vistoda/fcm-push-listener-ring-LICENSE
COPY --from=builder /source/target/release/ring-intercom-bridge /usr/local/bin/ring-intercom-bridge
USER 10001:10001
EXPOSE 8775
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/usr/local/bin/ring-intercom-bridge", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/ring-intercom-bridge"]
