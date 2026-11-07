Some requests fail with status 500 in `logs/service.log`. Find the handlers under `app/handlers/` that cause the
failures and fix them, so that a ratio with no requests is `0.0` and a score of no values is `0`, as their docstrings
say. Do not change the log or the behaviour of any handler otherwise.
