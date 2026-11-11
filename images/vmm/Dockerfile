# The per-sandbox VM host and rootfs builder (ADR-0006): Firecracker, the network and ext4 tools, the static guest
# init and the rootfs build script.
# Build from the repository root: docker build -f images/vmm/Dockerfile --target vmm -t tqa-vmm .

# Static guest init; the same toolchain image as the gateway, so the binary needs nothing from the guest's libc.
FROM rust:1.97.1-alpine@sha256:3c38f3f82c2f3d73da3b38e18d279393a04cb43ddded0e35088a8c3324d40900 AS vm-init-build
RUN apk add --no-cache musl-dev gcc binutils
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build --locked --release -p tqa-vm-init --bin tqa-vm-init && strip target/release/tqa-vm-init

# The one architecture decision (the only stage that reads the target architecture): the microVM backend is pinned and
# tested on x86_64 hosts with KVM only. On arm64 (Apple silicon under Docker Desktop has no KVM anyway) it says so
# plainly, before anything is downloaded. tools/test_check_images.py runs this stage's RUN; it runs
# on the build host (BUILDPLATFORM), so refusing a foreign architecture needs no emulation.
FROM --platform=$BUILDPLATFORM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS arch-check
ARG TARGETARCH
WORKDIR /arch
RUN test "${TARGETARCH:-amd64}" = amd64 || { echo "The microVM backend (Firecracker) is amd64-only; on ${TARGETARCH} use the Docker backend (the default), which needs no VM host image." >&2; exit 1; } \
    && echo x86_64 > machine

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS firecracker
# The release below is arch-check's machine; the pins are the x86_64 release's.
COPY --from=arch-check /arch/machine /tmp/machine
RUN test "$(cat /tmp/machine)" = x86_64
# Python and CA certificates for the download, from the vmm stage's signed snapshot (apt verifies every package hash).
RUN printf '%s\n' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20261020T000000Z/ trixie main' \
    > /etc/apt/sources.list \
    && rm /etc/apt/sources.list.d/debian.sources \
    && apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::http::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 install -y --no-install-recommends \
       python3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
ARG FIRECRACKER_VERSION=v1.17.0
# Both pins are the release's published values: the archive's SHA-256 and that of its published checksum file. The
# build fails on any other content, and the archive is verified again against the published file. The helper tries a
# file again after a network error (REL-U-012); a wrong file fails at once.
ARG FIRECRACKER_SHA256=06094a1108ae9e82aa4c23a775aa92758f53f1175d422270d9d6162cb9ade558
ARG FIRECRACKER_SUMS_SHA256=b89dea3edf551929f151b95d31168677ad2b8bb257d1d87a08470f7299749b7c
WORKDIR /tmp/release
RUN --mount=type=bind,source=images/fetch.py,target=/tmp/fetch.py python3 /tmp/fetch.py \
      https://github.com/firecracker-microvm/firecracker/releases/download/${FIRECRACKER_VERSION}/firecracker-${FIRECRACKER_VERSION}-x86_64.tgz \
      /tmp/release/firecracker-${FIRECRACKER_VERSION}-x86_64.tgz \
      https://github.com/firecracker-microvm/firecracker/releases/download/${FIRECRACKER_VERSION}/firecracker-${FIRECRACKER_VERSION}-x86_64.tgz.sha256.txt \
      /tmp/release/SHA256 \
    && printf '%s  %s\n' "$FIRECRACKER_SHA256" "firecracker-${FIRECRACKER_VERSION}-x86_64.tgz" \
       "$FIRECRACKER_SUMS_SHA256" SHA256 | sha256sum -c \
    && sha256sum -c --strict SHA256 \
    && tar --no-same-owner -xzf "firecracker-${FIRECRACKER_VERSION}-x86_64.tgz" -C /tmp \
    && release="/tmp/release-${FIRECRACKER_VERSION}-x86_64" \
    && install -D -m 0755 "$release/firecracker-${FIRECRACKER_VERSION}-x86_64" /out/usr/local/bin/firecracker \
    && install -D -m 0644 -t /out/usr/share/doc/firecracker "$release/LICENSE" "$release/NOTICE" "$release/THIRD-PARTY" \
    && mkdir -p /out/usr/local/lib/tqa \
    && printf '{\n  "version": "%s",\n  "archive_sha256": "%s",\n  "published_checksum_file_sha256": "%s",\n  "binary_sha256": "%s"\n}\n' \
       "$FIRECRACKER_VERSION" "$(cut -d' ' -f1 SHA256)" "$(sha256sum SHA256 | cut -d' ' -f1)" \
       "$(sha256sum /out/usr/local/bin/firecracker | cut -d' ' -f1)" >/out/usr/local/lib/tqa/firecracker.json

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS vmm
# A signed immutable snapshot pins transitive distribution versions; apt verifies the signed release and every package
# hash, so plain HTTP is safe here, and the slim base has no CA certificates for HTTPS.
RUN printf '%s\n' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20261020T000000Z/ trixie main' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/20261020T000000Z/ trixie-security main' \
    > /etc/apt/sources.list \
    && rm /etc/apt/sources.list.d/debian.sources \
    && apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::https::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::https::Timeout=30 install -y --no-install-recommends \
       e2fsprogs iproute2 nftables \
    && rm -rf /var/lib/apt/lists/*
COPY --from=firecracker /out/ /
# The rootfs pipeline runs in this image, so its identity pins the guest init and the build script (SBX-F-010).
COPY --from=vm-init-build /build/target/release/tqa-vm-init /usr/local/lib/tqa/tqa-vm-init
COPY --chmod=0755 images/rootfs/mkrootfs.sh /usr/local/lib/tqa/mkrootfs.sh
COPY --chmod=0755 images/vmm/launch.sh /usr/local/lib/tqa/launch
LABEL org.opencontainers.image.title="TensorQuay Agent VM host" \
      org.opencontainers.image.description="Firecracker and its per-VM network and storage tools" \
      org.opencontainers.image.licenses="Apache-2.0"
# The VMM's fixed unprivileged uid (ADR-0006). Setup steps that need the container's namespaced root say so explicitly.
USER 10000:10000
HEALTHCHECK NONE
