FROM rust:1.98-alpine AS build

ENV RUSTFLAGS="-C target-feature=+crt-static"
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --locked -p deploy \
    && mkdir -p /tmp/iridium-data \
    && chown 65534:65534 /tmp/iridium-data

FROM scratch AS iridium
COPY --from=build /src/target/release/iridium-server /iridium-server
COPY --from=build /src/target/release/healthcheck /healthcheck
COPY --from=build --chown=65534:65534 /tmp/iridium-data /data
USER 65534:65534
EXPOSE 7400
ENTRYPOINT ["/iridium-server"]
HEALTHCHECK --interval=2s --timeout=2s --start-period=2s --retries=3 CMD ["/healthcheck", "tcp", "127.0.0.1:7400"]

FROM scratch AS registry
COPY --from=build /src/target/release/argo-registry /argo-registry
COPY --from=build /src/target/release/healthcheck /healthcheck
USER 65534:65534
EXPOSE 7500
ENTRYPOINT ["/argo-registry"]
HEALTHCHECK --interval=2s --timeout=2s --start-period=2s --retries=3 CMD ["/healthcheck", "http", "127.0.0.1:7500", "/index"]

FROM scratch AS aurum
COPY --from=build /src/target/release/aurum-node /aurum-node
COPY --from=build /src/target/release/healthcheck /healthcheck
USER 65534:65534
EXPOSE 7600 7700/udp
ENTRYPOINT ["/aurum-node"]
HEALTHCHECK --interval=2s --timeout=2s --start-period=2s --retries=3 CMD ["/healthcheck", "http", "127.0.0.1:7600", "/health"]
