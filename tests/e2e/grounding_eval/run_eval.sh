#!/usr/bin/env bash
# The grounding evaluation's model calls (docs/evals/computer-grounding.md) against the team's local model: runner.py
# in a small Python container on the internal tqa-a-model network, since the host can't reach it, straight to vLLM
# (the eval measures the model; the product's own path is RT-I-047). The key is mounted read-only and never printed.
#
#   tests/e2e/grounding_eval/run_eval.sh DIR [runner options, such as --limit 10 or --ways coordinates]
#
# DIR holds targets.json and captures/. The model must be started with images on: TQA_LOCAL_MODEL_IMAGES=3.
# Each call is shaped as the local route sends it: its max_output (16384) and its extra_body (thinking on), four at a
# time, as many as the server batches (TQA_EVAL_MAX_TOKENS, TQA_EVAL_EXTRA_BODY and TQA_EVAL_CONCURRENCY override).
set -euo pipefail
dir=$(realpath "${1:?usage: run_eval.sh DIR [runner options]}")
shift
here=$(cd "$(dirname "$0")" && pwd)
key=${TQA_LOCAL_MODEL_KEY:-$HOME/.config/tensorquay/local-model.key}
[[ -r "$key" ]] || { echo "no readable key file at $key" >&2; exit 2; }
[[ -f "$dir/targets.json" && -f "$dir/captures/captures.json" ]] || {
  echo "$dir needs targets.json and captures/captures.json (targets.py, then the capture)" >&2; exit 2; }
# The runtime's own base image has Python and is already here, so nothing is pulled; only the standard library is used.
# Rootless Docker: the container's root is this user on the host, so it can write DIR.
image=${TQA_EVAL_IMAGE:-tqa-runtime:base}
docker run --rm --network tqa-a-model --user 0:0 --entrypoint python3 \
  -v "$here:/app:ro" -v "$dir:/eval" -v "$key:/run/local-model.key:ro" \
  "$image" /app/runner.py /eval \
  --route "${TQA_EVAL_ROUTE:-qwen3.8-27b}" --base-url http://local-model:8000/v1 --model qwen3.8-27b \
  --key-file /run/local-model.key --max-tokens "${TQA_EVAL_MAX_TOKENS:-16384}" \
  --extra-body "${TQA_EVAL_EXTRA_BODY:-{\"chat_template_kwargs\": {\"enable_thinking\": true}}}" \
  --concurrency "${TQA_EVAL_CONCURRENCY:-4}" "$@"
docker run --rm --network none --user 0:0 --entrypoint python3 -v "$here:/app:ro" -v "$dir:/eval" "$image" /app/scorer.py /eval
