# The grounding evaluation

The plan and the bar are in [docs/evals/computer-grounding.md](../../../docs/evals/computer-grounding.md). Four steps,
each a file here:

1. `targets.py DIR`: writes the 200 targets (`DIR/targets.json`) and their synthetic pages (`DIR/pages/`), seeded.
   `--canvas` adds 60 targets with no element of their own after them (`canvas_targets.py`: tools painted on a canvas,
   buttons drawn in a picture, on six pages, each found from its page's anchor heading); they are asked and scored by
   coordinates only, since no ref or mark can name them.
2. The capture, on the Desktop image: `TQA_GROUNDING_DIR=DIR computer-grounding-capture --ignored --test-threads=1`
   in the desktop-probe image built from the commit under test, with DIR mounted. It writes `DIR/captures/`: each
   screen unmarked and marked, the tool's text for it, every target's ref and box, and the tool's schema for each way.
3. `runner.py DIR --route NAME --base-url URL --model MODEL [--max-tokens N] [--extra-body JSON] [--concurrency N]`:
   one call per target and way against an OpenAI Chat endpoint, shaped as the route sends it with the product's own
   tool schema from the capture, appended to `DIR/answers.jsonl` with why each reply ended; it resumes where it
   stopped. `run_eval.sh DIR` runs it against the team's local model with the local route's settings, then the scorer.
   `--coordinate-space 1000` (exploratory, since 17.0) asks the coordinates way for x and y from 0
   to 1000 across the screenshot instead of its pixels, Qwen3-VL's family's convention; the product has no such mode
   yet. Its answers record their space, so it takes its own `--route` name.
   `detect.py` and `recall.py` (exploratory, since 17.0; no detector ships): a UI element detector's boxes on a capture's
   unmarked screens, in a throwaway container with no network, then how many targets those boxes reach with no model
   (IoU 0.5, or a box's centre inside), which caps any marks drawn from them.
   `grid_runner.py DIR --variant g12|g20 --route NAME ...` (exploratory, since 17.0): grid-then-zoom, the model naming
   labelled cells and never a coordinate (`grid.py`: the geometry, the drawing from the capture's unmarked screens with
   `pngio.py`, the words); its answers are coordinates lines in screen pixels, with the cells picked and each level's
   result, so the scorer reads them unchanged.
   `--variant z1000` (zoom + 0-1000): the same level 1, then the chosen cell plus 20 px with no grid, clicked by x and
   y from 0 to 1000 across that image; its tool is the recorded coordinates spec with `grid` and `cell`.
4. `scorer.py DIR`: `DIR/report.md` and `DIR/report.json`, with each way's hit rate, its Wilson interval, the
   breakdown by kind, and the `pixel_grounding` recommendation.

Unit tests: `python3 -m unittest discover -s tests/e2e/grounding_eval -p 'test_*.py'`.
