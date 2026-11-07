"""Stage 4: weekly.json and weekly.md (docs/PIPELINE.md)."""
import json


def write(figures, out):
    out.mkdir(parents=True, exist_ok=True)
    (out / "weekly.json").write_text(json.dumps(figures, indent=1, sort_keys=True) + "\n")
    rows = ["| Service | Requests | Errors | p95 (ms) | Users |", "|---|---|---|---|---|"]
    rows += [f"| {service} | {f['requests']} | {f['errors']} | {f['p95_ms']} | {f['users']} |"
             for service, f in sorted(figures.items())]
    (out / "weekly.md").write_text("\n".join(rows) + "\n")
