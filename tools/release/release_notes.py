"""The release notes skeleton (REL-U-005): pinned versions, the images by ID, the gate evaluation pages with their state,
and the known limits collected from each page. What a person writes is marked; nothing here is published."""

import re

PIN_NAMES = (("rust", "Rust"), ("node", "Node.js"), ("go", "Go"), ("firecracker", "Firecracker"),
             ("kernel", "Linux (guest kernel)"), ("debian_snapshot", "Debian snapshot"))


def _paragraph(lines, start):
    text = [lines[start]]
    for line in lines[start + 1:]:
        if not line.strip():
            break
        text.append(line.strip())
    return " ".join(text)


def evaluation(name, text):
    """A gate page's title, whether it is reached, its state sentence and the bullets of its limits section."""
    lines = text.splitlines()
    title = next((line[2:].strip() for line in lines if line.startswith("# ")), name)
    state_at = next((index for index, line in enumerate(lines) if line.startswith("**State:**")), None)
    if state_at is None:
        raise ValueError("{} has no **State:** line".format(name))
    state = _paragraph(lines, state_at)[len("**State:**"):].strip()
    reached = "the gate is **reached**" in state
    first = re.split(r"(?<=\.)\s", state, maxsplit=1)[0]
    limits, current, inside = [], None, False
    for line in lines:
        if line.startswith("## "):
            # A page's limits section: "Limits of the acceptance", or "Known limits" as gates 10 and 11 call it.
            inside = line.lower().startswith(("## limits of the acceptance", "## known limits"))
            continue
        # Some pages keep their limits as one "Known limits" bullet in the evaluation instead of a section.
        known = line.startswith("- **Known limits")
        if not inside and not known:
            if current is not None and not (line.startswith("  ") and line.strip()):
                limits.append(current)
                current = None
            elif current is not None:
                current += " " + line.strip()
            continue
        if line.startswith("- "):
            if current is not None:
                limits.append(current)
            current = line[2:].strip()
        elif line.startswith("  ") and current is not None and line.strip():
            current += " " + line.strip()
        elif not line.strip() and current is not None:
            limits.append(current)
            current = None
    if current is not None:
        limits.append(current)
    return {"page": name, "title": title, "reached": reached, "state": first, "limits": limits}


def _gate(page):
    return page["title"].split(":")[0]


def render(manifest, evaluations):
    version, commit = manifest["version"], manifest["commit"]
    tag = manifest["tag"]
    out = ["# TensorQuay Agent {}".format(version), ""]
    if not manifest["releasable"]:
        out += ["**Candidate dry run: not releasable.**", ""]
    out += ["Commit: `{}` ({}). Tag: {} ({}{}).".format(
                commit, manifest["commit_time"], tag["tag"], tag["state"],
                ", by the Product Owner" if tag["state"] == "to create" else ""),
            "Protocol revision: {}.".format(manifest["protocol_revision"]), "",
            "## Highlights", "", "_To write._", "",
            "## Breaking changes and upgrade", "", "_To write._",
            "See the upgrade notes in `deploy/README.md`.", "",
            "## Pinned versions", "", "| Component | Version |", "| --- | --- |"]
    out += ["| {} | {} |".format(label, manifest["pins"][key]) for key, label in PIN_NAMES if key in manifest["pins"]]
    out += ["", "## Images", "",
            "Referenced by image ID; registry digests exist once the Product Owner pushes them.", "",
            "| Image | ID | Size |", "| --- | --- | --- |"]
    out += ["| {} | `{}` | {:.0f} MB |".format(name, image["id"], image["size"] / 1e6)
            for name, image in sorted(manifest["images"].items())]
    out += ["", "## MicroVM artefacts", ""]
    out += ["- {} rootfs: SHA-256 `{}`".format(name, rootfs["sha256"])
            for name, rootfs in sorted(manifest["rootfs"].items())]
    kernel = manifest.get("kernel")
    out += ["- Guest kernel: " + ("`vmlinux` SHA-256 `{}`".format(kernel["sha256"]) if kernel
                                  else "not built for this release "
                                       "(shipping it is the Product Owner's decision)"), ""]
    vulns = manifest["vulnerabilities"]
    out += ["## Software bills of materials and known vulnerabilities", "",
            "SPDX 2.3 documents and package lists for every artefact are in `sbom/`. The release vulnerability gate "
            "(REL-U-003): {}, {} HIGH, {} MEDIUM, {} OPEN (no fix exists); details in `sbom/vulns.md`.".format(
                "passed" if vulns["passed"] else "FAILED", vulns["high"], vulns["medium"], vulns["open"]), "",
            "## Gate evaluations", "", "| Gate | State | Page |", "| --- | --- | --- |"]
    out += ["| {} | {} | `docs/evals/{}` |".format(
        page["title"], "reached" if page["reached"] else "not reached: " + page["state"], page["page"])
        for page in evaluations]
    out += ["", "## Known limits", "", "Collected from each evaluation page's limits of the acceptance.", ""]
    out += ["- {}: {}".format(_gate(page), limit) for page in evaluations for limit in page["limits"]]
    out += ["", "---", "", "Publishing this release needs the Product Owner's approval.", ""]
    return "\n".join(out)
