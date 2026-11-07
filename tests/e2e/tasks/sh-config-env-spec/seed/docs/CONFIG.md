# Configuration

`config/app.env` holds one `KEY=value` per line; blank lines and lines starting with `#` are ignored.

| Key | Required | Rule |
|---|---|---|
| NAME | yes | lowercase letters, digits and hyphens |
| PORT | yes | a whole number from 1024 to 65535 |
| LOG_LEVEL | yes | one of `debug`, `info`, `warn`, `error` |
| DATA_DIR | yes | an absolute path |
| WORKERS | no | a whole number from 1 to 16; default 2 |

No other keys are allowed. Each key appears at most once.
