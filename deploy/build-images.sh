#!/usr/bin/env bash
# Builds the images one installation needs from this checkout (deploy/README.md, "0. Build the images"), then prints
# the three `export` lines step 1 uses. Nothing is pushed or published; the images stay in the local Docker engine.
# Usage: deploy/build-images.sh [--workstation] [--desktop] [--fake-model]
#   --workstation  also the Workstation environment (Python, Node, Go and a browser; about 3.0 GB)
#   --desktop      also the Desktop environment: the Workstation with a desktop the agent sees and operates (ADR-0039;
#                  builds the Workstation too)
#   --fake-model   also a scripted OpenAI-compatible test model, for trying the stack without a model server
set -euo pipefail
cd "$(dirname "$0")/.."

workstation=0
desktop=0
fake_model=0
for flag in "$@"; do
  case "$flag" in
    --workstation) workstation=1 ;;
    --desktop) desktop=1; workstation=1 ;;
    --fake-model) fake_model=1 ;;
    *) echo "usage: deploy/build-images.sh [--workstation] [--desktop] [--fake-model]" >&2; exit 2 ;;
  esac
done

build() {
  echo "== $(date +%T) building $1"
  shift
  docker build "$@" .
}

# tqa-runtime:minimal and tqa-runtime:workstation are the canonical tags: the gateway registers the image each one
# names as that built-in environment (with its labels), so nobody types an image ID. :base stays for existing settings.
build "the runtime (the Minimal environment each task runs in)" --target base -f images/runtime/Dockerfile -t tqa-runtime:base -t tqa-runtime:minimal
build "the gateway" -f images/gateway/Dockerfile -t tqa-gateway:local
build "the web app" --build-arg SOURCE_DATE_EPOCH=0 -f images/web/Dockerfile -t tqa-web:local
if [ "$workstation" = 1 ]; then
  build "the Workstation environment" --target workstation -f images/runtime/Dockerfile -t tqa-runtime:workstation
fi
if [ "$desktop" = 1 ]; then
  build "the Desktop environment" --target desktop -f images/runtime/Dockerfile -t tqa-runtime:desktop
fi
if [ "$fake_model" = 1 ]; then
  build "the test model" -f images/fake-model/Dockerfile -t tqa-fake-model:local
fi

echo "== $(date +%T) done. For step 1:"
echo "export TQA_RUNTIME_IMAGE='$(docker image inspect --format '{{.Id}}' tqa-runtime:base)'"
echo "export TQA_GATEWAY_IMAGE='tqa-gateway:local'"
echo "export TQA_WEB_IMAGE='tqa-web:local'"
if [ "$workstation" = 1 ]; then
  echo "# Workstation is registered as an environment by itself at the gateway's next start (tqa-runtime:workstation)."
fi
if [ "$desktop" = 1 ]; then
  echo "# Desktop is registered the same way (tqa-runtime:desktop)."
fi
if [ "$fake_model" = 1 ]; then
  echo "# The test model as the route: set TQA_MODEL_BASE_URL='http://tqa-fake-model:8080/v1' and TQA_MODEL='fixture' in"
  echo "# step 1, and after step 3: docker run -d --name tqa-fake-model --network tqa_control tqa-fake-model:local"
fi
