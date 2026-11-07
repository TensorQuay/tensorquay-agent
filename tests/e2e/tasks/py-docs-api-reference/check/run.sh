# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v python3 >/dev/null || exit 2
cd /workspace || exit 2
PYTHONDONTWRITEBYTECODE=1 python3 - <<'PY'
import inspect, sys
sys.path.insert(0, ".")
import api
intro = "# Inventory API\n\nEvery function returns a plain dictionary or list and raises `ValueError` for invalid input.\n"
expected = intro
public = [(name, obj) for name, obj in vars(api).items() if inspect.isfunction(obj) and not name.startswith("_")]
for name, function in public:
    first = inspect.getdoc(function).splitlines()[0]
    expected += f"\n### {name}{inspect.signature(function)}\n\n{first}\n"
actual = open("docs/API.md").read()
if actual.rstrip("\n") + "\n" != expected:
    print('{"passed": false, "reason": "docs/API.md does not match the public functions of api.py"}')
    sys.exit(1)
print('{"passed": true, "reason": "every public function is documented with its exact signature, in order"}')
PY
