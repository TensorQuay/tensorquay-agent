`src/legacy/http.ts` is deprecated. Move every endpoint under `src/app/` to `src/net/client.ts`, following
`docs/MIGRATION.md`; note that `send` takes one options object. Every endpoint's `run()` must return exactly the same
line as before, and nothing under `src/app/` may use the legacy helper any more. Do not change `src/legacy/`,
`src/net/` or the documentation. Run the tests with `node --test test/*.test.ts`.
