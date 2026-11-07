#!/bin/sh
# Prints the deployment plan for one environment: sh deploy.sh dev|staging|prod
set -eu
env=${1:-}
case $env in
    dev)
        host=dev.internal.example
        replicas=1
        echo "deploy: target $env ($host)"
        echo "deploy: sync build/ to $host:/srv/app"
        echo "deploy: restart app with $replicas replica(s)"
        echo "deploy: check https://$host/healthz"
        ;;
    staging)
        host=staging.internal.example
        replicas=2
        echo "deploy: target $env ($host)"
        echo "deploy: sync build/ to $host:/srv/app"
        echo "deploy: restart app with $replicas replica(s)"
        echo "deploy: check https://$host/healthz"
        ;;
    prod)
        host=app.example
        replicas=6
        echo "deploy: target $env ($host)"
        echo "deploy: sync build/ to $host:/srv/app"
        echo "deploy: restart app with $replicas replica(s)"
        echo "deploy: check https://$host/healthz"
        ;;
    *)
        echo "deploy: unknown environment '$env'" >&2
        exit 2
        ;;
esac
