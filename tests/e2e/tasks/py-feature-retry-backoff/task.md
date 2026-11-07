Give `fetch_with_retry` in `client.py` an `attempts` parameter (default 3) and retry a call that raises
`TransientError` until it succeeds or `attempts` calls have failed; the last error is then raised. Between tries,
wait with exponential backoff, calling the injected `sleep` with `base_delay * 2**n` for the n-th retry (n from 0),
where `base_delay` is a new parameter defaulting to 0.5. Any other exception is raised at once, without retrying.
Keep the tests in `tests/` passing and add tests for the retries.
