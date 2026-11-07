#!/usr/bin/env bash
# Run the Rust tier-1 gates in the same order as CI, or with --kvm the tier-3 Firecracker gate G-13 on a prepared
# KVM host (deploy/firecracker/README.md): the KVM suites, the full stack on the microVM backend and the Gate 8 demo.
# --member is a member's run before a hand-in: every tier-1 gate but the whole-workspace coverage report
# (G-04-report, the largest build); G-04's 90 % threshold on the pure crates still runs. The integration runs it all.
set -euo pipefail

usage() {
    printf 'Usage: %s [--skip-audit] [--member] | --kvm\n' "$0" >&2
    exit 2
}
skip_audit=false kvm=false member=false
if [[ $# -eq 1 && $1 == --kvm ]]; then
    kvm=true
else
    for argument in "$@"; do
        case "$argument" in
            --skip-audit) skip_audit=true ;;
            --member) member=true ;;
            *) usage ;;
        esac
    done
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Every gate runs, whatever failed before it, so one failure never hides another; the run ends with the list.
failed=()
run_gate() {
    local gate="$1"
    shift
    if "$@"; then
        printf 'PASS %s\n' "$gate"
    else
        printf 'FAIL %s\n' "$gate" >&2
        failed+=("$gate")
    fi
}

finish() {
    if ((${#failed[@]} > 0)); then
        printf 'FAILED %d gate(s): %s\n' "${#failed[@]}" "${failed[*]}" >&2
        exit 1
    fi
    printf 'All gates passed\n'
    exit 0
}

if [[ "$kvm" == true ]]; then
    # The images and the kernel the suites boot, named before anything runs (tools/test_check_kvm.py); the KVM tests
    # name anything else they miss.
    for name in TQA_VMM_IMAGE TQA_FC_KERNEL_DIR TQA_FC_ROOTFS_DIR TQA_GATEWAY_IMAGE TQA_WEB_IMAGE TQA_RUNTIME_IMAGE \
            TQA_WORKSTATION_IMAGE TQA_FAKE_MODEL_IMAGE; do
        [[ -n ${!name:-} ]] || { printf 'check --kvm: %s is required (see deploy/firecracker/README.md)\n' "$name" >&2; exit 2; }
    done
fi

# G-13-unit: the microVM harness's own unit tests are plain Python (no KVM, no Docker), so every run has them, with or
# without --kvm; under --kvm only, a harness that no longer loads went unnoticed between KVM runs.
run_gate G-13-unit python3 -m unittest deploy/firecracker/test_host_setup.py deploy/firecracker/test_kvm_no_uaccess.py \
    deploy/firecracker/test_stack.py deploy/firecracker/test_history_checks.py deploy/firecracker/test_pids_checks.py \
    deploy/firecracker/test_guest_search.py deploy/firecracker/test_isolation_checks.py \
    deploy/firecracker/test_perf_io_vm.py deploy/firecracker/test_start_repro.py
# The run check below (G-13-images) reads the engine; its own logic is plain Python, so every run has it too.
run_gate G-13-images-unit python3 -m unittest tools/test_check_test_images.py

if [[ "$kvm" == true ]]; then
    export TQA_DOCKER_TESTS=1
    run_gate G-13-host bash deploy/firecracker/host-setup.sh --check
    run_gate G-13-kvm-acl bash deploy/firecracker/kvm-no-uaccess.sh --check
    run_gate G-13-suites tools/rootless-test.sh cargo nextest run -p tqa-sandbox --run-ignored all --retries 0 -j 1 \
        -E 'binary(firecracker_kvm) | binary(firecracker_contract) | test(sbx_f_)'
    # Each test removes the images it builds, also on failure; the probe cache keeps one image per variant (SBX-F-034).
    run_gate G-13-images python3 tools/check_test_images.py
    run_gate G-13-stack python3 deploy/firecracker/stack.py smoke
    run_gate G-13-oom python3 deploy/firecracker/stack.py oom
    run_gate G-13-start-failure python3 deploy/firecracker/stack.py start-failure
    run_gate G-13-egress python3 deploy/firecracker/contract.py egress
    run_gate G-13-escape python3 deploy/firecracker/contract.py escape
    run_gate G-13-full python3 deploy/firecracker/contract.py full
    run_gate G-13-destroy python3 deploy/firecracker/contract.py destroy
    run_gate G-13-lifetime python3 deploy/firecracker/contract.py lifetime
    run_gate G-13-reconcile python3 deploy/firecracker/contract.py reconcile
    run_gate G-13-reboot python3 deploy/firecracker/contract.py reboot
    run_gate G-13-history python3 deploy/firecracker/contract.py history
    run_gate G-13-demo python3 tests/e2e/gate8.py
    finish
fi

run_gate G-01 cargo fmt --all --check
run_gate G-02 cargo clippy --workspace --all-targets -- -D warnings
run_gate G-03 env TQA_TEST_DEADLINE_SCALE=4 cargo nextest run --workspace
# G-04: the 90 % line threshold applies to the pure crates (DEV-GUIDELINES section 4); the host-facing crates
# (runtime, sandbox, gateway, testkit) report coverage without a threshold because their integration tests need
# Docker or a KVM host and are skipped in tier 1.
# Coverage instrumentation slows every test several times over, so the test harnesses' liveness waits scale by 4
# (tqa_testkit::deadline; timing itself is asserted with fake clocks and is not scaled).
# Each coverage run starts clean: objects and profiles left by earlier runs (another commit, another package set)
# would otherwise be merged into the report, counting stale copies of changed files as uncovered lines.
run_gate G-04-clean cargo llvm-cov clean --workspace
run_gate G-04 env TQA_TEST_DEADLINE_SCALE=4 cargo llvm-cov nextest -p tqa-types -p tqa-llm -p tqa-context -p tqa-tools -p tqa-prompt -p tqa-core --fail-under-lines 90
if [[ "$member" == true ]]; then
    printf 'SKIP G-04-report-clean, G-04-report (--member: the integration runs the workspace report)\n'
else
    run_gate G-04-report-clean cargo llvm-cov clean --workspace
    run_gate G-04-report env TQA_TEST_DEADLINE_SCALE=4 cargo llvm-cov nextest --workspace
fi
run_gate G-05 cargo deny check
# G-05-zip (SEC-063): crates/ calls no zip extraction API (ADR-0017 point 9), and no zip advisory ignore outlives its need.
run_gate G-05-zip-unit python3 -m unittest tools/test_check_zip_reads.py
run_gate G-05-zip python3 tools/check-zip-reads.py
# G-05-images (SEC-030, ADR-0017): third-party images in the compose files are on the approved list, pinned, licensed.
run_gate G-05-images-unit python3 -m unittest tools/test_check_images.py
run_gate G-05-images python3 tools/check-images.py
# REL-U (ADR-0017 point 7): the release SBOM generator on fixture image trees (no Docker, no network).
run_gate REL-U python3 -m unittest discover -s tools/release -p 'test_*.py'
# DEP-U (deploy/README.md, after the Docker daemon was killed): the orphaned-container check on a fake runc root.
run_gate DEP-U python3 -m unittest discover -s deploy/tools -p 'test_*.py'
# DEPLOY-U: the deploy harness's own unit suite (compose, soak, load record, demo...), without Docker or the network,
# whatever the caller exports (--kvm exports TQA_DOCKER_TESTS above). Its fixtures are found from each test's own
# location, so it passes from any working directory.
run_gate DEPLOY-U env -u TQA_DOCKER_TESTS PYTHONPATH=deploy/tests python3 -m unittest discover -s deploy/tests -p 'test_*.py'
# DEPLOY-U's siblings: the other plain unit suites, run nowhere else until 1 Oct 2026 (the TEST-PLAN audit). None
# needs Docker or the network; a test that starts a fake server listens on loopback. The task fixtures under
# tests/e2e/tasks are seeds and hidden checks for the agent, not tests of ours, and discovery never reaches them.
# They run without TQA_DOCKER_TESTS whatever the caller exports, so the rootfs builds and the gate demos'
# Docker paths stay in their own steps. tools/test_check_all_gates.py stays out: it runs this script itself.
run_gate DEPLOY-U-e2e env -u TQA_DOCKER_TESTS python3 -m unittest discover -s tests/e2e -p 'test_*.py'
run_gate DEPLOY-U-e2e-cmp env -u TQA_DOCKER_TESTS python3 -m unittest discover -s tests/e2e/cmp -p 'test_*.py'
run_gate DEPLOY-U-e2e-live env -u TQA_DOCKER_TESTS python3 -m unittest discover -s tests/e2e/live -p 'test_*.py'
run_gate DEPLOY-U-images env -u TQA_DOCKER_TESTS python3 -m unittest discover -s images/runtime -p 'test_*.py'
run_gate DEPLOY-U-rootfs env -u TQA_DOCKER_TESTS python3 -m unittest discover -s images/rootfs -p 'test_*.py'
run_gate DEPLOY-U-testkit env -u TQA_DOCKER_TESTS python3 -m unittest discover -s crates/tqa-testkit/scripts -p 'test_*.py'
run_gate DEPLOY-U-tools env -u TQA_DOCKER_TESTS python3 -m unittest tools/test_check_kvm.py tools/test_rootless_test.py \
    tools/test_pipefail_first_line.py
# EGRESS-U (SEC-019): the Docker push fixture's searches look for every form of the credential, without Docker; and
# WEB-E2E-010's page-pair rule (what may differ between two loads of a page).
run_gate EGRESS-U env PYTHONPATH=crates/tqa-sandbox/tests python3 -m unittest egress.test_push_search egress.test_page_diff egress.test_stack_config egress.test_web_search egress.test_desktop egress.test_desktop_live egress.test_host_keys_microvm
# GATE-EVIDENCE-U: gate-evidence's mapping and the recorded results that stand in for an ID (soak and 15.1 files).
run_gate GATE-EVIDENCE-U bash -c 'python3 -m unittest tools/test_recorded_evidence.py && python3 tools/gate-evidence.py --self-test'
run_gate G-06 python3 tools/check-file-length.py
run_gate G-07 python3 tools/check-dep-direction.py
run_gate G-08 cargo run -q -p tqa-types --bin export-schema -- --check crates/tqa-types/schema/tqa-protocol-v1.json
run_gate G-08-decision cargo run -q -p tqa-types --bin export-schema -- --decision --check crates/tqa-types/schema/decision-v1.json
run_gate G-09 python3 tools/check-docs.py
run_gate G-09-catalog cargo run -q -p tqa-ext-check -- catalog lint docs/catalog
# G-09-complete (PRD NFR-8, MS5's "docs complete"): every ADR, testkit fake, named document and guide section the
# plans or the copy name exists; its own tests run first.
run_gate G-09-complete-unit python3 -m unittest tools/test_check_docs_complete.py
run_gate G-09-complete python3 tools/check-docs-complete.py
if [[ "$skip_audit" == true ]]; then
    printf 'SKIP G-10-audit (--skip-audit)\n'
else
    run_gate G-10-audit python3 tools/git-guard.py audit
fi
run_gate G-10-unit python3 -m unittest tools/test_git_guard.py
run_gate G-10-secrets python3 tools/check-secrets.py
finish
