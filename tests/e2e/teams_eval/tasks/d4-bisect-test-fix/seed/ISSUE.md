# Bug: wrap() breaks lines one character early

Since a recent change, `wrap("aaaa bbbb", 9)` gives `["aaaa", "bbbb"]`, but "aaaa bbbb" is exactly 9 characters and
should stay on one line. It worked in the first releases.
