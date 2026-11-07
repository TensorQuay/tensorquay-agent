"""Opt-in local Git clone contract; the fixture owns every container and temporary path."""
import os
import signal
import tempfile

from egress.git_clone import CloneStack, check
from egress.stack import compose, demo


def interrupted(_number, _frame):
    raise InterruptedError("Clone fixture interrupted or exceeded its eight-minute deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-clone-")
    directory = compose.installation(temporary)
    stack, failure = CloneStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(480)
    try:
        stack.prepare("clone")
        check(stack)
    except BaseException as error:
        failure = error
        print("Clone contract failed: " + compose.redact(str(error), env), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
