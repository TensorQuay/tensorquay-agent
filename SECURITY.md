# Security policy

## Reporting a vulnerability

Report a security problem privately: open this repository's **Security** tab and choose **Report a vulnerability**
(GitHub's private vulnerability reporting). Never put it in a public issue, pull request or discussion.

Please include what you found, the steps that reproduce it, the version (a release, or the commit you built), the
sandbox backend, and what an attacker could do with it. We reply within five working days, keep you informed while we
work on a fix, and agree with you when to publish. We credit reporters who want to be named.

## Supported versions

Fixes go into the next release; earlier releases get no backports. The releases are listed in the
[changelog](CHANGELOG.md).

## Scope

TensorQuay Agent runs an agent's code in a sandbox on purpose. What the design defends, and the risks it accepts, are
in [ARCHITECTURE §12](docs/ARCHITECTURE.md#12-security-model). In scope, for example:

- a sandbox that reaches the host, another sandbox or another person's workspace;
- a way past the network allow-list, or a secret that reaches a sandbox, a log or a page;
- signing in, sessions, roles and tokens;
- the gateway's secret store.

Out of scope: what ARCHITECTURE §12 accepts (for example data the agent sends to a host the administrator allowed),
faults in a model or a third-party service, and attacks that need an administrator's own account.
