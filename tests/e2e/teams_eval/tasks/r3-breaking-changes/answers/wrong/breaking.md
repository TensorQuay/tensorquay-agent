# Breaking changes in 2025

| Version | Change | Migration |
|---|---|---|
| 4.1.0 | BREAKING: the `--legacy-auth` flag is removed. | use `--auth token` |
| 4.1.0 | Removed `Client.fetch_all()`. | iterate with `Client.iter_pages()` |
| 4.2.0 | The config key `server.port` is renamed to `listen.port`; the old name is no longer read. | rename the key to `listen.port` |
| 4.3.0 | Python 3.8 is no longer supported. | run on Python 3.10 or later |
| 4.3.0 | The default format of `export` changed from CSV to JSON (breaking). | pass `--format csv` to keep CSV |
| 4.4.0 | `TQ_HOME` is no longer read. | set `TQ_CONFIG_DIR` instead |
| 4.5.0 | BREAKING: the `/v1/items` endpoint is gone. | call `/v2/items` |
| 4.5.0 | The `timeout` argument is now in seconds, not milliseconds (breaking). | divide existing values by 1000 |
| 4.6.0 | `sync --force` was removed. | use `sync --overwrite` |
| 4.7.0 | Plugins: `register(name, fn)` no longer works; `register()` takes a `Plugin` object. | wrap the function in a `Plugin` |
| 4.8.0 | `Record.id` is now a string instead of an integer (breaking). | compare ids as strings |
| 4.9.0 | `--quiet` is renamed to `--silent`; `--quiet` is no longer accepted. | use `--silent` |
| 4.2.0 | `--verbose` is deprecated in favour of `--log-level debug`; it still works. | none |
| 4.4.0 | `config.yaml` is deprecated in favour of `config.toml`; both are read until 6.0. | none |
| 4.6.0 | `Client.get_raw()` is deprecated and will be removed in 6.0. | none |
| 4.8.0 | `TQ_DEBUG` is deprecated; it still enables debug logging. | none |
