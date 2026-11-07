# store/: the repository interface

The service moves off direct SQLite calls onto repositories, so its modules can run on any storage. `store/` holds:

- the interface (`store/base.py`): a repository per table, with what the modules need; its shape is yours to design;
- `store/sqlite.py`: the SQLite implementation, the only module allowed to import `sqlite3`;
- `store/memory.py`: an in-memory implementation, for tests and for running without a database;
- a way for the modules to get the repositories for a path: `store.open(path)` returns them on SQLite, and
  `store.use_memory()` makes every later `store.open` return one shared in-memory set (`store.use_sqlite()` undoes
  it).

When the port is done, no module outside `store/` imports `sqlite3` or `app.db`, every module's functions keep their
names and arguments (the `path` argument stays and is passed to `store.open`), and the tests in `tests/` pass
unchanged on both implementations.
