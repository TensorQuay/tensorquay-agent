# Test-only endpoint; immutable multi-platform base indexes.
FROM rust:1.97.1-alpine@sha256:3c38f3f82c2f3d73da3b38e18d279393a04cb43ddded0e35088a8c3324d40900 AS build
RUN apk add --no-cache musl-dev gcc binutils
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build --locked --release -p tqa-testkit --bin tqa-fake-model && strip target/release/tqa-fake-model

FROM alpine:3.24@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6
COPY --from=build /build/target/release/tqa-fake-model /usr/local/bin/tqa-fake-model
COPY crates/tqa-testkit/scenarios/demo.json crates/tqa-testkit/scenarios/demo-long.json \
     crates/tqa-testkit/scenarios/reply-cut.json /scenarios/
USER 1000:1000
EXPOSE 8080
HEALTHCHECK --interval=5s --timeout=3s --start-period=5s --retries=3 CMD ["wget", "-q", "-O", "/dev/null", "--header", "Authorization: Bearer healthcheck", "http://127.0.0.1:8080/v1/models"]
ENTRYPOINT ["/usr/local/bin/tqa-fake-model"]
CMD ["--scenario", "/scenarios/demo.json"]
