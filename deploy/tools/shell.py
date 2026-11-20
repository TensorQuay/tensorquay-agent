"""What the installer (install.py) and the integration harness (deploy/tests/compose.py) print about a command that
failed: its name and its last stderr lines, with the installation's secrets removed. One copy, imported by both."""
import shlex
import subprocess
import sys

STDERR_TAIL_LINES = 40
# The generated database password and bootstrap token, and the harness's stack password.
SECRET_NAMES = ("TQA_POSTGRES_PASSWORD", "TQA_BOOTSTRAP_TOKEN", "TQA_STACK_PASSWORD")


class CommandFailed(subprocess.CalledProcessError):
    """A command that exited non-zero, with its stderr: `CommandFailed(args, returncode, stderr)`."""

    def __init__(self, args, returncode, stderr):
        super().__init__(returncode, list(args), stderr=stderr)


def redact(text, env, extra=()):
    """Remove the generated database password and bootstrap token (and any `extra` value) from diagnostics."""
    for name in SECRET_NAMES:
        if env.get(name):
            text = text.replace(env[name], "[REDACTED]")
    for value in extra:
        if value:
            text = text.replace(value, "[REDACTED]")
    return text


def command_failure(args, env, error, file=None, extra=()):
    """Name the failing command and print at most forty redacted stderr lines."""
    file = sys.stderr if file is None else file
    if isinstance(error, subprocess.CalledProcessError):
        reason = "exit {}".format(error.returncode)
    elif isinstance(error, subprocess.TimeoutExpired):
        reason = "timeout after {} s".format(error.timeout)
    else:
        reason = "could not start"
    display = " ".join(shlex.quote(str(argument)) for argument in args)
    print("Command failed ({}): {}".format(reason, redact(display, env, extra)), file=file)
    detail = str(error) if isinstance(error, OSError) else error.stderr
    if isinstance(detail, bytes):
        detail = detail.decode("utf-8", errors="replace")
    if detail:
        print("\n".join(redact(detail, env, extra).splitlines()[-STDERR_TAIL_LINES:]), file=file)
