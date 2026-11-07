# Rust 1.97.1 on Alpine 3.24; the multi-platform index is immutable.
FROM rust:1.97.1-alpine@sha256:3c38f3f82c2f3d73da3b38e18d279393a04cb43ddded0e35088a8c3324d40900 AS build
RUN apk add --no-cache musl-dev gcc binutils
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
ENV SQLX_OFFLINE=true
RUN cargo build --locked --release -p tqa-gateway --bin tqa-gateway && strip target/release/tqa-gateway

# The final filesystem contains only the static binary. No shell or package manager is shipped.
FROM scratch
# What this gateway's configuration accepts that older ones refuse, so a test harness can tell before it writes them:
# `[runtime.builtin_images]` (pin a built-in environment to an image by digest).
LABEL org.tensorquay.gateway.builtin-images="1"
COPY --from=build /build/target/release/tqa-gateway /tqa-gateway
USER 1000:1000
EXPOSE 8080
HEALTHCHECK --interval=5s --timeout=3s --start-period=30s --retries=12 CMD ["/tqa-gateway", "healthcheck", "127.0.0.1:8080"]
ENTRYPOINT ["/tqa-gateway"]
CMD ["/etc/tqa/gateway.toml"]
