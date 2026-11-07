The service in `app/` (24 entity modules and four reports) calls SQLite directly. Port it to repositories as
`STORE.md` describes: design the interface in `store/`, implement it on SQLite and in memory, and move every module
onto it, keeping every function's name and arguments. When you're done, no module outside `store/` imports `sqlite3`
or `app.db`, and the tests in `tests/` pass unchanged on SQLite and in memory. Don't change the tests; run them with
`python3 -m unittest discover -s tests -t .`.
