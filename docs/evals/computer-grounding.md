# Computer use: the grounding evaluation

**State:** measured twice (section 6), on 12 Oct 2026 and again on 15 Oct on 16i's capture. Qwen3.8 27B finds what
it's asked to act on by ref (200/200 both times) and by mark (199/200, then 196/200), and by coordinates only 46/200,
then 34/200, so it doesn't get `pixel_grounding`. The bar is decided (the technical lead, 5 Oct 2026, on the owner's
delegation): a route gets `pixel_grounding` only when the lower bound of its coordinate hit rate is at least 90 %. Not
yet measured on 16y's capture: the tool's words changed after 16i (section 6).

## 1. What it measures

For each route, three hit rates over the same targets:

| Way | What the model gets | A hit |
|---|---|---|
| **By ref** | the snapshot's text alone, as `observe` answers it | the call names the target's ref |
| **By mark** | the marked screenshot and its snapshot, as `screenshot` answers it | the call names the target's ref or its mark's number |
| **By coordinates** | the same screen unmarked, with the coordinate forms offered and the product's words for a screen where nothing has a mark ("no element has a mark: reach what you see with keys, or by x and y") | the point falls inside the target's box |

The first two measure reading and choosing; only the third measures pixel grounding, and only it decides the setting.
The first two are reported so a route that grounds badly can still be shown to work well with marks.

## 2. The targets

200 targets (`tests/e2e/grounding_eval/targets.py`, seeded, so the same set every time), each one instruction in plain
words and the element it means, named by the role and name the snapshot gives it:

| Kind | Targets | What |
|---|---|---|
| form | 40 | text fields, check boxes, radio buttons, lists and buttons on synthetic forms |
| menu | 30 | a menu bar and an open menu's items |
| icon | 30 | small icon-only buttons (24 px), asked for by what the icon shows |
| table | 30 | Edit and Open in dense invoice tables, asked for by the row ("the row for invoice INV-1234") |
| chinese | 25 | a Chinese page, asked in Chinese |
| dark | 25 | forms in a dark theme |
| desktop | 20 | Mousepad's menu bar and its open File menu, through AT-SPI |

- The pages are synthetic HTML, laid out with seeded offsets so the targets spread over the screen, and every page
  fits the browser's view without scrolling.
- Each target is unique on its screen by role and name; a repeated label ("Edit" in every row) is told apart by its
  row, as a person would.
- No canvas targets in this first set: canvas content has no element, so nothing to ask by ref or mark. It comes
  with a later set if the coordinates column needs it.

## 3. How a screen is captured and replayed

- Each screen is captured once on the Desktop image at 1280 × 800 (`computer_grounding_capture`, built into the
  desktop-probe image): the raw screen, the marked screenshot and its text exactly as the tool answers them, and each
  target's ref and box from the product's own marked snapshot. So the ground truth is the geometry the marks are drawn
  on: the page's DOM through one DOM snapshot, or the application's AT-SPI extents.
- Every route sees exactly those pixels and that text: the harness answers the model's `screenshot` and `observe`
  from the capture, so nothing on the screen moves between routes or runs.
- One call per target and way, a fresh conversation each, shaped as the route sends it: the `computer` tool's own
  schema for the way (the capture records the product's spec on a route without images, with images, and with
  `pixel_grounding`, so the evaluation never sends a copy that has drifted), no `tool_choice` (the product's Chat
  requests leave it to the model), the route's output bound and extra body (a thinking model's switch), and the
  server's own sampling defaults. Each answer records why the reply ended and its tokens, so a reply cut at its bound
  shows as that, not as a bad aim.
- The image is the capture at 1280 × 800, as the product sends it to a route at the default image side (1280). A route
  with a smaller side would need the capture scaled after marking; the runner doesn't do that yet.

## 4. The numbers

- A hit rate per way per route, with Wilson score intervals at 95 %, and the same broken down by kind
  (`tests/e2e/grounding_eval/scorer.py`). An unanswered target, an unparsable call or a point outside the image is a
  miss. A point is mapped to the screen as the tool maps it, so a route sent a scaled image is scored fairly.
- **The recommendation:** `pixel_grounding` on when the coordinates column's lower bound is at least 90 % over the
  whole set. The set's size sets how many hits that takes:

  | Targets | Hits needed | Observed rate at the bar | One fewer gives |
  |---|---|---|---|
  | 60 | 59 | 98.3 % | 88.6 % |
  | 100 | 96 | 96.0 % | 88.8 % |
  | **200** | **189** | **94.5 %** | 89.8 % |
  | 300 | 281 | 93.7 % | 89.9 % |

  With 200 targets (the technical lead, 5 Oct 2026), a route may miss 11. With 60 it could miss only one, a bar that
  measured the set's size more than the model.
- The admin guide lists each route measured, its three rates with their intervals, the date and the model's version.

## 5. Settled before it runs (12 Oct 2026)

- **The runner** (`runner.py`): one call per target and way, answering the model's `screenshot` or `observe` from the
  capture and appending to `answers.jsonl`, four calls at a time (as many as the local model server batches; accuracy
  is the measure, not wall time), resuming where it stopped. `run_eval.sh` gives it the local route's settings.
- **The capture's ground truth:** an application's target is found by its whole name as a person reads it (GTK's
  mnemonic, ellipsis and padding left out, as in "Open...      "), never by a prefix, which found "New" three times in
  Mousepad's File menu; an item that opens a submenu is a menu, not a menu item. A capture that can't find a target
  says what it found instead.
- **The images:** the capture runs in a desktop-probe image built from the commit under test, and the captures are
  regenerated from the seeded set (`targets.py`, then the capture), never committed.
- **What a miss did:** each answer names the call's action, and a miss keeps the call's arguments, so an answer
  without a point says whether the model looked again, named a ref, or did something else. (The first run, 12 Oct,
  had 79 coordinates answers without a point and couldn't say; its coordinates column also showed no refs without
  saying so. A rerun of that column starts from a copy without its coordinates lines, since the runner resumes by
  route, way and target.)
- **Still open:** scaling the capture for a route with a smaller image side (above); none of the routes measured first
  needs it.
- **Still open, a later item:** Qwen3.8 27B's coordinates fall short in x by about 1.5 while y is exact (section 6).
  Check the image the model actually receives, after the model server's image processor (any resize to its own sizes,
  any padding), in case a mismatch between what the product sends and what the model sees is part of it. The 15 Oct
  rerun shows the same shape (section 6), so this is still to check.

## 6. Results

### Qwen3.8 27B, 12 Oct 2026
Served by vLLM (NVFP4, 64k context, images on), with the local route's settings: thinking on, an output bound of
16,384, no `tool_choice`, four calls at a time. The capture: all 200 targets on the Desktop image at 1280 × 800, sent
unscaled (the route's default image side).

| Way | Hits | Rate (Wilson 95 %) |
|---|---|---|
| By ref | 200/200 | 100.0 % (98.1 % to 100.0 %) |
| By mark | 199/200 | 99.5 % (97.2 % to 99.9 %) |
| By coordinates | 46/200 | 23.0 % (17.7 % to 29.3 %) |

By kind, coordinates: form 15/40, menu 12/30, dark 11/25, chinese 4/25, icon 2/30, desktop 1/20, table 1/30. By ref
and by mark every kind is whole but icons by mark (29/30).

**`pixel_grounding`: no.** The bar needs 189 coordinate hits.

**How the coordinates were read.** The column was run twice; the table is the second.
- The first run showed the unmarked screen with the bare size line. 79 answers had no point (most asked to look again
  or named a ref that wasn't shown), and 36 hit. The second shows what the product shows for a screen with no marks
  ("no element has a mark: reach what you see with keys, or by x and y"): 10 answers asked to look again (`observe`),
  187 gave a point, 3 gave lists, and 46 hit.
- The misses aren't a coordinate space. Read as 0-1000 relative, the first run's points hit 9 times, fewer than as
  pixels. y is right: a point's y matches its target's centre (median ratio 1.000). x is short of it (median ratio
  1.4 to 1.5, widely spread), and even the best straight-line correction, fitted on the answers themselves, would
  reach about a quarter of the targets. Misses are far: a median of 94 pixels from the box.
- So for this model, the product's refs and marks are the way to act, as the computer tool already asks; coordinates
  stay off. A route that declares a coordinate space would still be worth measuring for another model family.

### Qwen3.8 27B, 15 Oct 2026, on 16i's capture
The same model and the local route's settings (`run_eval.sh`), served with images on in Gate 5's hold, 20:22 to 20:30
+08:00. The runner was 16i's (`a94681cf4`), and the capture was made in 16i's desktop-probe image on 14 Oct. Since
16e the Desktop's browser shows no warning bar, so every page sits 56 px higher: the 180 browser targets moved up 56
px, and the 20 application targets didn't move. A 30-call smoke run said go (no reply cut, no transport error).

| Way | Hits | Rate (Wilson 95 %) |
|---|---|---|
| By ref | 200/200 | 100.0 % (98.1 % to 100.0 %) |
| By mark | 196/200 | 98.0 % (95.0 % to 99.2 %) |
| By coordinates | 34/200 | 17.0 % (12.4 % to 22.8 %) |

By kind, coordinates: form 12/40, dark 11/25, menu 6/30, chinese 4/25, icon 1/30, desktop 0/20, table 0/30. By ref
every kind is whole; by mark, all but icons (26/30).

**`pixel_grounding`: no.** The bar needs 189 coordinate hits.

- **Against 12 Oct:** the coordinates intervals overlap (17.7 % to 29.3 % then), so this is no measured change, and
  by mark 4 icons missed instead of 1. Every reply of the 600 ended in a tool call; none was cut.
- **What the coordinate answers did:** 184 gave a point (3 of them lists; one was a scroll), 12 asked to look again
  (`observe`), and one each took a screenshot, pressed a key, clicked by ref or gave x alone.
- **The same shape as 12 Oct:** y is right (median ratio of the target's centre to the point, 1.000); x is short by
  a median ratio of 1.51 (quartiles 1.29 to 1.59). Misses are a median of 96 pixels from the box.

### What a run on 16y would change
No commit since 16i touches the marks or the capture, and the accessibility reader moved to zbus 6 (16v) with an
application's snapshot kept byte-identical (TOOL-U-067). Two things differ. The computer tool's descriptions now say
how to open a web address (TOOL-U-061, 16n), and the runner sends the tool's spec as the capture recorded it. And the
Desktop's root window is white since 16n (SBX-C-024), where an application's screen shows it. So a 16y capture would
send the model different words, and some screens different pixels, around the same targets. No run on 16y's capture
has been made.
