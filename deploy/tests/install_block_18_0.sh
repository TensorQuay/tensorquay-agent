#!/usr/bin/env bash
# The install guide's step 2 block as 18.0 shipped it (deploy/README.md), frozen: INST-U-001 checks that the
# installer and the guide's block still write these bytes. Never edit it to follow a change; a change is a decision.
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
