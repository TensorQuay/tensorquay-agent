"""The grounding evaluation's grid-then-zoom runner (c17, exploratory: the computer tool has no grid yet, so the runner
supplies its words and draws its images from the existing capture; no product change, no new capture).

    python3 grid_runner.py DIR --variant g12|g20 --route NAME --base-url URL --model MODEL [--key-file PATH]
        [--limit N] [--max-tokens N] [--extra-body JSON] [--concurrency N]

Per target, a conversation as the route would hold it: the instruction, a grid call already made with its level-1
image and words, then the model's own calls. A grid call with a cell gets that cell's zoom (drawn on demand, then
kept in DIR/grid-cache/), a click with a cell ends it; the point is the clicked cell's centre on the screen, written
as a coordinates answer in screen pixels, so scorer.py scores it unchanged. A pick whose next window can't hold the
target ends the target as a miss with no further call: no point inside that window can hit it. The model never
writes a coordinate; one that does misses. Each line also records the cells picked, whether each level's pick still
held the target, and (G20) whether it zoomed a third time. A route holds one variant only.

--variant z1000 (zoom + 0-1000): the level-1 grid as above, one grid call with a cell, then that cell plus 20 px
with no grid; the model clicks it by x and y from 0 to 1000 across that image (grid.crop_point maps it to the
screen as the scorer maps Arm 1's 0-1000). Its line also records the click as sent ("crop_x", "crop_y") and the
crop's window. Its spec is the coordinates route's recorded one (grid.crop_spec), not the mark route's.
"""

import argparse
import base64
import json
import os
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import grid  # noqa: E402
import pngio  # noqa: E402
import runner  # noqa: E402


def load(directory):
    """The capture: screens by name and the mark route's recorded spec, the base of the grid route's."""
    captures = json.loads((Path(directory) / "captures" / "captures.json").read_text(encoding="utf-8"))
    tools = captures.get("tools", {})
    if "mark" not in tools:
        sys.exit("captures.json has no mark route spec: capture again with this commit's capture")
    loaded = {"screens": {shot["screen"]: shot for shot in captures["screens"]}, "boxes": captures["boxes"],
              "spec": grid.tool_spec(tools["mark"])}
    if "coordinates" in tools:
        loaded["crop_spec"] = grid.crop_spec(tools["coordinates"])
    return loaded


class Job:
    """One variant over one capture: draws (and caches) each grid image once, however many targets ask for it."""

    def __init__(self, directory, variant, capture):
        self.directory, self.variant, self.capture = Path(directory), variant, capture
        self.screens, self.lock = {}, threading.Lock()
        self.crop = variant in grid.CROPS
        if self.crop and "crop_spec" not in capture:
            sys.exit("captures.json has no coordinates route spec, which a crop variant's tool is built from")
        self.spec = capture["crop_spec"] if self.crop else capture["spec"]

    def screen(self, name):
        with self.lock:
            if name not in self.screens:
                png = self.capture["screens"][name]["png"]
                self.screens[name] = pngio.decode((self.directory / "captures" / png).read_bytes())
            return self.screens[name]

    def image(self, name, view):
        """The grid image's PNG for a view on a screen, from the cache or drawn now."""
        cache = self.directory / "grid-cache" / self.variant / name / ("-".join(view.path) or "top")
        path = cache.with_suffix(".png")
        if not path.exists():
            drawn, _ = grid.render(self.screen(name), view, marks=not (self.crop and view.level > 1))
            path.parent.mkdir(parents=True, exist_ok=True)
            # A temporary name per writer: two threads may draw the same image at once, and a shared name lets the
            # first one's move take the file from under the second (the Saturday hold, 24 Oct 2026). Each move puts
            # the same bytes in place.
            temporary = path.with_name(f"{path.stem}.{os.getpid()}-{threading.get_ident()}.tmp")
            temporary.write_bytes(pngio.encode(drawn))
            temporary.replace(path)
        return path.read_bytes()

    def turn(self, name, view, call_id):
        """The tool result and its image for a grid view, as the product shapes an image result (LLM-U-052)."""
        data = base64.b64encode(self.image(name, view)).decode("ascii")
        return [{"role": "tool", "tool_call_id": call_id, "content": grid.words(view, self.variant)},
                {"role": "user", "content": [
                    {"type": "text", "text": f"The image from the result of tool call {call_id}:"},
                    {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{data}"}}]}]

    def answer(self, target, post, base_url, model, key, max_tokens, extra):
        """One target's conversation, call by call, to its answer line."""
        name = target["page"]
        shot = self.capture["screens"][name]
        box = self.capture["boxes"][target["id"]]["box"]
        view = grid.level1(shot["width"], shot["height"])
        start = {"id": "grid-1", "type": "function",
                 "function": {"name": "computer", "arguments": json.dumps({"action": "grid"})}}
        messages = [{"role": "system", "content": runner.SYSTEM}, {"role": "user", "content": target["instruction"]},
                    {"role": "assistant", "content": "", "tool_calls": [start]}] + self.turn(name, view, "grid-1")
        line = {"way": "coordinates", "target": target["id"], "variant": self.variant, "cells": [], "levels": [],
                "third_zoom": False, "calls": 0, "usage": {"prompt_tokens": 0, "completion_tokens": 0}}

        def miss(error, **more):
            return {**line, "error": error, **more}

        for _ in range(2 if self.crop else 1 + len(grid.VARIANTS[self.variant])):
            reply = post(base_url, model, {"messages": messages, "tools": [{"type": "function",
                                                                             "function": self.spec}],
                                           "max_tokens": max_tokens, **extra}, key)
            if "error" in reply and "choices" not in reply:
                return miss(reply["error"], transport=True)
            line["calls"] += 1
            for counter in ("prompt_tokens", "completion_tokens"):
                line["usage"][counter] += (reply.get("usage") or {}).get(counter) or 0
            choice = (reply.get("choices") or [{}])[0]
            line["finish_reason"] = choice.get("finish_reason")
            calls = choice.get("message", {}).get("tool_calls") or []
            if not calls:
                return miss("no tool call")
            try:
                arguments = json.loads(calls[0]["function"]["arguments"])
            except (KeyError, TypeError, ValueError):
                return miss("unparsable arguments")
            if not isinstance(arguments, dict):
                return miss("unparsable arguments")
            action, cell = arguments.get("action"), arguments.get("cell")
            if self.crop and view.level > 1:
                return self.clicked(line, view, box, action, arguments)
            if arguments.get("x") is not None or arguments.get("y") is not None:
                return miss("wrote coordinates", arguments=arguments)
            if not cell:
                return miss("no cell", arguments=arguments)
            if grid.parse(view, cell) is None:
                return miss("no such cell", arguments=arguments)
            picked = str(cell).strip().upper()
            line["cells"].append(picked)
            if action in ("click", "double_click"):
                point = grid.centre(view, picked)
                line["levels"].append({"level": view.level, "cell": picked, "holds_target": grid.inside(point, box)})
                return {**line, "action": action, "x": point[0], "y": point[1], "image": list(view.screen)}
            if action != "grid":
                return miss(f"action {action} with a cell", arguments=arguments)
            closer = grid.zoom(view, picked, self.variant)
            if closer is None:
                return miss("the cells can't get smaller")
            holds = grid.overlaps(closer.window, box)
            line["levels"].append({"level": view.level, "cell": picked, "holds_target": holds})
            if not holds:
                return miss("picked a cell whose window can't hold the target")
            if closer.level == 3:
                line["third_zoom"] = True
            call_id = f"grid-{len(line['cells']) + 1}"
            messages.append({"role": "assistant", "content": "", "tool_calls": [
                {"id": call_id, "type": "function", "function": {"name": "computer", "arguments": json.dumps(
                    {"action": "grid", "cell": picked})}}]})
            messages += self.turn(name, closer, call_id)
            view = closer
        return miss("no click after the last level")

    @staticmethod
    def clicked(line, view, box, action, arguments):
        """A crop variant's last call: a click by x and y from 0 to 1000 across the crop, ending the target."""
        def miss(error):
            return {**line, "error": error, "arguments": arguments}

        if action not in ("click", "double_click"):
            return miss(f"action {action} on the crop")
        if arguments.get("cell"):
            return miss("a cell on the crop")
        if arguments.get("x") is None or arguments.get("y") is None:
            return miss("no point")
        point = grid.crop_point(view, arguments["x"], arguments["y"])
        if point is None:
            return miss("a point outside 0 to 1000")
        line["levels"].append({"level": view.level, "cell": None, "holds_target": grid.inside(point, box)})
        return {**line, "action": action, "x": point[0], "y": point[1], "image": list(view.screen),
                "crop_x": arguments["x"], "crop_y": arguments["y"], "crop_window": list(view.window)}


def summary(lines, targets, truths):
    """Where the grid loses: per level, how many picks still held the target; and G20's third zooms, by size."""
    by_level, third = {}, {"under_20": [0, 0], "20_or_more": [0, 0]}
    for line in lines:
        for level in line.get("levels", []):
            counts = by_level.setdefault(str(level["level"]), {"held": 0, "of": 0})
            counts["held"] += level["holds_target"]
            counts["of"] += 1
        if line.get("variant") == "g20" and line["target"] in truths:
            box = truths[line["target"]]["box"]
            group = third["under_20" if min(box[2], box[3]) < 20 else "20_or_more"]
            group[0] += line.get("third_zoom", False)
            group[1] += 1
    errors = {}
    for line in lines:
        if "error" in line:
            errors[line["error"]] = errors.get(line["error"], 0) + 1
    return {"by_level": by_level, "third_zoom": third, "misses": errors, "lines": len(lines), "targets": len(targets)}


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("directory")
    parser.add_argument("--variant", choices=sorted(grid.VARIANTS) + sorted(grid.CROPS), required=True)
    parser.add_argument("--route", required=True)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--key-file")
    parser.add_argument("--limit", type=int)
    parser.add_argument("--max-tokens", type=int, default=16384)
    parser.add_argument("--extra-body", default="{}")
    parser.add_argument("--concurrency", type=int, default=1)
    args = parser.parse_args(argv)
    directory = Path(args.directory)
    key = Path(args.key_file).read_text(encoding="utf-8").strip() if args.key_file else None
    extra = json.loads(args.extra_body)
    targets = json.loads((directory / "targets.json").read_text(encoding="utf-8"))["targets"]
    capture = load(directory)
    path = directory / "answers.jsonl"
    done = set()
    if path.exists():
        for text in path.read_text(encoding="utf-8").splitlines():
            if text.strip():
                record = json.loads(text)
                if record["route"] == args.route and record.get("variant", args.variant) != args.variant:
                    sys.exit(f"{args.route} already holds {record.get('variant')} answers: use another --route")
                done.add((record["route"], record["way"], record["target"]))
    jobs = [target for target in targets[: args.limit] if (args.route, "coordinates", target["id"]) not in done]
    job, lock = Job(directory, args.variant, capture), threading.Lock()

    def one(target):
        line = {"route": args.route, **job.answer(target, runner.post, args.base_url, args.model, key,
                                                   args.max_tokens, extra)}
        with lock:
            out.write(json.dumps(line, ensure_ascii=False) + "\n")
            out.flush()

    with path.open("a", encoding="utf-8") as out, ThreadPoolExecutor(max_workers=max(1, args.concurrency)) as pool:
        list(pool.map(one, jobs))
    print(f"ANSWERED {len(jobs)} targets for {args.route} ({args.variant})", file=sys.stderr)


if __name__ == "__main__":
    main()
