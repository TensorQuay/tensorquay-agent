# Architecture

## Components

- `shopflow/api`: HTTP handlers: validates orders and publishes them to the queue
- `shopflow/auth`: checks API tokens for the api handlers
- `shopflow/queue`: an in-process job queue between the api and the worker
- `shopflow/worker`: takes jobs off the queue and runs them

## Data flow

A request reaches `api` (checked by `auth`), the order is saved by `storage` and a job is published to `queue`; `worker` takes it, `billing` charges it, `storage` records the state and `notifier` emails the customer.

## Adding a feature

Add a handler in `shopflow/api/handlers.py` and its route in `shopflow/api/routes.py`; background work goes through `shopflow/queue/jobs.py` and a branch in `shopflow/worker/loop.py`.
