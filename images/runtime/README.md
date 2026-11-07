# Runtime images

Both targets contain the same runtime executable and tini entrypoint, run as `1000:1000`, and support a read-only
root filesystem. `base` keeps the small tool set; `workstation` adds an offline coding and browser environment. The default build still
selects `base`. Neither target changes sandbox network policy. Both install their Debian packages from the same signed
snapshot, so an image's apt sources name that snapshot, not the live mirror: a template built `FROM` either installs
from the snapshot unless it sets its own sources.

```sh
docker build --target base -f images/runtime/Dockerfile -t tqa-runtime:base .
docker build --target workstation -f images/runtime/Dockerfile -t tqa-runtime:workstation .
```

`workstation` includes git, Python with pip, venv, pytest and the data libraries pandas, matplotlib and openpyxl
(Debian's; licence audit in [WORKSTATION-DATA-LICENCES.md](WORKSTATION-DATA-LICENCES.md)), Node.js LTS with npm and
Corepack, gcc, g++, make, ripgrep, fd-find (the `fdfind` command), jq, curl, wget, tree, sqlite3, unzip, zip, less, file
and procps, plus Chromium with Playwright and the Noto CJK and Noto Color Emoji fonts, so Chinese, Japanese, Korean and
emoji draw as text in the browser and on the Desktop. Wget supports ordinary file retrieval when policy permits; tree
and sqlite3 cover directory inspection and local data work. ImageMagick is omitted because no bundled workflow needs its
extra codecs. Debian additions come from the signed snapshot dated `20261020T000000Z`, including its security archive;
APT verifies the archive and package hashes. Node, npm and Corepack come from the digest-pinned official Node image. No
package manager downloads tools at runtime as part of image startup. Corepack itself is installed; extra package
managers and arbitrary project dependencies are not pre-cached. Go comes from the official release archive (1.27.1,
amd64 only), verified by the SHA-256 go.dev publishes and extracted in its own build stage, so the archive never reaches
a layer. It adds 296 MB. `go` is a small shim in `/usr/local/bin`, on the documented sandbox `PATH`, which pins
`GOTOOLCHAIN=local` so Go never downloads another toolchain; the image itself declares only `PATH` and `HOME`, as every
runtime image must (`gofmt` is linked beside it). The module cache (`$HOME/go/pkg/mod`) and the build cache
(`$HOME/.cache/go-build`) derive from the private home. The `go` shim on `PATH` sets `GOTMPDIR` to `$HOME/.cache/go-tmp`
(unless it is already set) and creates it, because the sandbox's `/tmp` is too small and `noexec` for building and
running test binaries. `go` on `PATH` is the supported entry point: tools that run `/usr/local/go/bin/go` directly
bypass the shim and fall back to `/tmp`. A cold build cache is about 61 MB, and Go's caches and temporary files count
against the user's workspace quota. Fetching modules (`go mod download`, `go get`) needs egress rules for
`proxy.golang.org:443` and `sum.golang.org:443`; sandboxes deny egress by default, so work offline with `GOPROXY=off` or
the standard library otherwise. Go counts under the `build` capability. Another architecture fails the build explicitly
rather than getting the wrong toolchain.

## Protocol revision label

Every target carries `org.tensorquay.protocol-revision`, set with the runtime binary in the target's own last
stage: the binary is each image's last layer, so a change to the program alone reuses every package layer below it
(REL-U-011), and `runtime-base` holds neither. Each copy of the label must equal `PROTOCOL_REVISION` in
`tqa-types`. SBX-U-001 fails when a target lacks the label, when a stage sets it without copying the binary or the
reverse, or when a copy differs from the constant, so raising the revision means changing every copy in the same
commit. The Docker backend's
`check_runtime_image` reads the label before any task starts. An image without the label counts as revision 1, and
an image below the gateway's floor fails with `sandbox.runtime_too_old` (ADR-0026). A build argument is deliberately
not used: every build command would have to pass it.

## Environment

Both images inherit only `PATH` and `HOME`, which the gateway supplies explicitly in `sandbox.env`. Build-only
package-manager settings stay on their install commands: extra inherited keys prevent sandbox image admission.

The shell tool creates its private home at `/workspace/.tqa/home` with mode 0700 before its first command. Pip and
npm use their normal caches beneath HOME. Commands invoked directly with `docker exec` should create HOME first
when needed; mounting an empty workspace hides directories from the image. Only the workspace and tmpfs need writes.

Use the distribution packages from a virtual environment without accessing a package index:

```sh
python3 -m venv --system-site-packages .venv
.venv/bin/python -m pytest
```

A plain isolated venv has pip but does not inherit the distribution's pytest. Runtime package-index access remains
subject to the sandbox's network policy.

## Offline browser

Chromium comes from the same Debian snapshot. Playwright and playwright-core are installed from the image-owned
`package-lock.json`, with SHA-512 integrity checks, lifecycle scripts disabled and browser downloads disabled.
The immutable `/node_modules` link makes `require('playwright')` available from a workspace script without installation.
Use the installed Chromium executable explicitly; no Playwright browser bundle is downloaded at run time.

```js
const { chromium } = require('playwright');

async function screenshot() {
  const context = await chromium.launchPersistentContext(`${process.env.HOME}/chromium-profile`, {
    executablePath: '/usr/bin/chromium',
    chromiumSandbox: false,
    headless: true,
    args: [`--disk-cache-dir=${process.env.HOME}/.cache/chromium`],
  });
  try {
    const page = await context.newPage();
    await page.goto('file:///workspace/index.html');
    await page.screenshot({ path: '/workspace/screenshot.png' });
  } finally {
    await context.close();
  }
}

screenshot().catch(error => { console.error(error); process.exitCode = 1; });
```

Chromium's inner sandbox is off; the container or microVM remains the security boundary (ARCHITECTURE §12).
Read-only root, uid 1000, dropped capabilities, no-new-privileges, builtin seccomp and resource limits all remain in
force. Profiles and caches use the private home; temporary files use tmpfs. Network access remains subject to sandbox
policy. The Debian Chromium and Playwright version pair is covered by SBX-C-017; update and test them together.

A browser the agent starts, the Desktop's included, makes no background call of its own, each of which the allow-list
would refuse and the task would show. A managed policy (`chromium-policy.json`, installed as
`/etc/chromium/policies/managed/tqa-quiet.json`) turns off what a policy reaches: network time, the default search
provider, sign-in and sync, updates, metrics, Safe Browsing's lookups, suggestions, prediction, translation, spell
check, and the password manager and address and card autofill, so no "Save password?" or "Save address?" bubble covers a
page the agent or a person is working on. A switches file (`chromium-quiet-flags.sh`, installed as
`/etc/chromium.d/tqa-quiet`, which Debian's `/usr/bin/chromium` sources after its own files at every start) does the
rest: it drops Debian's remote extensions, so the wrapper turns background networking off; turns off the optimization
guide's fetches and autofill's questions to its server about each form; and points the services no switch turns off
(push messaging's check-in, the component updater and the Google account checks) at a closed loopback port, which never
goes through the proxy. A page a person opens, accounts.google.com included, loads as any page does.

## Inventory and verification

The build generates `/usr/share/tqa/PACKAGES.md` from the installed Debian and npm metadata with the release SBOM
library (`tools/release/sbom_sources.py`, copied beside the script). Distribution copyright files and the Node notice
remain in the image. After building, regenerate the repository inventory or check it:

```sh
python3 images/runtime/packages.py
python3 images/runtime/packages.py --check
```

[PACKAGES.md](PACKAGES.md) lists the exact versions, licences (an exact SPDX expression, or a `LicenseRef` to the
package's own notice) and matching source locations for release publication under ADR-0017. The checked-in inventory
describes the recorded architecture; regenerate it for another architecture. Package snapshots and image digests are
updated together with this inventory during maintenance.

The ignored Docker tests fail if Docker or the selected image is missing. Run the existing eight runtime/link cases
on base, and add the offline toolchain, private cache and browser checks on Workstation. The long scenario runs on both:

```sh
TQA_RUNTIME_TEST_IMAGE=tqa-runtime:base cargo nextest run -p tqa-runtime --test docker_image --test link_runs --run-ignored all --retries 0 -E 'not test(rt_i_012) and not test(sbx_c_017)'
TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation cargo nextest run -p tqa-runtime --test docker_image --test link_runs --run-ignored all --retries 0
TQA_DOCKER_TESTS=1 TQA_RUNTIME_TEST_IMAGE=tqa-runtime:base cargo nextest run -p tqa-sandbox --test runtime_image --run-ignored all --retries 0
TQA_DOCKER_TESTS=1 TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation cargo nextest run -p tqa-sandbox --test runtime_image --run-ignored all --retries 0
```

RT-I-012 runs the offline toolchain with no network, a read-only root, dropped capabilities, no new privileges and
resource limits. Its companion test drives a real shell call through the runtime to verify that npm writes its cache
under the private home. SBX-C-017 opens local HTML with no network, interacts with its JavaScript and validates a
640×480 PNG owned by uid 1000, under the same restrictions. The long-scenario test executes all twelve demo calls and requires exactly one failed shell
exit followed by a passing run. These fixture checks do not replace the browser UX suite.
The sandbox admission check starts the actual image entrypoint with the gateway's environment allowlist, using the
production backend and its hardening settings; a raw `docker run` alone does not exercise that admission policy.

The explicit `clone-probe` target builds a test executable for the sandbox's local Git-server contract. It is copied
into an already admitted sandbox to call the runtime initializer with a fixture CA and separate fixture directories.
It is absent from `base` and `workstation`; both production targets build the runtime without test features. See the
[sandbox test instructions](../../crates/tqa-sandbox/README.md) for its required image pins and invocation.
