import re
from checklib import fail, held_out, read, run, visible

ROUTES = (("GET", "/health"), ("GET", "/items"), ("POST", "/items"), ("GET", "/items/ID"), ("PUT", "/items/ID"),
          ("DELETE", "/items/ID"))

def check(workspace):
    text = read(workspace, "API.md")
    normal = re.sub(r"(?:<id>|\{id\}|:id|<item_id>|\{item_id\})", "ID", text.replace("`", ""))
    missing = [f"{method} {path}" for method, path in ROUTES
               if not re.search(rf"\b{method}\s+{re.escape(path)}(?![/\w])", normal)]
    if missing:
        fail("API.md does not document every route", "missing: " + ", ".join(missing))
    visible(workspace)
    held_out(workspace)
    return "every route documented; items are validated as the README says"

run(check)
