Audit the repository here (about 300 files) for defects, security and correctness alike: there are twelve. Report
each one you find in `AUDIT.md` with its file and function, what's wrong and what it allows, and fix at least six of
them, each with a test in `tests/` that fails before your fix and passes after it. The existing tests must still pass;
run them with `python3 -m unittest discover -s tests -t .`.
