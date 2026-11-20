"""The installation's files, as the install guide's step 2 makes them (deploy/README.md) and as deploy/install.sh does:
one function, so an installation the script made is one made by hand. It refuses to overwrite an existing installation
and never prints a secret.

    python3 deploy/tools/install_files.py

reads step 1's variables from the environment, run from the repository root. The bytes are pinned by
deploy/tests/install_block_18_0.sh (INST-U-001).
"""
import json
import os
from pathlib import Path
import secrets
import stat

# What compose.env carries besides the paths and the two generated secrets, when set.
PASSED = ["TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE", "TQA_HTTP_PORT", "TQA_GATEWAY_CONTAINER", "TQA_SANDBOX_NETWORK",
          "COMPOSE_PROJECT_NAME", "TQA_DOCKER_SOCKET", "TQA_DOCKER_GID"]
KEY_FILE_BYTES = 8192


def create(env, repository):
    """Creates the installation `env` describes (step 1's variables); answers its folder and the names compose.env
    sets besides the defaults.

    Raises SystemExit with a sentence for a refused setting, FileExistsError when the folder exists."""
    root = Path(env["TQA_INSTALL_DIR"]).expanduser().resolve()
    key_path = Path(env["TQA_PROVIDER_KEY_FILE"]).expanduser().resolve()
    repository = Path(repository).resolve()
    if root.is_relative_to(repository) or key_path.is_relative_to(repository):
        raise SystemExit("The installation and the key file must be outside this checkout.")
    info = key_path.stat()
    if stat.S_IMODE(info.st_mode) != 0o600 or info.st_size > KEY_FILE_BYTES:
        raise SystemExit("The key file must be mode 0600 (chmod 600) and at most 8192 bytes.")
    key = key_path.read_text().rstrip("\r\n")
    if not key:
        raise SystemExit("The key file is empty; for a server that needs no key, put a placeholder such as local.")
    context = env.get("TQA_MODEL_CONTEXT", "32768")
    if not context.isdigit() or not 1024 <= int(context) <= 10_000_000:
        raise SystemExit("TQA_MODEL_CONTEXT must be the model's context in tokens, such as 32768")
    port = int(env.get("TQA_HTTP_PORT", "8080"))
    origin = env.get("TQA_PUBLIC_ORIGIN") or "http://localhost:" + str(port)
    container = env.get("TQA_GATEWAY_CONTAINER", "tqa-gateway")
    network = env.get("TQA_SANDBOX_NETWORK", "tqa-sandboxes")
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    storage = root / "storage"
    storage.mkdir(mode=0o700)

    def write(name, value, mode=0o600):
        path = root / name
        path.write_bytes(value if isinstance(value, bytes) else value.encode())
        path.chmod(mode)

    write("install.key", secrets.token_bytes(32))
    write("secrets.toml", '[secrets]\n"secrets/local" = ' + json.dumps(key) + "\n")
    write("models.toml", '[[routes]]\nname="local"\nprotocol="openai_chat"\nauth={kind="bearer"}\n'
          'key_ref="secrets/local"\nbase_url=' + json.dumps(env["TQA_MODEL_BASE_URL"]) + "\nmodel="
          + json.dumps(env["TQA_MODEL"]) + "\ncontext_window=" + context + "\n", 0o644)
    write("gateway.toml", 'listen="0.0.0.0:8080"\npublic_origin=' + json.dumps(origin)
          + '\nmodels_path="/etc/tqa/models.toml"\n[storage]\nroot=' + json.dumps(str(storage))
          + '\n[runtime]\nbackend="docker"\nimage=' + json.dumps(env["TQA_RUNTIME_IMAGE"]) + "\n[sandbox]\nnetwork="
          + json.dumps(network) + '\ngateway_url="http://gateway:8080"\ngateway_container=' + json.dumps(container)
          + '\n[sandbox.env]\nPATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"\nHOME="/tmp"\n'
          '[web]\nupstream="http://web:3000"\n', 0o644)
    chosen = [name for name in PASSED if env.get(name)]
    write("compose.env", "TQA_INSTALL_DIR=" + str(root) + "\nTQA_STORAGE_ROOT=" + str(storage)
          + "\nTQA_POSTGRES_PASSWORD=" + secrets.token_hex(32) + "\nTQA_BOOTSTRAP_TOKEN=" + secrets.token_hex(32)
          + "\n" + "".join(name + "=" + env[name] + "\n" for name in chosen))
    return root, chosen


def main():
    root, chosen = create(os.environ, Path.cwd())
    print("created " + str(root) + "; compose.env also sets: " + (", ".join(chosen) or "nothing else"))


if __name__ == "__main__":
    os.umask(0o077)
    main()
