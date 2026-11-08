"""An opt-in Docker contract; all files, containers and volumes belong to this invocation."""
import http.cookiejar
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

import readings

TOYBOX = "tianon/toybox:0.8.14@sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7"
COMPOSE = ["docker", "compose", "-f", "deploy/compose/docker.yml", "-f", "deploy/compose/test.yml"]
# These are the versions verified by the Compose readiness contract.
MINIMUM_ENGINE = "27.4.0"
MINIMUM_COMPOSE = "2.31.0"
REQUIREMENTS = ("Docker Engine {} or newer and Docker Compose {} or newer are required "
                "(minimum tested versions).").format(MINIMUM_ENGINE, MINIMUM_COMPOSE)
STDERR_TAIL_LINES = 40


def redact(text, env):
    """Remove the generated database password and bootstrap token from diagnostics."""
    for name in ("TQA_POSTGRES_PASSWORD", "TQA_BOOTSTRAP_TOKEN", "TQA_STACK_PASSWORD"):
        if env.get(name):
            text = text.replace(env[name], "[REDACTED]")
    return text


def command_failure(args, env, error):
    """Name the failing command and print at most forty redacted stderr lines."""
    if isinstance(error, subprocess.CalledProcessError):
        reason = "exit {}".format(error.returncode)
    elif isinstance(error, subprocess.TimeoutExpired):
        reason = "timeout after {} s".format(error.timeout)
    else:
        reason = "could not start"
    display = " ".join(shlex.quote(str(argument)) for argument in args)
    print("Command failed ({}): {}".format(reason, redact(display, env)), file=sys.stderr)
    detail = str(error) if isinstance(error, OSError) else error.stderr
    if isinstance(detail, bytes):
        detail = detail.decode("utf-8", errors="replace")
    if detail:
        print("\n".join(redact(detail, env).splitlines()[-STDERR_TAIL_LINES:]), file=sys.stderr)


def command(args, env, *, capture=False, cwd=None, timeout=90):
    try:
        return subprocess.run(args, env=env, cwd=cwd, check=True, timeout=timeout,
                              stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
                              stderr=subprocess.PIPE, text=True)
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        command_failure(args, env, error)
        raise


def version_number(value):
    """Parse stable CLI versions, including the Docker Desktop packaging suffix."""
    match = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)(?:-desktop\.\d+)?", value.strip())
    return tuple(int(part) for part in match.groups()) if match else ()


def preflight(env):
    """Refuse unavailable, unreadable or older versions before creating an owned stack."""
    try:
        engine = command(["docker", "version", "--format", "{{.Server.Version}}"], env, capture=True).stdout.strip()
        plugin = command(["docker", "compose", "version", "--short"], env, capture=True).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        raise SystemExit(REQUIREMENTS + " Could not query Docker Engine and Compose versions.") from None
    if (version_number(engine) < version_number(MINIMUM_ENGINE)
            or version_number(plugin) < version_number(MINIMUM_COMPOSE)):
        raise SystemExit(REQUIREMENTS + " Found Engine {!r}, Compose {!r}.".format(engine, plugin))


def write(path, content):
    path.write_bytes(content if isinstance(content, bytes) else content.encode())
    path.chmod(0o600)


def ensure_image(reference, env):
    """Pull a digest-pinned image only when it is not already present; the digest keeps a local copy exact.

    A pull on a saturated network can time out and fail a stack before it starts, although the image is present.
    A daemon that does not answer the inspect in time still fails here, since pulling would not help."""
    try:
        subprocess.run(["docker", "image", "inspect", "--format", "{{.Id}}", reference], env=env, check=True,
                       timeout=30, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return
    except subprocess.CalledProcessError:
        pass
    command(["docker", "pull", reference], env)


# The runtime's protocol revision label (tqa_types::RUNTIME_REVISION_LABEL) and the one constant it must meet.
REVISION_LABEL = "org.tensorquay.protocol-revision"
REVISION_SOURCE = Path(__file__).resolve().parents[2] / "crates/tqa-types/src/limits.rs"
# A local stand-in runtime: the helper image with the revision label, so readiness can check it (GW-I-017).
STAND_IN = "tqa-compose-runtime"


def protocol_revision(source=REVISION_SOURCE):
    """The gateway's protocol revision, read from its one definition in tqa-types."""
    found = re.search(r"pub const PROTOCOL_REVISION: u16 = (\d+);", Path(source).read_text())
    if found is None:
        raise ValueError("PROTOCOL_REVISION not found in " + str(source))
    return int(found.group(1))


def stand_in(env, revision=None):
    """The helper image labelled with the protocol revision, derived locally (no pull, no builder) when missing; returns
    its immutable image ID, since the gateway accepts a runtime image only by digest."""
    revision = protocol_revision() if revision is None else revision
    reference = "{}:rev{}".format(STAND_IN, revision)
    inspect = ["docker", "image", "inspect", "--format", "{{json .Config.Labels}}", reference]
    try:
        labels = subprocess.run(inspect, env=env, check=True, timeout=30, capture_output=True, text=True).stdout
        current = (json.loads(labels) or {}).get(REVISION_LABEL) == str(revision)
    except subprocess.CalledProcessError:
        current = False
    if not current:
        container = command(["docker", "create", "--network", "none", TOYBOX, "true"], env, capture=True).stdout.strip()
        try:
            command(["docker", "commit", "--change", "LABEL {}={}".format(REVISION_LABEL, revision), container,
                     reference], env)
        finally:
            command(["docker", "rm", "--force", container], env)
    identity = command(["docker", "image", "inspect", "--format", "{{.Id}}", reference], env, capture=True).stdout.strip()
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", identity):
        raise ValueError("The stand-in runtime must resolve to an immutable local image ID")
    return identity


def installation(temporary):
    """Create the installation directory inside an owned temporary root.

    prepare() gives the installation to the gateway user. On a rootless daemon that is a subordinate id the host
    user cannot read, so files that only the host Compose CLI reads (overlays) belong in the root beside it."""
    directory = Path(temporary.name).resolve() / "installation"
    directory.mkdir(mode=0o700)
    return directory


PROTOCOLS = ("openai_chat", "openai_responses", "anthropic_messages")


def models_toml(protocol):
    """The fake model's route on the chosen wire protocol; the Messages protocol requires an output limit."""
    if protocol not in PROTOCOLS:
        raise ValueError("TQA_DEMO_PROTOCOL must be one of " + ", ".join(PROTOCOLS))
    limit = "max_output=4096\n" if protocol == "anthropic_messages" else ""
    return ("[[routes]]\nname='local'\nprotocol='" + protocol + "'\nbase_url='http://fake-model:8080/v1'\n"
            "model='fixture'\nauth={kind='bearer'}\nkey_ref='secrets/local'\n" + limit)


def used_subnets(inspected):
    """Every subnet in `docker network inspect` output (IPv4 and IPv6)."""
    subnets = []
    for network in json.loads(inspected or "[]"):
        for config in (network.get("IPAM") or {}).get("Config") or []:
            if config.get("Subnet"):
                subnets.append(config["Subnet"])
    return subnets


def sandbox_pool(used, randbelow=secrets.randbelow):
    """A 10.213.X.0/24 that no existing network overlaps, drawn at random among the free ones. Another installation
    on the daemon, such as a long-lived review stack, already holds one; a clash makes the engine refuse this
    installation's sandbox networks."""
    taken = [ipaddress.ip_network(subnet, strict=False) for subnet in used]
    free = [candidate for candidate in (ipaddress.ip_network("10.213.{}.0/24".format(third)) for third in range(256))
            if not any(candidate.version == network.version and candidate.overlaps(network) for network in taken)]
    if not free:
        raise RuntimeError("no free 10.213.X.0/24 sandbox pool on this engine")
    return str(free[randbelow(len(free))])


# A network another job removed between the list and the inspect: Docker's one error line for it.
VANISHED = re.compile(r"Error response from daemon: network \S+ not found")


def engine_subnets(env):
    """The subnets of every network on the engine. A parallel stack job may remove networks between the list and the
    inspect: Docker then still prints the networks that exist and exits 1, and a vanished network's subnet is free
    again, so a failure whose every line says a network was not found is not one. Any other failure stops."""
    names = command(["docker", "network", "ls", "-q"], env, capture=True).stdout.split()
    if not names:
        return []
    arguments = ["docker", "network", "inspect", *names]
    result = subprocess.run(arguments, env=env, capture_output=True, text=True, timeout=90, check=False)
    lines = [line for line in result.stderr.splitlines() if line.strip()]
    if result.returncode != 0 and not (lines and all(VANISHED.fullmatch(line.strip()) for line in lines)):
        error = subprocess.CalledProcessError(result.returncode, arguments, result.stdout, result.stderr)
        command_failure(arguments, env, error)
        raise error
    return used_subnets(result.stdout)


IMAGE_ID = re.compile(r"sha256:[0-9a-f]{64}")


def image_id(reference, env):
    """`reference` as the engine's immutable image ID (an ID is returned as it is)."""
    if IMAGE_ID.fullmatch(reference):
        return reference
    inspect = ["docker", "image", "inspect", "--format", "{{.Id}}", reference]
    identity = command(inspect, env, capture=True).stdout.strip()
    if not IMAGE_ID.fullmatch(identity):
        raise ValueError("{} doesn't resolve to a local image ID".format(reference))
    return identity


def builtin_pins(env, runtime, *, stand):
    """The built-in environments' images under test, as IDs, for `[runtime.builtin_images]` (GW-U-120): Minimal is the
    runtime under test, Workstation the image `TQA_WORKSTATION_IMAGE` names. The engine is shared and its bare
    tqa-runtime:<name> tags may be another build's, so a run that doesn't name its Workstation is refused, never given
    the tag's. The stand-in stack (the compose contract) has no Workstation: pinning it to the stand-in, which isn't
    labelled Workstation, registers none. Data is not pinned; the sandbox image guard in `retire` catches its use."""
    minimal = image_id(runtime, env)
    if stand:
        return {"minimal": minimal, "workstation": minimal}
    named = env.get("TQA_WORKSTATION_IMAGE")
    if not named:
        raise ValueError("TQA_WORKSTATION_IMAGE must name the Workstation image under test: the engine's bare "
                         "tqa-runtime:workstation tag is shared and may be another build's")
    pins = {"minimal": minimal, "workstation": image_id(named, env)}
    # The Desktop built-in, only when a run names its image (SEC-057/058, SBX-C-020): other runs pin no desktop.
    desktop = env.get("TQA_DESKTOP_TEST_IMAGE")
    if desktop:
        pins["desktop"] = image_id(desktop, env)
    return pins


def pins_toml(pins):
    """The gateway.toml table that pins each built-in to its image."""
    return "\n[runtime.builtin_images]\n" + "".join(
        "{} = {}\n".format(name, json.dumps(image)) for name, image in sorted(pins.items()))


PINS_LABEL = "org.tensorquay.gateway.builtin-images"


def gateway_takes_pins(env):
    """Whether the gateway image accepts `[runtime.builtin_images]`: its image says so from that change on. An older
    gateway refuses unknown keys, so it must never be given the key."""
    labels = command(["docker", "image", "inspect", "--format", "{{json .Config.Labels}}", env["TQA_GATEWAY_IMAGE"]],
                     env, capture=True).stdout
    return (json.loads(labels) or {}).get(PINS_LABEL) == "1"


def pin_section(env, pins, *, takes_pins):
    """The gateway.toml text that pins the built-ins: the key for a gateway that takes it. An older gateway's stack is
    refused, unless the run explicitly takes the engine's tags (`TQA_UNPINNED_BUILTINS=1`, an A/B baseline against an
    older release); that run says so, and the sandbox image guard in `retire` still fails a sandbox on another build."""
    if takes_pins:
        return pins_toml(pins)
    image = env.get("TQA_GATEWAY_IMAGE", "")
    if env.get("TQA_UNPINNED_BUILTINS") != "1":
        raise ValueError("the gateway image {} predates [runtime.builtin_images], so its built-ins would come from the "
                         "engine's shared tags: use a gateway built with it, or set TQA_UNPINNED_BUILTINS=1 for a run "
                         "that must use this one (an A/B baseline)".format(image))
    print("Built-ins not pinned: the gateway image {} predates [runtime.builtin_images] (TQA_UNPINNED_BUILTINS=1); "
          "its built-ins come from the engine's tags, and the sandbox image guard still fails any sandbox on an image "
          "not under test".format(image), flush=True)
    return ""


def required_image(env, name):
    """The image `name` (a TQA_*_IMAGE variable) names: required, because the engine is shared and a bare tag such as
    tqa-gateway:local or tqa-runtime:base may be another member's or a candidate's build."""
    image = env.get(name)
    if not image:
        raise ValueError("{} must name the image under test: the engine is shared, so a bare tag may be another "
                         "build's".format(name))
    return image


def prepare(directory, env, *, runtime=None, port=0, host="127.0.0.1", max_live=4):
    """Write the installation; without a runtime, the labelled stand-in (the compose contract's own runtime)."""
    for name in ("TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE"):
        required_image(env, name)
    route = models_toml(env.get("TQA_DEMO_PROTOCOL", "openai_chat"))
    stand = runtime is None or runtime == TOYBOX
    if runtime is None:
        ensure_image(TOYBOX, env)
        runtime = stand_in(env)
    storage = directory / "storage"
    storage.mkdir(mode=0o700)
    env.update(TQA_INSTALL_DIR=str(directory), TQA_STORAGE_ROOT=str(storage),
               TQA_POSTGRES_PASSWORD=secrets.token_hex(32),
               TQA_BOOTSTRAP_TOKEN=secrets.token_hex(32), TQA_HTTP_PORT=str(port))
    env["COMPOSE_PROJECT_NAME"] = "tqa-compose-" + secrets.token_hex(5)
    env["TQA_SANDBOX_NETWORK"] = env["COMPOSE_PROJECT_NAME"] + "-sandboxes"
    # Parallel stacks on one daemon need their own gateway container name and their own slice of the sandbox
    # network pool (a /24 of /29s: 32 subnets, enough for max_live 16).
    env["TQA_GATEWAY_CONTAINER"] = env["COMPOSE_PROJECT_NAME"] + "-gateway"
    pool = sandbox_pool(engine_subnets(env))
    write(directory / "install.key", secrets.token_bytes(32))
    write(directory / "secrets.toml", '[secrets]\n"secrets/local" = "synthetic-compose-fixture"\n')
    write(directory / "models.toml", route)
    origin = "http://{}:{}".format(host, port) if port else "http://localhost:8080"
    # The start's bounds are written out (the gateway's defaults), not left implicit: the soak reads its readiness wait
    # from them (soak.ready_wait), so the harness and the gateway it drives use the same numbers.
    write(directory / "gateway.toml", "listen='0.0.0.0:8080'\nmetrics_listen=false\npublic_origin='" + origin + "'\nmodels_path='/etc/tqa/models.toml'\n[storage]\nroot=" + json.dumps(str(storage)) + "\n[runtime]\nbackend='docker'\nimage=" + json.dumps(runtime) + "\n[sandbox]\ncreate_budget_seconds=120\nready_seconds=30\ngateway_url=\"http://gateway:8080\"\nnetwork=" + json.dumps(env["TQA_SANDBOX_NETWORK"]) + "\ngateway_container=" + json.dumps(env["TQA_GATEWAY_CONTAINER"]) + "\n[sandbox.network_pool]\ncidr=" + json.dumps(pool) + "\nprefix=29\n[web]\nupstream=\"http://web:3000\"\n"
          # The short-title call would take a reply the fake model scripted for a task: off in test stacks.
          "[tasks]\nauto_title=false\n")
    if not stand:
        config = directory / "gateway.toml"
        write(config, config.read_text().replace("[sandbox]\n", "[sandbox]\nmax_live=" + str(max_live) + "\n")
              + "[sandbox.env]\nPATH='/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'\nHOME='/tmp'\n")
    # The built-ins run the images under test, never the shared engine's tags; `retire` checks every sandbox did.
    pins = builtin_pins(env, runtime, stand=stand)
    config = directory / "gateway.toml"
    write(config, config.read_text() + pin_section(env, pins, takes_pins=gateway_takes_pins(env)))
    env["TQA_PINNED_IMAGES"] = ",".join(sorted(set(pins.values())))
    env["TQA_STACK_SINCE"] = str(int(time.time()) - 1)
    ensure_image(TOYBOX, env)
    ownership(directory, env, "1000:1000")
    # The database and load readings beside the stack, from now until `retire` (readings.py).
    readings.start(env)


def host_owner(env):
    """The owner, as seen inside a container, that gives files back to the invoking host user."""
    options = command(["docker", "info", "--format", "{{json .SecurityOptions}}"], env, capture=True).stdout
    # A rootless daemon maps the invoking host user to uid 0 inside containers; handing files back as the host
    # uid:gid would give them to a subordinate id that the host user can neither read nor remove.
    if "name=rootless" in (json.loads(options) or []):
        return "0:0"
    return "{}:{}".format(os.getuid(), os.getgid())


def ownership(directory, env, owner=None):
    """Chown the directory tree from a container; without an owner, hand it back to the invoking host user."""
    if owner is None:
        owner = host_owner(env)
    command(["docker", "run", "--rm", "--network", "none", "--mount",
             f"type=bind,source={directory},target=/install", TOYBOX,
             "chown", "-R", owner, "/install"], env)


def request(client, origin, path, body=None):
    encoded = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(origin + path, data=encoded, headers={
        "Content-Type": "application/json", "Origin": "http://localhost:8080"})
    with client.open(req, timeout=10) as response:
        data = response.read(65537)
        assert len(data) <= 65536, "HTTP fixture body exceeded its bound"
        return response.status, data


def verify(env):
    config = json.loads(command(COMPOSE + ["config", "--format", "json"], env, capture=True).stdout)
    assert not config["services"]["web"].get("ports"), "Only the gateway may publish a port"
    assert config["networks"]["web"]["internal"] is True, "The page server needs an internal network"
    assert "web" in config["services"]["gateway"]["networks"]
    command(COMPOSE + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "60"], env)
    port = command(COMPOSE + ["port", "gateway", "8080"], env, capture=True).stdout.strip().rsplit(":", 1)[1]
    origin = "http://127.0.0.1:" + port
    client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    assert request(client, origin, "/readyz")[0] == 200
    status, page = request(client, origin, "/login")
    assert status == 200 and b"Sign in" in page, "The gateway must serve the actual web application"
    password = "synthetic-passphrase-" + secrets.token_hex(16)
    request(client, origin, "/api/v1/auth/bootstrap", {
        "email": "compose@example.invalid", "password": password,
        "token": env["TQA_BOOTSTRAP_TOKEN"]})
    request(client, origin, "/api/v1/auth/login", {"email": "compose@example.invalid", "password": password})
    _, body = request(client, origin, "/readyz?detail=1")
    detail = json.loads(body)
    assert detail["ready"] is True and all(detail["checks"].values()), "readiness checks failed"
    print(json.dumps(detail, sort_keys=True))


# A failed run's stack logs, kept per container: the latest lines only, so a long run can't fill the disk.
LOG_LINES = 5000
SANDBOX_IDENTIFIER = re.compile(r"[0-9a-f]{12,64}")


def save_logs(env, folder, *, configuration=COMPOSE):
    """On a failure, before anything is torn down: every Compose service's log (the gateway's, the web's, the model's,
    the database's) and each of this installation's sandbox containers', redacted and bounded, in `folder`. A log that
    can't be read is reported and the rest are still kept; this never raises, so it never hides the run's own failure.
    Returns the files written."""
    folder = Path(folder)
    written = []

    def both(result):
        """A container's stdout and stderr: `docker logs` answers with each on its own stream."""
        streams = [result.stdout, getattr(result, "stderr", None)]
        return "".join(stream for stream in streams if isinstance(stream, str))

    def keep(name, read):
        try:
            text = read()
            folder.mkdir(parents=True, exist_ok=True)
            path = folder / name
            write(path, redact(text, env))
            written.append(path)
        except Exception as error:
            print("Could not keep the {} log: {}".format(name, redact(str(error), env)), file=sys.stderr)

    keep("compose.log", lambda: command(configuration + ["logs", "--no-color", "--timestamps", "--tail", str(LOG_LINES)],
                                        env, capture=True).stdout)
    if "TQA_SANDBOX_NETWORK" in env:
        try:
            identities = command(["docker", "ps", "-aq", "--filter", "label=tqa.instance=" + env["TQA_SANDBOX_NETWORK"]],
                                 env, capture=True).stdout.split()
        except Exception as error:
            identities = []
            print("Could not list the sandboxes: " + redact(str(error), env), file=sys.stderr)
        for identity in identities:
            if not SANDBOX_IDENTIFIER.fullmatch(identity):
                print("Unexpected sandbox identifier; its log is not read", file=sys.stderr)
                continue
            keep("sandbox-" + identity + ".log", lambda identity=identity: both(
                command(["docker", "logs", "--timestamps", "--tail", str(LOG_LINES), identity], env, capture=True)))
    written += readings.keep(env, folder)
    if written:
        print("Kept the stack's logs: " + str(folder), file=sys.stderr)
    return written


def retire(env, failure, *, configuration=COMPOSE):
    """Stop admission and remove only the runtime containers, then the per-sandbox networks, belonging to this owned
    installation. Compose owns neither, and a gateway that is gone never sweeps its own networks, so each one left behind
    can later overlap a new installation's sandbox pool."""
    if "COMPOSE_PROJECT_NAME" not in env:
        return failure
    # The readings are told to stop now, without waiting, so teardown isn't read as an anomaly; they are waited for and
    # kept at the end (after any failure too, whatever the harness), so the sandbox cleanup never waits on them.
    readings.stopping(env)
    try:
        failure = _retire(env, failure, configuration)
        return failure
    finally:
        readings.stop(env, failure if failure is not None else sys.exc_info()[1])


def _retire(env, failure, configuration):
    label = "label=tqa.instance=" + env["TQA_SANDBOX_NETWORK"]
    def remove(listing, removal, kind):
        identities = command(listing + ["--filter", label], env, capture=True).stdout.split()
        if not all(re.fullmatch(r"[0-9a-f]{12,64}", value) for value in identities):
            raise ValueError("Unexpected sandbox " + kind + " identifier")
        if identities:
            command(removal + identities, env)
    actions = (lambda: command(configuration + ["stop", "--timeout", "10", "gateway"], env),
               lambda: remove(["docker", "ps", "-aq"], ["docker", "rm", "--force"], "container"),
               # Only the gateway's per-sandbox networks carry tqa.subnet; Compose's own sandbox network shares the
               # instance label, and `compose down` removes it once nothing is attached.
               lambda: remove(["docker", "network", "ls", "-q", "--filter", "label=tqa.subnet"], ["docker", "network", "rm"],
                              "network"))
    for action in actions:
        try:
            action()
        except Exception as error:
            print("Sandbox cleanup failed: " + redact(str(error), env), file=sys.stderr)
            if failure is None:
                failure = error
    return guard_sandbox_images(env, failure)


def foreign_sandboxes(events, allowed, resolve):
    """(container, image ID) for each sandbox in `docker events` lines (`<container> <image>`) whose image, by ID, is
    not in `allowed`; an image recorded by name is resolved to its ID first."""
    foreign = []
    for line in events.splitlines():
        parts = line.split(None, 1)
        if len(parts) != 2:
            continue
        container, image = parts[0], parts[1].strip()
        identity = image if IMAGE_ID.fullmatch(image) else resolve(image)
        if identity not in allowed:
            foreign.append((container, identity))
    return foreign


def guard_sandbox_images(env, failure):
    """Every sandbox this installation ever created (the engine's create events since `prepare`, so removed ones too)
    ran a pinned image or one the suite declares it built (`TQA_BUILT_IMAGES`). A sandbox on another build fails the
    run, named; the run's own failure stays first. Stacks `prepare` didn't pin are not checked."""
    if "TQA_PINNED_IMAGES" not in env:
        return failure
    allowed = {image for key in ("TQA_PINNED_IMAGES", "TQA_BUILT_IMAGES")
               for image in env.get(key, "").split(",") if image}
    try:
        events = command(["docker", "events", "--since", env["TQA_STACK_SINCE"], "--until", str(int(time.time()) + 1),
                          "--filter", "type=container", "--filter", "event=create", "--filter",
                          "label=tqa.instance=" + env["TQA_SANDBOX_NETWORK"],
                          "--format", '{{.Actor.ID}} {{index .Actor.Attributes "image"}}'], env, capture=True).stdout
        foreign = foreign_sandboxes(events, allowed, lambda image: image_id(image, env))
    except Exception as error:
        foreign = []
        problem = AssertionError("the sandbox images could not be checked: " + redact(str(error), env))
    else:
        problem = AssertionError("; ".join("sandbox {} ran {}, not an image under test ({})".format(
            container[:12], image, ", ".join(sorted(allowed))) for container, image in foreign)) if foreign else None
    if problem is None:
        return failure
    print("Sandbox image guard: " + str(problem), file=sys.stderr)
    return failure if failure is not None else problem


# A loaded daemon can take minutes to remove a stack; a teardown that gives up early leaves networks from the small
# default address pools behind.
TEARDOWN_TIMEOUT = 300
# Docker object identifiers and Compose volume names, as the removal commands receive them.
IDENTIFIER = re.compile(r"[0-9A-Za-z][0-9A-Za-z_.-]{0,254}")


def remove_project(env):
    """Removes this owned project's containers, networks and volumes by its Compose label, without Compose: the fallback
    when `compose down` fails or times out, so no stack outlives its run."""
    label = "label=com.docker.compose.project=" + env["COMPOSE_PROJECT_NAME"]
    for listing, removal in (
            (["docker", "ps", "-aq"], ["docker", "rm", "--force"]),
            (["docker", "network", "ls", "-q"], ["docker", "network", "rm"]),
            (["docker", "volume", "ls", "-q"], ["docker", "volume", "rm", "--force"])):
        identities = command(listing + ["--filter", label], env, capture=True, timeout=TEARDOWN_TIMEOUT).stdout.split()
        if not all(IDENTIFIER.fullmatch(value) for value in identities):
            raise ValueError("Unexpected Compose object identifier")
        if identities:
            command(removal + identities, env, timeout=TEARDOWN_TIMEOUT)


def down(compose, env):
    """`compose down`, then the removal by label in every case: a `compose up` killed at its timeout leaves creates the
    daemon finishes after `down` has listed the project, so a successful `down` can still leave containers behind."""
    try:
        command(compose + ["down", "--volumes", "--remove-orphans", "--timeout", "10"], env, timeout=TEARDOWN_TIMEOUT)
    except (OSError, subprocess.SubprocessError):
        print("Compose down failed; removing the project by its label", file=sys.stderr)
    remove_project(env)


def teardown_on_signals():
    """A SIGTERM or SIGHUP ends the harness through its cleanup (the `finally` that brings the stack down), like Ctrl-C."""
    def interrupted(number, _frame):
        raise KeyboardInterrupt("signal {}".format(number))
    for number in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(number, interrupted)


def cleanup(directory, env, temporary, failure, *, compose=COMPOSE):
    """Attempt all owned cleanup operations, preserving the first contract or cleanup error."""
    actions = [("temporary files", temporary.cleanup)]
    if "COMPOSE_PROJECT_NAME" in env:
        actions = [
            ("Compose down", lambda: down(compose, env)),
            ("ownership", lambda: ownership(directory, env)),
        ] + actions
    for label, action in actions:
        try:
            action()
        except Exception as error:
            print("Cleanup failed ({}): {}".format(label, redact(str(error), env)), file=sys.stderr)
            if failure is None:
                failure = error
    if failure is not None:
        raise failure


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    teardown_on_signals()
    preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-compose-")
    directory = Path(temporary.name).resolve()
    failure = None
    try:
        prepare(directory, env)
        verify(env)
    except BaseException as error:
        failure = error
        if not isinstance(error, subprocess.SubprocessError):
            print("Compose contract failed: " + redact(str(error), env), file=sys.stderr)
    finally:
        cleanup(directory, env, temporary, failure)


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.SubprocessError, urllib.error.URLError):
        raise SystemExit("Compose contract failed; see diagnostics above") from None
