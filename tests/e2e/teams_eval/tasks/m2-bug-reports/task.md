The folder `issues/` holds three bug reports against `calc/stats.py`. Reproduce each one with a failing test under
`tests/`, then fix the ones that are real bugs. A report that describes documented behaviour is not a bug: leave the
code as it is and mark its test with `@unittest.expectedFailure`. Run the tests with `python3 -m unittest discover -s tests -t .`.
