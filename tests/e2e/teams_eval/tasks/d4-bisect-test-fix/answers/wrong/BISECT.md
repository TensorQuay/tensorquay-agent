# The regression

Commit 4aceb36329b3 ("wrap: cheaper length check") counts the joining space twice, so a line exactly `width` long breaks one word early. Found with `git bisect` and the test in `project/tests/test_regression.py`, which fails on it and passes on its parent.
