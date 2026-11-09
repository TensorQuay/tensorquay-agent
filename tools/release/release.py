#!/usr/bin/env python3
"""The local release pipeline (DEV-PLAN §7, Phase 9 item 3). It builds and records everything a release names, from
a clean export of one commit, and publishes nothing: the Product Owner pushes the images, tags and uploads.

    TQA_DOCKER_TESTS=1 python3 tools/release/release.py --version X.Y.Z --commit <sha> [--out dist] [--kernel]
                                                       [--tracker <debian tracker json>] [--candidate]

Before anything is built it refuses:
- a bad version;
- a dirty checkout;
- a commit not on main (unless --candidate, which is never releasable);
- a tag that names another commit;
- an existing release directory.

Then:
1. export the commit;
2. build the five images with SOURCE_DATE_EPOCH (local tags only);
3. build the two template rootfs, and with --kernel the guest kernel;
4. write the SBOMs (the completeness check included);
5. run the vulnerability gate (REL-U-003; HIGH fails);
6. write release.json, RELEASE-NOTES.md and SHA256SUMS.

It works in dist/.X.Y.Z.partial and renames it to dist/X.Y.Z only when everything passed; a failure leaves FAILED
there. It is heavy: run it through the team's stack queue.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

import release_checks as checks
import release_manifest as manifest
import release_notes as notes

FORBIDDEN = {("docker", "push"), ("docker", "login"), ("git", "tag"), ("git", "push")}
IMAGES = (
    ("gateway", "images/gateway/Dockerfile", None, "tqa-gateway"),
    ("web", "images/web/Dockerfile", None, "tqa-web"),
    ("runtime-base", "images/runtime/Dockerfile", "base", "tqa-runtime"),
    ("runtime-workstation", "images/runtime/Dockerfile", "workstation", "tqa-runtime"),
    ("runtime-desktop", "images/runtime/Dockerfile", "desktop", "tqa-runtime"),
    ("vmm", "images/vmm/Dockerfile", "vmm", "tqa-vmm"),
)
TEMPLATES = (("base", "runtime-base"), ("workstation", "runtime-workstation"), ("desktop", "runtime-desktop"))
REVISION_LABEL = "org.tensorquay.protocol-revision"
BUILD_TIMEOUT = 3600


class Failed(Exception):
    """A release step failed; the partial directory's FAILED file says which and why."""


def guard(run):
    """The runner as the pipeline sees it: a publishing command is refused before it runs."""
    def guarded(arguments, **options):
        arguments = [str(argument) for argument in arguments]
        if tuple(arguments[:2]) in FORBIDDEN:
            raise checks.Refused("the release pipeline never runs `{}`: publishing needs the Product Owner".format(
                " ".join(arguments[:2])))
        return run(arguments, **options)
    return guarded


def step_of(arguments):
    """Which pipeline step a command belongs to (for the log and the tests)."""
    text = " ".join(str(argument) for argument in arguments)
    if arguments[:2] == ["tar", "-x"] or "archive" in arguments:
        return "export"
    if arguments[:2] == ["docker", "build"]:
        return "kernel" if "--output" in arguments else "build"
    if "build_rootfs.py" in text:
        return "rootfs"
    if "sbom_vulns.py" in text:
        return "vulnerabilities"
    if "sbom.py" in text:
        return "sbom"
    return None


def subprocess_runner(arguments, cwd=None, env=None, timeout=BUILD_TIMEOUT):
    done = subprocess.run([str(argument) for argument in arguments], cwd=cwd, env=env, timeout=timeout, check=True,
                          capture_output=True, text=True)
    return done.stdout


class Pipeline:
    def __init__(self, *, repo, out, version, commit, candidate, kernel, tracker, run, env):
        self.repo, self.out, self.version_text, self.ref = Path(repo), Path(out), version, commit
        self.candidate, self.with_kernel, self.tracker = candidate, kernel, tracker
        self.run_command, self.env = guard(run), env
        self.step = "preflight"

    def git(self, *arguments):
        return self.run_command(["git", "-C", self.repo, *arguments])

    def preflight(self):
        self.version = checks.version(self.version_text)
        checks.clean_checkout(self.git("status", "--porcelain"))
        self.commit = checks.commit(self.git("rev-parse", "--verify", self.ref + "^{commit}"))
        try:
            self.git("merge-base", "--is-ancestor", self.commit, "main")
            on_main = True
        except subprocess.CalledProcessError:
            on_main = False
        self.releasable = checks.releasable(on_main, self.candidate)
        try:
            tagged = self.git("rev-parse", "-q", "--verify", "refs/tags/v{}^{{commit}}".format(self.version))
        except subprocess.CalledProcessError:
            tagged = ""
        self.tag = checks.tag(self.version, self.commit, tagged)
        self.final = checks.release_directory(self.out, self.version)
        if self.env.get("TQA_DOCKER_TESTS") != "1":
            raise checks.Refused("the release builds images: set TQA_DOCKER_TESTS=1 (the stack tests' opt-in)")
        for tool in ("docker", "git", "debugfs"):
            try:
                self.run_command(["which", tool])
            except (subprocess.CalledProcessError, FileNotFoundError):
                raise checks.Refused("{} is not installed".format(tool)) from None

    def run(self):
        self.preflight()
        self.partial = self.out / ".{}.partial".format(self.version)
        self.work = self.out / ".{}.work".format(self.version)
        self.partial.mkdir()
        self.work.mkdir()
        try:
            self.export()
            images = self.build_images()
            rootfs = self.build_rootfs(images)
            kernel = self.build_kernel()
            self.sboms(images, kernel)
            vulnerabilities = self.vulnerabilities()
            self.write(images, rootfs, kernel, vulnerabilities)
        except (subprocess.CalledProcessError, OSError, ValueError, KeyError) as error:
            detail = getattr(error, "stderr", "") or ""
            (self.partial / "FAILED").write_text("step: {}\nerror: {}\n{}\n".format(self.step, error, detail))
            raise Failed("the release failed in step '{}': {}".format(self.step, error)) from error
        shutil.rmtree(self.work)
        self.partial.rename(self.final)
        return self.final

    def export(self):
        self.step = "export"
        self.source = self.work / "source"
        self.source.mkdir()
        archive = self.work / "source.tar"
        self.git("archive", "--format=tar", "-o", archive, self.commit)
        self.run_command(["tar", "-x", "-f", archive, "-C", self.source])
        self.epoch = self.git("show", "-s", "--format=%ct", self.commit).strip()
        self.commit_time = self.git("show", "-s", "--format=%cI", self.commit).strip()
        self.tree = self.git("rev-parse", self.commit + "^{tree}").strip()
        self.runner_commit = self.git("rev-parse", "HEAD").strip()

    def build_images(self):
        self.step = "build"
        revision = manifest.protocol_revision(self.source)
        images = {}
        for name, dockerfile, target, repository in IMAGES:
            tag = "{}:{}".format(repository, self.version if target in (None, "vmm") else target + "-" + self.version)
            command = ["docker", "build", "-q", "--build-arg", "SOURCE_DATE_EPOCH=" + self.epoch, "-f",
                       self.source / dockerfile, "-t", tag]
            if target:
                command += ["--target", target]
            identity = self.run_command(command + [self.source], timeout=BUILD_TIMEOUT).strip()
            record = json.loads(self.run_command(["docker", "image", "inspect", identity]))[0]
            labels = record.get("Config", {}).get("Labels") or {}
            if name.startswith("runtime") and labels.get(REVISION_LABEL) != str(revision):
                raise ValueError("{} speaks protocol revision {}, the commit's types {}".format(
                    name, labels.get(REVISION_LABEL), revision))
            images[name] = {"id": identity, "tag": tag, "size": record.get("Size", 0), "labels": labels}
        return images

    def build_rootfs(self, images):
        self.step = "rootfs"
        out = self.work / "rootfs"
        out.mkdir()
        (self.partial / "rootfs").mkdir()
        rootfs = {}
        for template, image in TEMPLATES:
            identity = images[image]["id"]
            self.run_command(["python3", self.source / "images/rootfs/build_rootfs.py", "--image", identity,
                              "--vmm-image", images["vmm"]["id"], "--out", out], timeout=BUILD_TIMEOUT)
            built = out / identity.removeprefix("sha256:")
            record = json.loads((built / "manifest.json").read_text())
            shutil.copy2(built / "manifest.json", self.partial / "rootfs" / (template + ".manifest.json"))
            rootfs[template] = {"sha256": record["rootfs"]["sha256"], "source_image": identity, "directory": built}
        return rootfs

    def build_kernel(self):
        if not self.with_kernel:
            return None
        self.step = "kernel"
        built = self.work / "kernel"
        self.run_command(["docker", "build", "-f", self.source / "images/rootfs/kernel/Dockerfile", "--target",
                          "kernel", "--output", "type=local,dest={}".format(built),
                          self.source / "images/rootfs/kernel"], timeout=BUILD_TIMEOUT)
        (self.partial / "kernel").mkdir()
        for name in ("manifest.json", "config"):
            shutil.copy2(built / name, self.partial / "kernel" / name)
        record = json.loads((built / "manifest.json").read_text())
        return {"sha256": record["vmlinux_sha256"], "directory": built}

    def sboms(self, images, kernel):
        self.step = "sbom"
        sbom = self.partial / "sbom"
        tool = self.source / "tools/release/sbom.py"
        env = dict(self.env, SOURCE_DATE_EPOCH=self.epoch)
        for name, image in images.items():
            self.run_command(["python3", tool, "--artefact", name, "--image", image["id"], "--release", self.version,
                              "--out", sbom], env=env)
        for template, image in TEMPLATES:
            self.run_command(["python3", tool, "--artefact", "rootfs-" + template, "--rootfs",
                              self.work / "rootfs" / images[image]["id"].removeprefix("sha256:"),
                              "--release", self.version, "--out", sbom], env=env)
        if kernel:
            self.run_command(["python3", tool, "--artefact", "kernel", "--kernel", kernel["directory"], "--release",
                              self.version, "--out", sbom], env=env)

    def vulnerabilities(self):
        self.step = "vulnerabilities"
        command = ["python3", self.source / "tools/release/sbom_vulns.py", "--sbom", self.partial / "sbom"]
        if self.tracker:
            command += ["--tracker", self.tracker]
        self.run_command(command, env=self.env)
        return json.loads((self.partial / "sbom" / "vulns.json").read_text())

    def write(self, images, rootfs, kernel, vulnerabilities):
        self.step = "manifest"
        pages = sorted((self.source / "docs" / "evals").glob("gate-*.md"))
        evaluations = [notes.evaluation(page.name, page.read_text(encoding="utf-8")) for page in pages]
        record = manifest.release(
            version=self.version, commit=self.commit, commit_time=self.commit_time, tree=self.tree,
            releasable=self.releasable, tag=self.tag, runner_commit=self.runner_commit,
            pins=manifest.pins(self.source), protocol_revision=manifest.protocol_revision(self.source),
            images=images, rootfs={name: {"sha256": item["sha256"], "source_image": item["source_image"]}
                                   for name, item in rootfs.items()},
            kernel=None if kernel is None else {"sha256": kernel["sha256"]},
            vulnerabilities=vulnerabilities, evaluations=evaluations)
        (self.partial / "release.json").write_text(json.dumps(record, indent=1, sort_keys=True) + "\n")
        (self.partial / "RELEASE-NOTES.md").write_text(notes.render(record, evaluations))
        (self.partial / "SHA256SUMS").write_text(manifest.sha256sums(self.partial))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--out", type=Path, default=Path("dist"))
    parser.add_argument("--kernel", action="store_true",
                        help="also build the guest kernel (shipping it is the Product Owner's decision)")
    parser.add_argument("--tracker", type=Path, help="a saved Debian security tracker file (offline gate)")
    parser.add_argument("--candidate", action="store_true", help="a dry run of a commit not on main; not releasable")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    repo = Path(__file__).resolve().parents[2]
    pipeline = Pipeline(repo=repo, out=args.out.resolve(), version=args.version, commit=args.commit,
                        candidate=args.candidate, kernel=args.kernel, tracker=args.tracker,
                        run=subprocess_runner, env=dict(os.environ))
    started = time.monotonic()
    try:
        final = pipeline.run()
    except (checks.Refused, Failed) as error:
        print("release refused" if isinstance(error, checks.Refused) else "release failed", ":", error,
              file=sys.stderr)
        return 1
    print("release {} written to {} in {:.0f} s; nothing was published".format(
        pipeline.version, final, time.monotonic() - started))
    return 0


if __name__ == "__main__":
    sys.exit(main())
