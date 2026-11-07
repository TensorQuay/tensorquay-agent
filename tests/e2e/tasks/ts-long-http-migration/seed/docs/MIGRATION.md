# Migrating from legacy/http.ts to net/client.ts

`legacy/http.ts` is deprecated and will be removed. Every caller moves to `net/client.ts`:

| Before | After |
|---|---|
| `import { request } from "../legacy/http.ts";` | `import { send } from "../net/client.ts";` |
| `request(url)` | `send({ url })` |
| `request(url, { retries: n })` | `send({ url, attempts: n })` |
| `DEFAULT_RETRIES` | `DEFAULT_ATTEMPTS` |

The responses are identical, so behaviour must not change.
