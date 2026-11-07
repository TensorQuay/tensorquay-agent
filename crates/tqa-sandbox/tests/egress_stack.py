"""SBX-C-004, SEC-001, and in a Desktop sandbox SEC-057/058, on an owned fixed-image Docker stack; no public upstream
required."""
import argparse
import os
import signal
import tempfile

from egress import checks, confinement, desktop
from egress.stack import Stack, compose, demo, keep_logs


def interrupted(_number, _frame):
    raise InterruptedError("Egress fixture interrupted or exceeded its eight-minute deadline")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", choices=["network", "security", "public", "confinement", "desktop"])
    args = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    if args.case == "public" and os.environ.get("TQA_PUBLIC_NETWORK_TESTS") != "1":
        raise SystemExit("Public-network probes require TQA_PUBLIC_NETWORK_TESTS=1")
    if args.case == "desktop" and not os.environ.get("TQA_DESKTOP_TEST_IMAGE"):
        raise SystemExit("The desktop case needs TQA_DESKTOP_TEST_IMAGE, the Desktop image under test")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-egress-")
    directory = compose.installation(temporary)
    stack, failure = Stack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(480)
    try:
        stack.prepare(args.case)
        stream = stack.start()
        if args.case == "confinement":
            confinement.check(stack, stream)
        elif args.case == "desktop":
            desktop.screen_text(stack, stream)
            checks.network(stack)
            desktop.egress(stack)
            desktop.secrets(stack)
        else:
            checks.security(stack, stream)
        if args.case == "network":
            checks.network(stack)
        elif args.case == "public":
            checks.public_network(stack)
    except BaseException as error:
        failure = error
        print("Egress contract failed: " + compose.redact(str(error), env), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        keep_logs(env, failure, stack.configuration)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
