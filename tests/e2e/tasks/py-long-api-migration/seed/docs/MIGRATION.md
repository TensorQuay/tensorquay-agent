# Migrating from lib.old_api to lib.new_api

`lib.old_api` is deprecated and will be removed. Every caller moves to `lib.new_api`:

| Before | After |
|---|---|
| `from lib import old_api` | `from lib import new_api` |
| `old_api.fetch(url, retries=n)` | `new_api.get(url, attempts=n)` |
| `old_api.fetch(url, n)` | `new_api.get(url, n)` |
| `old_api.DEFAULT_RETRIES` | `new_api.DEFAULT_ATTEMPTS` |

The responses are identical, so behaviour must not change.
