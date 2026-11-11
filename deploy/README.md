# One-box Docker deployment

The stack runs PostgreSQL, the gateway and the web application. Supply a content-pinned runtime image that
implements the TQA protocol (v1, revision 10 or newer; this version's runtime is revision 13). The gateway image contains only a static binary and runs as UID/GID 1000; its health check
uses that binary's `healthcheck` command. The integration suite uses Toybox only to test image presence, not to run
an agent.

Docker Engine 27.4.0 or newer and Docker Compose 2.31.0 or newer are required (minimum tested versions); check with
`docker version --format '{{.Server.Version}}'` and `docker compose version --short` (on a Mac, these are Docker
Desktop's).
Published runtimes use `name@sha256:<64 lowercase hex digits>`. For a runtime built on the same Docker host,
`runtime.image` also accepts the bare local ID reported by `docker image inspect --format '{{.Id}}' tqa-runtime:base`.
Local IDs are immutable and are never pulled from a registry. A tag by itself remains invalid.
Use Linux containers or Docker Desktop; Python 3 and a model endpoint are also required (on a Mac, the first
`python3` offers to install Apple's Command Line Tools: accept it). The integration contract
checks the Engine and Compose versions before creating files, pulling images or starting containers.
The gateway has normal networking for the model endpoint and an internal bridge shared with its sandboxes.
The read-only Docker socket still grants daemon control: the gateway is a trusted control-plane component.
Set `TQA_DOCKER_GID` to the socket's numeric group when it differs from the Compose default, 0. Ports bind to
loopback; remote access needs an authenticated TLS reverse proxy and the matching `public_origin`.

## Install

### 0. Build the images

There are no prebuilt images, so build them on the machine that runs the stack, from a release: clone the newest tag
from the [Releases page](https://github.com/TensorQuay/tensorquay-agent/releases/latest) (for example
`git clone --branch v17.4.0 https://github.com/TensorQuay/tensorquay-agent.git`), not `main`. From the repository root:

```sh
deploy/build-images.sh
```

It builds three images from this checkout: the runtime each task runs in (`tqa-runtime:base`, the Minimal environment),
the gateway (`tqa-gateway:local`) and the web app (`tqa-web:local`). At the end it prints the three `export` lines step
1 needs. Add `--workstation` for the Workstation environment (Python with pandas, matplotlib and openpyxl, Node, Go and
a browser; about 3.0 GB, of which the data libraries are about 670 MB, and the longest build), and `--fake-model` for a
scripted test model to try the stack without a model server. Nothing is pushed or published.

The environments register themselves: nobody types an image ID. The runtime images say which environment they are in
their labels, and the script also tags each one by its environment (`tqa-runtime:minimal`, `tqa-runtime:workstation`).
At every start the gateway adds the environment each of those tags names, when the image's label agrees, with the
capabilities its label names: Workstation appears in the picker after the first start that finds it, and becomes the
default environment when nobody has chosen one (no `[[templates]]` in `gateway.toml`, and no environment set up by an
administrator), with no step on the
Environments page. A later build moves the tag, and the gateway's next start moves the environment to it (audited, and
shown on the Environments page); tasks follow at their next session. An image built FROM Workstation carries its labels
but not its tag, so it never replaces Workstation: pick it on the Environments page to make an environment of its own.

**Which image an environment runs.** The tags are the host's, not the installation's: a `tqa-runtime:workstation`
already on this host from another build (an earlier release, another checkout) becomes this installation's
Workstation at its first start, even when this build had no `--workstation`. Before the first task, open
**Settings › Environments** and each environment's page, which shows its image (or, as an administrator,
`GET /api/v1/admin/templates`, each entry's `image`), and compare it with
`docker image inspect --format '{{.Id}}' tqa-runtime:workstation`. To run only this build's images, build with
`--workstation`, or pin each one ([Two or more installations on one host](#two-or-more-installations-on-one-host)).

- **Time.** The first build takes a while: about 20 to 45 minutes on a recent machine, more with `--workstation`.
  Later builds reuse Docker's cache and take a few minutes.
- **Docker Desktop on a Mac with Apple silicon** builds arm64 images natively, which is what the stack runs there,
  `--workstation` included. Tasks run in Docker sandboxes: the microVM backend needs KVM on an x86_64 Linux host, so
  its images (`images/vmm`, the guest kernel) stop with a message on arm64.
- **Resources.** Give Docker at least 8 GB of memory (Docker Desktop: **Settings › Resources**; the Rust builds need
  it). The images and the build cache take about 20 GB, more with `--workstation`. On Docker Desktop that space is
  inside its own disk image, not free space on the Mac, so check the disk limit under **Settings › Resources**.
- **Without a GPU**, a local model runs on the processor: it works, but each step of a task takes minutes, too slow for
  helpers or the desktop (the user guide's [Running without a GPU](../docs/user-guide/tasks.md#running-without-a-gpu)
  section has the figures).
- **Without a model server**, build with `--fake-model` and use the test model as the route. In step 1 set
  `TQA_MODEL_BASE_URL='http://tqa-fake-model:8080/v1'` and `TQA_MODEL='fixture'`, and put any placeholder in the key
  file. After step 3 has started the stack, run it on the stack's control network:
  `docker run -d --name tqa-fake-model --network tqa_control tqa-fake-model:local` (with `COMPOSE_PROJECT_NAME` set,
  the network is `<that name>_control`). It follows a fixed script whatever the task says: it creates
  `hello/README.md` and runs one command. That's enough to see a task run end to end, but it's not an assistant.

More environments come from recipes: `deploy/build-environment.sh recipe.toml` adds packages to a built-in and tags the
image `tqa-env:<name>` for the Environments page (the admin guide's Environments section has a recipe and each message).

### 1. Choose the settings

Keep the provider key in a private, mode 0600 UTF-8 file outside this repository. Set these values in the shell; the
model URL and model name are placeholders for the operator's actual model server, and the image lines take the images
from step 0:

```sh
umask 077
export TQA_INSTALL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/tensorquay"
export TQA_PROVIDER_KEY_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/tensorquay/model-key"
export TQA_MODEL_BASE_URL='https://models.example/v1'
export TQA_MODEL='example-model'
export TQA_MODEL_CONTEXT=32768
export TQA_RUNTIME_IMAGE="$(docker image inspect --format '{{.Id}}' tqa-runtime:base)"
export TQA_GATEWAY_IMAGE='tqa-gateway:local'
export TQA_WEB_IMAGE='tqa-web:local'
export TQA_HTTP_PORT=8080
# A rootless Docker daemon (docker info lists name=rootless) needs its socket and the socket's group as containers
# see it; see Rootless Docker below.
if docker info --format '{{.SecurityOptions}}' | grep -q 'name=rootless'; then
  export TQA_DOCKER_SOCKET="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/docker.sock"
  export TQA_DOCKER_GID="$(docker run --rm --network none \
    --mount "type=bind,source=$TQA_DOCKER_SOCKET,target=/var/run/docker.sock,readonly" \
    tianon/toybox:0.8.14@sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7 \
    stat -c %g /var/run/docker.sock)"
fi
```

- **Images.** The three image lines take step 0's images: the gateway and web app by their `:local` tags, the runtime by
  its bare local ID (a runtime tag by itself is refused). Once there are published releases, use one release's images
  instead, each pinned by digest (`name@sha256:...`).
- **Model endpoint.** `TQA_MODEL_BASE_URL` is called from inside the gateway's container, so `localhost` there is the
  container itself. Use an address the container can reach:
  - a model server on the same Mac (with Docker Desktop): `http://host.docker.internal:<port>/v1`, for example
    `http://host.docker.internal:8000/v1` for a server listening on the Mac's port 8000;
  - on Linux, there is no `host.docker.internal` (unless you add one), and a server listening on `127.0.0.1` can't be
    reached from a container: under rootless Docker a container never reaches the host's loopback. Either make the
    server listen on the host's LAN address (Ollama listens on `127.0.0.1:11434` by default: set
    `OLLAMA_HOST=0.0.0.0:11434`) and use that address, or run it as a container and join it to the stack's control
    network after step 3 (`docker network connect tqa_control <its container>`, or `<COMPOSE_PROJECT_NAME>_control`),
    then use its container name, for example `http://ollama:11434/v1`;
  - a hosted provider: its API base URL.
- **A local model needs room.** Tasks need at least 16,384 tokens of context, and 32,768 works better. Unless told
  otherwise, Ollama gives a model 4,096 when the GPU memory it detects is under 23 GiB (Ollama's log says which it chose
  at start: "vram-based default context"), and it then quietly drops the oldest part of a task's conversation, the
  request first. Before the first task, raise it: on a Mac,
  `launchctl setenv OLLAMA_CONTEXT_LENGTH 32768` and quit and reopen Ollama (or the app's context length setting); on
  Linux, `Environment="OLLAMA_CONTEXT_LENGTH=32768"` in `systemctl edit ollama.service`, then restart it; in a
  container, `-e OLLAMA_CONTEXT_LENGTH=32768`. `ollama ps` shows the model's context under CONTEXT.
  `TQA_MODEL_CONTEXT` is the context the route says the model has (its **Context window**), so tasks compact in
  time: keep it equal to the server's number, or for a hosted provider the model's documented context. The route's **Check** reads
  Ollama's context and fails below 16,384 (see the admin guide's
  [Local models need room](../docs/admin-guide/web-settings.md#local-models-need-room)).
- **The key file.** Create it with the provider's key (after the `umask 077` above, it is private):
  `mkdir -p "$(dirname "$TQA_PROVIDER_KEY_FILE")" && printf '%s\n' 'the-key' > "$TQA_PROVIDER_KEY_FILE"`. It is
  required and must not be empty even for a local server that asks for no key: put any placeholder in it then (for
  example `local`). The route sends it as a bearer token, which such a server ignores.
- **Port.** The gateway publishes `TQA_HTTP_PORT` on the host's loopback only. The browser address, the first
  administrator's page and API clients' `Origin` all use it: `http://localhost:$TQA_HTTP_PORT`.
- **Rootless Docker.** On a rootless daemon the block's last lines set `TQA_DOCKER_SOCKET` and `TQA_DOCKER_GID`, as
  [Rootless Docker](#rootless-docker) explains; the next step writes them into the installation. Without them the
  gateway gets `/var/run/docker.sock`, the host's root daemon, and stops at start ("sandbox configure failed").
- **A second installation on this host** needs its own names too; see
  [Two or more installations on one host](#two-or-more-installations-on-one-host).

### 2. Create the installation

This command refuses to overwrite an existing installation and never prints a secret:

```sh
python3 - <<'PY'
import json, os, secrets, stat
from pathlib import Path
root = Path(os.environ['TQA_INSTALL_DIR']).expanduser().resolve()
key_path = Path(os.environ['TQA_PROVIDER_KEY_FILE']).expanduser().resolve()
repository = Path.cwd().resolve()
assert not root.is_relative_to(repository) and not key_path.is_relative_to(repository)
assert stat.S_IMODE(key_path.stat().st_mode) == 0o600 and key_path.stat().st_size <= 8192
key = key_path.read_text().rstrip('\r\n')
assert key
port = int(os.environ.get('TQA_HTTP_PORT', '8080'))
container = os.environ.get('TQA_GATEWAY_CONTAINER', 'tqa-gateway')
network = os.environ.get('TQA_SANDBOX_NETWORK', 'tqa-sandboxes')
root.mkdir(mode=0o700, parents=True, exist_ok=False)
storage = root / 'storage'
storage.mkdir(mode=0o700)
def write(name, value, mode=0o600):
    path = root / name
    path.write_bytes(value if isinstance(value, bytes) else value.encode())
    path.chmod(mode)
write('install.key', secrets.token_bytes(32))
write('secrets.toml', '[secrets]\n"secrets/local" = ' + json.dumps(key) + '\n')
context = os.environ.get('TQA_MODEL_CONTEXT', '32768')
if not context.isdigit() or not 1024 <= int(context) <= 10_000_000:
    raise SystemExit('TQA_MODEL_CONTEXT must be the model\'s context in tokens, such as 32768')
write('models.toml', '[[routes]]\nname="local"\nprotocol="openai_chat"\nauth={kind="bearer"}\nkey_ref="secrets/local"\nbase_url=' + json.dumps(os.environ['TQA_MODEL_BASE_URL']) + '\nmodel=' + json.dumps(os.environ['TQA_MODEL']) + '\ncontext_window=' + context + '\n', 0o644)
write('gateway.toml', 'listen="0.0.0.0:8080"\npublic_origin=' + json.dumps('http://localhost:' + str(port)) + '\nmodels_path="/etc/tqa/models.toml"\n[storage]\nroot=' + json.dumps(str(storage)) + '\n[runtime]\nbackend="docker"\nimage=' + json.dumps(os.environ['TQA_RUNTIME_IMAGE']) + '\n[sandbox]\nnetwork=' + json.dumps(network) + '\ngateway_url="http://gateway:8080"\ngateway_container=' + json.dumps(container) + '\n[sandbox.env]\nPATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"\nHOME="/tmp"\n[web]\nupstream="http://web:3000"\n', 0o644)
passed = ['TQA_GATEWAY_IMAGE', 'TQA_WEB_IMAGE', 'TQA_HTTP_PORT', 'TQA_GATEWAY_CONTAINER', 'TQA_SANDBOX_NETWORK', 'COMPOSE_PROJECT_NAME', 'TQA_DOCKER_SOCKET', 'TQA_DOCKER_GID']
chosen = [name for name in passed if os.environ.get(name)]
write('compose.env', 'TQA_INSTALL_DIR=' + str(root) + '\nTQA_STORAGE_ROOT=' + str(storage) + '\nTQA_POSTGRES_PASSWORD=' + secrets.token_hex(32) + '\nTQA_BOOTSTRAP_TOKEN=' + secrets.token_hex(32) + '\n' + ''.join(name + '=' + os.environ[name] + '\n' for name in chosen))
print('created ' + str(root) + '; compose.env also sets: ' + (', '.join(chosen) or 'nothing else'))
PY
```

`[sandbox.env]` must set every environment key the runtime image declares (the runtime images declare only `PATH` and
`HOME`, and the values above suit both templates: the Workstation image's `go` is in `/usr/local/bin`, and the shell
tool gives each command its private home under the workspace); the backend refuses to let an image add environment the gateway did not choose. When a key is missing, the
gateway log shows `sandbox specification refused` with the keys to add.

The install key is exactly 32 random bytes. The seed is imported transactionally at startup; PostgreSQL holds
authenticated ciphertext. Both files are mounted read-only and checked for mode 0600. Back up the install key
separately from the database. At each startup a seed entry is applied only when its value in the file changed since it
was last imported (the gateway keeps a keyed digest, never the value or a plain hash). So a key rotated in the admin
Secrets page survives restarts, and editing the value in the seed file still wins at the next start. An entry removed
from the file is kept and shown as no longer seeded. So once the gateway has started and the Secrets page lists your
keys, you can remove the entries from the seed file, leaving only its `[secrets]` line: the keys stay in the store, and
only the store and the install key then hold them. Keep the file itself, since the stack mounts it. The original
provider key, seed, install key and environment file stay outside the repository.

The model relay accepts request bodies up to 16 MiB by default, so a run can send screenshots to a vision model. A
smaller `[relay] body_bytes` in `gateway.toml` (1024-16777216) lowers it; the gateway tells each runtime the limit in
its session configuration, and larger requests are refused with `413`.

A route check waits up to a minute, or 2 minutes for a local model (on this computer or the owner's network: a
loopback address, `localhost`, the container host, a single-label name such as a compose service, a `.local` or
`.internal` name, or a private, link-local, Tailscale or unique-local address; or a route without a key); `[relay] local_check_timeout` (1-600 seconds) sets the local limit.

Two more settings are file-only and apply to custom environment templates; the admin guide describes both
([Credential files in an image](../docs/admin-guide/web-settings.md#credential-files-in-an-image)).
`acknowledged_secret_files` in a `[[templates]]` entry lists credential files that image may carry (absolute paths, at
most 16). `[template_checks]` `max_scan_entries` and `max_scan_bytes` bound the credential-file scan of each template
(160,000 entries and 6 GiB by default); a template past either bound is refused (`image_scan_limit`). A malformed value
in either stops the gateway at start.

A `[[templates]]` entry may also carry `description`, one line (1 to 160 characters) on what the environment is for, shown
where people pick one; an invalid one stops the gateway at start (`description_invalid`). Removing `description` from
the file doesn't clear one: a description set in **Settings › Environments** stays until it is cleared there.

### 3. Start it

Give the container account its files, then fetch the images and start. Pull an image only when it is a registry
reference: images from step 0 (a bare local ID `sha256:...` or a `:local` tag) are already on this host and are never
pulled.

```sh
case "$TQA_RUNTIME_IMAGE" in sha256:*) ;; *) docker pull "$TQA_RUNTIME_IMAGE" ;; esac
docker run --rm --network none --user 0:0 \
  --mount "type=bind,source=$TQA_INSTALL_DIR,target=/install" \
  tianon/toybox:0.8.14@sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7 \
  chown -R 1000:1000 /install/storage /install/install.key /install/secrets.toml
case "$TQA_GATEWAY_IMAGE" in sha256:*|*:local) ;; *) docker compose --env-file "$TQA_INSTALL_DIR/compose.env" -f deploy/compose/docker.yml pull gateway web ;; esac
docker compose --env-file "$TQA_INSTALL_DIR/compose.env" -f deploy/compose/docker.yml up -d --no-build --wait
curl -fsS "http://localhost:${TQA_HTTP_PORT:-8080}/readyz" && echo ready
```

`storage.root` uses the same absolute path inside and outside the gateway so Docker can bind-mount the workspace,
runtime state and private token files into sibling sandboxes. Keep the installation on a local filesystem shared
with Docker Desktop (never NFS). The storage directory is private and writable only by the container account. Its
own filesystem is recommended, not required: the stack runs with the storage root anywhere local, as step 2 creates
it. But on Docker, workspace quotas are measured between runs, so a run that fills its workspace fills the storage
root's filesystem; when that is the root or Docker's data filesystem, the host and every container on it run out of
space too. On a host that matters, give the storage root a filesystem of its own.

On a Mac with Docker Desktop, the installation lives on the Mac's disk and reaches the containers through Docker
Desktop's file sharing (VirtioFS), where ownership doesn't behave as on Linux: the `chown` above may not take effect as
it would there. If the gateway then stops at start with a permission error on `install.key` or `secrets.toml`, or
can't write under the storage directory, that's the cause; send its log from `docker compose logs gateway`. On a
single-disk Mac, a separate filesystem for storage isn't practical; for trying the stack, the default location is fine.

## First administrator, route and task

1. **First administrator.** Open `http://localhost:<port>/setup`, with the port from step 1 (`TQA_HTTP_PORT` in
   `$TQA_INSTALL_DIR/compose.env`, 8080 if it isn't there), and create the first administrator with the bootstrap token: `TQA_BOOTSTRAP_TOKEN` in `$TQA_INSTALL_DIR/compose.env`. Creating it signs you in and opens
   **New task**. Only the gateway publishes a port; the page server is reachable only on its internal network.
2. **First route.** Open the menu under your name at the bottom left and choose **Settings**, then **Models** (under
   Administration). It lists `local`, the route seeded from `models.toml`, with its key from `secrets.toml`. Choose
   **⋯** on the route's row, then **Check**, then **Run check**, to confirm the gateway reaches the model with that key: it
   sends one short request (a few tokens, up to a minute), and a failing check names the reason. To add another route, choose **Add route**; **Paste key** stores its key as a secret.
   The [administrator's guide](../docs/admin-guide/web-settings.md#models) describes every field.
3. **First task.** Choose **New task**, pick the model, write what the agent should do and choose **Send**. The first
   task starts a sandbox from `TQA_RUNTIME_IMAGE`, so its first reply takes a little longer; the
   [user guide](../docs/user-guide/tasks.md) covers the rest.

For API clients, bootstrap uses `POST /api/v1/auth/bootstrap` with `{email,password,token}`, and login uses
`POST /api/v1/auth/login` with `{email,password}`. Both calls require `Origin: http://localhost:$TQA_HTTP_PORT`.
Then `GET /readyz?detail=1` with that cookie returns database, secret-store, backend-preflight and image-presence
checks plus the network mode. Members receive 403; anonymous requests receive 401. `/metrics` is served only on the
gateway's separate internal listener, never the public port.

Sign-in limit: login allows ten attempts per minute per client address, successful or not; further attempts in that
minute, including the right password, get 429. Forwarded-address headers are not trusted, so put no proxy in front of
the gateway that hides client addresses (SEC-009).

## Settings and operations

### Two or more installations on one host

Each installation needs its own directory, Compose project, gateway container name, sandbox network, port and
sandbox network pool; otherwise the second one's `up` replaces or collides with the first. Before step 2, set for
example:

```sh
export TQA_INSTALL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/tensorquay-staging"
export COMPOSE_PROJECT_NAME=tensorquay-staging
export TQA_GATEWAY_CONTAINER=tqa-gateway-staging
export TQA_SANDBOX_NETWORK=tqa-sandboxes-staging
export TQA_HTTP_PORT=8081
```

Step 2 writes them into `compose.env` (which every `docker compose --env-file` command reads) and `gateway.toml`
(`[sandbox] gateway_container` and `network`, and `public_origin`). Step 2 checks the names it copied: it prints the
ones `compose.env` sets besides the defaults, so a stray variable from the shell shows up there.

The second installation's sandbox network pool is required, not optional: both default to `10.213.0.0/16`, and a
gateway refuses to start on a pool that overlaps another installation's networks. After step 2 and before step 3,
append one that nothing else uses, then check it against the host's routes as in [Sandbox networks](#sandbox-networks):

```sh
printf '[sandbox.network_pool]\ncidr = "10.214.0.0/16"\nprefix = 29\n' >> "$TQA_INSTALL_DIR/gateway.toml"
```

Images are the host's, not an installation's. `deploy/build-images.sh`, from whichever checkout, moves
`tqa-gateway:local`, `tqa-web:local`, `tqa-runtime:base` and the environment tags (`tqa-runtime:minimal`,
`tqa-runtime:workstation`) to its own build. An installation whose `compose.env` names a `:local` tag runs the newest
build at its next `up`, and every gateway on the host moves its built-in environments to what the environment tags
name at its next start. To keep an installation on its own images, pin each by ID. In step 1, take the gateway and
web app by ID as the runtime line does:

```sh
export TQA_GATEWAY_IMAGE="$(docker image inspect --format '{{.Id}}' tqa-gateway:local)"
export TQA_WEB_IMAGE="$(docker image inspect --format '{{.Id}}' tqa-web:local)"
```

and after step 2, name the built-in environments' images in `gateway.toml` (a built-in named there is that image, its
label still checked, never what its tag names later; only the built-ins `minimal`, `workstation`, `data` and
`desktop`, by ID only, or the gateway stops at start):

```sh
printf '[runtime.builtin_images]\nminimal = "%s"\nworkstation = "%s"\n' \
  "$(docker image inspect --format '{{.Id}}' tqa-runtime:minimal)" \
  "$(docker image inspect --format '{{.Id}}' tqa-runtime:workstation)" >> "$TQA_INSTALL_DIR/gateway.toml"
```

An ID pins an image only while the image is kept. With Docker's containerd image store (`docker info` shows
`io.containerd.snapshotter.v1`), an image whose last tag moves to a newer build is deleted, its ID with it: the next
build would take away the runtime this installation runs (`/readyz?detail=1` then reports `image_present` false,
while the plain `/readyz` still answers 200). So give each pinned image a tag of the installation's own before
another build, for example:

```sh
for image in gateway:local web:local runtime:minimal runtime:workstation; do
  docker tag "tqa-$image" "tqa-staging-${image%%:*}:${image#*:}"
done
```

After a build, start the new images: `docker compose --env-file "$TQA_INSTALL_DIR/compose.env" -f
deploy/compose/docker.yml up -d --no-build --wait --force-recreate gateway web` (step 3's `up` alone may leave the
gateway on its old image), then check `/readyz?detail=1` as an administrator, as in
[First administrator, route and task](#first-administrator-route-and-task).

### Sandbox networks

Every sandbox gets its own internal network, with no route out, created when the sandbox starts and removed with
it. The gateway container (`[sandbox] gateway_container`, the compose `container_name`) joins each of these networks
under the name `gateway` and is the only peer a sandbox can reach, so sandboxes cannot reach each other. Their subnets
come from a dedicated pool, by default `10.213.0.0/16` split into `/29`s (8192 subnets; each sandbox uses 5 addresses
of 8). The pool must hold at least twice `sandbox.max_live` subnets; the gateway refuses to start otherwise.

Pick a pool that nothing else on the host uses. The gateway refuses to start if the pool overlaps another Docker
network or one of its own routes, and logs the conflict. It cannot see the host's routes from its container, so check
those once when you install, and again if the host's networks change. Give the pool after `python3 -`: the default
`10.213.0.0/16`, or your `[sandbox.network_pool]` cidr:

```sh
python3 - 10.213.0.0/16 <<'PY'
import ipaddress, subprocess, sys
pool = ipaddress.ip_network(sys.argv[1])  # the pool, given after `python3 -`
routes = subprocess.run(['ip', '-4', 'route'], capture_output=True, text=True, check=True).stdout
for line in routes.splitlines():
    destination = line.split()[0]
    if destination != 'default' and ipaddress.ip_network(destination, strict=False).overlaps(pool):
        raise SystemExit('the sandbox network pool overlaps the host route: ' + line)
print('the sandbox network pool overlaps no host route')
PY
```

On a Mac, `ip` doesn't exist, so the check above fails. Docker Desktop's containers and networks live in its own
Linux VM, so the Mac's routes meet the pool only through a VPN that routes private addresses: check with
`netstat -rn -f inet | grep '^10\.213'` instead, with your pool's first part in place of `10\.213` (nothing printed
means no overlap).

To use another pool, set it in `gateway.toml`:

```toml
[sandbox.network_pool]
cidr = "10.213.0.0/16"   # private IPv4 (RFC 1918 or 100.64.0.0/10), at most a /8
prefix = 29              # each sandbox's subnet, 24 through 29
```

If the pool ever runs out (only leaked networks can cause that, since it is sized to twice `max_live`), new sandboxes
wait in the queue, the gateway logs an error once, and `tqa_sandbox_network_pool_exhausted_total` counts it; a sweep
that runs every five minutes, and at once in that case, removes networks whose sandbox is gone.

Upgrading from a version where sandboxes shared one network: at start the gateway retires sandboxes still on the
shared network, and their runs resume in new, isolated sandboxes. Before starting the new version, add
`gateway_container` under `[sandbox]` in `gateway.toml`, set to the gateway's container name (`tqa-gateway` unless
`TQA_GATEWAY_CONTAINER` in `compose.env` changes it); without it the gateway stops at start with an error naming the
key. If your hosts run other stacks, give each one its own `[sandbox.network_pool]` that overlaps no other network
(the gateway refuses a pool that does, and logs the conflict). The development mode with the gateway on the host
(`network_mode = "dev_host_gateway"`) still uses one shared network and gives no isolation between sandboxes.

### Log level

The gateway logs JSON at `info` (its chattier dependencies only warn). To diagnose, set `TQA_LOG` in `compose.env` to
a `RUST_LOG`-style filter and recreate the gateway, for example `TQA_LOG=info,tqa_gateway::relay=debug` for the model
relay. Unset or empty keeps the default. An invalid filter, or one over 1024 bytes, stops the gateway at start rather
than logging at another level. Raise only the part you need, as above: a global `debug` also enables dependency
logging (the HTTP client and server), which is verbose and has not been reviewed for what it prints. Turn it off again
when done.

### Rootless Docker

A rootless daemon is supported with the same hardening: sandboxes keep a read-only root, no capabilities,
`no-new-privileges`, the built-in seccomp profile and their own PID namespace. Point Compose at the daemon's socket
and give the gateway the socket's group as containers see it, which differs from the host's number. Set both before
creating the installation ([step 2](#2-create-the-installation)), which writes them into `compose.env`:

```sh
export TQA_DOCKER_SOCKET="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/docker.sock"
export TQA_DOCKER_GID="$(docker run --rm --network none \
  --mount "type=bind,source=$TQA_DOCKER_SOCKET,target=/var/run/docker.sock,readonly" \
  tianon/toybox:0.8.14@sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7 \
  stat -c %g /var/run/docker.sock)"
```

Always change ownership from a container, as above: the host user is UID 0 inside containers, and the container
account 1000 is a subordinate host ID. Run the gateway only in its container. A gateway started directly on the host
cannot hand the runtime token to that account, so it refuses to create sandboxes.

#### After the Docker daemon was killed

If the daemon is killed (the kernel's OOM killer, or `systemd-oomd` ending the user's Docker service), its containerd
shims die with it but the containers' own processes can survive. The daemon no longer knows them, and their runc state
stays under the daemon's run directory (`$XDG_RUNTIME_DIR/docker/runtime-runc/moby` when rootless). When the daemon
restarts and starts one of those containers again, runc refuses it:

```text
runc create failed: container with given ID already exists
```

The stack then stays down. A database container can be worse: the old postgres may still run on the same data volume
beside the new one, which risks the data.

1. See what was left, read-only: `python3 deploy/tools/docker_orphans.py check`. It lists each runc state whose
   container Docker does not report running as `orphan` (its process still runs) or `stale`, and exits 1 if there is
   any. A container Docker reports running is never listed.
2. If an orphan is a database on a volume a running container also uses, stop the running stack
   (`docker compose stop`) before anything else. Then stop the orphan yourself with a fast shutdown, `kill -INT <pid>`
   (the pid `check` printed), and wait for it to exit: `clean` sends SIGTERM, which postgres treats as a smart shutdown
   that waits for its clients, and kills it after 30 s. Check the database (`pg_amcheck --all` from a container on the
   volume) and restore it from your latest dump if the check reports corruption.
3. Remove the rest: `python3 deploy/tools/docker_orphans.py clean --yes` stops each orphan gracefully (SIGTERM, then
   SIGKILL after 30 s) and deletes the states, checking again just before each one; `--exclude ID` leaves one alone.
   A line `failed ...: still running` means the process did not exit in time: run `check` again after a minute and
   `clean --yes` once more.
4. Start the stack again: `docker compose up -d --force-recreate`.

Keep the daemon out of reach of the OOM killer where the host allows it, and watch for a stack that did not come back:
`docker compose ps --status exited` lists its stopped containers.

### Skills

An install tier of read-only skills is optional. Set `[skills] root` in `gateway.toml` to an absolute host directory,
put the same path in `TQA_SKILLS_ROOT` in `compose.env`, and add the overlay to every Compose command:
`-f deploy/compose/docker.yml -f deploy/compose/skills.yml`. The overlay binds the directory into the gateway at the
same path, read-only, because the engine binds a sandbox's mounts by host path. Each user's own tier needs nothing: it
lives under the storage root.

- **Keep it separate.** The directory must not be, contain, or sit inside `TQA_INSTALL_DIR` or the storage root. The
  gateway refuses overlaps it can see, but it sees the install files at their container paths, so only you can keep
  the install directory out of the tree.
- **Readable by the container account 1000.** Sandboxes read the tree as that account. On a rootless daemon it is a
  subordinate host ID, so a tree readable only by the host user appears empty inside sandboxes. Change ownership
  from a container, as for storage (replace `/srv/tqa-skills` with your path):
  `docker run --rm --network none --user 0:0 --mount type=bind,source=/srv/tqa-skills,target=/skills tianon/toybox:0.8.14@sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7 chown -R 1000:1000 /skills`.

To stop the installation:

```sh
docker compose --env-file "$TQA_INSTALL_DIR/compose.env" -f deploy/compose/docker.yml down
```

Ordinary shutdown retains database and workspace data. Startup reconciles owned sandboxes and resumes durable
queued actions. Removing volumes is appropriate only when intentionally discarding that installation's data.

Every start-up step that fails is logged at ERROR with the step's name and cause before the gateway exits, so an exit
is never silent. With `backend = "firecracker"`, a busy container engine may answer one of the backend's start-up
calls late: its container creates (the start-up probes) are bounded at 60 seconds each, as on Docker, its other calls
by `io_timeout`. The gateway retries the backend's connect, waiting 1, 2, 4, 8 and then 10 seconds between attempts,
within `[sandbox] startup_budget_seconds` (default 80, at least 60): each attempt's calls are cut to what is left of
it, and no attempt starts after it. Any other failure (a preflight refusal, for one) stops start-up at once. The
default ends before the image's health check gives up (a 30-second start period plus 12 checks 5 seconds apart, 90 s)
and before a `docker compose up --wait` of 120 seconds, so a gateway that can't start says why while its container is
still starting; an engine that never answered in time is named in plain words: start the gateway again when the host
is less busy.

Under host I/O pressure, database commits can take several seconds. A runtime link waits for one frame's journal
append for up to `io_timeout` per attempt and three of them in all, so the link survives a slow commit. It closes with
`store_unavailable` only when the database stays unavailable, and the runtime then redials and resends from its
write-ahead log. A heartbeat's workspace lease renewal that fails the same way is tried again on the next heartbeat
while at least half the lease remains. Each case logs a WARN line naming the operation (`journal_append` or
`lease_renewal`).

## Upgrade notes

### The Git relay, `git_push`, Git credentials, `repositories` and `git_url` are removed (ADR-0037 §7)
Back up the database first: migration 0088 removes data and can't be undone.

- **`[git_push]` in `gateway.toml` stops the start**, naming the fix: move its `ca_file` to `[egress] ca_file`, which
  is read the same way, and delete `[git_push]`. A `[[profiles]]` entry with `repositories`, or with `git_push` in its
  `tools`, stops the start too: delete the key.
- **Git credentials:** migration 0088 removes those an earlier start already copied into keys, with their sealed
  values. At the first start after it, the gateway copies any not yet copied (an install upgrading from before the
  copy existed loses none) and removes each original once its key is saved. One whose address isn't a web address
  can't become a key: it is kept, unused, and **Settings › Status** counts it; its owner adds the key under **Keys** by
  hand.
- **API:** the Git credentials' routes (`/api/v1/git/credentials`, `/reads`) are gone (`404`); a profile write with
  `repositories`, or a workspace create with `git_url`, is `400 invalid_request` naming the field (reason `removed`),
  never ignored; a workspace's read no longer has `source_url`. A workspace an earlier release made from a git URL is
  still cloned by its first environment.
- Tasks clone and push with plain `git` and a key for the git host under **Keys**.

### Host keys need this version's runtime image
A task's keys for web addresses (ADR-0037) reach its runtime with the message that starts each run, and the
certificate authority its sandbox trusts for them with the task's configuration. Only this version's runtime can take
them. Protocol revision 10 stays the floor, so environments built before this version keep working, without keys: a
task whose sender has keys says "This environment is too old to use your keys" and runs on without them. To use keys,
set `runtime.image` to this version's runtime image, rebuild each custom template from it, and on microVM installs also
rebuild each template's root filesystem, as in the notes below.

### The ready environments register themselves
At its first start on this version the gateway adds the environments the install built, from the images on the engine
host: Workstation when `deploy/build-images.sh --workstation` built it, and later Data. It needs this version's
images, which say which environment they are and carry the tag the script gives them (`tqa-runtime:workstation`); an
image built before this version says nothing and is left alone. Run `deploy/build-images.sh --workstation` again to get
Workstation in the picker. Minimal keeps coming from `runtime.image`, as before. On an install where nobody chose a
default (no `[[templates]]`, and no administrator has changed environments), Workstation also becomes the default once
its checks pass; otherwise the default stays as it was. On microVM installs that needs Workstation's own rootfs
(`images/rootfs/build_rootfs.py` for its image); until it is built, Minimal stays the default and new tasks start on it.
A later change of `runtime.image` never moves it back to Minimal.

- Nothing you made changes: an environment an administrator made keeps its name, and so does a `[[templates]]` entry in
  `gateway.toml`, even one an administrator deleted; the built-in of that name is then not added.
- A built-in an administrator deletes stays deleted, whatever is built later. To bring it back, add it on the
  Environments page from its image.
- The gateway never pulls, and it never waits for the engine to start: if the engine doesn't answer, the environments
  stay as they were, the log says so once at WARN ("built-in environments not refreshed"), and the gateway tries again
  in the background, after 5 s and then three times as long each time up to 2 minutes, until the engine answers; the
  log says so once more when it does.

### A cut model reply has its own failure; protocol revision 10 is the floor
A model reply that stops at its output limit while the task still has output budget left used to fail as a spent
budget. The cause is usually a model server's own context or max tokens (Ollama and LM Studio set small defaults) or
the route's Max output. Such a reply now fails as `reply_cut`, and the task says so in plain words: "The model's reply
was cut off at its output limit. Raise Max output on the route, or the model server's context or max tokens." A reply
that really spends the task's output budget still fails as before. A cut tool call is never run.

**Rebuild every environment from this release's images.** Protocol revision 10 is now the floor, so a revision 9
runtime is refused before it links (`runtime_too_old`, and its environment is retired): it can't read a task history
that holds the new failure. Set `runtime.image` to this version's runtime image, rebuild each custom template from it,
and on microVM installs also rebuild each template's root filesystem, as in the notes below.

### Each environment has its own size
An environment (template) now sets its own CPUs (in steps of 0.5), memory, processes and the disk of a workspace
created for it, within the installation's `[sandbox.ceiling]` in `gateway.toml` (defaults: 8 CPUs, 16 GiB, 4096
processes, 100 GiB). An environment without its own size gets `[sandbox]` `cpu_millicores`, `memory_bytes` and
`pids`, and `[workspaces] quota_bytes` for a new workspace, as before. At the first start:

- the Limits page's **CPU**, **Memory** and **Processes** overrides, if any, move into every environment without a
  size of its own; each environment changed and each override removed is audited. A CPU value rounds up to a half
  CPU (1.2 becomes 1.5);
- a size above the ceiling (an override above 8 CPUs, 16 GiB or 4096 processes, with the default ceiling) is lowered
  to it, logged as a warning, and the environment is marked "lowered" in **Settings → Environments** until an
  administrator saves it. Tasks then get less than before: to keep such an override, raise `[sandbox.ceiling]` before
  upgrading. The same happens at any later start after the ceiling is lowered, and raising it again gives a
  lowered size back (up to the new ceiling) while the environment is still marked;
- an override moved into an environment from `[[templates]]` stays when that entry next changes, unless the entry
  names that size itself: copy the size into the entry to keep it in the file;
- an environment's memory below 256 MiB rises to 256 MiB;
- existing workspaces keep the installation's default disk; a new one gets its environment's.

Start-up stops when a `[[templates]]` entry, or a `[sandbox]`/`[workspaces]` default, is above the ceiling.

Admission still counts environments, not their size, so size the host for the worst case: host memory at least
`max_live` × the ceiling's memory on Docker, and `max_live` × (the ceiling's memory + `[firecracker]
vmm_overhead_bytes`) on microVM installs. With the default ceiling of 16 GiB that is a lot: on a smaller host, lower
`[sandbox.ceiling] memory_mib` (and `cpus`) to what it can give every environment at once.

### Project repositories are a shared profile's switch (removed since; see above)
Working on a project's repository (starting a workspace from a `git_url`, and the agent's `git_push`) is now a shared
agent profile's **repositories** switch, which only administrators set. At the first start:

- a shared profile that listed `git_push` among its tools gets the switch on, and the change is audited;
- a personal profile that listed it loses it: personal profiles can't have the switch;
- tasks already made keep what they had: their profile copy gets the switch when it listed `git_push` or listed no
  tools at all;
- a `[[profiles]]` entry in `gateway.toml` whose `tools` name `git_push` stops start-up, naming the key: remove it
  from `tools` and set `repositories = true` on the entry instead.

A task made without a profile is unchanged: it can still push to its workspace's repository. The switch is a feature
setting for shared profiles, not a security boundary. A task made from a profile without the switch can't push: the
gateway refuses it, whatever the task's tools. Creating a workspace from a `git_url` now needs `profile`, naming a
profile with the switch.

### Approvals and per-call permissions are removed (ADR-0035)
Runs no longer stop to ask for approval. The guardrails are the sandbox, the credentials the gateway keeps out of it,
and the [Internet access](../docs/admin-guide/web-settings.md#internet-access) allow-list, which the agent is now told
about. This version:

- **stops at start-up** when `gateway.toml` still has `[permissions]`, `[sandbox] park_after_seconds`, `policy` or
  `rules` in a `[[profiles]]` entry, or `class` in a `[[connectors]]` or `[[mcp.servers]]` entry. The message names
  the key; delete it and start again.
- at its first start, removes stored approvals and session grants, the **Park a waiting run after** limit, the
  profiles' policy and rules, the connectors' class, and the workspaces' push setting. A shared profile or connector
  changed this way gets a settings-audit entry.
- ends a run that was waiting for an approval as failed, with "approvals were removed; send a message to continue":
  one parked for it, or one still asking. A message sent to that task starts a new run. Any messages waiting behind
  the run are dropped; they show as dropped and can be sent again.
- lets a run push a branch to a writable workspace's source repository without approval or branch rules. The Git host's
  own rules decide, and its refusal reaches the agent.
- removes `POST /api/v1/conversations/{id}/permissions/{request_id}`, the profile fields `policy` and `rules`, the
  connector field `class`, the workspace field `push`, the limit `sandbox.park_after_seconds`, and the run status
  `waiting`.

**Rebuild every environment from this release's images.** Every runtime built for candidate 7 or earlier must go, not
only the old revisions. Set `runtime.image` to this version's runtime image, rebuild each custom template from it, and on
microVM installs also rebuild each template's root filesystem, as in the note below. Protocol revision 9 is now the
floor, so a revision 8 runtime is refused before it links (`runtime_too_old`, and its environment is retired). A
candidate 7 runtime also says revision 9, so the floor does not stop it: it links, and the first time it sends an
approval or park frame the gateway refuses it, closes its link and stores nothing; it is refused again on each
reconnect, so that run makes no progress. The gateway's log shows a closed runtime link with the code `forbidden`. A
rebuilt environment is the only fix.

### Channels need this version's runtime image (2026-09-28)
Channels need no `gateway.toml` setting. Their agent tools (`channel_post`, `channel_read`, `channel_wait` and
`task_claim`) are in the runtime, and the protocol revision did not change for them, so the gateway still accepts a
runtime image from before channels: a run in a channel member's conversation then starts without the tools, and with no
error. After the upgrade, set `runtime.image` to this version's runtime image. Rebuild each custom template from that
image too, since a template keeps the runtime binary of the base it was built on. On microVM installs, rebuild each
template's root filesystem as in the note below.

### Route keys pasted into `key_ref` (security)

A route's `key_ref` (the route form's **Secret** field) names a secret in the secret store, `secrets/<name>`. Earlier
versions also accepted anything else there, so a provider key pasted into it was saved in plain text in the database.
The gateway now refuses such a value everywhere (the admin API, `models.toml` and start-up; `key_ref_not_reference`)
and, at its first start after the upgrade, in one transaction:

- removes every pasted key from the stored routes, and every cached admin response that held one;
- logs an ERROR naming each route, never the key;
- leaves those routes out of use (other routes drop them from their fallbacks) and shows them in the admin page as
  needing their key again.

**Rotate every key that was pasted into a route:** with the provider, issue a new key, store it as a secret (or
with the route form's **Paste key**), point the route at it, then revoke the old key. The old key sat in plain text in
the database, and so possibly in its backups and dumps; removing it from the live database does not remove those
copies. A `models.toml` that has a key in `key_ref` stops the gateway at start until it names a secret instead.

### microVM installs: rebuild every template's root filesystem (2026-09-27)
On installs that run tasks in microVMs (`backend = "firecracker"`), this version needs a new root filesystem for every
template. Two changes make the old ones unusable:

- the rootfs manifest is schema 2: it records the image's policy inputs (its environment key names and declared
  volumes), and a schema 1 rootfs is refused at create with "the rootfs was built by an older builder; rebuild it
  (images/rootfs)";
- protocol revision 8 raises the runtime floor, so a rootfs carrying an older runtime is refused (`runtime_too_old`).

After installing the new images (gateway, runtime and VM host), follow `deploy/firecracker/README.md`: build the guest
kernel if its pins changed, then, for each template image, remove its old `<storage.root>/images/<image id>/` directory
(the builder never overwrites one) and run `images/rootfs/build_rootfs.py` with the new VM host image. Until a template
has its new rootfs, tasks cannot start in it: the Environments page shows it refused with the rootfs reason, and the
readiness detail reports `image_present: false` for the default template. Existing workspaces and conversations are kept.

## Integration contract

Build once, then run the ignored test explicitly. It uses random private files and credentials, an isolated Compose
project and a synthetic model route, checks administrator readiness, then removes its containers, networks and volume.

```sh
docker build -f images/gateway/Dockerfile -t tqa-gateway:local .
docker build --build-arg SOURCE_DATE_EPOCH=0 -f images/web/Dockerfile -t tqa-web:local .
TQA_DOCKER_TESTS=1 cargo nextest run -p tqa-gateway --test compose --run-ignored all
```

The test fails when selected without `TQA_DOCKER_TESTS=1`, and without `TQA_GATEWAY_IMAGE` and `TQA_WEB_IMAGE`
naming the images under test (a stack never falls back to a bare tag: the engine is shared).
The builder and separately deployed server images are pinned by digest; `scratch` is the
empty final filesystem, not a mutable registry image.

The harness reports failed commands with the last 40 stderr lines and redacts its generated database password and
bootstrap token. It attempts every cleanup and preserves the first failure. Its diagnostics and version preflight
can be tested without Docker:

```sh
python3 -m unittest discover -s deploy/tests -p 'test_*.py' -v
```

The web image is reproducible: the build identity is a digest of the web sources and every copied file carries the
`SOURCE_DATE_EPOCH` timestamp, so two builds of the same source differ only in the files where Next.js writes fresh
per-build secrets (`.next/prerender-manifest.json` and `.next/server/server-reference-manifest.*`), which are never pinned.
`python3 deploy/tests/compare_images.py IMAGE_A IMAGE_B` compares two builds file by file and ignores exactly those.

## Full-stack browser demo

The test-only `demo.yml` overlay adds a scripted model on the control network. The demo uses the real runtime
to write `hello/README.md` and run a shell command. Install the web package's pinned dependencies and Chromium
before running the contract:

```sh
docker build --target base -f images/runtime/Dockerfile -t tqa-runtime:base .
docker build --target workstation -f images/runtime/Dockerfile -t tqa-runtime:workstation .
docker build -f images/fake-model/Dockerfile -t tqa-fake-model:local .
docker build -f images/gateway/Dockerfile -t tqa-gateway:local .
docker build --build-arg SOURCE_DATE_EPOCH=0 -f images/web/Dockerfile -t tqa-web:local .
(cd web && corepack pnpm install --frozen-lockfile && corepack pnpm exec playwright install chromium)
TQA_DOCKER_TESTS=1 TQA_WORKSTATION_IMAGE=<your Workstation build, its ID or a tag of yours> python3 deploy/tests/demo.py
```

The driver resolves both runtime images to immutable local image IDs and configures Workstation as the default
template alongside Minimal. Every stack driver requires each `TQA_*_IMAGE` it runs (gateway, web, runtime, fake model,
Workstation, and the VM host on microVMs) and refuses to start without it, never falling back to a bare tag such as
`tqa-gateway:local` or `tqa-runtime:base`, which on a shared engine may be another member's or a candidate's build; a
candidate's tag file names them all. Every stack driver (this one, the soak, the
robustness and the PERF stacks) pins the gateway's built-in environments to the images under test with
`[runtime.builtin_images]`: Minimal to the runtime, Workstation to `TQA_WORKSTATION_IMAGE`, which is required, because
the engine is shared and its bare `tqa-runtime:workstation` tag may be another build's (a candidate's). Before the
stack is removed, every sandbox it created must have run one of those images (or one the suite built itself, which it
declares in `TQA_BUILT_IMAGES`); a sandbox on any other build fails the run, naming it. A gateway image from before
`[runtime.builtin_images]` (without the label `org.tensorquay.gateway.builtin-images`) would refuse the key, so its
stack is refused, unless the run sets `TQA_UNPINNED_BUILTINS=1` (the Gate 9 run's A/B baseline does): it then says its
built-ins come from the engine's tags, and the sandbox image guard still applies.
`TQA_DEMO_PROTOCOL` selects the wire protocol of the fake model's route: `openai_chat` (the default),
`openai_responses` or `anthropic_messages`, which also gets the output limit that protocol requires. Any other value
fails before a file is written. `TQA_FAKE_MODEL_IMAGE` names the fake model's image, required like the other
`TQA_*_IMAGE` values.
It prints its gateway URL, runs WEB-E2E-004
against that URL, verifies the host workspace, and removes
the temporary installation. Before it stops the stack it reads the task's usage rows and secret-read rows (E2E-AUDIT-001,
`deploy/tests/demo_run_ids.py`) and prints them: every usage row and the route key's read must name the task's run,
which the runtime sends as `X-TQA-Run-ID`. A row without a run is found by its task, since the relay records the task
from the runtime token. `TQA_WEB_EVIDENCE_DIR` optionally selects a directory for light and dark screenshots.
On any failure, before it removes the sandboxes, the driver keeps the stack's logs (every Compose service's and each
of the installation's sandbox containers', the last 5000 lines of each, redacted) in `TQA_WEB_EVIDENCE_DIR/stack-logs`,
or else in a new `tqa-demo-logs-*` folder under the system's temporary directory, and prints where. The real-stack
robustness contracts (`deploy/tests/robustness.py`, behind GW-I-018, GW-E-020, GW-I-066 and E2E-IDLE-019) do the same
on a failure, in `TQA_ROBUSTNESS_EVIDENCE_DIR/stack-logs` or a new `tqa-robustness-logs-*` folder.
The existing browser suite uses its fake front end; this separate contract requires `TQA_STACK_URL` and the
synthetic credentials supplied by the driver.
`TQA_STACK_BROWSER` selects the browser for every suite: `chromium` (the default) or `webkit`, which stands in
for Safari and needs `playwright install webkit` and its host libraries; any other value fails before a file is
written. The log names the browser.
`TQA_STACK_GREP` runs only the suite's browser tests whose titles match it (Playwright's `--grep`); the suite's own
check of what a whole run leaves behind is then skipped, and the log says so. A failure the driver finds after the
browser's output is named in the log with its type and redacted message.

Run `TQA_DOCKER_TESTS=1 python3 deploy/tests/demo.py --suite ux` for WEB-E2E-018. It selects `demo-long.json`,
discovers a non-loopback IPv4 interface and binds only that address. `TQA_UX_HOST` can select another local IPv4
interface. This test-only LAN binding exercises the browser's plain-HTTP behavior using synthetic credentials.
The driver allows twelve owned runtime instances for the six width/theme cases, gives the browser a ten-minute
deadline, checks each conversation's corrected code and README on the host, and removes its installation on failure
as well as success. No production limits or bindings change. The suite compares reviewed screenshots and writes
bounded JSON observations, including failures, when `TQA_WEB_EVIDENCE_DIR` is set. See `web/README.md` for candidate
capture and visual-review instructions.

Run `TQA_DOCKER_TESTS=1 python3 deploy/tests/demo.py --suite environment` for E2E-ENV-010. It adds the
`environment.yml` overlay with a local scripted scenario: create a test file in Minimal, switch to Workstation,
run pytest on the retained file, and use the installed headless browser to screenshot a local page. The browser
checks the recorded switch and successful tool output; the driver verifies the unchanged file and PNG in the same
host workspace before cleanup. No packages or browser binaries are downloaded by the scenario.

Run `TQA_DOCKER_TESTS=1 python3 deploy/tests/demo.py --suite queue` for the queued WEB-E2E-018 path.
Use a gateway with admission support. The owned configuration sets `[admission] slots=1, per_user=1`. The scripted
model uses prompt selection (TK-U-036): a task whose first message says "Hold the slot" gets a held reply (`wait_ms`,
TK-U-035) that keeps its run active and the slot taken until the holder presses Stop; a task that says "Wait for a free
slot" answers at once. The browser checks the queued response, disabled controls, reload, waking, completion and a
second queued turn over earlier idle history at both desktop widths and themes. No idle deadline is shortened; nothing
is checked on disk, since no tool runs.

Run `TQA_DOCKER_TESTS=1 python3 deploy/tests/demo.py --suite egress` for the web side of SEC-001. The owned configuration
enables the policy proxy with one unrelated allowed host; the scripted model fetches a reserved `.invalid` name, which the proxy
refuses without any outside network. The browser checks the live refusal frame, the single quiet line, the denied fetch line and
replay, and records the observed destination for the driver.

Run `TQA_DOCKER_TESTS=1 python3 deploy/tests/demo.py --suite files` for WEB-E2E-021 (task 068c). The browser uploads a
small CSV and attaches it to a message; the scripted model reads it with a shell command and writes `report.md` and
`rows.txt`. The files panel lists and previews them, the Changes tab shows both as added with a diff, and a second message
revises the report, shown as modified with its removed and added lines. An upload over the 25 MiB limit is refused with
`upload_too_large`. The driver then checks the disk: the stored upload, the revised report, and one snapshot per message
under the gateway's storage, outside the workspace and without `uploads/` or `.tqa/`.

Use `--keep` to explore the stack manually instead. The driver prints the private file containing its bootstrap
token and keeps the stack running until Ctrl-C. That shutdown removes the demo's containers, volumes and files.

The soak uses `TQA_DOCKER_TESTS=1 python3 -B deploy/tests/soak.py --hours 2 --seed 731 --output <new-evidence-directory>`
(SOAK-002's short run) or `--hours 24` (SOAK-001…003, PRD M5), with `TQA_GATEWAY_IMAGE` and `TQA_RUNTIME_IMAGE` set to the
accepted builds. The driver resolves immutable image IDs, creates its own installation, runs eight conversations
(`--conversations 4` for the earlier short shape) with two independent SSE clients each, then compares every client's
ordered full receipts with paginated storage. Over a long run each conversation slot starts a new conversation (a
generation) two cycles before its script or message cap runs out, never while a fault in that slot or a stack-wide
fault (an engine stall, a database outage, a full disk) awaits its witness;
the retiring conversation gets the same receipt and run checks as the end of the run. Seeded stream/link cuts occur every 30–120 seconds. The owned gateway restarts every 30 minutes in the 2 h run, and
every 2 hours through hour 12 of the 24 h run (SOAK-003); the final drain has a separate ten-minute bound. Link faults pause an owned runtime for a seeded
121–150 seconds, then unpause it, requiring gateway link closure, runtime re-registration and unchanged container, address
and credential binding. This needs no extra runtime capability and works without Linux SOCK_DESTROY support. An
unprivileged PID-namespace observer samples the gateway's RSS and open descriptors every 30 seconds, next to the database's
size and connection count; it has no host namespace, mounts, daemon socket or network. Growth is judged per gateway
lifetime of at least 4 hours: after a 1-hour warm-up, the median of the next hour against the last hour, and a
least-squares slope projected to 24 hours, both under 5 % for RSS, descriptors and connections. The 24 h run's last
12 hours are free of restarts, so PRD M5 is judged on that one 12-hour lifetime; the 2 h run reports growth as not judged.
Evidence includes every client's receipt file per generation, fault witnesses, restart retry attempts, RSS/DB
high-water marks, and the gateway's and model's logs hour by hour (each hour cut at 8 MiB with a marker). Any link-cut run failure fails the soak; only exhausted provider retries within an announced gateway
restart window are allowed. A model attempt the runtime gave back because its own link to the gateway was down (a `gateway_link` retry, CORE-U-062) is not counted among the spent attempts, and the result lists those waits as `gateway_link_retries`. A `store_unavailable` notice outside every database fault (the gateway's store timed out on a busy host) is accepted as load only if that conversation completes a run within 180 seconds of it; notices within 10 seconds of each other are one stall, more than 5 stalls in any hour fail, and the result counts them as `store_unavailable_under_load`. Tool timeouts are recorded only for tools already running when their runtime was paused, through
ten seconds after unpause; other tool errors fail, and each conversation must still perform successful shell and file-read work.
Besides the reconnect plan, a seeded disruption plan (`deploy/tests/soak_disruptions.py`) kills the gateway (every 2 hours, only in hours 0–12 of the 24 h run), restarts, stops (60–180 s) and crashes the database (every 3, 6 and 12 hours), kills a random sandbox every 15 minutes (SOAK-004) and cuts a runtime's network for 5–60 seconds every 20–40 minutes, and stalls the engine every 45–90 minutes: for 60–120 seconds, the calls that change containers (create, start, stop, kill, wait, delete) are held for 20–90 seconds, answered with a 500, or dropped. Each stall retires one idle conversation's sandbox as it starts, so its replacement goes through the stalled engine; the stall counts as witnessed only when at least one call met it and that replacement runs. The gateway reaches the engine through a small proxy (`deploy/tests/engine_proxy.py`) for this; its socket sits in a directory only the running user can enter, since it reaches that user's whole engine, and nothing on the host is loaded. Each engine connection's proxy threads end with it. The harness's own engine commands (kill, stop, start, pause, inspect, network) wait up to 180 seconds and are tried once more after a timeout; each one slower than 15 seconds is recorded as a measurement (`engine_slow` in the journal and `slow_engine_commands` in the result: the command, its seconds, the host's I/O pressure then), never as the soak's failure, and a retry that finds the first try took effect counts as done. Only both tries timing out fails the run. The driver also checks its own thread count on every pass and stops with a plain error past 1000, since threads count against the user's process limit that the user's other work shares; each resource sample records the count (`driver_threads`). The storage root sits on a bounded tmpfs (`--storage-mib`, 64 MiB by default) mounted at its path inside the rootless engine's own mount namespace, where the gateway's and the sandboxes' bind mounts resolve; every 4 hours a throwaway, unprivileged helper container fills it for 60–120 seconds and then frees it (`deploy/tests/soak_disk.py`). Inside that window plus a 180-second margin, a tool may fail with `No space left on device`, and a conversation may meet the full disk as the `io` notice (its runtime's storage failed), the `sandbox_failed` notice (a replacement could not be created), or a `run.resumed` in a replaced sandbox, whose tool in flight when the disk filled then comes back `interrupted`, as after a kill. The gateway's own bound applies: it replaces a sandbox that keeps stopping at most 4 times in 10 minutes, waiting 0, 5, 15 and 45 seconds, and a 5th loss is a retryable `sandbox_failed` followed by the `failed` state. So up to 4 resumes per conversation per window pass, each at least its delay after the one before. Every client checks the notices: each loss is announced as `destroyed` with its delay, its replacement comes as `creating` no sooner than that, there are at most 4 in 10 minutes, and `failed` comes only after `sandbox_failed`. A 5th resume, or one sooner than its delay, fails (the loop guard). What decides is recovery: every such conversation must complete a run within 180 seconds of the free. While the disk is full the other conversations send nothing, and one idle conversation sends a message on purpose: it passes on a clean typed refusal (an HTTP error naming a code) or on acceptance followed by the same recovery. The probe waits at most half the fill for a conversation with no run in flight; if none is idle by then it is skipped and recorded, never sent into a busy run (which is answered `busy`). Each replacement's destroy, create and resume times are recorded. Nothing else is excused. The tmpfs is unmounted before the installation is removed. This needs rootless Docker; `--reconnect-only` uses neither the proxy nor the tmpfs. `--exclude KIND` (repeatable) leaves one disruption kind out on purpose, for example `disk_full` while a product fix for it is pending; the result records `excluded_kinds`, and without `disk_full` the storage root stays on the host disk. Each kind happens at least once in the 2 h run. `--backend firecracker --hours 8` is the microVM run (R3, on a KVM host with `TQA_VMM_IMAGE` and `TQA_FC_KERNEL_DIR` as for `deploy/firecracker`). It installs through the microVM stack's own steps. It restarts the gateway every 2 hours through hour 4, so hours 4–8 are one gateway lifetime judged for growth. It plans every kind except the link cut (the guest's network runs through a tap inside the VM host) and the full disk (each workspace is its own block image; SBX-C-015 covers that path). A paused VM's link counts as back once the gateway closed it and a client received a new snapshot. A cut runtime network (Docker) counts as back once the runtime logs its link's replay, a client receives a new snapshot, or new events made after the restore reach the conversation's clients: a cut shorter than the gateway's 90-second link liveness bound leaves the link up, so a run in flight streaming on is the witness. `--model real --hours 4 --conversations 4` is the real-model run (R4, SOAK-001's run with a real local model). The operator starts an OpenAI-compatible model server on a GPU reserved for the run, and names it with `TQA_SOAK_MODEL_URL` (reachable from the gateway container) and `TQA_SOAK_MODEL_NAME` (`TQA_SOAK_MODEL_CONTEXT`, default 32768). The `local` route then points there. Each message is a small shell and `read_file` task. The run keeps to reconnect faults and a gateway restart every hour. Progress counts completed model responses, and the tool-name check allows any registered tool. After the drain, the orphan audit (`deploy/tests/soak_orphans.py`, PRD M5) requires, within two minutes, that every sandbox container of the installation is running and belongs to one of the soak's conversations (one each), that containers and `sandbox_instances` rows match one to one and every row is live, that every per-sandbox network has a container, and that no labelled volume is unused. Each fault has a recovery window that no other fault enters; while the database is down the driver sends nothing and skips its database sample. A client dropped while the database is down is witnessed once it is back, since its replay needs the database: its 60-second witness time starts when the database's window ends. Until then its reconnects get typed errors, accepted only inside the database window: 502 or 503, or 504 `timeout` when a database call hangs to its bound. The client waits the answer's `Retry-After` (1 to 5 seconds) before trying again. A stream already open must stay open through it. An unsequenced control frame the client doesn't know (such as `message.queue`, a message held while a run finishes) is counted per conversation as `control_frames` and never fails the run; an unknown sequenced frame still does. After a sandbox kill, the in-flight tool may return `interrupted` and the run may resume once, within 180 seconds; nothing else is excused. A gate run's plan must cover every client and runtime, checked before the stack starts, and every planned fault must run. `--reconnect-only` keeps the Gate 2 shape without disruptions. `--rehearsal` runs a five-minute plumbing check and never counts as gate evidence. The hidden `--rotate-after N` is test-only and needs `--rehearsal`: each conversation is replaced after N responses, so a rehearsal proves the generation path. The driver removes
its containers and temporary installation on exit; keep its bounded evidence directory outside the repository.

