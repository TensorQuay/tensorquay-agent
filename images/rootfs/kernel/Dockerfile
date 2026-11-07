# Pinned microVM guest kernel (ADR-0006): Linux 6.18.54 longterm with Firecracker v1.17.0's 6.18 guest configuration.
# Build: docker build -f images/rootfs/kernel/Dockerfile --target kernel --output type=local,dest=<dir> images/rootfs/kernel
# The output is vmlinux plus manifest.json. The kernel is GPL-2.0-only; it is built locally and not distributed.

# The one architecture decision (the only stage that reads the target architecture): the guest kernel is
# Firecracker's x86_64 configuration, and the microVM backend is amd64-only, so arm64 is refused plainly, before any
# download or package install. tools/test_check_images.py runs this stage's RUN; it runs
# on the build host (BUILDPLATFORM), so refusing a foreign architecture needs no emulation.
FROM --platform=$BUILDPLATFORM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a AS arch-check
ARG TARGETARCH
WORKDIR /arch
RUN test "${TARGETARCH:-amd64}" = amd64 || { echo "The microVM guest kernel is built for amd64 only; on ${TARGETARCH} use the Docker backend (the default), which needs no kernel." >&2; exit 1; } \
    && echo x86_64 > machine

FROM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a AS build
# The configuration below is arch-check's machine; the pins are x86_64's.
COPY --from=arch-check /arch/machine /tmp/machine
RUN test "$(cat /tmp/machine)" = x86_64
# A signed immutable snapshot; apt verifies the signed release and every package hash, so plain HTTP is safe here, and
# the slim base has no CA certificates for HTTPS.
RUN printf '%s\n' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20260923T000000Z/ trixie main' \
    > /etc/apt/sources.list \
    && rm /etc/apt/sources.list.d/debian.sources \
    && apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::https::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::https::Timeout=30 install -y --no-install-recommends \
       bc bison build-essential flex libelf-dev libssl-dev xz-utils \
    && rm -rf /var/lib/apt/lists/*
ARG KERNEL_VERSION=6.18.54
ARG KERNEL_SHA256=9df30b02dd8102bbd0be52556288ef6889ddbe7f1ddb96fbf847d0becf3eacac
ARG FIRECRACKER_VERSION=v1.17.0
ARG CONFIG_SHA256=ba22401a0c7292a4c024ebcd10a562d4a1f1bfd2faed671406d3b159c0cf5215
ARG JOBS=8
ADD --checksum=sha256:9df30b02dd8102bbd0be52556288ef6889ddbe7f1ddb96fbf847d0becf3eacac \
    https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-${KERNEL_VERSION}.tar.xz /src/linux.tar.xz
ADD --checksum=sha256:ba22401a0c7292a4c024ebcd10a562d4a1f1bfd2faed671406d3b159c0cf5215 \
    https://raw.githubusercontent.com/firecracker-microvm/firecracker/${FIRECRACKER_VERSION}/resources/guest_configs/microvm-kernel-ci-x86_64-6.18.config \
    /src/base.config
COPY tqa.config /src/tqa.config
COPY build.sh /src/build.sh
# A fixed timestamp, user and host make the build independent of when and where it runs.
ENV KBUILD_BUILD_TIMESTAMP="1970-01-01 00:00:00 UTC" KBUILD_BUILD_USER=tqa KBUILD_BUILD_HOST=build
RUN bash /src/build.sh

FROM scratch AS kernel
COPY --from=build /out/ /
