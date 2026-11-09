# Immutable multi-platform indexes; compilation and the final filesystem have separate lifetimes.
ARG NODE_SOURCE=node:24.21.0-trixie-slim@sha256:8ec5d7557396cfe32d21c3f9c13072355ceab22b584578ca4bb28af31120cffe
FROM ${NODE_SOURCE} AS node-toolchain

FROM rust:1.97.1-bookworm@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97 AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build --locked --release -p tqa-runtime --bin tqa-runtime && strip target/release/tqa-runtime

FROM build AS clone-probe-build
RUN cargo test --locked --release -p tqa-runtime --test workspace_clone_probe --no-run \
    && install -d /probe \
    && find target/release/deps -maxdepth 1 -type f -name 'workspace_clone_probe-*' -executable \
       -exec install -m 755 '{}' /probe/workspace-clone-probe ';' \
    && test -x /probe/workspace-clone-probe

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS runtime-base
# `python` is python3 through a plain link: Debian's python-is-python3 package is GPL-3.0, and a symlink is no binary.
# The same signed snapshot as the Workstation's, so a cached base layer is an exact, reviewed package set (REL-U-010).
# Over http, as the image's own live source was: the slim image has no CA certificates until this step installs them;
# apt checks the snapshot's signed Release file and every package's hash. The Workstation switches to https.
RUN printf '%s\n' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20261029T120000Z/ trixie main' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/20261029T120000Z/ trixie-security main' \
    > /etc/apt/sources.list \
    && rm /etc/apt/sources.list.d/debian.sources \
    && apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::http::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 install -y --no-install-recommends \
    bash coreutils findutils grep sed git python3 ca-certificates procps tini util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -o 1000 -g 1000 /workspace \
    && echo 'tqa:x:1000:1000:TensorQuay sandbox:/workspace:/usr/sbin/nologin' >> /etc/passwd \
    && echo 'tqa:x:1000:' >> /etc/group \
    && ln -s python3 /usr/bin/python
# The sandbox user, uid 1000, has a name: the gateway runs every sandbox as 1000:1000, and a program that looks its user
# up (the Desktop's session bus, dbus-daemon) refuses a uid with no passwd entry (REL-I-003). No login, no home made.
# Distribution copyright files remain in /usr/share/doc; the programs are an image aggregate.
LABEL org.opencontainers.image.title="TensorQuay Agent Runtime" \
      org.opencontainers.image.description="Sandbox agent process and workspace tools" \
      org.opencontainers.image.licenses="Apache-2.0"
# No runtime binary and no protocol label here: each image copies the binary last, with the label (REL-U-011), so a
# change to the program alone reuses this stage and every package and toolchain layer built on it.
USER 1000:1000
ENV HOME=/tmp
WORKDIR /workspace
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/tqa-runtime"]

FROM runtime-base AS clone-probe
# This explicit test target is never selected by either production runtime target.
COPY --from=clone-probe-build /probe/workspace-clone-probe /usr/local/bin/workspace-clone-probe
# Must equal tqa_types::PROTOCOL_REVISION in every stage that copies the binary; SBX-U-001 fails when one differs.
LABEL org.tensorquay.protocol-revision="14"
COPY --from=build /build/target/release/tqa-runtime /usr/local/bin/tqa-runtime
ENTRYPOINT ["/usr/local/bin/workspace-clone-probe"]

# The Go toolchain's one architecture decision (the only stage that reads TARGETARCH): which official archive, and the
# SHA-256 go.dev publishes for it. amd64 and arm64 (Apple silicon under Docker Desktop) are pinned; any other
# architecture stops here, before any download or package install. tools/test_check_images.py runs this stage's RUN; it runs
# on the build host (BUILDPLATFORM), so refusing a foreign architecture needs no emulation.
FROM --platform=$BUILDPLATFORM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS arch-check
ARG TARGETARCH
ARG GO_VERSION=1.27.1
ARG GO_SHA256_AMD64=63d339f0da5ab53635a56f2490a7984dfe12dfcff22ad749f63edaf590168445
ARG GO_SHA256_ARM64=3450b45a3f9ee8568792736a5c5e70a1f2e9b36c35a8f74958c03e51d7d92bec
WORKDIR /arch
RUN case "${TARGETARCH:-amd64}" in \
      amd64) sum="$GO_SHA256_AMD64" ;; \
      arm64) sum="$GO_SHA256_ARM64" ;; \
      *) echo "the Go toolchain is pinned for amd64 and arm64 only, not ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && printf 'https://go.dev/dl/go%s.linux-%s.tar.gz\n' "$GO_VERSION" "${TARGETARCH:-amd64}" > go-url \
    && printf '%s  /tmp/go.tar.gz\n' "$sum" > go-sha256

# The official Go release as arch-check chose it, verified by its SHA-256 and extracted in its own stage, so the archive
# never reaches an image layer. Fetched with Python rather than ADD, whose checksum can't come from another stage; a
# mismatch stops the build before anything is extracted.
FROM runtime-base AS go-toolchain
USER 0:0
COPY --from=arch-check /arch/go-url /arch/go-sha256 /tmp/
RUN python3 -c 'import sys, urllib.request; urllib.request.urlretrieve(open(sys.argv[1]).read().strip(), "/tmp/go.tar.gz")' \
       /tmp/go-url \
    && sha256sum -c /tmp/go-sha256 \
    && tar --no-same-owner -C /usr/local -xzf /tmp/go.tar.gz \
    && rm /tmp/go.tar.gz /tmp/go-url /tmp/go-sha256

# The Workstation's files, without the runtime binary: the workstation target (below) copies it last.
FROM runtime-base AS workstation-os
USER 0:0
# pandas, matplotlib and openpyxl for data work (F24) are Debian's, as decided on 30 Sep 2026; their licence audit
# is WORKSTATION-DATA-LICENCES.md, and PACKAGES.md lists every package of each build.
# A signed immutable snapshot pins transitive distribution versions and verifies package hashes. Its date, 2026-10-29
# 12:00 UTC, is after Debian 13's security updates for the c17 release dry run's vulnerability gate: Chromium
# 156.0.8078.73, libheif 1.23.6, brotli 1.1.0-2+deb13u1 and ca-certificates +deb13u1, all in the security archive's
# snapshot of 07:24 UTC that day (the main archive is still 13.7 then, as on 2026-10-20). REL-I-003 checks the versions
# shipped, REL-U-010 the date. The base digest carries xz-utils' fix (liblzma5 u2): nothing here upgrades a package the
# base already has.
# Noto CJK and Noto Color Emoji draw Chinese, Japanese, Korean and emoji as text, not empty boxes, in Chromium and on
# the Desktop, which builds on this stage (the c17 canvas capture, 23 Oct 2026); REL-I-005 checks fontconfig finds
# both. The fonts are SIL OFL 1.1; PACKAGES.md links each package's copyright file.
RUN printf '%s\n' \
    'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20261029T120000Z/ trixie main' \
    'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/20261029T120000Z/ trixie-security main' \
    > /etc/apt/sources.list \
    && apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::https::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::https::Timeout=30 install -y --no-install-recommends \
    python3-pip python3-venv python3-pytest build-essential ripgrep fd-find jq curl unzip zip less file \
    wget tree sqlite3 chromium fonts-liberation fonts-noto-cjk fonts-noto-color-emoji \
    python3-pandas python3-matplotlib python3-openpyxl \
    && rm -rf /var/lib/apt/lists/*
# A browser the agent starts stays quiet, the Desktop's included: no call of its own that the allow-list would refuse
# and the task would show (E2E-ENV-010). The managed policy turns off what a policy reaches (network time, the search
# provider, sign-in, sync, updates, metrics, Safe Browsing's lookups, suggestions, translation, the password manager and
# autofill's saving); Debian's wrapper sources the switches file at every start for what no policy reaches (background
# networking, push messaging's check-in, the component updater, the optimization guide, autofill's server queries). Both
# apply whatever flags the agent passes.
COPY images/runtime/chromium-policy.json /etc/chromium/policies/managed/tqa-quiet.json
COPY --chmod=0644 images/runtime/chromium-quiet-flags.sh /etc/chromium.d/tqa-quiet
COPY --from=node-toolchain /usr/local/bin/node /usr/local/bin/node
COPY --from=node-toolchain /usr/local/lib/node_modules/ /usr/local/lib/node_modules/
# The npm release Node bundles carries vulnerable dependencies (dependency audit A4); this one bundles fixed ones.
# Its tarball holds its dependencies, so unpacking it is the whole install, with no registry access and no scripts.
ARG NPM_VERSION=11.20.0
ADD --checksum=sha256:d1a92f40e6c407b84c3a00c3cf978a10b24fd42f153c527e2016cef7bb34a483 \
    https://registry.npmjs.org/npm/-/npm-${NPM_VERSION}.tgz /tmp/npm.tgz
# No npm release yet bundles the fixed undici and brace-expansion (11.21.0 and 12.2.0 still carry 6.28.0 and 5.0.9), so
# those two bundled packages are replaced by their fixed patch releases, same dependencies (c15 release gate, 8 Oct
# 2026: GHSA-rfgv-xxqx-mfg5, GHSA-3wwx-pv8p-q78v; GHSA-6j4f-fj2g-mc7p, GHSA-qhr7-859c-m2p7, GHSA-q2hr-2g5m-vwhr).
# undici is 6.29.1 since the c17 release gate (6.28.1 HIGH; 6.29.1 has no dependencies, as 6.28.1).
# Drop them when an npm release bundles these versions or newer.
ADD --checksum=sha256:29c4b8e0b2d86cb8795571c1959012ce057a14ecfd34858335f47c31e770a04f \
    https://registry.npmjs.org/undici/-/undici-6.29.1.tgz /tmp/undici.tgz
ADD --checksum=sha256:ef8448ec78f20b692f04fa6d01f39b5ab34c66404bea3429f5a39c6c9e0be8b4 \
    https://registry.npmjs.org/brace-expansion/-/brace-expansion-5.0.12.tgz /tmp/brace-expansion.tgz
RUN rm -rf /usr/local/lib/node_modules/npm && install -d /usr/local/lib/node_modules/npm \
    && tar --no-same-owner --strip-components=1 -C /usr/local/lib/node_modules/npm -xzf /tmp/npm.tgz \
    && for p in undici brace-expansion; do \
         d=/usr/local/lib/node_modules/npm/node_modules/$p; rm -rf "$d" && install -d "$d" \
         && tar --no-same-owner --strip-components=1 -C "$d" -xzf "/tmp/$p.tgz"; done \
    && rm /tmp/npm.tgz /tmp/undici.tgz /tmp/brace-expansion.tgz
COPY --from=node-toolchain /usr/local/LICENSE /usr/share/doc/node/LICENSE
COPY --from=go-toolchain /usr/local/go /usr/local/go
COPY --from=go-toolchain /usr/local/go/LICENSE /usr/share/doc/go/LICENSE
# Before the inventory step, so the in-image PACKAGES.md lists Go. The image declares only PATH and HOME (the backend
# refuses keys the gateway does not set), so `go` is the shim in /usr/local/bin, on the documented PATH; it sets
# GOTOOLCHAIN=local and GOTMPDIR (the sandbox /tmp is too small and noexec for building and running tests).
COPY --chmod=0755 images/runtime/go-shim.sh /usr/local/bin/go
COPY images/runtime/packages.py tools/release/sbom_sources.py /usr/local/lib/tqa/
COPY images/runtime/package.json images/runtime/package-lock.json /usr/local/lib/tqa/browser/
ARG NODE_SOURCE
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && ln -s ../lib/node_modules/corepack/dist/corepack.js /usr/local/bin/corepack \
    && ln -s ../go/bin/gofmt /usr/local/bin/gofmt \
    && HOME=/tmp PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm ci --prefix /usr/local/lib/tqa/browser \
       --ignore-scripts --omit=optional --no-audit --no-fund --cache=/tmp/npm-cache \
       --fetch-retries=2 --fetch-timeout=60000 \
    && rm -rf /tmp/npm-cache \
    && ln -s /usr/local/lib/tqa/browser/node_modules /node_modules \
    && install -d /usr/share/tqa \
    && printf '%s\n' "$NODE_SOURCE" > /usr/share/tqa/node-origin.txt \
    && python3 /usr/local/lib/tqa/packages.py --inside > /usr/share/tqa/PACKAGES.md
USER 1000:1000
ENV HOME=/workspace/.tqa/home

FROM workstation-os AS workstation
# The built-in environment this target is: what it really has (Python, Node, Go and C builds, Chromium, and Python's
# data libraries).
LABEL org.tensorquay.environment="workstation" \
      org.tensorquay.environment.name="Workstation" \
      org.tensorquay.environment.capabilities="shell,python,node,build,browser,data"
LABEL org.tensorquay.protocol-revision="14"
COPY --from=build /build/target/release/tqa-runtime /usr/local/bin/tqa-runtime

# ADR-0039 slice 1, the computer-use spike: the Workstation with a virtual display, for RT-I-046's browser cases. A
# test image: it carries no environment labels, so the gateway never offers it. The Desktop template's stage (slice 2)
# builds on it and replaces its init. The runtime starts the headed Chromium itself (its profile in the temporary
# space); the init only starts the display.
FROM workstation-os AS desktop-spike-os
USER 0:0
RUN apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::https::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::https::Timeout=30 install -y --no-install-recommends \
    xvfb xauth fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
COPY --chmod=0755 images/runtime/desktop-spike-init.sh /usr/local/bin/tqa-desktop-init
USER 1000:1000
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/tqa-desktop-init"]

FROM desktop-spike-os AS desktop-spike
LABEL org.tensorquay.protocol-revision="14"
COPY --from=build /build/target/release/tqa-runtime /usr/local/bin/tqa-runtime

# ADR-0039 slice 2: the Desktop template, the built-in environment with a desktop the agent sees and operates. On the
# spike's display it adds the window manager, a session bus with the accessibility (AT-SPI) bus, xdotool, a VNC server
# (loopback only, no password: the runtime's screen bridge is the only way in) and a few applications. Every addition
# is a Debian package from the Workstation's pinned snapshot, run as its own program and never linked with project code
# (ADR-0017 point 7; ADR-0039 section 1): nothing vendored, patched or downloaded. Its init replaces the spike's.
FROM desktop-spike-os AS desktop-os
USER 0:0
RUN apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::https::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::https::Timeout=30 install -y --no-install-recommends \
    openbox dbus-daemon dbus-bin at-spi2-core xdotool x11-utils x11vnc \
    xterm pcmanfm mousepad libreoffice-writer libreoffice-calc libreoffice-gtk3 fonts-dejavu \
    && rm -rf /var/lib/apt/lists/* \
    && python3 /usr/local/lib/tqa/packages.py --inside > /usr/share/tqa/PACKAGES.md \
    && update-alternatives --set x-www-browser /usr/bin/chromium \
    && update-alternatives --set gnome-www-browser /usr/bin/chromium \
    && install -d /usr/libexec/tqa \
    && dpkg-divert --rename --divert /usr/libexec/tqa/xdotool --add /usr/bin/xdotool
# x11-utils is named, not left to x11vnc's, xterm's and Chromium's dependencies: the grounding capture reads a desktop
# screen's client area with its xwininfo.
# xdotool is the computer tool's, off the sandbox user's PATH: screen input from the shell skips the tool's action cap,
# settle check and step pictures (TOOL-U-064). dpkg-divert keeps it xdotool's file for dpkg and the SBOM.
COPY --chmod=0755 images/runtime/desktop-init.sh /usr/local/bin/tqa-desktop-init
# Every headed browser started by hand on the desktop uses the runtime's profile (ADR-0039 section 6).
COPY --chmod=0644 images/runtime/chromium-desktop-profile.sh /etc/chromium.d/tqa-desktop-profile
USER 1000:1000
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/tqa-desktop-init"]

FROM desktop-os AS desktop
LABEL org.tensorquay.environment="desktop" \
      org.tensorquay.environment.name="Desktop" \
      org.tensorquay.environment.capabilities="shell,python,node,build,browser,data,desktop"
LABEL org.tensorquay.protocol-revision="14"
COPY --from=build /build/target/release/tqa-runtime /usr/local/bin/tqa-runtime

# RT-I-046 (ADR-0039 slices 1 and 2): the computer tool's two probes, built as clone-probe is and run on the Desktop
# image's display: the browser cases, then the AT-SPI cases against Mousepad. Test-only, like clone-probe: no
# production target selects it.
FROM build AS desktop-probe-build
RUN cargo test --locked --release -p tqa-tools --test computer_browser_probe --test computer_desktop_probe \
      --test computer_grounding_capture --no-run \
    && install -d /probe \
    && for probe in computer_browser_probe computer_desktop_probe computer_grounding_capture; do \
         find target/release/deps -maxdepth 1 -type f -name "${probe}-*" -executable \
           -exec install -m 755 '{}' "/probe/$(echo "$probe" | tr _ -)" ';' \
         && test -x "/probe/$(echo "$probe" | tr _ -)" || exit 1; \
       done

FROM desktop AS desktop-probe
# Test-only: xsel (MIT-style licence) sets and reads the X selections for RT-I-046's clipboard case; the product image
# has no clipboard tool, and nothing in it needs one.
USER 0:0
RUN apt-get -o APT::Update::Error-Mode=any -o Acquire::Retries=3 -o Acquire::https::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::https::Timeout=30 install -y --no-install-recommends xsel \
    && rm -rf /var/lib/apt/lists/*
USER 1000:1000
COPY --from=desktop-probe-build /probe/ /usr/local/bin/
# Both probes, each whatever the one before did, the clipboard case last and alone (the script says why).
COPY --chmod=0755 images/runtime/desktop-probe-run.sh /usr/local/bin/tqa-desktop-probe-run
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/tqa-desktop-init", "--exec", "/usr/local/bin/tqa-desktop-probe-run"]

# Keep the default build equivalent to the base target.
FROM runtime-base AS base
# The built-in environment this target is (design-environments): the gateway registers it from these labels, so
# nobody types an image ID. Only this target and workstation carry them; test images built from runtime-base don't.
LABEL org.tensorquay.environment="minimal" \
      org.tensorquay.environment.name="Minimal" \
      org.tensorquay.environment.capabilities="shell,python"
LABEL org.tensorquay.protocol-revision="14"
COPY --from=build /build/target/release/tqa-runtime /usr/local/bin/tqa-runtime
