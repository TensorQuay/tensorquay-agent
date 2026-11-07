Some requests fail with status 500 in `logs/service.log`. Find which handler under `app/handlers/` causes the
failures and fix it so that a request with no hits recorded returns `0.0` instead of crashing. Do not change the
log or the behaviour of any other handler.
