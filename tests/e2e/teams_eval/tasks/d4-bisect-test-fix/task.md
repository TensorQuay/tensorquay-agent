`ISSUE.md` describes a regression in textkit. Run `python3 make_repo.py` to create `project/`, a Git repository with
its history. Find the commit that brought the regression and name it (its hash) and what it changed in `BISECT.md`;
add a test to `project/tests/` that fails on that commit and passes on the one before it; and fix the regression in
`project/` so that all its tests pass. Run the tests from `project/` with `python3 -m unittest discover -s tests -t .`.
