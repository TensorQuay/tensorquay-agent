#!/bin/sh
# Starts the service; with --check, only validates config/app.env against docs/CONFIG.md.
set -u
config=config/app.env
errors=0
problem() { printf 'config: %s\n' "$1" >&2; errors=$((errors + 1)); }
seen=" "
while IFS= read -r line || [ -n "$line" ]; do
    case $line in ''|'#'*) continue ;; esac
    key=${line%%=*}
    value=${line#*=}
    case $seen in *" $key "*) problem "$key appears twice" ;; esac
    seen="$seen$key "
    case $key in
        NAME) printf '%s' "$value" | grep -Eq '^[a-z0-9-]+$' || problem "NAME is invalid" ;;
        PORT) { printf '%s' "$value" | grep -Eq '^[0-9]+$' && [ "$value" -ge 1024 ] && [ "$value" -le 65535 ]; } || problem "PORT is out of range" ;;
        LOG_LEVEL) case $value in debug|info|warn|error) ;; *) problem "LOG_LEVEL is invalid" ;; esac ;;
        DATA_DIR) case $value in /*) ;; *) problem "DATA_DIR is not absolute" ;; esac ;;
        WORKERS) { printf '%s' "$value" | grep -Eq '^[0-9]+$' && [ "$value" -ge 1 ] && [ "$value" -le 16 ]; } || problem "WORKERS is out of range" ;;
        *) problem "unknown key $key" ;;
    esac
done < "$config"
for key in NAME PORT LOG_LEVEL DATA_DIR; do
    case $seen in *" $key "*) ;; *) problem "$key is missing" ;; esac
done
if [ "$errors" -gt 0 ]; then
    exit 1
fi
if [ "${1:-}" = "--check" ]; then
    echo "config: ok"
    exit 0
fi
echo "starting $config"
