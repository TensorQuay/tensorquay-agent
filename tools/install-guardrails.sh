#!/bin/sh
# One-time setup per clone: the guardrail hooks (AGENTS.md → Git guardrails). A maintainer adds --maintainer, which
# also sets the company identity every maintainer commit carries and turns on the identity and remote rules; a
# contributor keeps their own identity.
set -e
usage() { echo "usage: $0 [--maintainer]" >&2; exit 2; }
[ $# -le 1 ] || usage
case "${1-}" in
    --maintainer) maintainer=true ;;
    "") maintainer=false ;;
    *) usage ;;
esac
cd "$(git rev-parse --show-toplevel)"
if [ "$maintainer" = true ]; then
    git config user.name "TensorQuay"
    git config user.email "dev@tensorquay.com"
    git config tq.guard.maintainer true
    deny="${TQ_GUARD_DENYLIST:-$HOME/.config/tensorquay/guard-denylist}"
    [ -s "$deny" ] || echo "warning: create $deny (one personal identifier per line; never commit it)"
fi
git config core.hooksPath .githooks
chmod +x .githooks/* tools/git-guard.py
python3 tools/git-guard.py audit
