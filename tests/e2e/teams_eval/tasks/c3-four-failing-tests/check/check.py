from checklib import fail, run, tests, HERE

def check(workspace):
    suite = workspace / "suite"
    passed, output = tests(suite, suite / "tests", suite)
    if not passed:
        fail("a test in suite/ fails", output)
    passed, output = tests(suite, HERE / "hidden")
    if not passed:
        fail("a held-out test fails: a fix is special-cased or incomplete", output)
    return "the four tests pass, unchanged, and the held-out cases too"

run(check)
